//! MCP stdio(S5-5): 소유 python3 fixture 서버로 initialize·목록·호출, 환경 허용 목록·비밀 가림,
//! 실행 전 검사(표지 파일이 생기지 않음), 스트림 닫기 → 프로세스 그룹 종료, 저장소 파일 출처의 검토·명령 일치.
mod support;
use devbox_api::*;
use serde_json::{Value, json};
use std::time::Duration;
use support::api::*;
use support::lines::{Lines, open};
use support::mcp::*;

const T: Duration = Duration::from_secs(10);

fn pair(name: &str, value: &str) -> Pair {
    Pair {
        name: name.into(),
        value: value.into(),
    }
}

fn stdio(script: &str, cwd: Option<&str>, env: Vec<Pair>) -> McpTransport {
    McpTransport::Stdio {
        command: "python3".into(),
        args: vec!["-I".into(), script.into()],
        cwd: cwd.map(Into::into),
        env,
    }
}

fn draft(transport: McpTransport) -> McpConnect {
    McpConnect {
        transport,
        env: EnvSnapshot::default(),
        source: McpSource::Draft,
    }
}

async fn connect(h: &Harness, p: McpConnect) -> Result<Lines, Rpc> {
    open(|ctx| h.api.mcp_connect(ctx, p, &h.collections)).await
}

async fn ready(lines: &mut Lines) -> Value {
    let (ready, before) = lines.until("ready", T).await;
    ready.unwrap_or_else(|| panic!("ready, got {before:?}"))
}

/// 입력을 보내고 그 id의 결과·오류 이벤트를 기다린다.
async fn ask(lines: &mut Lines, input: McpInput) -> Value {
    let id = match &input {
        McpInput::List { id, .. }
        | McpInput::Call { id, .. }
        | McpInput::Read { id, .. }
        | McpInput::GetPrompt { id, .. } => id.clone(),
    };
    lines
        .send(serde_json::to_value(&input).expect("input"))
        .await;
    loop {
        let event = lines
            .next(T)
            .await
            .unwrap_or_else(|| panic!("answer for {id}"));
        if event["id"] == id.as_str() {
            return event;
        }
    }
}

fn call(id: &str, name: &str, args: &str) -> McpInput {
    McpInput::Call {
        id: id.into(),
        name: name.into(),
        arguments_json: args.into(),
    }
}

#[tokio::test]
async fn mcp_stdio_initializes_lists_and_calls_tools() {
    let h = Harness::new().await;
    let script = script(h.dir.path());
    let cwd = workdir(&h).await;
    let mut p = draft(stdio(&script, Some(&cwd), vec![]));
    p.env = EnvSnapshot {
        variables: vec![EnvVar {
            name: "x".into(),
            value: "VALUE".into(),
            sensitive: false,
            origin: VarOrigin::Environment,
        }],
    };
    let mut lines = connect(&h, p).await.expect("connect");
    let ready = ready(&mut lines).await;
    assert_eq!(ready["serverName"], "fixture");
    assert_eq!(ready["protocolVersion"], "2025-06-18");
    let first = ask(
        &mut lines,
        McpInput::List {
            id: "l1".into(),
            what: McpList::Tools,
            cursor: None,
        },
    )
    .await;
    let page: Value = serde_json::from_str(first["json"].as_str().expect("json")).expect("page");
    assert_eq!(page["tools"][0]["name"], "echo");
    assert_eq!(page["nextCursor"], "p2");
    let second = ask(
        &mut lines,
        McpInput::List {
            id: "l2".into(),
            what: McpList::Tools,
            cursor: Some("p2".into()),
        },
    )
    .await;
    assert!(second["json"].as_str().expect("json").contains("\"env\""));
    // 인자는 치환하지 않고 그대로 보낸다.
    let echo = ask(&mut lines, call("c1", "echo", r#"{"text":"{{x}}","n":1}"#)).await;
    assert_eq!(echo["type"], "result", "{echo}");
    let text = echo["json"].as_str().expect("json");
    assert!(text.contains("{{x}}") && !text.contains("VALUE"), "{text}");
    let unknown = ask(&mut lines, call("c2", "nope", "{}")).await;
    assert_eq!(
        (unknown["type"].as_str(), unknown["code"].as_i64()),
        (Some("error"), Some(-32602))
    );
    let bad = ask(&mut lines, call("c3", "echo", "[1]")).await;
    assert_eq!(
        (bad["type"].as_str(), bad["message"].as_str()),
        (Some("error"), Some("arguments_json"))
    );
    // 서버가 먼저 보낸 ping에 답한다.
    let ping = ask(&mut lines, call("c4", "ping-me", "{}")).await;
    assert!(
        ping["json"]
            .as_str()
            .expect("json")
            .contains("\"pong\":true"),
        "{ping}"
    );
    lines.close();
    lines.all(T).await;
}

#[tokio::test]
async fn mcp_stdio_environment_is_an_allowlist_and_secrets_are_masked() {
    let h = Harness::new().await;
    h.secret("T", "fixture-mcp-secret").await;
    let script = script(h.dir.path());
    let env = vec![pair("TOKEN", "{{secret:T}}"), pair("PLAIN", "p")];
    let cwd = workdir(&h).await;
    let mut lines = connect(&h, draft(stdio(&script, Some(&cwd), env)))
        .await
        .expect("connect");
    ready(&mut lines).await;
    let result = ask(&mut lines, call("e", "env", "{}")).await;
    let text = result["json"].as_str().expect("json").to_owned();
    assert!(!text.contains("fixture-mcp-secret"), "{text}");
    let value: Value = serde_json::from_str(&text).expect("result");
    let env = value["env"].as_object().expect("env");
    assert_eq!(
        env["TOKEN"], REDACTED,
        "the child got the secret, the screen sees it masked"
    );
    assert_eq!(env["PLAIN"], "p");
    let allowed = [
        "PATH",
        "HOME",
        "TMPDIR",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
        "XDG_RUNTIME_DIR",
        "TOKEN",
        "PLAIN",
    ];
    for key in env.keys() {
        // python이 스스로 넣는 LC_CTYPE 외에는 허용 목록만(테스트 프로세스의 CARGO_* 등은 넘기지 않음).
        assert!(
            allowed.contains(&key.as_str()),
            "unexpected child variable {key}"
        );
    }
    assert!(
        std::env::vars().any(|(k, _)| k.starts_with("CARGO")),
        "canary: cargo env exists in the parent"
    );
}

#[tokio::test]
async fn mcp_stdio_inputs_are_checked_before_spawning() {
    let h = Harness::new().await;
    let script = script(h.dir.path());
    let project = h.dir.path().join("proj");
    std::fs::create_dir_all(project.join("sub")).expect("project dir");
    h.projects
        .add(devbox_projects::AddProject {
            path: project.display().to_string(),
        })
        .await
        .expect("register project");
    let outside = tempfile::tempdir().expect("unregistered dir");
    let marker = |n: &str| h.dir.path().join(format!("marker-{n}"));
    let m = |n: &str| pair("FIXTURE_MARKER", &marker(n).display().to_string());
    let cases: Vec<(&str, McpTransport, (&str, &str, &str))> = vec![
        (
            "reserved",
            stdio(&script, None, vec![m("a"), pair("PATH", "/evil")]),
            ("api.invalid_input", "env", "reserved"),
        ),
        (
            "name",
            stdio(&script, None, vec![m("b"), pair("A-B", "x")]),
            ("api.invalid_input", "env", "invalid_name"),
        ),
        (
            "relative cwd",
            stdio(&script, Some("proj"), vec![m("c")]),
            ("api.invalid_input", "cwd", "relative"),
        ),
        (
            "outside cwd",
            stdio(
                &script,
                Some(&outside.path().display().to_string()),
                vec![m("d")],
            ),
            ("api.invalid_input", "cwd", "outside_projects"),
        ),
        (
            "nul command",
            McpTransport::Stdio {
                command: "pyth\0on3".into(),
                args: vec![],
                cwd: None,
                env: vec![m("e")],
            },
            ("api.invalid_input", "command", "invalid"),
        ),
    ];
    for (case, transport, (code, field, reason)) in cases {
        let err = connect(&h, draft(transport))
            .await
            .err()
            .unwrap_or_else(|| panic!("{case}"));
        assert_eq!(
            (
                err.code(),
                err.detail()["field"].as_str(),
                err.detail()["reason"].as_str()
            ),
            (code, Some(field), Some(reason)),
            "{case}"
        );
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    for n in ["a", "b", "c", "d", "e"] {
        assert!(!marker(n).exists(), "case {n} must not run anything");
    }
    // 등록 프로젝트 안 cwd는 실제 경로로 쓴다.
    let sub = project.join("sub").display().to_string();
    let mut lines = connect(&h, draft(stdio(&script, Some(&sub), vec![m("ok")])))
        .await
        .expect("connect");
    ready(&mut lines).await;
    let result = ask(&mut lines, call("e", "env", "{}")).await;
    let value: Value =
        serde_json::from_str(result["json"].as_str().expect("json")).expect("result");
    assert_eq!(
        value["cwd"],
        std::fs::canonicalize(&sub)
            .expect("real")
            .display()
            .to_string()
    );
    assert!(marker("ok").exists());
}

#[tokio::test]
async fn mcp_stdio_closing_the_stream_ends_the_process_group() {
    let h = Harness::new().await;
    let script = script(h.dir.path());
    let (server_pid, grand_pid) = (
        h.dir.path().join("server.pid"),
        h.dir.path().join("grand.pid"),
    );
    let env = vec![
        pair("FIXTURE_MARKER", &server_pid.display().to_string()),
        pair("FIXTURE_GRANDCHILD", &grand_pid.display().to_string()),
    ];
    let cwd = workdir(&h).await;
    let mut lines = connect(&h, draft(stdio(&script, Some(&cwd), env)))
        .await
        .expect("connect");
    ready(&mut lines).await;
    let read = |p: &std::path::Path| -> u32 {
        std::fs::read_to_string(p)
            .expect("pid file")
            .trim()
            .parse()
            .expect("pid")
    };
    let (server, grand) = (read(&server_pid), read(&grand_pid));
    assert!(!gone(server) && !gone(grand), "both running");
    lines.close();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while !(gone(server) && gone(grand)) && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(gone(server), "server process ended");
    assert!(gone(grand), "grandchild in the same process group ended");
}

#[tokio::test]
async fn mcp_spawn_failure_is_reported_on_the_stream() {
    let h = Harness::new().await;
    let cwd = workdir(&h).await;
    let p = draft(McpTransport::Stdio {
        command: "/nonexistent/devbox-mcp-fixture".into(),
        args: vec![],
        cwd: Some(cwd),
        env: vec![],
    });
    let mut lines = connect(&h, p)
        .await
        .expect("stream opens; spawn happens after");
    let events = lines.all(T).await;
    let failed = events
        .iter()
        .find(|e| e["type"] == "failed")
        .expect("failed");
    assert_eq!(failed["error"]["code"], "api.mcp");
    assert!(
        !serde_json::to_string(&events)
            .expect("events")
            .contains("nonexistent")
    );
}

fn profile(script: &str, marker: &std::path::Path, extra: &[&str]) -> String {
    profile_in(script, marker, extra, None)
}

fn profile_in(script: &str, marker: &std::path::Path, extra: &[&str], cwd: Option<&str>) -> String {
    let mut args = vec!["-I".to_owned(), script.to_owned()];
    args.extend(extra.iter().map(|a| (*a).to_owned()));
    json!({
        "version": 1, "kind": "mcp", "name": "srv",
        "transport": {"kind": "stdio", "command": "python3", "args": args, "cwd": cwd,
            "env": [{"name": "FIXTURE_MARKER", "value": marker.display().to_string()}]}
    })
    .to_string()
}

fn from_file(
    script: &str,
    marker: &std::path::Path,
    root: &CollectionRoot,
    path: &str,
    digest: &str,
) -> McpConnect {
    McpConnect {
        transport: stdio(
            script,
            None,
            vec![pair("FIXTURE_MARKER", &marker.display().to_string())],
        ),
        env: EnvSnapshot::default(),
        source: McpSource::File {
            root: root.clone(),
            path: path.into(),
            digest: digest.into(),
        },
    }
}

#[tokio::test]
async fn mcp_project_profile_runs_only_when_reviewed_and_identical() {
    let h = Harness::new().await;
    let script = script(h.dir.path());
    let project_dir = h.dir.path().join("repo");
    std::fs::create_dir_all(&project_dir).expect("repo");
    let project = h
        .projects
        .add(devbox_projects::AddProject {
            path: project_dir.display().to_string(),
        })
        .await
        .expect("register project");
    let root = CollectionRoot::Project {
        project_id: project.id.clone(),
    };
    let write = |path: &str, text: String| {
        let collections = &h.collections;
        let root = root.clone();
        let path = path.to_owned();
        async move {
            collections
                .write(WriteFile {
                    root,
                    path,
                    text,
                    expect: CollectionExpect::Absent,
                })
                .await
                .unwrap_or_else(|_| panic!("write profile"))
                .digest
        }
    };
    let marker = |n: &str| h.dir.path().join(format!("profile-{n}"));
    // ① 검토 전: 실행하지 않는다.
    let d1 = write("a.mcp.json", profile(&script, &marker("1"), &[])).await;
    let err = connect(
        &h,
        from_file(&script, &marker("1"), &root, "a.mcp.json", &d1),
    )
    .await
    .err()
    .expect("unreviewed");
    assert_eq!(err.code(), "api.review_required");
    // ② 검토 뒤 같은 정의: 실행한다.
    h.collections
        .review(ReviewFile {
            root: root.clone(),
            path: "a.mcp.json".into(),
            digest: d1.clone(),
        })
        .await
        .unwrap_or_else(|_| panic!("review"));
    let mut lines = connect(
        &h,
        from_file(&script, &marker("1"), &root, "a.mcp.json", &d1),
    )
    .await
    .expect("reviewed");
    ready(&mut lines).await;
    assert!(marker("1").exists());
    lines.close();
    // ③ 검토한 파일인데 요청 명령이 다름(인자 하나 더): 실행하지 않는다.
    let d2 = write("b.mcp.json", profile(&script, &marker("2"), &[])).await;
    h.collections
        .review(ReviewFile {
            root: root.clone(),
            path: "b.mcp.json".into(),
            digest: d2.clone(),
        })
        .await
        .unwrap_or_else(|_| panic!("review"));
    let mut p = from_file(&script, &marker("2"), &root, "b.mcp.json", &d2);
    if let McpTransport::Stdio { args, .. } = &mut p.transport {
        args.push("--extra".into());
    }
    let err = connect(&h, p).await.err().expect("mismatch");
    assert_eq!(
        (err.code(), err.detail()["reason"].as_str()),
        ("api.invalid_input", Some("mismatch"))
    );
    // ④ 검토 뒤 밖에서 바뀐 파일: 이전 digest는 충돌, 새 digest는 다시 검토 필요.
    let path = project_dir.join(".devbox/api/b.mcp.json");
    std::fs::write(&path, profile(&script, &marker("3"), &[])).expect("edit outside");
    let err = connect(
        &h,
        from_file(&script, &marker("3"), &root, "b.mcp.json", &d2),
    )
    .await
    .err()
    .expect("stale");
    assert_eq!(err.code(), "api.collection_conflict");
    let d3 = sha(&std::fs::read(&path).expect("read"));
    let err = connect(
        &h,
        from_file(&script, &marker("3"), &root, "b.mcp.json", &d3),
    )
    .await
    .err()
    .expect("edited");
    assert_eq!(err.code(), "api.review_required");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !marker("2").exists() && !marker("3").exists(),
        "nothing ran"
    );
    // ⑤ 개인 루트 파일은 검토 없이 쓴다(사용자가 쓴 것).
    // 개인 파일에는 프로젝트가 없으니 cwd를 적는다(등록 프로젝트 안).
    let personal = CollectionRoot::Personal;
    let personal_cwd = std::fs::canonicalize(&project_dir)
        .expect("real")
        .display()
        .to_string();
    let d4 = h
        .collections
        .write(WriteFile {
            root: personal.clone(),
            path: "p.mcp.json".into(),
            text: profile_in(&script, &marker("4"), &[], Some(&personal_cwd)),
            expect: CollectionExpect::Absent,
        })
        .await
        .unwrap_or_else(|_| panic!("personal write"))
        .digest;
    let mut p = from_file(&script, &marker("4"), &personal, "p.mcp.json", &d4);
    if let McpTransport::Stdio { cwd, .. } = &mut p.transport {
        *cwd = Some(personal_cwd.clone());
    }
    let mut lines = connect(&h, p).await.expect("personal");
    ready(&mut lines).await;
}

fn sha(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    data_encoding::HEXLOWER.encode(&Sha256::digest(bytes))
}
