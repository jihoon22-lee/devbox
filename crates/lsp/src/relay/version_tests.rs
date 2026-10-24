use super::*;

#[tokio::test]
async fn renderer_diagnostics_map_local_versions_only_for_current_text_snapshots() {
    let mut relay = initialized().await;
    let uri = "file:///owned/a.rs";
    let open = |version, text, lease: Option<&str>| {
        RelayMessage { json: json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":uri,"languageId":"rust","version":version,"text":text}}}).to_string(), lease_id: lease.map(str::to_owned) }
    };
    relay
        .client(1, open(0, "disk", Some("L1")), &access())
        .await
        .unwrap();
    relay
        .client(2, open(42, "disk", None), &access())
        .await
        .unwrap();
    let diagnostics = |version: Option<i64>| {
        json!({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":uri,"version":version,"diagnostics":[]}}).to_string()
    };
    let out = relay
        .server(&diagnostics(Some(1)), &access())
        .await
        .unwrap();
    assert_eq!(
        out.clients
            .iter()
            .map(|(id, raw)| (*id, parse(raw)["params"]["version"].as_i64().unwrap()))
            .collect::<Vec<_>>(),
        [(1, 0), (2, 42)]
    );
    let change = |version, text| {
        RelayMessage { json: json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":uri,"version":version},"contentChanges":[{"text":text}]}}).to_string(), lease_id: Some("L1".into()) }
    };
    relay.client(1, change(1, "new"), &access()).await.unwrap();
    assert!(
        relay
            .server(&diagnostics(Some(1)), &access())
            .await
            .unwrap()
            .clients
            .is_empty()
    );
    let current = relay
        .server(&diagnostics(Some(2)), &access())
        .await
        .unwrap();
    assert_eq!(current.clients.len(), 1);
    assert_eq!(current.clients[0].0, 1);
    assert_eq!(parse(&current.clients[0].1)["params"]["version"], 1);
    assert_eq!(
        current.diagnostics.len(),
        1,
        "empty diagnostics still clear Problems"
    );
    assert!(
        relay
            .server(&diagnostics(Some(3)), &access())
            .await
            .unwrap()
            .clients
            .is_empty(),
        "future server version is never relabeled current"
    );
    let unversioned = relay.server(&diagnostics(None), &access()).await.unwrap();
    assert_eq!(unversioned.clients.len(), 1);
    assert!(parse(&unversioned.clients[0].1)["params"]["version"].is_null());
    relay
        .client(2, open(100, "new", Some("L2")), &access())
        .await
        .unwrap();
    let handoff = relay
        .server(&diagnostics(Some(3)), &access())
        .await
        .unwrap();
    assert_eq!(
        handoff
            .clients
            .iter()
            .map(|(id, raw)| (*id, parse(raw)["params"]["version"].as_i64().unwrap()))
            .collect::<Vec<_>>(),
        [(1, 1), (2, 100)]
    );
    relay.detach(2);
    relay.attach(2).unwrap();
    relay
        .client(
            2,
            packet(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}})),
            &access(),
        )
        .await
        .unwrap();
    relay
        .client(2, open(0, "new", Some("L2")), &access())
        .await
        .unwrap();
    let reconnected = relay
        .server(&diagnostics(Some(4)), &access())
        .await
        .unwrap();
    assert_eq!(
        parse(
            &reconnected
                .clients
                .iter()
                .find(|(id, _)| *id == 2)
                .unwrap()
                .1
        )["params"]["version"],
        0
    );
}

#[tokio::test]
async fn versioned_workspace_edits_are_request_bound_and_rejected_as_a_whole() {
    let mut relay = initialized().await;
    let a = "file:///owned/a.rs";
    let b = "file:///owned/b.rs";
    for (client, version, lease) in [(1, 0, Some("L1")), (2, 7, None)] {
        for uri in [a, b] {
            relay.client(client, RelayMessage { json: json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":uri,"version":version,"languageId":"rust","text":"disk"}}}).to_string(), lease_id: lease.map(str::to_owned) }, &access()).await.unwrap();
        }
    }
    let changes = |v| json!({"documentChanges":[{"textDocument":{"uri":a,"version":1},"edits":[]},{"textDocument":{"uri":b,"version":v},"edits":[]}]});
    for (client, local) in [(1, 0), (2, 7)] {
        let request = relay.client(client, packet(json!({"jsonrpc":"2.0","id":20,"method":"textDocument/rename","params":{"textDocument":{"uri":a},"newName":"next"}})), &access()).await.unwrap();
        let id = parse(&request.server[0])["id"].clone();
        let out = relay
            .server(
                &json!({"jsonrpc":"2.0","id":id,"result":changes(1)}).to_string(),
                &access(),
            )
            .await
            .unwrap();
        assert_eq!(
            parse(&out.clients[0].1)["result"]["documentChanges"][0]["textDocument"]["version"],
            local
        );
    }
    let request = relay.client(1, packet(json!({"jsonrpc":"2.0","id":21,"method":"textDocument/rename","params":{"textDocument":{"uri":a},"newName":"next"}})), &access()).await.unwrap();
    let id = parse(&request.server[0])["id"].clone();
    let stale = relay
        .server(
            &json!({"jsonrpc":"2.0","id":id,"result":changes(0)}).to_string(),
            &access(),
        )
        .await
        .unwrap();
    assert_eq!(stale.clients.len(), 1);
    assert_eq!(parse(&stale.clients[0].1)["error"]["code"], -32801);
    assert!(parse(&stale.clients[0].1).get("result").is_none());
    for (method, expected) in [
        ("textDocument/rename", Value::Null),
        ("test/unrelated", json!(1)),
    ] {
        let request = relay
            .client(
                2,
                packet(json!({"jsonrpc":"2.0","id":22,"method":method,"params":{}})),
                &access(),
            )
            .await
            .unwrap();
        let id = parse(&request.server[0])["id"].clone();
        let mut edit = changes(1);
        if method == "textDocument/rename" {
            edit["documentChanges"][0]["textDocument"]["version"] = Value::Null;
            edit["documentChanges"][1]["textDocument"]
                .as_object_mut()
                .unwrap()
                .remove("version");
        }
        let out = relay
            .server(
                &json!({"jsonrpc":"2.0","id":id,"result":edit}).to_string(),
                &access(),
            )
            .await
            .unwrap();
        assert_eq!(
            parse(&out.clients[0].1)["result"]["documentChanges"][0]["textDocument"]["version"],
            expected
        );
    }
    let out = relay.server(&json!({"jsonrpc":"2.0","id":"apply","method":"workspace/applyEdit","params":{"edit":changes(1)}}).to_string(), &access()).await.unwrap();
    assert_eq!(out.clients.len(), 1);
    assert_eq!(out.clients[0].0, 1);
    assert_eq!(
        parse(&out.clients[0].1)["params"]["edit"]["documentChanges"][0]["textDocument"]["version"],
        0
    );
    let failed = relay.server(&json!({"jsonrpc":"2.0","id":"stale","method":"workspace/applyEdit","params":{"edit":changes(8)}}).to_string(), &access()).await.unwrap();
    assert!(failed.clients.is_empty());
    assert_eq!(parse(&failed.server[0])["result"]["applied"], false);
    assert!(relay.ready());
    assert!(failed.close.is_empty());
}
