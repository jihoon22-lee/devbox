#![cfg(all(target_os = "linux", feature = "helper"))]
use serde_json::{json, Value};
use std::{
    io,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    time::{Duration, Instant},
};
use workspace_wsl::{Request, Response, VERSION};
struct Helper {
    child: Child,
    input: Option<ChildStdin>,
    output: ChildStdout,
    session: String,
    sequence: u64,
}
impl Helper {
    fn start() -> Self {
        let session = uuid::Uuid::new_v4().to_string();
        let mut child = Command::new(env!("CARGO_BIN_EXE_devbox-workspace-wsl"))
            .args(["--session", &session])
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take();
        let output = child.stdout.take().unwrap();
        Self {
            child,
            input,
            output,
            session,
            sequence: 0,
        }
    }
    fn call(&mut self, method: &str, root: Option<&str>, args: Value) -> Response {
        self.sequence += 1;
        let request = Request {
            version: VERSION,
            session_id: self.session.clone(),
            request_id: uuid::Uuid::new_v4().to_string(),
            sequence: self.sequence,
            budget_ms: 5000,
            method: method.into(),
            root_token: root.map(str::to_owned),
            args,
        };
        workspace_wsl::write_frame(self.input.as_mut().unwrap(), &request).unwrap();
        let response: Response = workspace_wsl::read_frame(&mut self.output)
            .unwrap()
            .unwrap();
        assert_eq!(response.version, VERSION);
        assert_eq!(response.session_id, request.session_id);
        assert_eq!(response.request_id, request.request_id);
        assert_eq!(response.sequence, request.sequence);
        response
    }
    fn exited(&mut self, expected: i32) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert_eq!(status.code(), Some(expected));
                break;
            }
            assert!(Instant::now() < deadline, "owned helper did not exit");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Helper {
    fn drop(&mut self) {
        self.input.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
#[test]
fn actual_helper_pipe_binds_file_reads_and_eof_retires_open_grants() {
    let directory = tempfile::Builder::new()
        .prefix(".wsl-pipe-fixture-")
        .tempdir_in(env!("CARGO_MANIFEST_DIR"))
        .unwrap();
    let file = directory.path().join("한글.txt");
    std::fs::write(&file, b"original\r\n").unwrap();
    let mut helper = Helper::start();
    assert_eq!(
        helper.call("hello", None, json!({})).result.unwrap()["version"],
        VERSION
    );
    let report = helper
        .call("observe_root", None, json!({"path":directory.path()}))
        .result
        .unwrap();
    let root = report["token"].as_str().unwrap();
    let context = json!({"projectId":"project","worktreeId":"tree","revision":1,"target":{"kind":"wsl","distroId":uuid::Uuid::new_v4().to_string()}});
    helper
        .call("files_attach", Some(root), json!({"context":context}))
        .result
        .unwrap();
    let opened = helper
        .call(
            "files_open",
            Some(root),
            json!({"context":context,"request":{"path":file,"encoding":null}}),
        )
        .result
        .unwrap();
    assert_eq!(opened["text"], "original\n");
    let mut other = context.clone();
    other["revision"] = json!(2);
    assert_eq!(helper.call("files_sync_editor",Some(root),json!({"context":other,"path":opened["path"],"nativeRevision":opened["nativeRevision"],"text":"unsaved"})).result.unwrap_err(),"file_context_changed");
    helper.call("files_sync_editor",Some(root),json!({"context":context,"path":opened["path"],"nativeRevision":opened["nativeRevision"],"text":"unsaved"})).result.unwrap();
    helper.input.take();
    helper.exited(0);
    assert_eq!(std::fs::read(&file).unwrap(), b"original\r\n");
}
#[test]
fn actual_helper_rejects_a_replayed_pipe_sequence() {
    let mut helper = Helper::start();
    helper.call("hello", None, json!({})).result.unwrap();
    let request = Request {
        version: VERSION,
        session_id: helper.session.clone(),
        request_id: uuid::Uuid::new_v4().to_string(),
        sequence: helper.sequence,
        budget_ms: 5000,
        method: "hello".into(),
        root_token: None,
        args: json!({}),
    };
    workspace_wsl::write_frame(helper.input.as_mut().unwrap(), &request).unwrap();
    helper.exited(2);
    let closed = workspace_wsl::read_frame::<_, Response>(&mut helper.output);
    assert!(match closed {
        Ok(None) => true,
        Err(error) => error.kind() == io::ErrorKind::UnexpectedEof,
        Ok(Some(_)) => false,
    });
}
