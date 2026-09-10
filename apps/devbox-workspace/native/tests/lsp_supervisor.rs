#![cfg(all(target_os = "linux", feature = "helper"))]
use code_pad_lib::lsp::{
    EnvironmentAllowlist, LspProcess, ProcessSpec, RequestCancellation, ResolvedProcess,
    ResolvedRuntime, RuntimeError, RuntimeKind, RuntimeResolver,
};
use std::{
    collections::BTreeMap,
    os::unix::fs::PermissionsExt,
    path::Path,
    time::{Duration, Instant},
};

const DETACHED: &str = "echo $PPID > owner; setsid /bin/sh -c 'echo $$ > \"$1/detached\"; /bin/sleep 20 & echo $! > \"$1/leaf\"; wait' fixture \"$PWD\" & while [ ! -s leaf ]; do /bin/sleep 0.01; done; ";
fn supervisor() -> std::path::PathBuf {
    // The owned Windows/WSL fixture copies both compiled Linux executables
    // into its disposable distro; Cargo's build-machine path does not exist there.
    std::env::var_os("DEVBOX_LSP_SUPERVISOR_FIXTURE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_devbox-workspace-wsl").into())
}
async fn marker(root: &Path, name: &str) -> i32 {
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        if let Ok(value) = std::fs::read_to_string(root.join(name)) {
            if let Ok(pid) = value.trim().parse::<i32>() {
                return pid;
            }
        }
        assert!(Instant::now() < until, "owned descendant marker missing");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
async fn gone(pid: i32) {
    let until = Instant::now() + Duration::from_secs(5);
    while Path::new(&format!("/proc/{pid}")).exists() {
        assert!(Instant::now() < until, "owned descendant was not reaped");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
async fn launch(root: &Path, ending: &str) -> LspProcess {
    LspProcess::spawn(
        ProcessSpec::new("/bin/sh", root)
            .args(["-c", &format!("{DETACHED}{ending}")])
            .env("PATH", "/usr/bin:/bin")
            .with_linux_supervisor(supervisor()),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn lsp_root_exit_retires_detached_pipe_holders_before_completion() {
    let directory = tempfile::tempdir().unwrap();
    let process = launch(directory.path(), "exit 7").await;
    let detached = marker(directory.path(), "detached").await;
    let leaf = marker(directory.path(), "leaf").await;
    assert!(process.wait_for_exit(Duration::from_secs(5)).await);
    gone(detached).await;
    gone(leaf).await;
    assert!(matches!(
        process.state().await,
        code_pad_lib::lsp::ProcessState::Exited { code: Some(7) }
    ));
}

#[tokio::test]
async fn lsp_cancel_and_drop_retire_only_their_supervised_job() {
    let other_root = tempfile::tempdir().unwrap();
    let other = launch(other_root.path(), "wait").await;
    let other_leaf = marker(other_root.path(), "leaf").await;
    for drop_owner in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let process = launch(directory.path(), "wait").await;
        let detached = marker(directory.path(), "detached").await;
        let leaf = marker(directory.path(), "leaf").await;
        if drop_owner {
            drop(process);
        } else {
            let _ = process.shutdown_with_timeout(Duration::ZERO).await;
            assert!(process.wait_for_exit(Duration::from_secs(5)).await);
        }
        gone(detached).await;
        gone(leaf).await;
        gone(marker(directory.path(), "owner").await).await;
        assert!(Path::new(&format!("/proc/{other_leaf}")).exists());
        assert!(!other.wait_for_exit(Duration::from_millis(10)).await);
    }
    let _ = other.shutdown_with_timeout(Duration::ZERO).await;
    assert!(other.wait_for_exit(Duration::from_secs(5)).await);
    gone(other_leaf).await;
}

fn runtime(root: &Path, ending: &str) -> ResolvedProcess {
    let executable = root.join("synthetic-node");
    std::fs::write(&executable, format!("#!/bin/sh\n{DETACHED}{ending}\n")).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    ResolvedProcess {
        executable: executable.clone(),
        args: vec![],
        current_dir: root.into(),
        env: BTreeMap::new(),
        runtime: Some(ResolvedRuntime {
            kind: RuntimeKind::Node,
            executable,
            version_requirement: None,
        }),
    }
}

#[tokio::test]
async fn version_probe_success_retires_detached_stdout_holders() {
    let directory = tempfile::tempdir().unwrap();
    let process = runtime(directory.path(), "printf 'v22.0.0\\n'; exit 0");
    let resolver = RuntimeResolver::new()
        .with_environment(EnvironmentAllowlist::with_path("/usr/bin:/bin"))
        .with_linux_supervisor(supervisor());
    resolver.probe_managed_runtime(&process).await.unwrap();
    gone(marker(directory.path(), "detached").await).await;
    gone(marker(directory.path(), "leaf").await).await;
    gone(marker(directory.path(), "owner").await).await;
}

#[tokio::test]
async fn cancelled_version_probe_keeps_owner_until_detached_children_retire() {
    let directory = tempfile::tempdir().unwrap();
    let process = runtime(directory.path(), "wait");
    let resolver = RuntimeResolver::new()
        .with_environment(EnvironmentAllowlist::with_path("/usr/bin:/bin"))
        .with_linux_supervisor(supervisor());
    let cancellation = RequestCancellation::new();
    let cancel = async {
        let leaf = marker(directory.path(), "leaf").await;
        cancellation.cancel();
        leaf
    };
    let (result, leaf) = tokio::join!(
        resolver.probe_managed_runtime_cancellable(&process, &cancellation),
        cancel
    );
    assert!(matches!(result, Err(RuntimeError::RuntimeProbeCancelled)));
    gone(marker(directory.path(), "detached").await).await;
    gone(leaf).await;
    gone(marker(directory.path(), "owner").await).await;
}
