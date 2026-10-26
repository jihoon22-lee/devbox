use crate::{
    windows::PreparedCopy,
    windows_copy::CopyIdentity,
    windows_copy_store::{Attempt, AttemptPhase, CopyRef, Reader, ReaderState},
    *,
};
use devbox_core::{Env, Instance, Paths};
use devbox_protocol::windows::{ObservationAssigned, ObservationControl, PathObservation};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
pub(crate) struct Owned {
    pub f: service_tests::Fixture,
    pub config: ExecutionConfig,
    pub copy: PreparedCopy,
}
struct TestEnv(PathBuf);
impl Env for TestEnv {
    fn var(&self, key: &str) -> Option<String> {
        matches!(key, "HOME" | "XDG_RUNTIME_DIR").then(|| self.0.to_string_lossy().into_owned())
    }
    fn uid(&self) -> u32 {
        1000
    }
}
pub(crate) async fn owned() -> Owned {
    let f = service_tests::Fixture::new().await;
    let source = f.root.path().join("repo");
    let git = std::process::Command::new("git")
        .args(["init", "-q"])
        .arg(&source)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("owned Git fixture");
    assert!(git.status.success());
    std::fs::write(source.join("input.txt"), "source v1").expect("owned input");
    let paths = Paths::resolve(
        Instance::parse("test-copy").expect("instance"),
        &TestEnv(f.root.path().into()),
    )
    .expect("owned paths");
    paths.ensure_dirs().expect("owned directories");
    std::fs::write(
        paths.instance_config_dir.join("env"),
        "PATH=/usr/bin:/bin\n",
    )
    .expect("owned captured environment");
    let mut config = ExecutionConfig::new(paths);
    config.windows_root = Some("C:\\owned-copy".into());
    config.windows_owner = "owned-fixture|test-copy|accept|test-distro".into();
    config.windows_paths = Arc::new(|_| Box::pin(async { PathObservation::Safe }));
    let mut identity =
        CopyIdentity::new(&config, &f.project, "main", &source).expect("copy identity");
    // Private Rust fixture maps the prepared identity to its own Linux tempdir.
    // No production config flag/environment can replace the Windows path mapping.
    identity.mapped_base = f
        .root
        .path()
        .join("copies/owner")
        .join(&f.project)
        .join("main");
    let copy = PreparedCopy {
        source,
        target: identity.target().expect("mapped target"),
        cwd: identity.directory(),
        relative_cwd: PathBuf::new(),
        identity,
    };
    Owned { f, config, copy }
}
pub(crate) fn service(o: &Owned) -> Arc<RuntimeService> {
    Arc::new(
        RuntimeService::new(o.f.db.clone(), o.f.hub.clone(), o.f.projects.clone())
            .with_execution(o.config.clone()),
    )
}
pub(crate) fn reader(o: &Owned, launch: &str, revision: &str) -> Reader {
    Reader {
        copy: CopyRef {
            key: o.copy.identity.key.clone(),
            revision: revision.into(),
            launch_id: launch.into(),
        },
        project_id: o.f.project.clone(),
        run_id: format!("run-{launch}"),
        unit: format!("devbox-test-copy-run-{launch}.service"),
        state: ReaderState::Reserved,
        owner: crate::windows_copy_service::process_owner().expect("coordinator identity"),
        previous_attempt: None,
    }
}
pub(crate) fn attempt(o: &Owned, r: &Reader, id: &str, n: u8) -> Attempt {
    let nonce = format!("{n:032x}");
    let dir = o.copy.identity.control_directory();
    Attempt {
        run_id: r.run_id.clone(),
        attempt_id: id.into(),
        control: ObservationControl {
            schema: 1,
            launch_id: r.copy.launch_id.clone(),
            job_name: format!("Local\\Devbox.Observe.{nonce}"),
            assigned_path: format!("{dir}\\{nonce}.assigned.json"),
            grant_path: format!("{dir}\\{nonce}.grant.json"),
            closed_path: format!("{dir}\\{nonce}.closed.json"),
            nonce,
        },
        phase: AttemptPhase::Intent,
        assigned: None,
        owner: r.owner.clone(),
    }
}
pub(crate) fn assigned(a: &Attempt) -> ObservationAssigned {
    ObservationAssigned {
        schema: 1,
        run_id: a.run_id.clone(),
        launch_id: a.control.launch_id.clone(),
        attempt_id: a.attempt_id.clone(),
        nonce: a.control.nonce.clone(),
        job_name: a.control.job_name.clone(),
        pid: 101,
        creation_time: "132000000000000000".into(),
        sid: "S-1-5-21-123".into(),
        session_id: 1,
    }
}
pub(crate) fn marker(path: &Path) -> PathBuf {
    path.join(".devbox-owner.json")
}
