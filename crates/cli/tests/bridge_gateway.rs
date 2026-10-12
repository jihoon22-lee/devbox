mod support;
use futures::{SinkExt, StreamExt};
use std::process::Stdio;
use support::{TestDaemon, bounded};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

fn command(d: &TestDaemon) -> tokio::process::Command {
    let mut c = tokio::process::Command::from(d.environment(env!("CARGO_BIN_EXE_devbox")));
    c.kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    c
}
fn hello(d: &TestDaemon) -> Vec<u8> {
    let body = serde_json::to_vec(&serde_json::json!({"type":"hello","version":devbox_cli::VERSION,"protocol":devbox_protocol::PROTOCOL_VERSION,"client":"test","instance":d.instance})).expect("serialize fixture message");
    let mut f = ((5 + body.len()) as u32).to_le_bytes().to_vec();
    f.push(0);
    f.extend(0u32.to_le_bytes());
    f.extend(body);
    f
}
#[tokio::test]
async fn bridge_magic_prompt_relay_and_stdin_eof() {
    let d = TestDaemon::start();
    let mut child = command(&d)
        .arg("bridge")
        .spawn()
        .expect("spawn fixture process");
    let mut out = child.stdout.take().expect("fixture stdout is piped");
    let mut magic = [0; 16];
    bounded(out.read_exact(&mut magic))
        .await
        .expect("read fixture bytes");
    assert_eq!(&magic, b"DEVBOX-BRIDGE/1\n");
    let mut input = child.stdin.take().expect("fixture stdin is piped");
    input
        .write_all(&hello(&d))
        .await
        .expect("write fixture bytes");
    let mut len = [0; 4];
    bounded(out.read_exact(&mut len))
        .await
        .expect("read fixture bytes");
    let mut frame = vec![0; u32::from_le_bytes(len) as usize];
    bounded(out.read_exact(&mut frame))
        .await
        .expect("read fixture bytes");
    assert!(String::from_utf8_lossy(&frame[5..]).contains("welcome"));
    drop(input);
    assert!(
        bounded(child.wait())
            .await
            .expect("wait for fixture process")
            .success()
    );
}
async fn gateway(d: &TestDaemon) -> (tokio::process::Child, String) {
    gateway_token(d, "fixture").await
}
async fn gateway_token(d: &TestDaemon, token: &str) -> (tokio::process::Child, String) {
    let mut child = command(d)
        .args([
            "dev-gateway",
            "--port",
            "0",
            "--origin",
            "http://localhost:1450",
            "--token",
            token,
        ])
        .spawn()
        .expect("spawn fixture process");
    let mut line = String::new();
    bounded(
        BufReader::new(child.stdout.take().expect("fixture stdout is piped")).read_line(&mut line),
    )
    .await
    .expect("read fixture line");
    let url = line
        .trim()
        .strip_prefix("dev-gateway ")
        .expect("readiness")
        .to_owned();
    assert!(!url.contains(":0/"));
    (child, url)
}
fn request(url: &str, origin: &str) -> tokio_tungstenite::tungstenite::http::Request<()> {
    let mut r = url.into_client_request().expect("create WebSocket request");
    r.headers_mut()
        .insert("Origin", origin.parse().expect("parse fixture value"));
    r
}
#[tokio::test]
async fn gateway_gates_and_relays() {
    let d = TestDaemon::start();
    let (_child, url) = gateway(&d).await;
    for (u, o, status) in [
        (
            url.replace("fixture", "wrong"),
            "http://localhost:1450",
            401,
        ),
        (url.clone(), "http://evil.example", 403),
    ] {
        let e = bounded(tokio_tungstenite::connect_async(request(&u, o)))
            .await
            .expect_err("fixture operation must fail");
        match e {
            tokio_tungstenite::tungstenite::Error::Http(r) => {
                assert_eq!(r.status().as_u16(), status)
            }
            _ => panic!("expected HTTP rejection"),
        }
    }
    let (mut ws, _) = bounded(tokio_tungstenite::connect_async(request(
        &url,
        "http://localhost:1450",
    )))
    .await
    .expect("connect fixture WebSocket");
    ws.send(Message::Ping(vec![1].into()))
        .await
        .expect("send fixture message");
    assert!(matches!(
        bounded(ws.next()).await,
        Some(Ok(Message::Pong(_)))
    ));
    ws.send(Message::Binary(hello(&d).into()))
        .await
        .expect("send fixture message");
    let Some(Ok(Message::Binary(f))) = bounded(ws.next()).await else {
        panic!("binary welcome")
    };
    assert!(String::from_utf8_lossy(&f[9..]).contains("welcome"));
    ws.close(None).await.expect("close fixture WebSocket");
}
#[tokio::test]
async fn gateway_rejects_each_malformed_message() {
    let d = TestDaemon::start();
    let (_child, url) = gateway(&d).await;
    let f = hello(&d);
    let mut multiple = f.clone();
    multiple.extend(&f);
    let mut trailing = f.clone();
    trailing.push(0);
    let mut invalid = f.clone();
    invalid[4] = 255;
    let oversized = vec![255, 255, 255, 127, 1];
    for message in [
        Message::Binary(f[..6].to_vec().into()),
        Message::Binary(multiple.into()),
        Message::Binary(trailing.into()),
        Message::Binary(invalid.into()),
        Message::Binary(oversized.into()),
        Message::Text("unexpected".into()),
    ] {
        let (mut ws, _) = bounded(tokio_tungstenite::connect_async(request(
            &url,
            "http://localhost:1450",
        )))
        .await
        .expect("connect fixture WebSocket");
        ws.send(message).await.expect("send fixture message");
        assert!(!matches!(
            bounded(ws.next()).await,
            Some(Ok(Message::Binary(_)))
        ));
    }
}

fn fake_systemctl(d: &TestDaemon, script: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let bin = d.home.path().join("bin");
    std::fs::create_dir(&bin).expect("create fixture directory");
    let file = bin.join("systemctl");
    std::fs::write(&file, format!("#!/bin/sh\n{script}\n")).expect("write fixture file");
    std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o700))
        .expect("set fixture permissions");
    bin
}
#[tokio::test]
async fn bridge_binary_streaming_and_daemon_eof_with_stdin_open() {
    let d = TestDaemon::fixture();
    let listener = tokio::net::UnixListener::bind(&d.socket).expect("bind fixture socket");
    let mut child = command(&d)
        .arg("bridge")
        .spawn()
        .expect("spawn fixture process");
    let (mut peer, _) = bounded(listener.accept())
        .await
        .expect("accept fixture peer");
    let mut output = child.stdout.take().expect("fixture stdout is piped");
    let mut input = child.stdin.take().expect("fixture stdin is piped");
    let mut magic = [0; 16];
    bounded(output.read_exact(&mut magic))
        .await
        .expect("read fixture bytes");
    assert_eq!(&magic, b"DEVBOX-BRIDGE/1\n");
    let payload: Vec<u8> = (0..8 * 1024 * 1024).map(|n| (n % 251) as u8).collect();
    let echo = async {
        let (mut r, mut w) = peer.split();
        tokio::io::copy(&mut r, &mut w)
            .await
            .expect("echo fixture stream");
    };
    let transfer = async {
        let send = async {
            input
                .write_all(&payload)
                .await
                .expect("write fixture bytes");
        };
        let receive = async {
            let mut received = vec![0; payload.len()];
            output
                .read_exact(&mut received)
                .await
                .expect("read fixture bytes");
            assert_eq!(received, payload);
        };
        bounded(async {
            tokio::join!(send, receive);
        })
        .await;
    };
    tokio::select! { _=echo=>panic!("early daemon EOF"), _=transfer=>{} }
    drop(peer);
    assert!(
        bounded(child.wait())
            .await
            .expect("wait for fixture process")
            .success()
    );
    drop(input);
}
#[tokio::test]
async fn bridge_activation_once_stdout_clean_and_retry() {
    let d = TestDaemon::fixture();
    let bin = fake_systemctl(
        &d,
        "echo pollution\nprintf '%s\\n' \"$@\" >> \"$HOME/activation\"",
    );
    let mut child = command(&d)
        .env("PATH", &bin)
        .arg("bridge")
        .spawn()
        .expect("spawn fixture process");
    let mut output = child.stdout.take().expect("fixture stdout is piped");
    let mut magic = [0; 16];
    bounded(output.read_exact(&mut magic))
        .await
        .expect("read fixture bytes");
    bounded(async {
        while !d.home.path().join("activation").exists() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await;
    let listener = tokio::net::UnixListener::bind(&d.socket).expect("bind fixture socket");
    let (mut peer, _) = bounded(listener.accept())
        .await
        .expect("accept fixture peer");
    peer.write_all(b"owned fixture")
        .await
        .expect("write fixture bytes");
    let mut bytes = [0; 13];
    bounded(output.read_exact(&mut bytes))
        .await
        .expect("read fixture bytes");
    assert_eq!(&bytes, b"owned fixture");
    drop(peer);
    assert!(
        bounded(child.wait())
            .await
            .expect("wait for fixture process")
            .success()
    );
    assert_eq!(
        std::fs::read_to_string(d.home.path().join("activation")).expect("read fixture text"),
        format!("--user\nstart\ndevbox@{}.socket\n", d.instance)
    );
    let mut rest = Vec::new();
    output
        .read_to_end(&mut rest)
        .await
        .expect("read remaining fixture output");
    assert!(rest.is_empty());
    assert_eq!(&magic, b"DEVBOX-BRIDGE/1\n");
}
#[tokio::test]
async fn bridge_activation_command_has_total_deadline() {
    let d = TestDaemon::fixture();
    let bin = fake_systemctl(&d, "echo pollution\nexec /bin/sleep 30");
    let start = std::time::Instant::now();
    let mut child = command(&d)
        .env("PATH", bin)
        .arg("bridge")
        .spawn()
        .expect("spawn fixture process");
    let status = tokio::time::timeout(std::time::Duration::from_secs(7), child.wait())
        .await
        .expect("wait for fixture process")
        .expect("wait for fixture process");
    assert!(!status.success());
    assert!(start.elapsed() < std::time::Duration::from_secs(7));
    let mut out = Vec::new();
    child
        .stdout
        .take()
        .expect("read piped fixture output")
        .read_to_end(&mut out)
        .await
        .expect("read piped fixture output");
    assert_eq!(out, b"DEVBOX-BRIDGE/1\n");
}
#[tokio::test]
async fn bridge_invalid_socket_does_not_activate() {
    let d = TestDaemon::fixture();
    std::fs::write(&d.socket, b"not a socket").expect("write fixture file");
    let bin = fake_systemctl(&d, "touch \"$HOME/activation\"");
    let mut child = command(&d)
        .env("PATH", bin)
        .arg("bridge")
        .spawn()
        .expect("spawn fixture process");
    assert!(
        !bounded(child.wait())
            .await
            .expect("wait for fixture process")
            .success()
    );
    assert!(!d.home.path().join("activation").exists());
}
#[tokio::test]
async fn gateway_rejects_empty_configuration() {
    let d = TestDaemon::fixture();
    for (origin, token) in [("", "fixture"), ("http://localhost:1450", "")] {
        let mut child = command(&d)
            .args([
                "dev-gateway",
                "--port",
                "0",
                "--origin",
                origin,
                "--token",
                token,
            ])
            .spawn()
            .expect("spawn fixture process");
        assert!(
            !bounded(child.wait())
                .await
                .expect("wait for fixture process")
                .success()
        );
        let mut out = Vec::new();
        child
            .stdout
            .take()
            .expect("read piped fixture output")
            .read_to_end(&mut out)
            .await
            .expect("read piped fixture output");
        assert!(out.is_empty());
    }
}

#[tokio::test]
async fn bridge_null_and_file_stdin_exit_cleanly() {
    let d = TestDaemon::fixture();
    let listener = tokio::net::UnixListener::bind(&d.socket).expect("bind fixture socket");
    let file = d.home.path().join("input");
    std::fs::write(&file, b"file input").expect("write fixture file");
    for stdin in [
        Stdio::null(),
        Stdio::from(std::fs::File::open(&file).expect("open fixture input file")),
    ] {
        let mut child = command(&d)
            .stdin(stdin)
            .arg("bridge")
            .spawn()
            .expect("spawn fixture process");
        let (_peer, _) = bounded(listener.accept())
            .await
            .expect("accept fixture peer");
        assert!(
            bounded(child.wait())
                .await
                .expect("wait for fixture process")
                .success()
        );
    }
}

#[tokio::test]
async fn gateway_downstream_splits_frames_and_rejects_partial_eof() {
    let d = TestDaemon::fixture();
    let listener = tokio::net::UnixListener::bind(&d.socket).expect("bind fixture socket");
    let (_child, url) = gateway(&d).await;
    let (mut ws, _) = bounded(tokio_tungstenite::connect_async(request(
        &url,
        "http://localhost:1450",
    )))
    .await
    .expect("connect fixture WebSocket");
    let (mut peer, _) = bounded(listener.accept())
        .await
        .expect("accept fixture peer");
    let frame = hello(&d);
    let mut two = frame.clone();
    two.extend(&frame);
    peer.write_all(&two[..3])
        .await
        .expect("write fixture bytes");
    peer.write_all(&two[3..])
        .await
        .expect("write fixture bytes");
    for _ in 0..2 {
        let Some(Ok(Message::Binary(actual))) = bounded(ws.next()).await else {
            panic!("individual frame")
        };
        assert_eq!(actual.as_ref(), frame);
    }
    peer.write_all(&frame[..6])
        .await
        .expect("write fixture bytes");
    drop(peer);
    assert!(!matches!(
        bounded(ws.next()).await,
        Some(Ok(Message::Binary(_)))
    ));
}

#[tokio::test]
async fn gateway_large_duplex_does_not_block_reverse_reads() {
    let d = TestDaemon::fixture();
    let listener = tokio::net::UnixListener::bind(&d.socket).expect("bind fixture socket");
    let (_child, url) = gateway(&d).await;
    let (ws, _) = bounded(tokio_tungstenite::connect_async(request(
        &url,
        "http://localhost:1450",
    )))
    .await
    .expect("connect fixture WebSocket");
    let (peer, _) = bounded(listener.accept())
        .await
        .expect("accept fixture peer");
    let mut daemon =
        tokio_util::codec::Framed::new(peer, devbox_protocol::frame::FrameCodec::default());
    let (mut send, mut receive) = ws.split();
    let body = vec![b'x'; 512 * 1024];
    let mut frame = ((5 + body.len()) as u32).to_le_bytes().to_vec();
    frame.push(0);
    frame.extend(0u32.to_le_bytes());
    frame.extend(body);
    let echo = async {
        for _ in 0..32 {
            let f = daemon
                .next()
                .await
                .expect("receive fixture frame")
                .expect("receive fixture frame");
            daemon.send(f).await.expect("send fixture message");
        }
    };
    let transmit = async {
        for _ in 0..32 {
            send.send(Message::Binary(frame.clone().into()))
                .await
                .expect("send fixture message");
        }
    };
    let collect = async {
        for _ in 0..32 {
            let Some(Ok(Message::Binary(actual))) = receive.next().await else {
                panic!("echo frame")
            };
            assert_eq!(actual.as_ref(), frame);
        }
    };
    bounded(async {
        tokio::join!(echo, transmit, collect);
    })
    .await;
}

#[tokio::test]
async fn bridge_stdin_eof_exits_while_stdout_is_backpressured() {
    let d = TestDaemon::fixture();
    let listener = tokio::net::UnixListener::bind(&d.socket).expect("bind fixture socket");
    let mut child = command(&d)
        .arg("bridge")
        .spawn()
        .expect("spawn fixture process");
    let (mut peer, _) = bounded(listener.accept())
        .await
        .expect("accept fixture peer");
    // Leave stdout unread. Fill it and the socket, then signal stdin EOF.
    let fill = async { peer.write_all(&vec![0; 8 * 1024 * 1024]).await };
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(200), fill)
            .await
            .is_err()
    );
    drop(child.stdin.take());
    assert!(
        bounded(child.wait())
            .await
            .expect("wait for fixture process")
            .success()
    );
}

#[tokio::test]
async fn gateway_readiness_url_encodes_explicit_token() {
    let d = TestDaemon::start();
    let (_child, url) = gateway_token(&d, "+&#%= fixture").await;
    let (mut ws, _) = bounded(tokio_tungstenite::connect_async(request(
        &url,
        "http://localhost:1450",
    )))
    .await
    .expect("connect fixture WebSocket");
    ws.send(Message::Binary(hello(&d).into()))
        .await
        .expect("send fixture message");
    assert!(matches!(
        bounded(ws.next()).await,
        Some(Ok(Message::Binary(_)))
    ));
}

#[tokio::test]
#[allow(unsafe_code)]
async fn bridge_does_not_change_inherited_pipe_flags_even_after_kill() {
    use std::os::fd::{AsRawFd, FromRawFd};
    let d = TestDaemon::fixture();
    let listener = tokio::net::UnixListener::bind(&d.socket).expect("bind fixture socket");
    let mut fds = [0; 2];
    // SAFETY: pipe writes two owned descriptors on success.
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
    // SAFETY: each fresh descriptor is transferred exactly once to File.
    let mut reader = unsafe { std::fs::File::from_raw_fd(fds[0]) };
    let writer = unsafe { std::fs::File::from_raw_fd(fds[1]) };
    // SAFETY: writer owns a live descriptor throughout this test.
    let flags = unsafe { libc::fcntl(writer.as_raw_fd(), libc::F_GETFL) };
    assert!(flags >= 0);
    let mut child = command(&d)
        .stdout(Stdio::from(
            writer.try_clone().expect("clone fixture handle"),
        ))
        .arg("bridge")
        .spawn()
        .expect("spawn fixture process");
    let (_peer, _) = bounded(listener.accept())
        .await
        .expect("accept fixture peer");
    let mut magic = [0; 16];
    std::io::Read::read_exact(&mut reader, &mut magic).expect("read fixture magic");
    // SAFETY: same live parent-owned descriptor, while bridge is running.
    assert_eq!(
        unsafe { libc::fcntl(writer.as_raw_fd(), libc::F_GETFL) },
        flags
    );
    child.kill().await.expect("stop fixture process");
    bounded(child.wait())
        .await
        .expect("wait for fixture process");
    // SAFETY: descriptor stays live even after abrupt child termination.
    assert_eq!(
        unsafe { libc::fcntl(writer.as_raw_fd(), libc::F_GETFL) },
        flags
    );
}
