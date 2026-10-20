use crate::containers::*;
use crate::*;
use tokio_util::sync::CancellationToken;

const ROW: &str = r#"{"ID":"abc","Names":"owned-web","Image":"nginx:alpine","State":"running","Status":"Up 2 hours","Ports":"127.0.0.1:8080->80/tcp","Labels":"other=hello,com.docker.compose.project=owned,com.docker.compose.service=web"}"#;

#[test]
fn containers_argv_and_json_compose_fixture() {
    assert_eq!(
        container_args("abc", ContainerAction::Restart),
        ["restart", "abc"]
    );
    assert_eq!(
        container_args("abc", ContainerAction::Start),
        ["start", "abc"]
    );
    assert_eq!(
        container_args("abc", ContainerAction::Stop),
        ["stop", "abc"]
    );
    let parsed = parse_ps(ROW.as_bytes()).unwrap();
    assert_eq!(parsed.items[0].compose_project.as_deref(), Some("owned"));
    assert_eq!(parsed.items[0].ports, "127.0.0.1:8080->80/tcp");
    assert!(parsed.available);
}

#[tokio::test]
async fn containers_missing_cli_is_unavailable_not_empty() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = DockerContainers::new(dir.path().join("missing"));
    let result = adapter.list(CancellationToken::new()).await.unwrap();
    assert!(!result.available);
    assert_eq!(result.reason, Some(ContainerUnavailableReason::CliMissing));
    assert!(result.items.is_empty());
}

fn fake(dir: &std::path::Path, body: &str) -> DockerContainers {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("docker");
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\ncd -- \"${{0%/*}}\" || exit 91\nprintf '%s\\n' \"$@\" > args\n{body}\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    DockerContainers::new(path)
}
fn request(action: ContainerAction) -> ContainerActionParams {
    ContainerActionParams {
        id: "owned-web_1.0".into(),
        action,
    }
}
#[tokio::test]
async fn containers_fake_argv_actions_and_stop_undo() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = fake(dir.path(), "exit 0");
    for action in [
        ContainerAction::Start,
        ContainerAction::Stop,
        ContainerAction::Restart,
    ] {
        let result = adapter
            .action(request(action), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("args")).unwrap(),
            format!("{}\nowned-web_1.0\n", container_args("", action)[0])
        );
        assert_eq!(
            result.undo,
            (action == ContainerAction::Stop).then(|| request(ContainerAction::Start))
        );
    }
    let result = adapter.list(CancellationToken::new()).await.unwrap();
    assert!(result.available && result.reason.is_none() && result.items.is_empty());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("args")).unwrap(),
        "ps\n-a\n--format\njson\n"
    );
    assert_eq!(
        <RuntimeContainerAction as devbox_protocol::Method>::CONFIRM,
        devbox_protocol::Confirm::Reversible
    );
}
#[tokio::test]
async fn containers_fake_ndjson_unavailable_and_failure() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("rows"), format!("{ROW}\n\n{ROW}\n")).unwrap();
    let adapter = fake(dir.path(), "/bin/cat rows");
    assert_eq!(
        adapter
            .list(CancellationToken::new())
            .await
            .unwrap()
            .items
            .len(),
        2
    );
    let adapter = fake(dir.path(), "echo private-diagnostic >&2; exit 1");
    let result = adapter.list(CancellationToken::new()).await.unwrap();
    assert!(!result.available);
    assert_eq!(
        result.reason,
        Some(ContainerUnavailableReason::DaemonUnavailable)
    );
    let error = adapter
        .action(request(ContainerAction::Stop), CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        devbox_protocol::Fail::Domain(ContainerError::Failed)
    ));
}
#[tokio::test]
async fn containers_invalid_id_never_spawns() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = fake(dir.path(), "exit 0");
    for id in ["", "-q", "--help", "a b", "a;echo", "a/b", "한글"] {
        let error = adapter
            .action(
                ContainerActionParams {
                    id: id.into(),
                    action: ContainerAction::Start,
                },
                CancellationToken::new(),
            )
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            devbox_protocol::Fail::Domain(ContainerError::InvalidId)
        ));
    }
    assert!(!dir.path().join("args").exists());
}
#[tokio::test]
async fn containers_malformed_and_output_bounds() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = fake(dir.path(), "/bin/cat rows");
    for bytes in [
        b"{oops}".to_vec(),
        vec![255],
        ROW.replace("abc", "--help").into_bytes(),
    ] {
        std::fs::write(dir.path().join("rows"), bytes).unwrap();
        assert!(matches!(
            adapter.list(CancellationToken::new()).await,
            Err(devbox_protocol::Fail::Domain(ContainerError::InvalidOutput))
        ));
    }
    std::fs::write(dir.path().join("rows"), vec![b' '; OUTPUT_LIMIT + 1]).unwrap();
    assert!(matches!(
        adapter.list(CancellationToken::new()).await,
        Err(devbox_protocol::Fail::Domain(ContainerError::OutputLimit))
    ));
    let adapter = fake(dir.path(), "exec /bin/cat rows >&2");
    assert!(matches!(
        adapter.list(CancellationToken::new()).await,
        Err(devbox_protocol::Fail::Domain(ContainerError::OutputLimit))
    ));
}
async fn pid(dir: &std::path::Path) -> u32 {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if let Ok(text) = std::fs::read_to_string(dir.join("pid")) {
                if let Ok(pid) = text.trim().parse() {
                    return pid;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap()
}
async fn reaped(pid: u32) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while std::path::Path::new(&format!("/proc/{pid}")).exists() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("owned fake CLI must be killed and reaped");
}
#[tokio::test]
async fn containers_timeouts_kill_and_reap_owned_child() {
    let dir = tempfile::tempdir().unwrap();
    let mut adapter = fake(dir.path(), "echo $$ > pid; exec /bin/sleep 60");
    adapter.list_timeout = std::time::Duration::from_millis(100);
    adapter.action_timeout = std::time::Duration::from_millis(100);
    assert!(matches!(
        adapter.list(CancellationToken::new()).await,
        Err(devbox_protocol::Fail::Domain(ContainerError::Timeout))
    ));
    reaped(pid(dir.path()).await).await;
    std::fs::remove_file(dir.path().join("pid")).unwrap();
    assert!(matches!(
        adapter
            .action(request(ContainerAction::Stop), CancellationToken::new())
            .await,
        Err(devbox_protocol::Fail::Domain(ContainerError::Timeout))
    ));
    reaped(pid(dir.path()).await).await;
}
#[tokio::test]
async fn containers_cancellation_and_dropped_future_reap_owned_child() {
    for drop_future in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let adapter = fake(dir.path(), "echo $$ > pid; exec /bin/sleep 60");
        let cancel = CancellationToken::new();
        let token = cancel.clone();
        let task = tokio::spawn(async move { adapter.list(token).await });
        let owned = pid(dir.path()).await;
        if drop_future {
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
        } else {
            cancel.cancel();
            assert!(matches!(
                task.await.unwrap(),
                Err(devbox_protocol::Fail::Domain(ContainerError::Cancelled))
            ));
        }
        reaped(owned).await;
    }
}

#[tokio::test]
async fn containers_output_limit_kills_and_reaps_owned_child() {
    for descriptor in [1, 2] {
        let dir = tempfile::tempdir().unwrap();
        let adapter = fake(
            dir.path(),
            &format!(
                "echo $$ > pid; exec /usr/bin/python3 -c 'import os,time; os.write({descriptor}, b\"x\" * {}); time.sleep(60)'",
                OUTPUT_LIMIT + 1
            ),
        );
        assert!(matches!(
            adapter.list(CancellationToken::new()).await,
            Err(devbox_protocol::Fail::Domain(ContainerError::OutputLimit))
        ));
        reaped(pid(dir.path()).await).await;
    }
}
