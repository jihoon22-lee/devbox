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
    fn send(&mut self, method: &str, root: Option<&str>, args: Value) -> Request {
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
        request
    }
    fn call(&mut self, method: &str, root: Option<&str>, args: Value) -> Response {
        let request = self.send(method, root, args);
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

fn save_args(context: &Value, opened: &Value, text: &str) -> Value {
    json!({"context":context,"nativeRevision":opened["nativeRevision"],"request":{
        "path":opened["path"],"text":text,"encoding":opened["encoding"],"lineEnding":opened["lineEnding"],
        "expectedMtimeNanos":opened["mtimeNanos"],"expectedSize":opened["size"],"expectedContentHash":opened["contentHash"],"sourceLossy":false
    }})
}
#[test]
fn actual_helper_save_rotates_authority_and_eof_never_leaves_partial_file_contents() {
    let directory = tempfile::Builder::new()
        .prefix(".wsl-save-pipe-")
        .tempdir_in(env!("CARGO_MANIFEST_DIR"))
        .unwrap();
    let file = directory.path().join("source.txt");
    std::fs::write(&file, b"original\r\n").unwrap();
    let mut helper = Helper::start();
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
    let open_args = json!({"context":context,"request":{"path":file,"encoding":null}});
    let opened = helper
        .call("files_open", Some(root), open_args.clone())
        .result
        .unwrap();
    let save = save_args(&context, &opened, "saved\n");
    let saved = helper
        .call("files_save", Some(root), save.clone())
        .result
        .unwrap();
    assert_ne!(saved["nativeRevision"], opened["nativeRevision"]);
    assert_eq!(std::fs::read(&file).unwrap(), b"saved\r\n");
    assert_eq!(
        helper
            .call("files_save", Some(root), save)
            .result
            .unwrap_err(),
        "file_snapshot_changed"
    );
    let opened = helper
        .call("files_open", Some(root), open_args)
        .result
        .unwrap();
    let large = "x".repeat(8 * 1024 * 1024) + "\n";
    helper.send(
        "files_save",
        Some(root),
        save_args(&context, &opened, &large),
    );
    helper.input.take();
    helper.exited(0);
    let bytes = std::fs::read(&file).unwrap();
    assert!(bytes == b"saved\r\n" || bytes == large.replace('\n', "\r\n").as_bytes());
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn actual_helper_lists_and_mutates_only_its_opened_native_revision() {
    let directory = tempfile::Builder::new()
        .prefix(".wsl-actions-pipe-")
        .tempdir_in(env!("CARGO_MANIFEST_DIR"))
        .unwrap();
    let file = directory.path().join("source.txt");
    let sentinel = directory.path().join("existing.txt");
    std::fs::write(&file, b"original\r\n").unwrap();
    std::fs::write(&sentinel, b"preserved").unwrap();
    let mut helper = Helper::start();
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
    let listed = helper
        .call(
            "files_list",
            Some(root),
            json!({"context":context,"path":directory.path()}),
        )
        .result
        .unwrap();
    assert_eq!(listed["files"].as_array().unwrap().len(), 2);
    let opened = helper
        .call(
            "files_open",
            Some(root),
            json!({"context":context,"request":{"path":file,"encoding":null}}),
        )
        .result
        .unwrap();
    let mut rename = json!({"context":context,"nativeRevision":opened["nativeRevision"],"request":{
        "path":opened["path"],"expectedMtimeNanos":opened["mtimeNanos"],"expectedSize":opened["size"],"expectedContentHash":opened["contentHash"],"newName":"existing.txt"
    }});
    assert_eq!(
        helper
            .call("files_rename", Some(root), rename.clone())
            .result
            .unwrap_err(),
        "file_rename_conflict"
    );
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"preserved");
    rename["request"]["newName"] = json!("renamed 한글.md");
    let renamed = helper
        .call("files_rename", Some(root), rename)
        .result
        .unwrap();
    assert!(!file.exists());
    assert_eq!(
        std::fs::read(directory.path().join("renamed 한글.md")).unwrap(),
        b"original\r\n"
    );
    let preview = helper.call("files_preview", Some(root), json!({
        "context":context,"path":renamed["path"],"content":"# Native preview","workspaceRoot":directory.path()
    })).result.unwrap();
    assert_eq!(preview["kind"], "markdown");
    assert!(preview["html"].as_str().unwrap().contains("Native preview"));
    let mut delete = json!({"context":context,"nativeRevision":opened["nativeRevision"],"request":{
        "path":renamed["path"],"expectedMtimeNanos":renamed["mtimeNanos"],"expectedSize":renamed["size"],"expectedContentHash":renamed["contentHash"]
    }});
    assert_eq!(
        helper
            .call("files_delete", Some(root), delete.clone())
            .result
            .unwrap_err(),
        "file_snapshot_changed"
    );
    delete["nativeRevision"] = renamed["nativeRevision"].clone();
    helper
        .call("files_delete", Some(root), delete)
        .result
        .unwrap();
    helper.input.take();
    helper.exited(0);
    assert!(!directory.path().join("renamed 한글.md").exists());
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"preserved");
}
