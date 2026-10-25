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

#[tokio::test]
async fn rename_provenance_uses_real_code_authority_raw_rev_and_dirty_buffer_until_finish() {
    use devbox_lsp::{WorkspaceIdentity, WorkspaceProof, WorkspaceValidation};
    use sha2::{Digest, Sha256};
    let mut d = TestDaemon::fixture();
    let bin = d.home.path().join("lsp-bin");
    std::fs::create_dir(&bin).expect("owned bin");
    let exe = std::env::current_exe()
        .expect("exe")
        .parent()
        .expect("deps")
        .parent()
        .expect("debug")
        .join("examples/fake_lsp");
    assert!(exe.is_file(), "build owned fake_lsp first");
    std::os::unix::fs::symlink(exe, bin.join("rust-analyzer")).expect("owned launcher");
    d.extra_env
        .push(("PATH".into(), format!("{}:/usr/bin:/bin", bin.display())));
    d.launch();
    let client = d.client().await;
    let other = d.client().await;
    let root = d.home.path().join("project");
    std::fs::create_dir(&root).expect("owned root");
    std::fs::write(root.join("a.rs"), "const a = 1;\n").expect("a");
    std::fs::write(root.join("b.rs"), "\u{feff}const b = 1;\r\n").expect("b");
    let project = d.add_project(&client, root.to_str().expect("utf8")).await;
    let key = |path: &str| DocKey {
        root: format!("project:{project}"),
        path: path.into(),
    };
    let claim = |path: &str| ClaimDocument {
        key: key(path),
        policy: DocPolicy {
            kind: DocKind::Code,
        },
        window_id: "owner".into(),
    };
    let a = bounded(client.call::<DocumentsClaim>(&claim("a.rs")))
        .await
        .expect("a lease");
    let lease_a = a.lease_id.expect("a owner");
    let stream = open(&client, &project).await;
    let stranger = open(&other, &project).await;
    let uri_a = devbox_lsp::uri::file_uri(&root.join("a.rs")).expect("a URI");
    let uri_b = devbox_lsp::uri::file_uri(&root.join("b.rs")).expect("b URI");
    send(&stream,json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":uri_a,"version":0,"languageId":"rust","text":"const a = 2;\n"}}}),Some(&lease_a)).await;
    let request = |id| json!({"jsonrpc":"2.0","id":id,"method":"textDocument/rename","params":{"textDocument":{"uri":uri_a},"newName":"next","changes":{uri_a.clone():[],uri_b.clone():[]}}});
    send(&stream, request(2), Some(&lease_a)).await;
    let result = response(&stream).await;
    let proof: WorkspaceProof =
        serde_json::from_value(result["result"]["proof"].clone()).expect("daemon proof");
    assert_eq!(proof.request_id, "2");
    assert_eq!(proof.targets.len(), 2);
    assert_eq!(
        proof.targets[0].text_hash,
        format!("{:x}", Sha256::digest(b"const a = 2;\n"))
    );
    assert_eq!(
        proof.targets[1].text_hash,
        format!("{:x}", Sha256::digest(b"const b = 1;\n"))
    );
    assert_ne!(
        proof.targets[1]
            .disk_rev
            .as_ref()
            .expect("raw revision")
            .hash,
        proof.targets[1].text_hash
    );
    let b = bounded(client.call::<DocumentsClaim>(&claim("b.rs")))
        .await
        .expect("newly opened b");
    let lease_b = b.lease_id.expect("b owner");
    let identity = |proof: &WorkspaceProof| WorkspaceIdentity {
        token: proof.token.clone(),
        request_id: proof.request_id.clone(),
        root_uri: proof.root_uri.clone(),
        generation: proof.generation.clone(),
    };
    let validation = |proof: &WorkspaceProof| {
        let mut targets = proof.targets.clone();
        targets[1].lease_id = Some(lease_b.clone());
        WorkspaceValidation {
            identity: identity(proof),
            targets,
        }
    };
    send(&stranger,json!({"jsonrpc":"2.0","id":3,"method":"devbox/validateWorkspaceEdit","params":validation(&proof)}),None).await;
    assert_eq!(
        response(&stranger).await["result"],
        false,
        "wrong actual stream"
    );
    std::fs::write(root.join("b.rs"), "const external = 3;\n").expect("external edit");
    send(&stream,json!({"jsonrpc":"2.0","id":3,"method":"devbox/validateWorkspaceEdit","params":validation(&proof)}),None).await;
    assert_eq!(
        response(&stream).await["result"],
        false,
        "late disk cannot become request baseline"
    );
    send(
        &stream,
        json!({"jsonrpc":"2.0","method":"devbox/finishWorkspaceEdit","params":identity(&proof)}),
        None,
    )
    .await;
    // Explicit fresh request captures the changed file and receives a distinct proof.
    send(&stream, request(4), Some(&lease_a)).await;
    let fresh = response(&stream).await;
    let fresh: WorkspaceProof =
        serde_json::from_value(fresh["result"]["proof"].clone()).expect("fresh proof");
    assert_ne!(fresh.token, proof.token);
    send(&stream,json!({"jsonrpc":"2.0","id":5,"method":"devbox/validateWorkspaceEdit","params":validation(&fresh)}),None).await;
    assert_eq!(
        response(&stream).await["result"],
        true,
        "unchanged captured unopened target and dirty opened source"
    );
    send(
        &stream,
        json!({"jsonrpc":"2.0","method":"devbox/finishWorkspaceEdit","params":identity(&fresh)}),
        None,
    )
    .await;
    send(&stream,json!({"jsonrpc":"2.0","id":6,"method":"devbox/validateWorkspaceEdit","params":validation(&fresh)}),None).await;
    assert_eq!(
        response(&stream).await["result"],
        false,
        "ACK consumes once"
    );
    send(
        &stranger,
        json!({"jsonrpc":"2.0","id":7,"method":"test/echo","params":"shared generation alive"}),
        None,
    )
    .await;
    assert_eq!(
        response(&stranger).await["result"],
        "shared generation alive"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("a.rs")).expect("disk a"),
        "const a = 1;\n"
    );
    let saved = bounded(client.call::<devbox_documents::DocumentsSave>(
        &devbox_documents::SaveDocument {
            lease_id: lease_a.clone(),
            seq: 1,
            base_rev: a.rev.clone(),
            expect: devbox_documents::Expect::Present,
            text: "const a = 2;\n".into(),
        },
    ))
    .await
    .expect("explicit user save");
    send(&stream,json!({"jsonrpc":"2.0","method":"textDocument/didSave","params":{"textDocument":{"uri":uri_a},"text":"const a = 2;\n"}}),Some(&lease_a)).await;
    send(&stream, request(8), Some(&lease_a)).await;
    let after_save = response(&stream).await;
    let after_save: WorkspaceProof =
        serde_json::from_value(after_save["result"]["proof"].clone()).expect("Rename after save");
    assert_eq!(after_save.targets[0].disk_rev, Some(saved.rev));
    send(&stream,json!({"jsonrpc":"2.0","id":9,"method":"devbox/validateWorkspaceEdit","params":validation(&after_save)}),None).await;
    assert_eq!(
        response(&stream).await["result"],
        true,
        "saving then renaming without another edit remains valid"
    );
    send(&stream,json!({"jsonrpc":"2.0","method":"devbox/finishWorkspaceEdit","params":identity(&after_save)}),None).await;
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(d.child.as_ref().expect("daemon").0.id() as i32),
        nix::sys::signal::Signal::SIGTERM,
    )
    .expect("owned stop");
    assert!(
        d.child
            .as_mut()
            .expect("daemon")
            .0
            .wait()
            .expect("owned wait")
            .success()
    );
}
