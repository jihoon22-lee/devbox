mod support;
use devbox_documents::*;
use devbox_protocol::frame::FrameCodec;
use devbox_protocol::message::{
    ClientKind, ClientMessage, Control, ServerMessage, control_frame, json_frame, parse_json,
};
use futures::{SinkExt, StreamExt};
use std::time::{Duration, Instant};
use support::{TestDaemon, bounded};
use tokio::net::UnixStream;
use tokio_util::codec::Framed;

type Wire = Framed<UnixStream, FrameCodec>;
type Reply = Result<serde_json::Value, (String, Option<String>)>;

fn daemon(extra: &[(&str, String)]) -> TestDaemon {
    let mut d = TestDaemon::fixture();
    let notes = d.home.path().join("notes");
    std::fs::create_dir_all(&notes).expect("create fixture vault");
    for (name, text) in [("a.md", "a\n"), ("b.md", "b\n")] {
        std::fs::write(notes.join(name), text).expect("write fixture note");
    }
    d.extra_env
        .extend(extra.iter().map(|(k, v)| (k.to_string(), v.clone())));
    d.launch();
    d
}
/// Raw connection with Hello on channel 1 (multiple connections reuse the number).
async fn wire(d: &TestDaemon) -> Wire {
    let mut wire = Framed::new(
        UnixStream::connect(&d.socket)
            .await
            .expect("connect fixture"),
        FrameCodec::default(),
    );
    let hello = ClientMessage::Hello {
        version: devbox_cli::VERSION.into(),
        protocol: devbox_protocol::PROTOCOL_VERSION,
        client: ClientKind::Test,
        instance: d.instance.clone(),
    };
    wire.send(json_frame(1, &hello)).await.expect("send Hello");
    assert!(matches!(
        next(&mut wire).await,
        ServerMessage::Welcome { .. }
    ));
    wire
}
async fn next(wire: &mut Wire) -> ServerMessage {
    let frame = bounded(wire.next())
        .await
        .expect("frame")
        .expect("decode frame");
    parse_json(&frame).expect("server message")
}
async fn send(
    wire: &mut Wire,
    id: u32,
    method: &str,
    params: serde_json::Value,
    crid: Option<&str>,
) {
    let request = ClientMessage::Request {
        id,
        method: method.into(),
        params: serde_json::value::to_raw_value(&params).expect("params"),
        client_request_id: crid.map(Into::into),
    };
    wire.send(json_frame(1, &request))
        .await
        .expect("send request");
}
async fn reply(wire: &mut Wire, want: u32) -> Reply {
    loop {
        if let ServerMessage::Response {
            id, result, error, ..
        } = next(wire).await
            && id == want
        {
            return match (result, error) {
                (Some(result), _) => Ok(serde_json::from_str(result.get()).expect("result")),
                (_, Some(e)) => Err((e.code, e.detail.map(|d| d.get().to_owned()))),
                _ => panic!("empty response"),
            };
        }
    }
}
async fn call(wire: &mut Wire, id: u32, method: &str, params: serde_json::Value) -> Reply {
    send(wire, id, method, params, None).await;
    reply(wire, id).await
}
fn claim(path: &str, window: &str) -> serde_json::Value {
    serde_json::json!({ "key": { "root": "vault", "path": path }, "policy": { "kind": "note" }, "windowId": window })
}
/// Claims from a fresh connection; dropping that connection releases the lease.
async fn client_claim(d: &TestDaemon, path: &str, window: &str) -> Claimed {
    claim_on(&d.client().await, path, window).await
}
async fn claim_on(client: &devbox_cli::client::Client, path: &str, window: &str) -> Claimed {
    let params = ClaimDocument {
        key: DocKey {
            root: "vault".into(),
            path: path.into(),
        },
        policy: DocPolicy {
            kind: DocKind::Note,
        },
        window_id: window.into(),
    };
    client
        .call::<DocumentsClaim>(&params)
        .await
        .expect("claim over RPC")
}
/// Polls claims from a third connection until it gets the lease (or 1 s passes).
async fn leased_within_a_second(d: &TestDaemon, path: &str) -> Claimed {
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        let claimed = client_claim(d, path, "wC").await;
        if claimed.lease_id.is_some() || Instant::now() > deadline {
            return claimed;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
fn elsewhere(c: &Claimed) -> bool {
    c.read_only.as_ref().map(|r| r.reason) == Some(ReadOnlyReason::LeasedElsewhere)
}

#[tokio::test]
async fn claim_save_read_round_trip_over_rpc() {
    let d = daemon(&[]);
    let client = d.client().await;
    let claimed = claim_on(&client, "a.md", "w1").await;
    assert_eq!(claimed.text.as_deref(), Some("a\n"));
    let saved = client
        .call::<DocumentsSave>(&SaveDocument {
            lease_id: claimed.lease_id.clone().expect("lease"),
            seq: 1,
            base_rev: claimed.rev.clone(),
            expect: Expect::Present,
            text: "saved over rpc\n".into(),
        })
        .await
        .expect("save over RPC");
    let read = client
        .call::<DocumentsRead>(&ReadDocument {
            key: claimed.key.clone(),
            policy: None,
        })
        .await
        .expect("read over RPC");
    assert_eq!(read.rev, Some(saved.rev));
    assert_eq!(read.text.as_deref(), Some("saved over rpc\n"));
    let disk = std::fs::read_to_string(d.home.path().join("notes/a.md")).expect("read vault");
    assert_eq!(disk, "saved over rpc\n");
}

#[tokio::test]
async fn channel_close_releases_its_leases() {
    let d = daemon(&[]);
    let (mut a, mut b) = (wire(&d).await, wire(&d).await);
    let first = call(&mut a, 1, "documents.claim", claim("a.md", "wA"))
        .await
        .expect("A claims");
    assert!(first["leaseId"].is_string());
    let second = call(&mut b, 1, "documents.claim", claim("b.md", "wB"))
        .await
        .expect("B claims");
    assert!(second["leaseId"].is_string());
    a.send(control_frame(1, &Control::ChannelClose))
        .await
        .expect("close A channel 1");
    let freed = leased_within_a_second(&d, "a.md").await;
    assert!(freed.lease_id.is_some() && freed.read_only.is_none());
    assert!(
        elsewhere(&client_claim(&d, "b.md", "wC").await),
        "B's channel 1 on another connection keeps its lease"
    );
}

#[tokio::test]
async fn connection_drop_releases_its_leases() {
    let d = daemon(&[]);
    let mut a = wire(&d).await;
    call(&mut a, 1, "documents.claim", claim("a.md", "wA"))
        .await
        .expect("A claims");
    assert!(elsewhere(&client_claim(&d, "a.md", "wC").await));
    drop(a);
    let freed = leased_within_a_second(&d, "a.md").await;
    assert!(freed.lease_id.is_some(), "no 60 s fallback wait");
}

#[tokio::test]
async fn inflight_claim_on_a_closing_channel_leaves_no_lease() {
    let gate = tempfile::tempdir().expect("owned gate directory");
    let d = daemon(&[(
        "DEVBOX_TEST_DOCUMENTS_GATE",
        gate.path().display().to_string(),
    )]);
    let mut a = wire(&d).await;
    send(&mut a, 1, "documents.claim", claim("a.md", "wA"), None).await;
    let deadline = Instant::now() + Duration::from_secs(5);
    while !gate.path().join("reached").exists() {
        assert!(Instant::now() < deadline, "claim reaches the gate");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    a.send(control_frame(1, &Control::ChannelClose))
        .await
        .expect("close A channel 1");
    tokio::time::sleep(Duration::from_millis(100)).await;
    std::fs::write(gate.path().join("release"), b"").expect("open the gate");
    match tokio::time::timeout(Duration::from_millis(300), a.next()).await {
        Err(_) => {}
        Ok(Some(Ok(frame))) => match parse_json::<ServerMessage>(&frame).expect("message") {
            ServerMessage::Response { error, .. } => {
                assert_eq!(error.map(|e| e.code).as_deref(), Some("cancelled"))
            }
            other => panic!("unexpected {other:?}"),
        },
        Ok(other) => panic!("unexpected {other:?}"),
    }
    let next = leased_within_a_second(&d, "a.md").await;
    assert!(next.lease_id.is_some(), "the cancelled claim left no lease");
}

#[tokio::test]
async fn cancel_after_claim_does_not_release_the_lease() {
    let d = daemon(&[]);
    let mut a = wire(&d).await;
    let claimed = call(&mut a, 1, "documents.claim", claim("a.md", "wA"))
        .await
        .expect("claim");
    a.send(json_frame(1, &ClientMessage::Cancel { id: 1 }))
        .await
        .expect("cancel");
    tokio::time::sleep(Duration::from_millis(50)).await;
    let save = serde_json::json!({
        "leaseId": claimed["leaseId"], "seq": 1, "baseRev": claimed["rev"],
        "expect": "present", "text": "kept\n",
    });
    call(&mut a, 2, "documents.save", save)
        .await
        .expect("lease survives Cancel");
    assert!(elsewhere(&client_claim(&d, "a.md", "wC").await));
}

async fn req(
    wire: &mut Wire,
    ids: &mut u32,
    method: &str,
    params: serde_json::Value,
    crid: &str,
) -> Reply {
    *ids += 1;
    send(wire, *ids, method, params, Some(crid)).await;
    reply(wire, *ids).await
}

#[tokio::test]
async fn claim_bypasses_dedupe_but_other_mutations_do_not() {
    let d = daemon(&[]);
    let mut a = wire(&d).await;
    let mut ids = 0;
    let one = req(
        &mut a,
        &mut ids,
        "documents.claim",
        claim("a.md", "w1"),
        "same",
    )
    .await
    .expect("claim");
    let two = req(
        &mut a,
        &mut ids,
        "documents.claim",
        claim("a.md", "w1"),
        "same",
    )
    .await
    .expect("again");
    assert_eq!(
        one["leaseId"], two["leaseId"],
        "windowId makes claim idempotent"
    );
    let other = req(
        &mut a,
        &mut ids,
        "documents.claim",
        claim("b.md", "w2"),
        "same",
    )
    .await
    .expect("different params under the same clientRequestId still claim");
    assert!(other["leaseId"].is_string());
    let save = |text: &str| {
        serde_json::json!({
            "leaseId": one["leaseId"], "seq": 1, "baseRev": one["rev"],
            "expect": "present", "text": text,
        })
    };
    let saved = req(&mut a, &mut ids, "documents.save", save("first\n"), "save").await;
    assert!(saved.is_ok(), "{saved:?}");
    let reused = req(&mut a, &mut ids, "documents.save", save("second\n"), "save").await;
    assert_eq!(reused.expect_err("dedupe").0, "invalid_params");
}

#[tokio::test]
async fn daemon_files_stay_out_of_scope_even_under_a_home_project() {
    let d = daemon(&[]);
    let client = d.client().await;
    let home = client
        .call::<devbox_projects::ProjectsAdd>(&devbox_projects::AddProject {
            path: d.home.path().display().to_string(),
        })
        .await
        .expect("register the fixture home as a project");
    let rel = |p: &std::path::Path| {
        p.strip_prefix(d.home.path())
            .expect("daemon folders live under the fixture home")
            .display()
            .to_string()
    };
    let (data, state) = (rel(&d.paths.data_dir), rel(&d.paths.state_dir));
    let units = d.paths.xdg_config_home.join("systemd/user");
    let channel = d.paths.instance.channel().config();
    let (daemon_unit, tmux_unit) = (channel.daemon_unit, channel.tmux_unit);
    let instance = d.instance.clone();
    let service_dropin = units.join(format!("{daemon_unit}@{instance}.service.d"));
    for dir in [&d.paths.bin_dir, &service_dropin] {
        std::fs::create_dir_all(dir).expect("create fixture folder");
    }
    for file in [
        d.paths.bin_dir.join("devbox"),
        units.join(format!("{daemon_unit}@.service")),
        units.join(format!("{tmux_unit}@.service")),
        service_dropin.join("paths.conf"),
    ] {
        std::fs::write(file, "owned fixture\n").expect("write fixture file");
    }
    let wants = units.join("default.target.wants");
    std::fs::create_dir_all(&wants).expect("create fixture folder");
    std::os::unix::fs::symlink(
        units.join(format!("{daemon_unit}@.service")),
        wants.join(format!("{daemon_unit}@{instance}.service")),
    )
    .expect("link fixture unit");
    let (bin, units) = (rel(&d.paths.bin_dir), rel(&units));
    let project = |path: String| DocKey {
        root: format!("project:{}", home.id),
        path,
    };
    let keys = [
        DocKey {
            root: "data".into(),
            path: "devbox.db".into(),
        },
        project(format!("{data}/devbox.db")),
        project(format!("{data}/.daemon.lock")),
        project(format!("{data}/keys/local.key")),
        project(format!("{state}/logs/daemon.log")),
        project(format!("{bin}/devbox")),
        project(format!("{units}/{daemon_unit}@.service")),
        project(format!("{units}/{daemon_unit}@.socket")),
        project(format!("{units}/{tmux_unit}@.service")),
        project(format!(
            "{units}/{daemon_unit}@{instance}.service.d/paths.conf"
        )),
        project(format!(
            "{units}/{daemon_unit}@{instance}.socket.d/paths.conf"
        )),
        project(format!(
            "{units}/{tmux_unit}@{instance}.service.d/paths.conf"
        )),
    ];
    for key in keys {
        let params = ClaimDocument {
            key: key.clone(),
            policy: DocPolicy {
                kind: DocKind::Code,
            },
            window_id: "w1".into(),
        };
        match client.call::<DocumentsClaim>(&params).await {
            Err(devbox_cli::client::ClientError::Rpc { code, .. }) => {
                assert_eq!(code, "documents.out_of_scope", "{key:?}")
            }
            other => panic!("{key:?} must be refused, got {other:?}"),
        }
    }
    let moved = client
        .call::<DocumentsRename>(&RenameDocument {
            lease_id: None,
            key: project(format!("{data}/devbox.db")),
            to: project("moved.db".into()),
        })
        .await;
    assert!(
        matches!(&moved, Err(devbox_cli::client::ClientError::Rpc { code, .. }) if code == "documents.out_of_scope"),
        "{moved:?}"
    );
    let link = format!("{units}/default.target.wants/{daemon_unit}@{instance}.service");
    let moved = client
        .call::<DocumentsRename>(&RenameDocument {
            lease_id: None,
            key: project(link),
            to: project("moved.service".into()),
        })
        .await;
    assert!(
        matches!(&moved, Err(devbox_cli::client::ClientError::Rpc { code, .. }) if code == "documents.out_of_scope"),
        "the enable link itself is refused: {moved:?}"
    );
    assert!(d.paths.db_path.exists(), "the database stays in place");
}
