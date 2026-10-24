mod support;
use devbox_cli::client::{Client, ClientStream};
use devbox_documents::{ClaimDocument, DocKey, DocKind, DocPolicy, DocumentsClaim};
use devbox_lsp::{Language, LspRestart, LspServers, OpenLsp, RelayMessage, ServersQuery};
use devbox_observe::{ObserveProblems, ProblemsQuery};
use serde_json::{Value, json};
use std::time::Duration;
use support::{TestDaemon, bounded};
async fn send(stream: &ClientStream, value: Value, lease: Option<&str>) {
    let mut bytes = serde_json::to_vec(&RelayMessage {
        json: value.to_string(),
        lease_id: lease.map(str::to_owned),
    })
    .expect("packet");
    bytes.push(b'\n');
    stream.write(&bytes).await.expect("stream input");
}
async fn response(stream: &ClientStream) -> Value {
    let bytes = stream.read_until(b"\n", Duration::from_secs(5)).await;
    stream.credit(bytes.len() as u32).await.expect("credit");
    let packet: RelayMessage =
        serde_json::from_slice(bytes.split(|b| *b == b'\n').next().expect("line")).expect("packet");
    serde_json::from_str(&packet.json).expect("LSP JSON")
}
async fn open(client: &Client, id: &str) -> ClientStream {
    let stream = bounded(client.open_stream("lsp.open", json!({"projectId":id,"language":"rust"})))
        .await
        .expect("LSP stream");
    send(
        &stream,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        None,
    )
    .await;
    assert_eq!(response(&stream).await["result"]["initializeCount"], 1);
    stream
}
#[tokio::test]
async fn lsp_rpc_relay_uses_document_authority_and_replaces_problem_diagnostics() {
    let mut d = TestDaemon::fixture();
    let bin = d.home.path().join("lsp-bin");
    std::fs::create_dir(&bin).expect("bin");
    let exe = std::env::current_exe()
        .expect("exe")
        .parent()
        .expect("deps")
        .parent()
        .expect("debug")
        .join("examples/fake_lsp");
    assert!(
        exe.is_file(),
        "build owned devbox-lsp example fixture before CLI test"
    );
    std::os::unix::fs::symlink(&exe, bin.join("rust-analyzer")).expect("owned launcher");
    d.extra_env
        .push(("PATH".into(), format!("{}:/usr/bin:/bin", bin.display())));
    d.launch();
    let a = d.client().await;
    let b = d.client().await;
    let root = d.home.path().join("lsp-project");
    std::fs::create_dir(&root).expect("project");
    let file = "가 😀.rs";
    std::fs::write(root.join(file), "fn main() {}\n").expect("file");
    let id = d.add_project(&a, root.to_str().expect("utf8")).await;
    let servers = bounded(a.call::<LspServers>(&ServersQuery {
        project_id: id.clone(),
    }))
    .await
    .expect("servers RPC");
    assert!(
        servers.items.iter().all(|s| s.pid.is_none()),
        "PATH detection never starts a server"
    );
    let key = DocKey {
        root: format!("project:{id}"),
        path: file.into(),
    };
    let claimed = bounded(a.call::<DocumentsClaim>(&ClaimDocument {
        key: key.clone(),
        policy: DocPolicy {
            kind: DocKind::Code,
        },
        window_id: "a".into(),
    }))
    .await
    .expect("claim");
    let lease = claimed.lease_id.expect("lease");
    let readonly = bounded(b.call::<DocumentsClaim>(&ClaimDocument {
        key,
        policy: DocPolicy {
            kind: DocKind::Code,
        },
        window_id: "b".into(),
    }))
    .await
    .expect("readonly");
    assert!(readonly.lease_id.is_none());
    let owner = open(&a, &id).await;
    let reader = open(&b, &id).await;
    let uri = devbox_lsp::uri::file_uri(&root.join(file)).expect("URI");
    for (stream, lease) in [(&owner, Some(lease.as_str())), (&reader, None)] {
        send(stream,json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":uri,"languageId":"rust","version":1,"text":"fn main() {}"}}}),lease).await;
    }
    send(
        &owner,
        json!({"jsonrpc":"2.0","id":2,"method":"test/state"}),
        None,
    )
    .await;
    assert_eq!(response(&owner).await["result"]["opens"], 1);
    send(&reader,json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":"not owner"}]}}),None).await;
    assert!(reader.wait_end(Duration::from_secs(3)).await);
    send(&owner,json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":"fn broken()"}]}}),Some(&lease)).await;
    assert_eq!(
        response(&owner).await["method"],
        "textDocument/publishDiagnostics"
    );
    let result = bounded(a.call::<ObserveProblems>(&ProblemsQuery {
        project_id: id.clone(),
        limit: None,
        cursor: None,
    }))
    .await
    .expect("problems");
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].source, "lsp:rust-analyzer");
    assert_eq!(result.items[0].file, root.join(file).display().to_string());
    assert_eq!((result.items[0].line, result.items[0].col), (1, Some(4)));
    assert_eq!(
        std::fs::read_to_string(root.join(file)).expect("disk"),
        "fn main() {}\n"
    );
    send(
        &owner,
        json!({"jsonrpc":"2.0","method":"test/log","params":{}}),
        None,
    )
    .await;
    assert_eq!(response(&owner).await["method"], "window/logMessage");
    bounded(a.call::<LspRestart>(&OpenLsp {
        project_id: id.clone(),
        language: Language::Rust,
    }))
    .await
    .expect("restart");
    assert!(owner.wait_end(Duration::from_secs(2)).await);
    let empty = bounded(a.call::<ObserveProblems>(&ProblemsQuery {
        project_id: id,
        limit: None,
        cursor: None,
    }))
    .await
    .expect("empty after generation");
    assert!(empty.items.is_empty());
    // TestDaemon owns the fixture process; ask it to shut down cleanly so it reaps its fake server.
    use std::os::unix::process::ExitStatusExt;
    let pid = d.child.as_ref().expect("daemon").0.id();
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(pid as i32),
        nix::sys::signal::Signal::SIGTERM,
    )
    .expect("owned stop");
    let status = d.child.as_mut().expect("daemon").0.wait().expect("wait");
    assert_eq!(status.signal(), None);
    let mut logs = vec![d.home.path().join("stderr.log")];
    if let Ok(entries) = std::fs::read_dir(&d.paths.log_dir) {
        logs.extend(
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.is_file()),
        );
    }
    for path in logs {
        let text = std::fs::read_to_string(path).expect("owned daemon log");
        assert!(
            !text.contains("PRIVATE_LSP_LOG_SENTINEL"),
            "raw LSP log body must never reach daemon logs"
        );
    }
}
