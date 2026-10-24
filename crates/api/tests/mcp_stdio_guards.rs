//! MCP stdio 실행 방어(S5-5 검토 1): 로더·인터프리터 주입 환경 이름 거절(검토한 파일이어도), 비밀 치환 뒤 값
//! 재검사, cwd 규칙(초안은 필수·프로젝트 파일은 그 프로젝트 루트), 한 줄 4MiB 상한, 오류 문장·serverInfo 가림,
//! 지원하지 않는 프로토콜 버전 거절. 모두 소유 임시 폴더와 python3 fixture만 쓴다.
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

fn stdio(args: Vec<String>, cwd: Option<&str>, env: Vec<Pair>) -> McpTransport {
    McpTransport::Stdio {
        command: "python3".into(),
        args,
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

fn reason(err: &Rpc) -> (String, String, String) {
    let d = err.detail();
    (
        err.code().to_owned(),
        d["field"].as_str().unwrap_or("").to_owned(),
        d["reason"].as_str().unwrap_or("").to_owned(),
    )
}

async fn register(h: &Harness, name: &str) -> (std::path::PathBuf, CollectionRoot) {
    let dir = h.dir.path().join(name);
    std::fs::create_dir_all(&dir).expect("project dir");
    let project = h
        .projects
        .add(devbox_projects::AddProject {
            path: dir.display().to_string(),
        })
        .await
        .expect("register project");
    (
        std::fs::canonicalize(&dir).expect("real"),
        CollectionRoot::Project {
            project_id: project.id,
        },
    )
}

async fn reviewed(h: &Harness, root: &CollectionRoot, path: &str, text: String) -> String {
    let digest = h
        .collections
        .write(WriteFile {
            root: root.clone(),
            path: path.into(),
            text,
            expect: CollectionExpect::Absent,
        })
        .await
        .unwrap_or_else(|_| panic!("write {path}"))
        .digest;
    if matches!(root, CollectionRoot::Project { .. }) {
        h.collections
            .review(ReviewFile {
                root: root.clone(),
                path: path.into(),
                digest: digest.clone(),
            })
            .await
            .unwrap_or_else(|_| panic!("review {path}"));
    }
    digest
}

fn from_file(
    transport: McpTransport,
    root: &CollectionRoot,
    path: &str,
    digest: &str,
) -> McpConnect {
    McpConnect {
        transport,
        env: EnvSnapshot::default(),
        source: McpSource::File {
            root: root.clone(),
            path: path.into(),
            digest: digest.into(),
        },
    }
}

fn profile(transport: &McpTransport) -> String {
    json!({"version": 1, "kind": "mcp", "name": "srv", "transport": transport}).to_string()
}

async fn ask(lines: &mut Lines, id: &str, name: &str) -> Value {
    lines
        .send(json!({"type": "call", "id": id, "name": name, "argumentsJson": "{}"}))
        .await;
    loop {
        let event = lines.next(T).await.unwrap_or_else(|| panic!("answer {id}"));
        if event["id"] == id || event["type"] == "failed" {
            return event;
        }
    }
}

#[tokio::test]
async fn mcp_stdio_loader_variables_are_refused_even_for_reviewed_files() {
    let h = Harness::new().await;
    let script = script(h.dir.path());
    let (project, root) = register(&h, "repo").await;
    // 검토 화면에 `python3 <script>`만 보여도 PYTHONPATH의 sitecustomize가 코드를 실행할 수 있다(검토 1 probe).
    let inject = h.dir.path().join("inject");
    std::fs::create_dir_all(&inject).expect("inject dir");
    let marker = h.dir.path().join("injected");
    std::fs::write(
        inject.join("sitecustomize.py"),
        format!(
            "open({:?}, 'w').write('ran')\n",
            marker.display().to_string()
        ),
    )
    .expect("sitecustomize");
    let transport = stdio(
        vec![script.clone()],
        None,
        vec![pair("PYTHONPATH", &inject.display().to_string())],
    );
    let digest = reviewed(&h, &root, "a.mcp.json", profile(&transport)).await;
    let err = connect(&h, from_file(transport, &root, "a.mcp.json", &digest))
        .await
        .err()
        .expect("refused");
    assert_eq!(
        reason(&err),
        ("api.invalid_input".into(), "env".into(), "forbidden".into())
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!marker.exists(), "injected code must not run");
    // 대소문자와 관계없이, 목록의 이름·접두어 모두.
    let cwd = project.display().to_string();
    for name in [
        "LD_PRELOAD",
        "ld_library_path",
        "DYLD_INSERT_LIBRARIES",
        "NODE_OPTIONS",
        "Node_Path",
        "PYTHONSTARTUP",
        "PYTHONHOME",
        "PERL5OPT",
        "PERLLIB",
        "RUBYOPT",
        "RUBYLIB",
        "BASH_ENV",
        "ENV",
        "GIT_SSH_COMMAND",
        "JAVA_TOOL_OPTIONS",
        "_JAVA_OPTIONS",
        // 검토 2: npm 설정 env로 node 옵션 주입(npx), 패키지 색인·설정 파일·Docker 데몬·설정 폴더 바꾸기.
        "npm_config_node_options",
        "NPM_CONFIG_USERCONFIG",
        "PIP_INDEX_URL",
        "pip_extra_index_url",
        "PIP_CONFIG_FILE",
        "UV_INDEX_URL",
        "UV_EXTRA_INDEX_URL",
        "UV_INDEX",
        "UV_DEFAULT_INDEX",
        "uv_config_file",
        "DOCKER_HOST",
        "DOCKER_CONFIG",
        "XDG_CONFIG_HOME",
        "PYTHONPATH",
        "PYTHONUSERBASE",
        "PYTHONEXECUTABLE",
        "PYTHONINSPECT",
        "PYTHONBREAKPOINT",
        "PYTHONPLATLIBDIR",
        "PYTHONSAFEPATH",
        // 검토 3: 허용 목록 밖의 PYTHON* 전부(점이 든 경고 범주는 시작할 때 모듈을 import), 브라우저 실행 명령.
        "PYTHONWARNINGS",
        "pythonwarnings",
        "PYTHONPYCACHEPREFIX",
        "PYTHONSOMETHINGNEW",
        "BROWSER",
    ] {
        let p = draft(stdio(
            vec![script.clone()],
            Some(&cwd),
            vec![pair(name, "x")],
        ));
        let err = connect(&h, p)
            .await
            .err()
            .unwrap_or_else(|| panic!("{name}"));
        assert_eq!(
            reason(&err),
            ("api.invalid_input".into(), "env".into(), "forbidden".into()),
            "{name}"
        );
    }
    // 비슷하지만 목록에 없는 이름은 쓴다.
    let ok = draft(stdio(
        vec!["-I".into(), script.clone()],
        Some(&cwd),
        vec![
            pair("MY_LD_PRELOAD", "x"),
            pair("ENVIRONMENT", "x"),
            // 코드를 읽어 들이지 않는 python 출력 설정은 쓴다(검토 2 N4).
            pair("PYTHONUNBUFFERED", "1"),
            pair("PYTHONIOENCODING", "utf-8"),
            pair("PYTHONUTF8", "1"),
            pair("pythondontwritebytecode", "1"),
        ],
    ));
    let mut lines = connect(&h, ok).await.expect("similar names are fine");
    lines.until("ready", T).await.0.expect("ready");
}

#[tokio::test]
async fn mcp_stdio_env_values_are_checked_after_secret_substitution() {
    let h = Harness::new().await;
    // 치환 전 모양(`{{secret:BIG}}`)은 작지만, 풀린 값은 env 합계 상한(256KiB)을 넘는다(비밀 저장소는 NUL을 막는다).
    let big = "s".repeat(256 * 1024);
    h.secret("BIG", &big).await;
    let script = script(h.dir.path());
    let cwd = workdir(&h).await;
    let marker = h.dir.path().join("nul-marker");
    let p = draft(stdio(
        vec!["-I".into(), script],
        Some(&cwd),
        vec![
            pair("FIXTURE_MARKER", &marker.display().to_string()),
            pair("TOKEN", "{{secret:BIG}}"),
        ],
    ));
    let err = connect(&h, p).await.err().expect("refused before spawning");
    assert_eq!(
        reason(&err),
        (
            "api.invalid_input".into(),
            "env".into(),
            "invalid_value".into()
        )
    );
    assert!(!err.detail_text().contains(&big[..64]));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!marker.exists());
}

#[tokio::test]
async fn mcp_stdio_cwd_is_required_for_drafts_and_defaults_to_the_project_root() {
    let h = Harness::new().await;
    let script = script(h.dir.path());
    let marker = h.dir.path().join("cwd-marker");
    let env = vec![pair("FIXTURE_MARKER", &marker.display().to_string())];
    // 초안: cwd가 없으면 실행하지 않는다(홈 폴더로 가지 않음).
    let err = connect(
        &h,
        draft(stdio(vec!["-I".into(), script.clone()], None, env.clone())),
    )
    .await
    .err()
    .expect("draft without cwd");
    assert_eq!(
        reason(&err),
        ("api.invalid_input".into(), "cwd".into(), "required".into())
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!marker.exists());
    // 프로젝트 파일: cwd가 비면 그 프로젝트 루트.
    let (project, root) = register(&h, "repo").await;
    let transport = stdio(vec!["-I".into(), script.clone()], None, env.clone());
    let digest = reviewed(&h, &root, "a.mcp.json", profile(&transport)).await;
    let mut lines = connect(
        &h,
        from_file(transport.clone(), &root, "a.mcp.json", &digest),
    )
    .await
    .expect("project file");
    lines.until("ready", T).await.0.expect("ready");
    let result = ask(&mut lines, "e", "env").await;
    let value: Value = serde_json::from_str(result["json"].as_str().expect("json")).expect("env");
    assert_eq!(value["cwd"], project.display().to_string());
    lines.close();
    // 개인 파일: 프로젝트가 없으니 cwd가 필요하다.
    let personal = CollectionRoot::Personal;
    let digest = reviewed(&h, &personal, "p.mcp.json", profile(&transport)).await;
    let err = connect(&h, from_file(transport, &personal, "p.mcp.json", &digest))
        .await
        .err()
        .expect("personal without cwd");
    assert_eq!(
        reason(&err),
        ("api.invalid_input".into(), "cwd".into(), "required".into())
    );
}

#[tokio::test]
async fn mcp_stdio_oversized_line_fails_the_session_cleanly() {
    let h = Harness::new().await;
    let script = script(h.dir.path());
    let cwd = workdir(&h).await;
    let pid_file = h.dir.path().join("big.pid");
    let p = draft(stdio(
        vec!["-I".into(), script],
        Some(&cwd),
        vec![pair("FIXTURE_MARKER", &pid_file.display().to_string())],
    ));
    let mut lines = connect(&h, p).await.expect("connect");
    lines.until("ready", T).await.0.expect("ready");
    let failed = ask(&mut lines, "b", "big").await;
    assert_eq!(failed["type"], "failed", "{failed}");
    assert_eq!(failed["error"]["code"], "api.too_large");
    assert_eq!(failed["error"]["detail"]["limit"], 4 * 1024 * 1024);
    let rest = lines.all(T).await;
    assert!(rest.iter().all(|e| e["type"] != "result"));
    let pid: u32 = std::fs::read_to_string(&pid_file)
        .expect("pid")
        .trim()
        .parse()
        .expect("pid");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while !gone(pid) && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(gone(pid), "server process ended");
}

#[tokio::test]
async fn mcp_stdio_error_messages_and_server_info_are_masked() {
    let h = Harness::new().await;
    h.secret("T", "fixture-guard-secret").await;
    let script = script(h.dir.path());
    let cwd = workdir(&h).await;
    let p = draft(stdio(
        vec!["-I".into(), script],
        Some(&cwd),
        vec![
            pair("TOKEN", "{{secret:T}}"),
            pair("FIXTURE_NAME", "srv {{secret:T}}"),
        ],
    ));
    let mut lines = connect(&h, p).await.expect("connect");
    let (ready, before) = lines.until("ready", T).await;
    let ready = ready.unwrap_or_else(|| panic!("ready, got {before:?}"));
    let name = ready["serverName"].as_str().expect("name");
    assert!(
        name.contains(REDACTED) && !name.contains("fixture-guard-secret"),
        "{name}"
    );
    let error = ask(&mut lines, "f", "fail-secret").await;
    assert_eq!(error["type"], "error", "{error}");
    let message = error["message"].as_str().expect("message");
    assert!(
        message.contains(REDACTED) && !message.contains("fixture-guard-secret"),
        "{message}"
    );
}

#[tokio::test]
async fn mcp_stdio_unsupported_protocol_version_is_refused() {
    let h = Harness::new().await;
    let script = script(h.dir.path());
    let cwd = workdir(&h).await;
    let p = draft(stdio(
        vec!["-I".into(), script],
        Some(&cwd),
        vec![pair("FIXTURE_PROTOCOL", "2099-01-01")],
    ));
    let mut lines = connect(&h, p).await.expect("connect");
    let events = lines.all(T).await;
    assert!(events.iter().all(|e| e["type"] != "ready"), "{events:?}");
    let failed = events
        .iter()
        .find(|e| e["type"] == "failed")
        .expect("failed");
    assert_eq!(
        (
            failed["error"]["code"].as_str(),
            failed["error"]["detail"]["reason"].as_str()
        ),
        (Some("api.mcp"), Some("protocol_version"))
    );
}
