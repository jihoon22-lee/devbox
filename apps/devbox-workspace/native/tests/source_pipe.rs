#![cfg(all(target_os = "linux", feature = "helper"))]
use serde_json::{json, Value};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};
use workspace_wsl::{
    control::{ControlInput, ControlOutput, Output},
    Request, Response, VERSION,
};

fn wait(child: &mut Child) {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if child.try_wait().unwrap().is_some() {
            return;
        }
        assert!(Instant::now() < until, "owned fixture child did not exit");
        thread::sleep(Duration::from_millis(5));
    }
}
fn git(root: &Path, args: &[&str]) -> String {
    let mut child = Command::new("/usr/bin/git")
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("HOME", root)
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_CONFIG_SYSTEM", root.join("empty.config"))
        .env("GIT_CONFIG_GLOBAL", root.join("empty.config"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait(&mut child);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().into()
}
struct Helper {
    child: Child,
    input: Option<ChildStdin>,
    frames: Option<Receiver<Result<Output, String>>>,
    reader: Option<thread::JoinHandle<()>>,
    session: String,
    sequence: u64,
    request_id: String,
    root: PathBuf,
    token: Option<String>,
    context: Value,
    digest: String,
    admissions: usize,
    allowed_roots: Vec<PathBuf>,
}
impl Helper {
    fn start(root: &Path) -> Self {
        Self::start_with_home(root, root)
    }
    fn start_with_home(root: &Path, environment_home: &Path) -> Self {
        let session = uuid::Uuid::new_v4().to_string();
        let mut child = Command::new(env!("CARGO_BIN_EXE_devbox-workspace-wsl"))
            .args(["--session", &session])
            .env_clear()
            .env("HOME", environment_home)
            .env("PATH", "/usr/bin:/bin")
            .env("GIT_CONFIG_SYSTEM", environment_home.join("empty.config"))
            .env("GIT_CONFIG_GLOBAL", environment_home.join("empty.config"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take();
        let mut output = child.stdout.take().unwrap();
        let (sender, frames) = mpsc::sync_channel(2);
        let reader = thread::spawn(move || loop {
            let frame = workspace_wsl::read_frame::<_, Output>(&mut output);
            let frame = match frame {
                Ok(Some(frame)) => Ok(frame),
                Ok(None) => Err("eof".into()),
                Err(error) => Err(error.to_string()),
            };
            let done = frame.is_err();
            if sender.send(frame).is_err() || done {
                break;
            }
        });
        let mut helper = Self {
            child,
            input,
            frames: Some(frames),
            reader: Some(reader),
            session,
            sequence: 0,
            request_id: String::new(),
            root: root.into(),
            token: None,
            context: json!({"projectId":"source-fixture","worktreeId":"source-worktree","revision":1,"target":{"kind":"wsl","distroId":uuid::Uuid::new_v4().to_string()}}),
            digest: String::new(),
            admissions: 0,
            allowed_roots: vec![root.into()],
        };
        helper.send("observe_root", json!({"path":root}));
        let report = helper.complete(true).result.unwrap();
        helper.token = Some(report["token"].as_str().unwrap().into());
        helper.send("source_capture", json!({"context":helper.context}));
        let report = helper.complete(true).result.unwrap();
        helper.digest = report["digest"].as_str().unwrap().into();
        helper.send("execution_prepare", json!({}));
        assert_eq!(
            helper.complete(true).result.unwrap(),
            json!({"retirement":true})
        );
        helper
    }
    fn send(&mut self, method: &str, args: Value) {
        if !matches!(
            method,
            "observe_root" | "execution_prepare" | "source_execute"
        ) {
            assert!(
                workspace_wsl::control::project_method(method),
                "Windows project gate rejected {method}"
            );
        }
        self.sequence += 1;
        self.request_id = uuid::Uuid::new_v4().to_string();
        workspace_wsl::write_frame(
            self.input.as_mut().unwrap(),
            &Request {
                version: VERSION,
                session_id: self.session.clone(),
                request_id: self.request_id.clone(),
                sequence: self.sequence,
                budget_ms: 15000,
                method: method.into(),
                root_token: if method == "execution_prepare" {
                    None
                } else {
                    self.token.clone()
                },
                args,
            },
        )
        .unwrap();
    }
    fn execute(&mut self, method: &str, args: Value) {
        self.send(
            "source_execute",
            json!({"context":self.context,"digest":self.digest,"method":method,"args":args}),
        );
    }
    fn reply(&mut self, frame: ControlOutput, approved: bool) {
        let ControlOutput::Admission {
            version,
            session_id,
            request_id,
            sequence,
            admission_id,
            target_root,
        } = frame
        else {
            panic!("unexpected retirement");
        };
        assert_eq!(version, VERSION);
        assert_eq!(session_id, self.session);
        assert_eq!(request_id, self.request_id);
        assert_eq!(sequence, self.sequence);
        assert!(self
            .allowed_roots
            .iter()
            .any(|root| root.to_string_lossy() == target_root));
        assert!(workspace_wsl::token(&admission_id));
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
    fn complete(&mut self, approved: bool) -> Response {
        loop {
            match self
                .frames
                .as_ref()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .expect("native reply deadline")
                .unwrap()
            {
                Output::Control(frame) => self.reply(frame, approved),
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
    fn retire(&mut self) {
        self.input.take();
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            assert!(Instant::now() < until, "native retirement deadline");
            match self
                .frames
                .as_ref()
                .unwrap()
                .recv_timeout(Duration::from_millis(100))
            {
                Ok(Ok(Output::Control(ControlOutput::Retired {
                    version,
                    session_id,
                    sequence,
                }))) => {
                    assert_eq!(version, VERSION);
                    assert_eq!(session_id, self.session);
                    assert_eq!(sequence, self.sequence);
                    break;
                }
                Ok(Ok(Output::Response(_))) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                other => panic!("missing retirement proof: {other:?}"),
            }
        }
        wait(&mut self.child);
        assert!(self.child.try_wait().unwrap().unwrap().success());
    }
}
impl Drop for Helper {
    fn drop(&mut self) {
        self.input.take();
        let until = Instant::now() + Duration::from_secs(5);
        while self.child.try_wait().ok().flatten().is_none() && Instant::now() < until {
            thread::sleep(Duration::from_millis(5));
        }
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        self.frames.take();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
fn fixture() -> tempfile::TempDir {
    let root = tempfile::Builder::new()
        .prefix(".source 한글 pipe-")
        .tempdir_in(env!("CARGO_MANIFEST_DIR"))
        .unwrap();
    fs::write(root.path().join("empty.config"), b"").unwrap();
    git(root.path(), &["init", "--quiet", "--initial-branch=main"]);
    git(root.path(), &["config", "user.name", "Synthetic Fixture"]);
    git(
        root.path(),
        &["config", "user.email", "fixture@example.test"],
    );
    fs::write(root.path().join("tracked.txt"), b"original\n").unwrap();
    git(root.path(), &["add", "--", "tracked.txt"]);
    git(root.path(), &["commit", "--quiet", "-m", "initial fixture"]);
    root
}
#[test]
fn source_runs_selected_stage_and_commit_only_after_native_per_command_approval() {
    let fixture = fixture();
    let root = fixture.path();
    let mut helper = Helper::start(root);
    fs::write(root.join("tracked.txt"), b"selected change\n").unwrap();
    fs::write(root.join("unselected.txt"), b"preserved\n").unwrap();
    helper.execute("repo_changes", json!({"request":{"path":root}}));
    helper.complete(true).result.unwrap();
    helper.execute(
        "repo_stage",
        json!({"request":{"path":root,"paths":["tracked.txt"],"operationId":"fixture-stage"}}),
    );
    helper.complete(true).result.unwrap();
    assert_eq!(
        git(root, &["diff", "--cached", "--name-only"]),
        "tracked.txt"
    );
    helper.execute("repo_commit",json!({"request":{"path":root,"message":"reviewed native commit","operationId":"fixture-commit"}}));
    helper.complete(true).result.unwrap();
    assert_eq!(
        git(root, &["log", "-1", "--format=%s"]),
        "reviewed native commit"
    );
    assert_eq!(git(root, &["show", "HEAD:tracked.txt"]), "selected change");
    assert_eq!(
        fs::read(root.join("unselected.txt")).unwrap(),
        b"preserved\n"
    );
    assert!(helper.admissions >= 3);
    helper.retire();
}
#[test]
fn denied_authority_and_unknown_methods_never_launch_a_commit_hook() {
    let fixture = fixture();
    let root = fixture.path();
    let hook = root.join(".git/hooks/pre-commit");
    fs::write(&hook, b"#!/bin/sh\ntouch must-not-exist\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(root.join("tracked.txt"), b"pending\n").unwrap();
    git(root, &["add", "--", "tracked.txt"]);
    let before = git(root, &["rev-parse", "HEAD"]);
    let mut helper = Helper::start(root);
    helper.execute("scan_root", json!({"path":root}));
    assert!(helper.complete(true).result.is_err());
    assert_eq!(helper.admissions, 0);
    helper.execute(
        "repo_commit",
        json!({"request":{"path":root,"message":"denied commit","operationId":"denied"}}),
    );
    assert!(helper.complete(false).result.is_err());
    assert!(!root.join("must-not-exist").exists());
    assert_eq!(git(root, &["rev-parse", "HEAD"]), before);
    helper.retire();
}
fn birth(pid: &str) -> Option<String> {
    fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()?
        .rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(19)
        .map(str::to_owned)
}
#[test]
fn eof_retires_git_and_detached_hook_children_before_acknowledging_completion() {
    let fixture = fixture();
    let root = fixture.path();
    let hook = root.join(".git/hooks/pre-commit");
    fs::write(&hook,b"#!/bin/sh\nprintf '%s\\n' \"$$\" > hook.pid\n/usr/bin/setsid /usr/bin/sleep 30 &\nprintf '%s\\n' \"$!\" > detached.pid\nwhile :; do /usr/bin/sleep 1; done\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(root.join("tracked.txt"), b"cancelled commit\n").unwrap();
    git(root, &["add", "--", "tracked.txt"]);
    let before = git(root, &["rev-parse", "HEAD"]);
    let mut helper = Helper::start(root);
    helper.execute(
        "repo_commit",
        json!({"request":{"path":root,"message":"cancelled commit","operationId":"cancel-owned"}}),
    );
    let until = Instant::now() + Duration::from_secs(5);
    while !root.join("detached.pid").exists() {
        assert!(Instant::now() < until, "owned Git hook did not start");
        match helper
            .frames
            .as_ref()
            .unwrap()
            .recv_timeout(Duration::from_millis(10))
        {
            Ok(Ok(Output::Control(frame))) => helper.reply(frame, true),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            other => panic!("unexpected commit completion: {other:?}"),
        }
    }
    let ids = ["hook.pid", "detached.pid"].map(|name| {
        fs::read_to_string(root.join(name))
            .unwrap()
            .trim()
            .to_owned()
    });
    let births = ids
        .each_ref()
        .map(|pid| birth(pid).expect("owned child identity"));
    helper.retire();
    for (pid, previous) in ids.iter().zip(births) {
        assert_ne!(
            birth(pid),
            Some(previous),
            "owned child survived retirement"
        );
    }
    assert_eq!(git(root, &["rev-parse", "HEAD"]), before);
}

#[test]
fn a_partial_command_after_preparation_still_confirms_no_child_execution() {
    use std::io::Write;
    let fixture = fixture();
    let root = fixture.path();
    let before = git(root, &["rev-parse", "HEAD"]);
    let mut helper = Helper::start(root);
    let request = Request {
        version: VERSION,
        session_id: helper.session.clone(),
        request_id: uuid::Uuid::new_v4().to_string(),
        sequence: helper.sequence + 1,
        budget_ms: 5000,
        method: "source_execute".into(),
        root_token: helper.token.clone(),
        args: json!({"context":helper.context,"digest":helper.digest,"method":"repo_commit","args":{"request":{"path":root,"message":"must not commit","operationId":"partial"}}}),
    };
    let mut frame = Vec::new();
    workspace_wsl::write_frame(&mut frame, &request).unwrap();
    helper
        .input
        .as_mut()
        .unwrap()
        .write_all(&frame[..frame.len() / 2])
        .unwrap();
    helper.input.take();
    let packet = helper
        .frames
        .as_ref()
        .unwrap()
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    let Output::Control(ControlOutput::Retired {
        version,
        session_id,
        sequence,
    }) = packet
    else {
        panic!("missing partial-frame retirement")
    };
    assert_eq!(version, VERSION);
    assert_eq!(session_id, helper.session);
    assert_eq!(sequence, helper.sequence);
    wait(&mut helper.child);
    assert_eq!(helper.child.try_wait().unwrap().unwrap().code(), Some(2));
    assert_eq!(git(root, &["rev-parse", "HEAD"]), before);
}

#[test]
fn a_forged_admission_ticket_retires_without_starting_the_commit() {
    let fixture = fixture();
    let root = fixture.path();
    let hook = root.join(".git/hooks/pre-commit");
    fs::write(&hook, b"#!/bin/sh\ntouch must-not-exist\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(root.join("tracked.txt"), b"pending\n").unwrap();
    git(root, &["add", "--", "tracked.txt"]);
    let before = git(root, &["rev-parse", "HEAD"]);
    let mut helper = Helper::start(root);
    helper.execute(
        "repo_commit",
        json!({"request":{"path":root,"message":"forged approval","operationId":"forged"}}),
    );
    let frame = helper
        .frames
        .as_ref()
        .unwrap()
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    let Output::Control(ControlOutput::Admission {
        version,
        session_id,
        request_id,
        sequence,
        admission_id,
        ..
    }) = frame
    else {
        panic!("missing native admission");
    };
    let forged = uuid::Uuid::new_v4().to_string();
    assert_ne!(forged, admission_id);
    workspace_wsl::write_frame(
        helper.input.as_mut().unwrap(),
        &ControlInput::AdmissionReply {
            version,
            session_id,
            request_id,
            sequence,
            admission_id: forged,
            approved: true,
        },
    )
    .unwrap();
    loop {
        match helper
            .frames
            .as_ref()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap()
        {
            Output::Response(response) => assert!(response.result.is_err()),
            Output::Control(ControlOutput::Retired {
                version,
                session_id,
                sequence,
            }) => {
                assert_eq!(version, VERSION);
                assert_eq!(session_id, helper.session);
                assert_eq!(sequence, helper.sequence);
                break;
            }
            other => panic!("unexpected admission after forged ticket: {other:?}"),
        }
    }
    wait(&mut helper.child);
    assert_eq!(helper.child.try_wait().unwrap().unwrap().code(), Some(2));
    assert!(!root.join("must-not-exist").exists());
    assert_eq!(git(root, &["rev-parse", "HEAD"]), before);
}

fn worktree_preview(helper: &mut Helper, branch: &str, target: &Path) -> Value {
    helper.send(
        "source_worktree_preview",
        json!({"context":helper.context,"digest":helper.digest,"branch":branch,"targetDir":target}),
    );
    helper.complete(true).result.unwrap()
}
#[test]
fn worktree_creation_consumes_native_destination_review_and_preserves_other_files() {
    let fixture = fixture();
    let root = fixture.path();
    let target = root.join("linked 한글 folder");
    let mut helper = Helper::start(root);
    let preview = worktree_preview(&mut helper, "reviewed-branch", &target);
    assert!(!target.exists());
    assert_eq!(helper.admissions, 0);
    fs::write(root.join("unselected.txt"), b"preserved\n").unwrap();
    helper.execute(
        "create_worktree",
        json!({"previewId":preview["previewId"],"operationId":"create-reviewed"}),
    );
    assert_eq!(
        helper.complete(true).result.unwrap()["path"],
        target.to_string_lossy().as_ref()
    );
    assert_eq!(
        git(&target, &["branch", "--show-current"]),
        "reviewed-branch"
    );
    assert_eq!(fs::read(target.join("tracked.txt")).unwrap(), b"original\n");
    assert_eq!(
        fs::read(root.join("unselected.txt")).unwrap(),
        b"preserved\n"
    );
    let admissions = helper.admissions;
    helper.execute(
        "create_worktree",
        json!({"previewId":preview["previewId"],"operationId":"replay-reviewed"}),
    );
    assert!(helper.complete(true).result.is_err());
    assert_eq!(helper.admissions, admissions);
    helper.retire();
}
#[test]
fn worktree_review_cannot_overwrite_a_destination_created_during_approval() {
    let fixture = fixture();
    let root = fixture.path();
    let target = root.join("concurrent destination");
    let mut helper = Helper::start(root);
    let preview = worktree_preview(&mut helper, "must-not-exist", &target);
    helper.execute(
        "create_worktree",
        json!({"previewId":preview["previewId"],"operationId":"concurrent-create"}),
    );
    let packet = helper
        .frames
        .as_ref()
        .unwrap()
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    let Output::Control(frame) = packet else {
        panic!("missing pre-command approval");
    };
    fs::create_dir(&target).unwrap();
    fs::write(target.join("owned.txt"), b"concurrent owner\n").unwrap();
    helper.reply(frame, true);
    assert!(helper.complete(true).result.is_err());
    assert_eq!(
        fs::read(target.join("owned.txt")).unwrap(),
        b"concurrent owner\n"
    );
    assert!(!target.join(".git").exists());
    assert_eq!(git(root, &["branch", "--list", "must-not-exist"]), "");
    helper.retire();
}
#[test]
fn a_wrong_worktree_token_cannot_be_retried_with_the_original_token() {
    let fixture = fixture();
    let root = fixture.path();
    let target = root.join("uncreated");
    let mut helper = Helper::start(root);
    let preview = worktree_preview(&mut helper, "wrong-token", &target);
    for id in [
        Value::String(uuid::Uuid::new_v4().to_string()),
        preview["previewId"].clone(),
    ] {
        helper.execute(
            "create_worktree",
            json!({"previewId":id,"operationId":"wrong-preview"}),
        );
        assert!(helper.complete(true).result.is_err());
    }
    assert!(!target.exists());
    assert_eq!(helper.admissions, 0);
    helper.retire();
}

fn cleanup_member(helper: &Helper, linked: &Path) -> Value {
    let mut sibling = Helper::start_with_home(linked, &helper.root);
    let mut context = helper.context.clone();
    context["worktreeId"] = json!("approved-sibling");
    let member = json!({"context":context,"root":linked,"digest":sibling.digest});
    sibling.retire();
    member
}
fn cleanup_execute(helper: &mut Helper, method: &str, args: Value, members: Value) {
    helper.send(
        "source_execute",
        json!({"context":helper.context,"digest":helper.digest,
        "method":method,"args":args,"members":members}),
    );
}
#[test]
fn cleanup_admits_only_explicit_native_siblings_and_removes_the_reviewed_worktree() {
    let fixture = fixture();
    let root = fixture.path();
    let linked = root.join("linked 한글 cleanup");
    git(
        root,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "merged-linked",
            linked.to_str().unwrap(),
        ],
    );
    let mut helper = Helper::start(root);
    let member = cleanup_member(&helper, &linked);
    let args = json!({"request":{"path":root,"operationId":"scope-preview"}});
    helper.execute("repo_cleanup_preview", args.clone());
    let denied = helper.complete(true).result.unwrap();
    let entry = denied["worktrees"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["path"] == linked.to_str().unwrap())
        .unwrap();
    assert_eq!(entry["eligible"], false);
    helper.allowed_roots.push(linked.clone());
    cleanup_execute(&mut helper, "repo_cleanup_preview", args, json!([member]));
    let preview = helper.complete(true).result.unwrap();
    let entry = preview["worktrees"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["path"] == linked.to_str().unwrap())
        .unwrap();
    assert_eq!(entry["eligible"], true, "{preview}");
    cleanup_execute(
        &mut helper,
        "repo_cleanup",
        json!({"request":{"path":root,"operationId":"scope-remove",
        "previewRevision":preview["revision"],"branchNames":[],"worktreePaths":[linked]}}),
        json!([member]),
    );
    let removed = helper.complete(true).result.unwrap();
    assert_eq!(removed["removed"], 1, "{removed}");
    assert!(!linked.exists());
    assert_eq!(fs::read(root.join("tracked.txt")).unwrap(), b"original\n");
    assert!(!git(root, &["branch", "--list", "merged-linked"]).is_empty());
    helper.retire();
}
#[test]
fn cleanup_rejects_foreign_duplicate_and_changed_evidence_before_git_admission() {
    let fixture = fixture();
    let root = fixture.path();
    let linked = root.join("linked");
    git(
        root,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "linked",
            linked.to_str().unwrap(),
        ],
    );
    let mut helper = Helper::start(root);
    let member = cleanup_member(&helper, &linked);
    let mut foreign = member.clone();
    foreign["context"]["projectId"] = json!("foreign");
    let mut changed = member.clone();
    changed["digest"] = json!("0".repeat(64));
    let other = self::fixture();
    let mut foreign_repository = member.clone();
    foreign_repository["root"] = json!(other.path());
    for members in [
        json!([foreign]),
        json!([foreign_repository]),
        json!([member.clone(), member.clone()]),
        json!([changed]),
    ] {
        cleanup_execute(
            &mut helper,
            "repo_cleanup_preview",
            json!({"request":{"path":root,"operationId":"rejected-scope"}}),
            members,
        );
        assert!(helper.complete(true).result.is_err());
        assert_eq!(helper.admissions, 0);
        assert!(linked.join("tracked.txt").is_file());
    }
    helper.retire();
}
#[test]
fn cleanup_revocation_after_native_preparation_preserves_the_linked_worktree() {
    let fixture = fixture();
    let root = fixture.path();
    let linked = root.join("linked");
    git(
        root,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "linked",
            linked.to_str().unwrap(),
        ],
    );
    let mut helper = Helper::start(root);
    helper.allowed_roots.push(linked.clone());
    let member = cleanup_member(&helper, &linked);
    cleanup_execute(
        &mut helper,
        "repo_cleanup_preview",
        json!({"request":{"path":root,"operationId":"revoked-preview"}}),
        json!([member]),
    );
    let preview = helper.complete(true).result.unwrap();
    cleanup_execute(
        &mut helper,
        "repo_cleanup",
        json!({"request":{"path":root,"operationId":"revoked-remove",
        "previewRevision":preview["revision"],"branchNames":[],"worktreePaths":[linked]}}),
        json!([member]),
    );
    assert!(helper.complete(false).result.is_err());
    assert!(linked.join("tracked.txt").is_file());
    helper.retire();
}

#[test]
fn dependency_inventory_uses_native_files_and_context_without_executing_tools() {
    let fixture = fixture();
    let root = fixture.path();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"local\"\nversion = \"0.1.0\"\n[dependencies]\nfixture = \"1\"\n",
    )
    .unwrap();
    let lock = "version = 3\n[[package]]\nname = \"fixture\"\nversion = \"1.0.0\"\ndependencies = [\"nested 2.0.0\"]\nsource = \"registry+https://private.invalid/synthetic-marker\"\n[[package]]\nname = \"nested\"\nversion = \"2.0.0\"\n";
    fs::write(root.join("Cargo.lock"), lock).unwrap();
    fs::write(
        root.join("build.rs"),
        "fn main() { panic!(\"must not execute\"); }",
    )
    .unwrap();
    let mut helper = Helper::start(root);
    helper.send("files_attach", json!({"context":helper.context}));
    helper.complete(true).result.unwrap();
    helper.send(
        "dependency_inventory",
        json!({"context":helper.context,"budgetMs":10000}),
    );
    let report = helper.complete(true).result.unwrap();
    assert_eq!(report["packageCount"], 2);
    assert_eq!(report["directCount"], 1);
    assert_eq!(report["transitiveCount"], 1);
    assert_eq!(report["summaryPublished"], false);
    assert!(!report.to_string().contains("synthetic-marker"));
    assert!(!report.to_string().contains(root.to_str().unwrap()));
    let mut stale = helper.context.clone();
    stale["revision"] = json!(2);
    helper.send(
        "dependency_inventory",
        json!({"context":stale,"budgetMs":10000}),
    );
    assert_eq!(
        helper.complete(true).result.unwrap_err(),
        "dependency_context_changed"
    );
    fs::write(
        root.join("Cargo.lock"),
        format!("{lock}\n# changed reviewed input\n"),
    )
    .unwrap();
    helper.send(
        "dependency_inventory",
        json!({"context":helper.context,"budgetMs":10000}),
    );
    let changed = helper.complete(true).result.unwrap();
    assert_ne!(changed["revision"], report["revision"]);
    assert_eq!(helper.admissions, 0);
    assert!(!root.join("target").exists());
    helper.retire();
}

#[test]
fn selected_stage_survives_an_unselected_nested_worktree() {
    let fixture = fixture();
    let root = fixture.path();
    let nested = root.join("nested 한글 tree");
    git(
        root,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "nested",
            nested.to_str().unwrap(),
        ],
    );
    fs::write(root.join("tracked.txt"), b"selected main change\n").unwrap();
    let status = git(
        root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    );
    assert!(status.contains("?? nested 한글 tree/\0"));
    let mut helper = Helper::start(root);
    helper.execute(
        "repo_stage",
        json!({"request":{"path":root,"paths":["tracked.txt"],"operationId":"stage-after-nested"}}),
    );
    helper.complete(true).result.unwrap();
    assert_eq!(
        git(root, &["diff", "--cached", "--name-only"]),
        "tracked.txt"
    );
    for (index, path) in ["nested 한글 tree", "nested 한글 tree/"]
        .into_iter()
        .enumerate()
    {
        helper.execute(
            "repo_stage",
            json!({"request":{"path":root,"paths":[path],"operationId":format!("reject-directory-stage-{index}")}}),
        );
        assert!(helper.complete(true).result.is_err());
    }
    assert_eq!(
        git(root, &["diff", "--cached", "--name-only"]),
        "tracked.txt"
    );
    assert_eq!(fs::read(nested.join("tracked.txt")).unwrap(), b"original\n");
    helper.retire();
}
