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
    fn dependency_inventory(&self) -> Value {
        let deadline = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            + 30_000;
        let access =
            crate::dependencies_host::access(self.host.clone(), self.context.clone(), deadline, ())
                .unwrap();
        tauri::async_runtime::block_on(repo_manager_lib::component::dispatch_dependencies(
            access,
            "dependency_inventory",
            json!({"request":{"path":self.root}}),
        ))
        .unwrap()
    }
    fn check_wsl_profiles(&self) {
        use workbench_lib::component::{ProfileTemplate, WslProfile};
        let projects = self.host.projects().unwrap();
        let resources = self.host.helper_directory().unwrap();
        let product_contract::ExecutionTarget::Wsl { distro_id } = &self.context.target else {
            panic!("WSL required")
        };
        let root = format!("{}/template 한글 folder", self.root);
        let directory = self.unc.join("template 한글 folder");
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("original.txt"), b"preserve template fixture").unwrap();
        let mut template = ProfileTemplate::new("WSL template fixture");
        template.id.clear();
        template.expected_ports = vec![4321];
        template.run_manager_service_ids = vec!["synthetic-service".into()];
        template.wsl = Some(WslProfile {
            distro: "Missing preset fixture".into(),
            path: "/missing/preset".into(),
        });
        let saved = projects
            .save_template(projects.snapshot().unwrap().revision, None, template)
            .unwrap();
        let entry = saved.imported_templates.last().unwrap().clone();
        let preview = || {
            projects.preview_template_profile_wsl(resources, serde_json::from_value(json!({
            "templateId":entry.id,"distroId":distro_id,"root":root,"name":"Created WSL profile","startStopped":false
        })).unwrap()).unwrap()
        };
        let cancelled = preview();
        assert_eq!(projects.snapshot().unwrap(), saved);
        projects.cancel(&cancelled.preview_id).unwrap();
        assert!(projects
            .apply(
                &cancelled.preview_id,
                "Cancelled",
                RegistrationAction::Register
            )
            .is_err());
        let stale = preview();
        let mut edited = entry.template.clone();
        edited.expected_ports = vec![4322];
        let edited = projects
            .save_template(saved.revision, Some(&entry.id), edited)
            .unwrap();
        assert!(projects
            .apply(&stale.preview_id, "Stale", RegistrationAction::Register)
            .is_err());
        assert_eq!(projects.snapshot().unwrap(), edited);
        let reviewed = preview();
        let candidate = reviewed.template_profile.as_ref().unwrap();
        assert_eq!(candidate.profile.wsl.as_ref().unwrap().distro, self.name);
        assert_eq!(candidate.profile.wsl.as_ref().unwrap().path, root);
        assert!(
            candidate.profile.windows_path.is_none() && candidate.profile.environment.is_none()
        );
        let (registered, context) = projects
            .apply(
                &reviewed.preview_id,
                "Created WSL profile",
                RegistrationAction::Register,
            )
            .unwrap();
        assert_eq!(context.target, self.context.target);
        let candidate = registered.imported_profile_for(&context).unwrap().unwrap();
        assert_eq!(
            candidate.source_template_id.as_deref(),
            Some(entry.id.as_str())
        );
        assert_eq!(candidate.profile.expected_ports, vec![4322]);
        assert_eq!(
            candidate.profile.run_manager_service_ids,
            vec!["synthetic-service"]
        );
        assert!(registered
            .worktrees
            .iter()
            .find(|tree| tree.id == context.worktree_id)
            .unwrap()
            .trusted_digest
            .is_none());
        assert!(projects
            .apply(&reviewed.preview_id, "Replay", RegistrationAction::Register)
            .is_err());
        let imported_id = candidate.id.clone();
        let unbound = projects
            .unbind_imported_profile(
                registered.revision,
                &imported_id,
                crate::core::legacy_profiles::ProfileTarget::Wsl,
            )
            .unwrap();
        assert_eq!(unbound.imported_profiles, registered.imported_profiles);
        assert_eq!(unbound.worktrees, registered.worktrees);
        assert!(matches!(
            projects.preview_imported_profile_wsl(
                resources,
                &imported_id,
                &uuid::Uuid::new_v4().to_string(),
                false
            ),
            Err("legacy_profile_distro_mismatch")
        ));
        let cancelled = projects
            .preview_imported_profile_wsl(resources, &imported_id, distro_id, false)
            .unwrap();
        projects.cancel(&cancelled.preview_id).unwrap();
        assert_eq!(projects.snapshot().unwrap(), unbound);
        let reviewed = projects
            .preview_imported_profile_wsl(resources, &imported_id, distro_id, false)
            .unwrap();
        assert_eq!(
            reviewed.imported_profile_id.as_deref(),
            Some(imported_id.as_str())
        );
        let (rebound, rebound_context) = projects
            .apply(
                &reviewed.preview_id,
                "Created WSL profile",
                RegistrationAction::Register,
            )
            .unwrap();
        assert_eq!(rebound_context, context);
        assert_eq!(rebound.imported_profiles, registered.imported_profiles);
        assert_eq!(
            rebound.imported_profile_bindings,
            registered.imported_profile_bindings
        );
        assert_eq!(
            fs::read(directory.join("original.txt")).unwrap(),
            b"preserve template fixture"
        );
        assert!(!directory.join(".git").exists());
        println!(
            "WSL Registry: template review, stale/cancel/replay and profile rebind checks passed"
        );
    }
    fn check_dependencies(&self) {
        fs::write(
            self.unc.join("Cargo.toml"),
            "[package]\nname = \"local\"\nversion = \"0.1.0\"\n[dependencies]\nfixture = \"1\"\n",
        )
        .unwrap();
        let lock = "version = 3\n[[package]]\nname = \"fixture\"\nversion = \"1.0.0\"\nsource = \"registry+https://private.invalid/native-fixture-source\"\n";
        fs::write(self.unc.join("Cargo.lock"), lock).unwrap();
        fs::write(
            self.unc.join("build.rs"),
            "fn main() { panic!(\"must not execute\"); }",
        )
        .unwrap();
        let report = self.dependency_inventory();
        assert_eq!(report["packageCount"], 1);
        assert_eq!(report["directCount"], 1);
        assert_eq!(report["summaryPublished"], true);
        assert!(!report.to_string().contains(&self.root));
        assert!(!report.to_string().contains("native-fixture-source"));
        assert!(!self.unc.join("target").exists());
        assert!(!self
            .host
            .component("common")
            .unwrap()
            .join("repo-manager/dependency-enrichment-v1.json")
            .exists());
        fs::write(
            self.unc.join("Cargo.lock"),
            format!("{lock}\n# changed native input\n"),
        )
        .unwrap();
        let changed = self.dependency_inventory();
        assert_ne!(changed["revision"], report["revision"]);
        assert_eq!(changed["summaryPublished"], true);
        let mut stale = self.context.clone();
        stale.revision += 1;
        assert!(crate::dependencies_host::access(self.host.clone(), stale, u64::MAX, ()).is_err());
    }
    fn approve_cleanup(&mut self, context: &ProjectContext) {
        let preview = self
            .source
            .manage(
                &self.host,
                &mut self.definitions,
                &self.context,
                "preview_cleanup_scope",
                json!({"worktreeIds":[context.worktree_id]}),
                Self::budget(),
            )
            .unwrap();
        self.source
            .manage(
                &self.host,
                &mut self.definitions,
                &self.context,
                "approve_cleanup_scope",
                json!({"previewId":preview["previewId"]}),
                Self::budget(),
            )
            .unwrap();
    }
    fn check_cleanup(&mut self) {
        let root = self.root.clone();
        let linked = format!("{root}/cleanup worktree 한글");
        self.git(&[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "cleanup-linked",
            &linked,
        ]);
        let product_contract::ExecutionTarget::Wsl { distro_id } = &self.context.target else {
            panic!("expected WSL target")
        };
        let projects = self.host.projects().unwrap();
        let proposal = projects
            .preview_wsl(
                self.host.helper_directory().unwrap(),
                distro_id,
                &linked,
                false,
            )
            .unwrap();
        let (_, context) = projects
            .apply(
                &proposal.preview_id,
                "Cleanup worktree",
                RegistrationAction::Register,
            )
            .unwrap();
        self.approve_cleanup(&context);
        let document = format!("{linked}/tracked.txt");
        self.files
            .lock()
            .unwrap()
            .set_wsl_document_fixture(&self.host, &context, &document, true)
            .unwrap();
        let revealed = std::cell::RefCell::new(None);
        self.files
            .lock()
            .unwrap()
            .reveal_wsl(
                &self.host,
                &context,
                json!({"path":document}),
                u64::MAX,
                &|path| {
                    *revealed.borrow_mut() = Some(path.to_owned());
                    Ok(())
                },
            )
            .unwrap();
        let target = revealed.into_inner().unwrap();
        let mapped = devbox_wsl::path::parse_wsl_unc_path(target.to_str().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(mapped.linux_path(), document);
        let mut stale = context.clone();
        stale.revision += 1;
        assert!(self
            .files
            .lock()
            .unwrap()
            .reveal_wsl(
                &self.host,
                &stale,
                json!({"path":document}),
                u64::MAX,
                &|_| panic!("stale context reached Explorer")
            )
            .is_err());
        let prepared = self.prepare(
            "repo_cleanup_preview",
            json!({"request":{"path":root,"operationId":"open-cleanup-preview"}}),
            "open-cleanup-preview",
            (),
        );
        assert!(matches!(
            prepared.finish_on_worker(),
            Err("source_cleanup_open_files")
        ));
        self.files
            .lock()
            .unwrap()
            .set_wsl_document_fixture(&self.host, &context, &document, false)
            .unwrap();
        assert!(self
            .files
            .lock()
            .unwrap()
            .reveal_wsl(
                &self.host,
                &context,
                json!({"path":document}),
                u64::MAX,
                &|_| panic!("closed document reached Explorer")
            )
            .is_err());
        let preview = self.execute(
            "repo_cleanup_preview",
            json!({"request":{"path":root,"operationId":"cleanup-preview"}}),
            "cleanup-preview",
        );
        let entry = preview["worktrees"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["path"] == linked)
            .unwrap();
        assert_eq!(entry["eligible"], true, "{preview}");
        let args = json!({"request":{"path":root,"operationId":"revoked-cleanup",
            "previewRevision":preview["revision"],"branchNames":[],"worktreePaths":[linked]}});
        let prepared = self.prepare("repo_cleanup", args, "revoked-cleanup", ());
        self.source
            .manage(
                &self.host,
                &mut self.definitions,
                &self.context,
                "revoke_cleanup_scope",
                json!({}),
                Self::budget(),
            )
            .unwrap();
        assert!(prepared.finish_on_worker().is_err());
        assert!(self.unc.join("cleanup worktree 한글/tracked.txt").is_file());
        self.approve_cleanup(&context);
        let preview = self.execute(
            "repo_cleanup_preview",
            json!({"request":{"path":root,"operationId":"approved-cleanup-preview"}}),
            "approved-cleanup-preview",
        );
        let result = self.execute(
            "repo_cleanup",
            json!({"request":{"path":root,"operationId":"approved-cleanup",
            "previewRevision":preview["revision"],"branchNames":[],"worktreePaths":[linked]}}),
            "approved-cleanup",
        );
        assert_eq!(result["removed"], 1, "{result}");
        assert!(!self.unc.join("cleanup worktree 한글").exists());
        assert!(!self.git(&["branch", "--list", "cleanup-linked"]).is_empty());
        assert_eq!(
            fs::read(self.unc.join("unselected.txt")).unwrap(),
            b"preserved unselected\n"
        );
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
        let began = Instant::now();
        println!("WSL Source {id}: starting {method}");
        let prepared = self.prepare(method, args, id, ());
        let ReadySource::Complete(value) = prepared.finish_on_worker().unwrap_or_else(|issue| {
            panic!(
                "WSL Source {id}: {method} failed with {issue} after {:?}",
                began.elapsed()
            )
        }) else {
            panic!("WSL used Windows Git")
        };
        println!("WSL Source {id}: {method} passed in {:?}", began.elapsed());
        value
    }
}
#[test]
#[ignore = "requires the current run-owned hosted WSL distro, Git and packaged helper"]
fn owned_source_stage_commit_revocation_and_cancellation_keep_native_ownership() {
    let mut fixture = Fixture::new();
    fixture.check_wsl_profiles();
    fixture.check_dependencies();
    println!("WSL Dependencies: native inventory, private summary and stale context checks passed");
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
    fixture.check_cleanup();
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
