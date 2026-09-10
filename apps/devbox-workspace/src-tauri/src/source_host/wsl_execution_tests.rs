//! Actual hosted Windows -> packaged native WSL Git execution. Every source,
//! script, process and private store belongs to this disposable run fixture.
use super::*;
use crate::{
    core::context_activity::ContextActivity, core::source_operations::Operations,
    files_host::FilesHost, project_owner::RegistrationAction,
};
use std::{
    fs,
    path::Path,
    process::{Command, Output, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    thread,
};

struct Fixture {
    host: Arc<Host>,
    _store: tempfile::TempDir,
    name: String,
    root: String,
    unc: PathBuf,
    context: ProjectContext,
    source: SourceHost,
    definitions: Definitions,
    files: Arc<Mutex<FilesHost>>,
    operations: Operations,
    activity: ContextActivity,
}
impl Fixture {
    fn new() -> Self {
        assert_eq!(std::env::var("GITHUB_ACTIONS").unwrap(), "true");
        assert_eq!(
            std::env::var("RUNNER_ENVIRONMENT").unwrap(),
            "github-hosted"
        );
        let name = std::env::var("DEVBOX_KNOWLEDGE_WSL_DISTRO").unwrap();
        let run = std::env::var("GITHUB_RUN_ID").unwrap();
        assert!(name.starts_with(&format!("DevboxKnowledgeFixture-{run}-")));
        let distro = crate::platform::wsl_distro::list()
            .unwrap()
            .into_iter()
            .find(|d| d.name == name)
            .unwrap();
        let nonce = uuid::Uuid::new_v4();
        let root = format!("/home/devbox-fixture/source execution 한글 {nonce}");
        let unc = PathBuf::from(format!(
            r"\\wsl.localhost\{name}\home\devbox-fixture\source execution 한글 {nonce}"
        ));
        fs::create_dir(&unc).unwrap();
        fs::write(unc.join("empty.config"), b"").unwrap();
        let store = tempfile::tempdir().unwrap();
        let host = Arc::new(
            Host::open_with_resources(store.path(), Path::new(env!("CARGO_MANIFEST_DIR")).into())
                .unwrap(),
        );
        host.start_empty().unwrap();
        let raw = |args: &[&str]| Self::linux(&name, args);
        assert!(raw(&[
            "/usr/bin/git",
            "-C",
            &root,
            "init",
            "--quiet",
            "--initial-branch=main"
        ])
        .status
        .success());
        assert!(raw(&[
            "/usr/bin/git",
            "-C",
            &root,
            "config",
            "user.name",
            "Synthetic Fixture"
        ])
        .status
        .success());
        assert!(raw(&[
            "/usr/bin/git",
            "-C",
            &root,
            "config",
            "user.email",
            "fixture@example.test"
        ])
        .status
        .success());
        fs::write(unc.join("tracked.txt"), b"original\n").unwrap();
        assert!(
            raw(&["/usr/bin/git", "-C", &root, "add", "--", "tracked.txt"])
                .status
                .success()
        );
        assert!(raw(&[
            "/usr/bin/git",
            "-C",
            &root,
            "commit",
            "--quiet",
            "-m",
            "initial fixture"
        ])
        .status
        .success());
        let projects = host.projects().unwrap();
        let proposed = projects
            .preview_wsl(host.helper_directory().unwrap(), &distro.id, &root, false)
            .unwrap();
        let (_, context) = projects
            .apply(
                &proposed.preview_id,
                "Native Source execution",
                RegistrationAction::Register,
            )
            .unwrap();
        Self {
            host,
            _store: store,
            name,
            root,
            unc,
            context,
            source: SourceHost::default(),
            definitions: Definitions::default(),
            files: Arc::new(Mutex::new(FilesHost::default())),
            operations: Operations::default(),
            activity: ContextActivity::default(),
        }
    }
    fn linux(name: &str, args: &[&str]) -> Output {
        use std::os::windows::process::CommandExt;
        let executable = crate::platform::wsl_distro::executable().unwrap();
        let system = executable.parent().unwrap();
        let mut child = Command::new(&executable)
            .args([
                "--distribution",
                name,
                "--user",
                "root",
                "--cd",
                "/",
                "--exec",
            ])
            .args(args)
            .current_dir(system)
            .env_clear()
            .env("SystemRoot", system.parent().unwrap())
            .env("PATH", system)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(0x08000000)
            .spawn()
            .unwrap();
        let until = Instant::now() + Duration::from_secs(15);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= until {
                let _ = child.kill();
                let _ = child.wait();
                panic!("owned Linux fixture setup timed out");
            }
            thread::sleep(Duration::from_millis(10));
        }
        child.wait_with_output().unwrap()
    }
    fn git(&self, args: &[&str]) -> String {
        let mut command = vec!["/usr/bin/git", "-C", self.root.as_str()];
        command.extend_from_slice(args);
        let output = Self::linux(&self.name, &command);
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap().trim().into()
    }
    fn budget() -> Budget {
        Budget {
            deadline_ms: u64::MAX,
            expires: Instant::now() + Duration::from_secs(29),
        }
    }
    fn approve(&mut self) {
        let preview = self
            .source
            .manage(
                &self.host,
                &mut self.definitions,
                &self.context,
                "preview_trust",
                json!({}),
                Self::budget(),
            )
            .unwrap();
        self.source
            .manage(
                &self.host,
                &mut self.definitions,
                &self.context,
                "approve_trust",
                json!({"previewId":preview["previewId"]}),
                Self::budget(),
            )
            .unwrap();
    }
    fn prepare<T: Send + Sync + 'static>(
        &mut self,
        method: &str,
        args: Value,
        id: &str,
        retained: T,
    ) -> PreparedSource {
        let key = serde_json::to_string(&self.context).unwrap();
        let admitted = self.operations.register(&key, Some(id)).unwrap();
        self.source
            .access(
                self.host.clone(),
                &mut self.definitions,
                Invocation {
                    files: self.files.clone(),
                    context: self.context.clone(),
                    method: method.into(),
                    args,
                    budget: Self::budget(),
                    admitted: admitted.clone(),
                },
                (self.activity.enter(true).unwrap(), admitted, retained),
            )
            .unwrap()
    }
    fn execute(&mut self, method: &str, args: Value, id: &str) -> Value {
        let prepared = self.prepare(method, args, id, ());
        let ReadySource::Complete(value) = prepared.finish_on_worker().unwrap() else {
            panic!("WSL used Windows Git")
        };
        value
    }
}
#[test]
#[ignore = "requires the current run-owned hosted WSL distro, Git and packaged helper"]
fn owned_source_stage_commit_revocation_and_cancellation_keep_native_ownership() {
    let mut fixture = Fixture::new();
    fixture.approve();
    fs::write(fixture.unc.join("tracked.txt"), b"selected native change\n").unwrap();
    fs::write(
        fixture.unc.join("unselected.txt"),
        b"preserved unselected\n",
    )
    .unwrap();
    let root = fixture.root.clone();
    fixture.execute(
        "repo_stage",
        json!({"request":{"path":root,"paths":["tracked.txt"],"operationId":"owned-stage"}}),
        "owned-stage",
    );
    assert_eq!(
        fixture.git(&["diff", "--cached", "--name-only"]),
        "tracked.txt"
    );
    fixture.execute("repo_commit",json!({"request":{"path":root,"message":"native selected commit","operationId":"owned-commit"}}),"owned-commit");
    assert_eq!(
        fixture.git(&["log", "-1", "--format=%s"]),
        "native selected commit"
    );
    assert_eq!(
        fixture.git(&["show", "HEAD:tracked.txt"]),
        "selected native change"
    );
    assert_eq!(
        fs::read(fixture.unc.join("unselected.txt")).unwrap(),
        b"preserved unselected\n"
    );
    let linked = format!("{root}/linked 한글 worktree");
    let preview = fixture
        .source
        .manage(
            &fixture.host,
            &mut fixture.definitions,
            &fixture.context,
            "preview_worktree",
            json!({"branch":"native-linked","targetDir":linked}),
            Fixture::budget(),
        )
        .unwrap();
    let linked_unc = fixture.unc.join("linked 한글 worktree");
    assert!(!linked_unc.exists());
    let created = fixture.execute(
        "create_worktree",
        json!({"previewId":preview["previewId"],"operationId":"owned-create"}),
        "owned-create",
    );
    assert_eq!(created["path"], linked);
    assert_eq!(
        fs::read(linked_unc.join("tracked.txt")).unwrap(),
        b"selected native change\n"
    );
    let projects = fixture.host.projects().unwrap();
    let product_contract::ExecutionTarget::Wsl { distro_id } = &fixture.context.target else {
        panic!("expected WSL target");
    };
    let proposal = projects
        .preview_wsl(
            fixture.host.helper_directory().unwrap(),
            distro_id,
            &linked,
            false,
        )
        .unwrap();
    let (_, linked_context) = projects
        .apply(
            &proposal.preview_id,
            "Native linked worktree",
            RegistrationAction::Register,
        )
        .unwrap();
    assert_eq!(linked_context.project_id, fixture.context.project_id);
    assert_ne!(linked_context.worktree_id, fixture.context.worktree_id);
    assert_eq!(linked_context.target, fixture.context.target);
    let original_context = std::mem::replace(&mut fixture.context, linked_context);
    let original_root = std::mem::replace(&mut fixture.root, linked.clone());
    let original_unc = std::mem::replace(&mut fixture.unc, linked_unc);
    fixture.approve();
    fs::write(fixture.unc.join("tracked.txt"), b"linked context change\n").unwrap();
    fixture.execute(
        "repo_stage",
        json!({"request":{"path":linked,"paths":["tracked.txt"],"operationId":"linked-stage"}}),
        "linked-stage",
    );
    fixture.execute("repo_commit", json!({"request":{"path":linked,"message":"linked native commit","operationId":"linked-commit"}}), "linked-commit");
    assert_eq!(fixture.git(&["branch", "--show-current"]), "native-linked");
    assert_eq!(
        fixture.git(&["show", "HEAD:tracked.txt"]),
        "linked context change"
    );
    fixture.context = original_context;
    fixture.root = original_root;
    fixture.unc = original_unc;
    assert_eq!(
        fixture.git(&["show", "HEAD:tracked.txt"]),
        "selected native change"
    );
    let cancelled_path = format!("{root}/cancelled worktree");
    let preview = fixture
        .source
        .manage(
            &fixture.host,
            &mut fixture.definitions,
            &fixture.context,
            "preview_worktree",
            json!({"branch":"cancelled-preview","targetDir":cancelled_path}),
            Fixture::budget(),
        )
        .unwrap();
    fixture
        .source
        .manage(
            &fixture.host,
            &mut fixture.definitions,
            &fixture.context,
            "cancel_worktree",
            json!({"previewId":preview["previewId"]}),
            Fixture::budget(),
        )
        .unwrap();
    assert!(!fixture.unc.join("cancelled worktree").exists());
    assert_eq!(fixture.git(&["branch", "--list", "cancelled-preview"]), "");
    fs::write(fixture.unc.join("tracked.txt"), b"revoked pending change\n").unwrap();
    let prepared = fixture.prepare(
        "repo_stage",
        json!({"request":{"path":root,"paths":["tracked.txt"],"operationId":"revoked-stage"}}),
        "revoked-stage",
        (),
    );
    fixture
        .source
        .manage(
            &fixture.host,
            &mut fixture.definitions,
            &fixture.context,
            "revoke_trust",
            json!({}),
            Fixture::budget(),
        )
        .unwrap();
    assert!(prepared.finish_on_worker().is_err());
    assert_eq!(fixture.git(&["diff", "--cached", "--name-only"]), "");
    let hook = fixture.unc.join(".git/hooks/pre-commit");
    fs::write(&hook,b"#!/bin/sh\nprintf '%s\\n' \"$$\" > hook.pid\n/usr/bin/setsid /usr/bin/sleep 30 &\nprintf '%s\\n' \"$!\" > detached.pid\nwhile :; do /usr/bin/sleep 1; done\n").unwrap();
    assert!(Fixture::linux(
        &fixture.name,
        &[
            "/usr/bin/chmod",
            "700",
            &format!("{root}/.git/hooks/pre-commit")
        ]
    )
    .status
    .success());
    fixture.approve();
    fixture.execute(
        "repo_stage",
        json!({"request":{"path":root,"paths":["tracked.txt"],"operationId":"cancel-stage"}}),
        "cancel-stage",
    );
    let previous = fixture.git(&["rev-parse", "HEAD"]);
    struct Retained(Arc<AtomicBool>);
    impl Drop for Retained {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    let dropped = Arc::new(AtomicBool::new(false));
    let prepared = fixture.prepare(
        "repo_commit",
        json!({"request":{"path":root,"message":"must cancel","operationId":"owned-cancel"}}),
        "owned-cancel",
        Retained(dropped.clone()),
    );
    let worker = thread::spawn(move || prepared.finish_on_worker());
    let until = Instant::now() + Duration::from_secs(15);
    while !fixture.unc.join("detached.pid").exists() {
        assert!(Instant::now() < until, "owned pre-commit did not start");
        assert!(
            !worker.is_finished(),
            "commit completed before the owned hook barrier"
        );
        thread::sleep(Duration::from_millis(20));
    }
    assert!(!dropped.load(Ordering::Acquire));
    assert!(fixture.activity.enter(true).is_err());
    let key = serde_json::to_string(&fixture.context).unwrap();
    assert!(fixture.operations.cancel(&key, "owned-cancel").unwrap());
    let until = Instant::now() + Duration::from_secs(15);
    while !worker.is_finished() {
        assert!(Instant::now() < until, "WSL Source retirement unconfirmed");
        thread::sleep(Duration::from_millis(20));
    }
    assert!(worker.join().unwrap().is_err());
    assert!(dropped.load(Ordering::Acquire));
    assert!(fixture.activity.enter(true).is_ok());
    for marker in ["hook.pid", "detached.pid"] {
        let pid = fs::read_to_string(fixture.unc.join(marker)).unwrap();
        let pid = pid.trim();
        assert!(pid.bytes().all(|b| b.is_ascii_digit()));
        assert!(
            !Fixture::linux(
                &fixture.name,
                &["/usr/bin/test", "-e", &format!("/proc/{pid}")]
            )
            .status
            .success(),
            "owned hook child survived confirmed retirement"
        );
    }
    assert_eq!(fixture.git(&["rev-parse", "HEAD"]), previous);
    assert_eq!(
        fs::read(fixture.unc.join("tracked.txt")).unwrap(),
        b"revoked pending change\n"
    );
    let path = fixture.unc.clone();
    drop(fixture);
    fs::remove_dir_all(path).unwrap();
}
