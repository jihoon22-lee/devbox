#![cfg(all(target_os = "linux", feature = "test-fixtures"))]
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};
use workspace_wsl::{
    control::{ControlInput, ControlOutput, Output},
    lsp_wire::Reply,
    Request, Response, VERSION,
};
struct Helper {
    child: Child,
    input: Option<ChildStdin>,
    frames: Receiver<Output>,
    session: String,
    sequence: u64,
    request_id: String,
    root: PathBuf,
    token: Option<String>,
    context: Value,
    digest: String,
    prepared: bool,
    admissions: usize,
}
impl Helper {
    fn start(root: &Path, context: Value) -> Self {
        Self::start_with_path(root, context, root.join("bin").as_os_str())
    }
    fn start_with_path(root: &Path, context: Value, path: &std::ffi::OsStr) -> Self {
        let session = uuid::Uuid::new_v4().to_string();
        let mut child = Command::new(env!("CARGO_BIN_EXE_devbox-workspace-wsl"))
            .args(["--session", &session])
            .env_clear()
            .env("HOME", root)
            .env("PATH", path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take();
        let mut output = child.stdout.take().unwrap();
        let (sender, frames) = mpsc::sync_channel(16);
        thread::spawn(move || {
            while let Ok(Some(frame)) = workspace_wsl::read_frame::<_, Output>(&mut output) {
                if sender.send(frame).is_err() {
                    break;
                }
            }
        });
        let mut helper = Self {
            child,
            input,
            frames,
            session,
            sequence: 0,
            request_id: String::new(),
            root: root.into(),
            token: None,
            context,
            digest: String::new(),
            prepared: false,
            admissions: 0,
        };
        let report = helper
            .call("observe_root", json!({"path":root}))
            .result
            .unwrap();
        helper.token = Some(report["token"].as_str().unwrap().into());
        helper
    }
    fn send(&mut self, method: &str, mut args: Value) {
        if workspace_wsl::control::project_method(method) {
            args["context"] = self.context.clone();
        }
        self.sequence += 1;
        self.request_id = uuid::Uuid::new_v4().to_string();
        let request = Request {
            version: VERSION,
            session_id: self.session.clone(),
            request_id: self.request_id.clone(),
            sequence: self.sequence,
            budget_ms: 29000,
            method: method.into(),
            root_token: if matches!(method, "observe_root" | "execution_prepare") {
                None
            } else {
                self.token.clone()
            },
            args,
        };
        workspace_wsl::write_frame(self.input.as_mut().unwrap(), &request).unwrap();
    }
    fn reply(&mut self, control: ControlOutput, approved: bool) {
        let ControlOutput::Admission {
            version,
            session_id,
            request_id,
            sequence,
            admission_id,
            target_root,
        } = control
        else {
            panic!("unexpected retirement")
        };
        assert_eq!(version, VERSION);
        assert_eq!(session_id, self.session);
        assert_eq!(request_id, self.request_id);
        assert_eq!(sequence, self.sequence);
        assert_eq!(target_root, self.root.to_str().unwrap());
        self.admissions += 1;
        workspace_wsl::write_frame(
            self.input.as_mut().unwrap(),
            &ControlInput::AdmissionReply {
                version,
                session_id,
                request_id,
                sequence,
                admission_id,
                approved,
            },
        )
        .unwrap();
    }
    fn finish(&mut self) -> Response {
        loop {
            match self
                .frames
                .recv_timeout(Duration::from_secs(30))
                .expect("native response deadline")
            {
                Output::Control(control) => self.reply(control, true),
                Output::Response(response) => {
                    assert_eq!(response.version, VERSION);
                    assert_eq!(response.session_id, self.session);
                    assert_eq!(response.request_id, self.request_id);
                    assert_eq!(response.sequence, self.sequence);
                    return response;
                }
            }
        }
    }
    fn call(&mut self, method: &str, args: Value) -> Response {
        self.send(method, args);
        self.finish()
    }
    fn capture(&mut self, servers: Value) {
        let config = json!({"version":1,"enabled":true,"workspace_root":self.root,"server_by_language":servers,"custom_servers":[],"update_policy":"manual"});
        self.capture_config(config);
    }
    fn capture_config(&mut self, config: Value) {
        let view = self
            .call("lsp_capture", json!({"config":config}))
            .result
            .unwrap();
        self.digest = view["digest"].as_str().unwrap().into();
        assert_eq!(
            self.call("execution_prepare", json!({})).result.unwrap(),
            json!({"retirement":true})
        );
        self.prepared = true;
    }
    fn send_lsp(&mut self, method: &str, args: Value, document: Option<Value>) {
        self.send("lsp_execute",json!({"context":self.context,"digest":self.digest,"method":method,"args":args,"document":document}));
    }
    fn lsp(&mut self, method: &str, args: Value, document: Option<Value>) -> Reply {
        self.send_lsp(method, args, document);
        serde_json::from_value(self.finish().result.unwrap()).unwrap()
    }
    fn cancel(&mut self) {
        workspace_wsl::write_frame(
            self.input.as_mut().unwrap(),
            &ControlInput::Cancel {
                version: VERSION,
                session_id: self.session.clone(),
                request_id: self.request_id.clone(),
                sequence: self.sequence,
            },
        )
        .unwrap();
    }
    fn until_initialized(&mut self, state: &Path) {
        let until = Instant::now() + Duration::from_secs(10);
        while !state.join("initializing").exists() || !state.join("detached.pid").exists() {
            assert!(Instant::now() < until, "native server did not initialize");
            match self.frames.recv_timeout(Duration::from_millis(10)) {
                Ok(Output::Control(control)) => self.reply(control, true),
                Ok(Output::Response(response)) => {
                    panic!("unexpected early response: {:?}", response.result)
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(error) => panic!("native pipe closed: {error}"),
            }
        }
    }
    fn close(&mut self) {
        self.input.take();
        let mut retired = !self.prepared;
        let until = Instant::now() + Duration::from_secs(15);
        loop {
            while let Ok(frame) = self.frames.try_recv() {
                if let Output::Control(ControlOutput::Retired {
                    version,
                    session_id,
                    sequence,
                }) = frame
                {
                    assert_eq!(version, VERSION);
                    assert_eq!(session_id, self.session);
                    assert_eq!(sequence, self.sequence);
                    retired = true;
                }
            }
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                assert!(retired, "no native retirement acknowledgement");
                break;
            }
            assert!(Instant::now() < until, "native retirement deadline");
            thread::sleep(Duration::from_millis(10));
        }
    }
}

/// This optional acceptance check uses already-provisioned, digest-reviewed
/// server artifacts. It never installs packages or writes outside its fixture.
#[test]
#[ignore = "requires DEVBOX_WSL_LSP_INSTALLED_TARGETS with reviewed native server artifacts"]
fn installed_native_language_servers_accept_versioned_documents() {
    let installed = PathBuf::from(
        std::env::var_os("DEVBOX_WSL_LSP_INSTALLED_TARGETS").expect("installed fixture targets"),
    );
    let node = installed.join("bin/node");
    let toolchain = PathBuf::from(
        std::env::var_os("DEVBOX_WSL_LSP_INSTALLED_TOOLCHAIN")
            .expect("installed Rust toolchain bin with cargo, rustc and rustfmt"),
    );
    for executable in ["cargo", "rustc", "rustfmt"] {
        assert!(toolchain.join(executable).is_file());
    }
    let cases = [
        (
            "rust",
            "rs",
            "fn main() { let value = 1; }\n",
            "bin/rust-analyzer",
        ),
        (
            "typescript",
            "ts",
            "const value = '한글🙂';\nvalue;\n",
            "node_modules/typescript-language-server/lib/cli.mjs",
        ),
        (
            "javascript",
            "js",
            "const value = '한글🙂';\nvalue;\n",
            "node_modules/typescript-language-server/lib/cli.mjs",
        ),
        (
            "python",
            "py",
            "value = '한글🙂'\nvalue\n",
            "node_modules/basedpyright/langserver.index.js",
        ),
        (
            "json",
            "json",
            "{\"name\":\"한글🙂\"}\n",
            "node_modules/vscode-langservers-extracted/bin/vscode-json-language-server",
        ),
        (
            "html",
            "html",
            "<div class=\"value\">한글🙂</div>\n",
            "node_modules/vscode-langservers-extracted/bin/vscode-html-language-server",
        ),
        (
            "css",
            "css",
            ".value { color: red; }\n",
            "node_modules/vscode-langservers-extracted/bin/vscode-css-language-server",
        ),
    ];
    for (language, extension, text, entry) in cases {
        let fixture = Fixture::new();
        let path = fixture.root.join(format!("문서 space.{extension}"));
        fs::write(&path, text).unwrap();
        if language == "rust" {
            fs::write(fixture.root.join("Cargo.toml"), "[package]\nname = \"installed_fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[workspace]\n[[bin]]\nname = \"fixture\"\npath = \"문서 space.rs\"\n").unwrap();
        }
        let mut files = fixture.helper();
        files.call("files_attach", json!({})).result.unwrap();
        let opened = files
            .call(
                "files_open",
                json!({"request":{"path":path,"encoding":null}}),
            )
            .result
            .unwrap();
        let revision = opened["nativeRevision"].clone();
        let paths = std::env::join_paths([
            fixture.root.join("bin"),
            installed.join("bin"),
            toolchain.clone(),
        ])
        .unwrap();
        let mut lsp = Helper::start_with_path(&fixture.root, fixture.context.clone(), &paths);
        let mut config = json!({"version":1,"enabled":true,"workspace_root":fixture.root,"server_by_language":{},"custom_servers":[],"update_policy":"manual"});
        if language == "rust" {
            config["server_by_language"][language] = json!({"kind":"local","installed_path":installed.join(entry),"executable":null,"args":[]});
        } else {
            config["custom_servers"] = json!([{"language_ids":[language],"executable":installed.join(entry),"args":["--stdio"],"runtime":{"kind":"node","executable":node,"min_version":">=22"},"source":"reviewed-test-artifacts","license":"unknown","version":"unknown"}]);
        }
        lsp.capture_config(config);
        let start = lsp.lsp(
            "start_language_server",
            json!({"languageId":language,"operationId":format!("installed-{language}")}),
            None,
        );
        assert!(
            start.result.is_ok(),
            "{language} startup: {:?}",
            start.result
        );
        let status = lsp
            .lsp("language_server_statuses", json!({}), None)
            .result
            .unwrap();
        assert_eq!(status[0]["status"], "ready", "{language}: {status}");
        assert_eq!(status[0]["capabilities"]["rename"], false);
        let opened = lsp
            .lsp(
                "open_lsp_document",
                json!({"languageId":language,"path":path,"nativeRevision":revision,"text":text}),
                Some(proof(&mut files, &path, &revision, true)),
            )
            .result
            .unwrap();
        let uri = opened["uri"].clone();
        assert!(uri.as_str().unwrap().contains("%20"));
        let changed = format!("{text}\n");
        let updated = lsp.lsp("change_lsp_document", json!({"languageId":language,"uri":uri,"nativeRevision":revision,"text":changed,"dirty":true}), Some(proof(&mut files, &path, &revision, false))).result.unwrap();
        assert_eq!(updated["version"], 2);
        let capabilities = lsp
            .lsp("language_server_statuses", json!({}), None)
            .result
            .unwrap()[0]["capabilities"]
            .clone();
        let mut features = Vec::new();
        for (capability, method) in [
            ("completion", "request_lsp_completion"),
            ("hover", "request_lsp_hover"),
            ("formatting", "request_lsp_formatting"),
        ] {
            if capabilities[capability] != true {
                continue;
            }
            let mut args = json!({"languageId":language,"uri":uri});
            if capability == "formatting" {
                args["tabSize"] = json!(2);
                args["insertSpaces"] = json!(true);
            } else {
                let (line, character) = match language {
                    "typescript" | "javascript" | "python" => (1, 2),
                    "css" => (0, 11),
                    "html" => (0, 2),
                    "rust" => (0, 18),
                    _ => (0, 1),
                };
                args["position"] = json!({"line":line,"character":character});
            }
            let reply = lsp.lsp(
                method,
                args,
                Some(proof(&mut files, &path, &revision, false)),
            );
            assert!(
                reply.result.is_ok(),
                "{language} {capability}: {:?}",
                reply.result
            );
            let result = reply.result.unwrap();
            if capability == "formatting" {
                for document in result["documents"]
                    .as_array()
                    .expect("buffered formatting result")
                {
                    assert_eq!(document["uri"], uri);
                    assert_eq!(document["version"], 3);
                }
            } else {
                assert_eq!(
                    result["metadata"]["version"], 2,
                    "{language} {capability}: {result}"
                );
                assert_eq!(result["stale"], false);
                if capability == "hover" && !matches!(language, "rust" | "json") {
                    assert!(
                        !result["value"].is_null(),
                        "{language} must return semantic hover content"
                    );
                }
            }
            features.push(capability);
        }
        assert!(
            !features.is_empty(),
            "{language} must expose an editor feature"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
        lsp.lsp(
            "close_lsp_document",
            json!({"languageId":language,"uri":uri}),
            None,
        )
        .result
        .unwrap();
        lsp.lsp("stop_all_language_servers", json!({}), None)
            .result
            .unwrap();
        lsp.close();
        files.close();
        eprintln!("installed native {language}: ready, UTF-16 version 2, features {features:?}, explicit shutdown acknowledged");
    }
}
impl Drop for Helper {
    fn drop(&mut self) {
        self.input.take();
        let until = Instant::now() + Duration::from_secs(10);
        while self.child.try_wait().ok().flatten().is_none() && Instant::now() < until {
            while self.frames.try_recv().is_ok() {}
            thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
struct Fixture {
    _directory: tempfile::TempDir,
    root: PathBuf,
    program: PathBuf,
    context: Value,
}
impl Fixture {
    fn new() -> Self {
        let directory = tempfile::Builder::new()
            .prefix(".wsl-lsp-transport-")
            .tempdir_in(env!("CARGO_MANIFEST_DIR"))
            .unwrap();
        let root = directory.path().join("project 한글 space");
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("bin")).unwrap();
        let program = root.join("bin/fixture-server");
        fs::copy(env!("CARGO_BIN_EXE_workspace-lsp-fixture"), &program).unwrap();
        let context = json!({"projectId":"lsp-project","worktreeId":"lsp-tree","revision":1,"target":{"kind":"wsl","distroId":uuid::Uuid::new_v4().to_string()}});
        Self {
            _directory: directory,
            root,
            program,
            context,
        }
    }
    fn server(&self, state: &str, extra: &[&str]) -> (Value, PathBuf) {
        let state = self.root.join(state);
        fs::create_dir(&state).unwrap();
        let mut args = vec!["--state".to_owned(), state.to_str().unwrap().into()];
        args.extend(extra.iter().map(|arg| (*arg).into()));
        (
            json!({"kind":"custom","executable":self.program,"args":args}),
            state,
        )
    }
    fn helper(&self) -> Helper {
        Helper::start(&self.root, self.context.clone())
    }
}
fn proof(files: &mut Helper, path: &Path, revision: &Value, verify: bool) -> Value {
    files
        .call(
            "files_lsp_snapshot",
            json!({"path":path,"nativeRevision":revision,"verifyDisk":verify}),
        )
        .result
        .unwrap()
}
fn pid(state: &Path, name: &str) -> u32 {
    fs::read_to_string(state.join(name))
        .unwrap()
        .parse()
        .unwrap()
}
fn alive(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}
#[test]
fn native_lsp_documents_bind_file_identity_versions_utf16_and_explicit_saves() {
    let fixture = Fixture::new();
    let (server, state) = fixture.server("server-state", &["--numeric-sync"]);
    let path = fixture.root.join("문서.rs");
    fs::write(&path, "a🙂b\n").unwrap();
    let mut files = fixture.helper();
    files.call("files_attach", json!({})).result.unwrap();
    let opened = files
        .call(
            "files_open",
            json!({"request":{"path":path,"encoding":null}}),
        )
        .result
        .unwrap();
    let revision = opened["nativeRevision"].clone();
    let mut lsp = fixture.helper();
    lsp.capture(json!({"rust":server}));
    lsp.lsp(
        "start_language_server",
        json!({"languageId":"rust","operationId":"start-rust"}),
        None,
    )
    .result
    .unwrap();
    assert!(state.join("server.pid").exists());
    assert!(lsp.admissions >= 2);
    assert!(lsp
        .lsp(
            "open_lsp_document",
            json!({"languageId":"rust","path":path,"text":"a🙂b\n","nativeRevision":revision}),
            None
        )
        .result
        .is_err());
    let granted = proof(&mut files, &path, &revision, true);
    let mut forged = granted.clone();
    forged["identity"][1] = json!(0);
    assert!(lsp
        .lsp(
            "open_lsp_document",
            json!({"languageId":"rust","path":path,"text":"a🙂b\n","nativeRevision":revision}),
            Some(forged)
        )
        .result
        .is_err());
    let document = lsp
        .lsp(
            "open_lsp_document",
            json!({"languageId":"rust","path":path,"text":"a🙂b\n","nativeRevision":revision}),
            Some(granted.clone()),
        )
        .result
        .unwrap();
    let uri = document["uri"].as_str().unwrap().to_owned();
    assert!(uri.starts_with("file:///"));
    assert!(uri.contains("%20"));
    let invalid = lsp.lsp(
        "request_lsp_completion",
        json!({"languageId":"rust","uri":uri,"position":{"line":0,"character":2}}),
        Some(granted.clone()),
    );
    assert!(invalid.result.is_err());
    let completed = lsp
        .lsp(
            "request_lsp_completion",
            json!({"languageId":"rust","uri":uri,"position":{"line":0,"character":3}}),
            Some(granted.clone()),
        )
        .result
        .unwrap();
    assert!(completed.to_string().contains("fixtureComplete"));
    let hover = lsp
        .lsp(
            "request_lsp_hover",
            json!({"languageId":"rust","uri":uri,"position":{"line":0,"character":3}}),
            Some(granted.clone()),
        )
        .result
        .unwrap();
    assert!(hover.to_string().contains("fixture hover"));
    let statuses = lsp
        .lsp("language_server_statuses", json!({}), None)
        .result
        .unwrap();
    assert_eq!(statuses[0]["capabilities"]["rename"], false);
    assert!(lsp.lsp("request_lsp_rename",json!({"languageId":"rust","uri":uri,"position":{"line":0,"character":0},"newName":"next"}),Some(granted.clone())).result.is_err());
    assert!(!state.join("rename-requested").exists());
    let formatted = lsp
        .lsp(
            "request_lsp_formatting",
            json!({"languageId":"rust","uri":uri,"tabSize":2,"insertSpaces":true}),
            Some(granted),
        )
        .result
        .unwrap();
    let text = formatted["documents"][0]["text"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(text, "// formatted\na🙂b\n");
    assert_eq!(fs::read_to_string(&path).unwrap(), "a🙂b\n");
    files
        .call(
            "files_sync_editor",
            json!({"path":path,"nativeRevision":revision,"text":text}),
        )
        .result
        .unwrap();
    let saved=files.call("files_save",json!({"nativeRevision":revision,"request":{"path":path,"text":text,"encoding":opened["encoding"],"lineEnding":opened["lineEnding"],"expectedMtimeNanos":opened["mtimeNanos"],"expectedSize":opened["size"],"expectedContentHash":opened["contentHash"],"sourceLossy":false}})).result.unwrap();
    let new_revision = saved["nativeRevision"].clone();
    assert_ne!(new_revision, revision);
    let saved_proof = proof(&mut files, &path, &new_revision, true);
    lsp.lsp(
        "save_lsp_document",
        json!({"languageId":"rust","uri":uri,"nativeRevision":new_revision,"text":text}),
        Some(saved_proof),
    )
    .result
    .unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
    lsp.lsp(
        "close_lsp_document",
        json!({"languageId":"rust","uri":uri}),
        None,
    )
    .result
    .unwrap();
    for _ in 0..3 {
        let reply = lsp.lsp("lsp_poll", json!({}), None);
        assert!(reply
            .events
            .iter()
            .all(|event| !matches!(event.name, workspace_wsl::lsp_wire::EventName::Diagnostics)));
    }
    let server_pid = pid(&state, "server.pid");
    let moved = fixture._directory.path().join("moved");
    fs::rename(&fixture.root, &moved).unwrap();
    lsp.lsp(
        "stop_all_language_servers",
        json!({"operationIds":[]}),
        None,
    )
    .result
    .unwrap();
    assert!(!alive(server_pid));
    lsp.close();
    files.close();
}
#[test]
fn cancellation_consumes_late_admission_and_reaps_only_the_starting_server() {
    let fixture = Fixture::new();
    let (first, first_state) = fixture.server("first-state", &[]);
    let (slow, slow_state) = fixture.server("slow-state", &["--slow", "--detach"]);
    let mut helper = fixture.helper();
    helper.capture(json!({"rust":first,"python":slow}));
    helper
        .lsp(
            "start_language_server",
            json!({"languageId":"rust","operationId":"first"}),
            None,
        )
        .result
        .unwrap();
    let first_pid = pid(&first_state, "server.pid");
    helper.send_lsp(
        "start_language_server",
        json!({"languageId":"python","operationId":"cancel-before-start"}),
        None,
    );
    let Output::Control(admission) = helper.frames.recv_timeout(Duration::from_secs(10)).unwrap()
    else {
        panic!("native admission required")
    };
    helper.cancel();
    helper.reply(admission, true);
    let result: Reply = serde_json::from_value(helper.finish().result.unwrap()).unwrap();
    assert!(result.result.is_err());
    assert!(alive(first_pid));
    assert!(!slow_state.join("server.pid").exists());
    helper.send_lsp(
        "start_language_server",
        json!({"languageId":"python","operationId":"cancel-created"}),
        None,
    );
    helper.until_initialized(&slow_state);
    let slow_pid = pid(&slow_state, "server.pid");
    let detached_pid = pid(&slow_state, "detached.pid");
    assert!(alive(slow_pid) && alive(detached_pid));
    helper.cancel();
    let result: Reply = serde_json::from_value(helper.finish().result.unwrap()).unwrap();
    assert!(result.result.is_err());
    assert!(!alive(slow_pid));
    assert!(!alive(detached_pid));
    assert!(alive(first_pid));
    let statuses = helper
        .lsp("language_server_statuses", json!({}), None)
        .result
        .unwrap();
    assert!(statuses
        .as_array()
        .unwrap()
        .iter()
        .any(|status| status["languageId"] == "rust" && status["processState"] == "running"));
    helper.close();
    assert!(!alive(first_pid));
}

#[test]
fn automatic_restart_waits_for_a_fresh_poll_and_replays_the_latest_document() {
    let fixture = Fixture::new();
    let (server, state) = fixture.server("restart-state", &["--crash-once"]);
    let (slow, slow_state) = fixture.server("slow-state", &["--slow", "--detach"]);
    let path = fixture.root.join("restart.rs");
    fs::write(&path, "original🙂\n").unwrap();
    let mut files = fixture.helper();
    files.call("files_attach", json!({})).result.unwrap();
    let opened = files
        .call(
            "files_open",
            json!({"request":{"path":path,"encoding":null}}),
        )
        .result
        .unwrap();
    let revision = opened["nativeRevision"].clone();
    let mut helper = fixture.helper();
    helper.capture(json!({"rust":server,"python":slow}));
    helper
        .lsp(
            "start_language_server",
            json!({"languageId":"rust","operationId":"first"}),
            None,
        )
        .result
        .unwrap();
    let initial=helper.lsp("open_lsp_document",json!({"languageId":"rust","path":path,"nativeRevision":revision,"text":"original🙂\n"}),Some(proof(&mut files,&path,&revision,true))).result.unwrap();
    let uri = initial["uri"].as_str().unwrap().to_owned();
    helper.send_lsp(
        "start_language_server",
        json!({"languageId":"python","operationId":"hold-retry"}),
        None,
    );
    helper.until_initialized(&slow_state);
    // A retry becoming due during a different explicit startup must not take
    // that command's cancellation/admission ticket or consume retry attempts.
    thread::sleep(Duration::from_millis(1200));
    assert_eq!(
        fs::read_to_string(state.join("starts"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    helper.cancel();
    let cancelled: Reply = serde_json::from_value(helper.finish().result.unwrap()).unwrap();
    assert!(cancelled.result.is_err());
    helper
        .lsp(
            "stop_language_server",
            json!({"languageId":"python","operationId":"hold-retry"}),
            None,
        )
        .result
        .unwrap();
    let text = "latest🙂 during backoff\n";
    files
        .call(
            "files_sync_editor",
            json!({"path":path,"nativeRevision":revision,"text":text}),
        )
        .result
        .unwrap();
    let changed=helper.lsp("change_lsp_document",json!({"languageId":"rust","uri":uri,"nativeRevision":revision,"text":text,"dirty":false}),Some(proof(&mut files,&path,&revision,false))).result.unwrap();
    let previous_admissions = helper.admissions;
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        helper.lsp("lsp_poll", json!({}), None).result.unwrap();
        if fs::read_to_string(state.join("starts"))
            .unwrap()
            .lines()
            .count()
            >= 2
            && fs::read_to_string(state.join("opened")).unwrap() == text
        {
            break;
        }
        assert!(
            Instant::now() < until,
            "automatic restart did not replay the latest document"
        );
        thread::sleep(Duration::from_millis(20));
    }
    assert!(helper.admissions > previous_admissions);
    let hover = helper
        .lsp(
            "request_lsp_hover",
            json!({"languageId":"rust","uri":uri,"position":{"line":0,"character":0}}),
            Some(proof(&mut files, &path, &revision, false)),
        )
        .result
        .unwrap();
    // A replacement session starts at version one while replaying the latest text.
    assert_eq!(changed["version"], 2);
    assert_eq!(hover["metadata"]["version"], 1);
    let old = fixture.root.join("bin/former-server");
    fs::rename(&fixture.program, &old).unwrap();
    fs::copy(&old, &fixture.program).unwrap();
    assert!(helper
        .lsp(
            "restart_language_server",
            json!({"languageId":"rust","operationId":"changed-executable"}),
            None
        )
        .result
        .is_err());
    assert_eq!(
        fs::read_to_string(state.join("starts"))
            .unwrap()
            .lines()
            .count(),
        2
    );
    helper.close();
    files.close();
}

#[test]
fn native_server_locations_cannot_escape_the_open_document_root() {
    let fixture = Fixture::new();
    let (server, _) = fixture.server("foreign-state", &["--foreign-uri"]);
    let path = fixture.root.join("source.rs");
    fs::write(&path, "value\n").unwrap();
    let mut files = fixture.helper();
    files.call("files_attach", json!({})).result.unwrap();
    let opened = files
        .call(
            "files_open",
            json!({"request":{"path":path,"encoding":null}}),
        )
        .result
        .unwrap();
    let revision = opened["nativeRevision"].clone();
    let mut helper = fixture.helper();
    helper.capture(json!({"rust":server}));
    helper
        .lsp(
            "start_language_server",
            json!({"languageId":"rust","operationId":"foreign"}),
            None,
        )
        .result
        .unwrap();
    let opened = helper
        .lsp(
            "open_lsp_document",
            json!({"languageId":"rust","path":path,"nativeRevision":revision,"text":"value\n"}),
            Some(proof(&mut files, &path, &revision, true)),
        )
        .result
        .unwrap();
    let uri = opened["uri"].clone();
    for method in ["request_lsp_definition", "request_lsp_references"] {
        let mut args = json!({"languageId":"rust","uri":uri,"position":{"line":0,"character":0}});
        if method == "request_lsp_references" {
            args["includeDeclaration"] = json!(true);
        }
        let response = helper
            .lsp(
                method,
                args,
                Some(proof(&mut files, &path, &revision, false)),
            )
            .result
            .unwrap();
        assert_eq!(response["value"]["locations"], json!([]), "{response}");
        assert_eq!(response["value"]["rejected"], 1, "{response}");
    }
    helper.close();
    files.close();
}
