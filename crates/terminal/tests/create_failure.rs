mod support;
use devbox_protocol::Fail;
use devbox_terminal::*;
use std::{sync::Arc, time::Duration};
use support::Harness;

async fn until(mut condition: impl AsyncFnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !condition().await {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("owned fixture deadline");
}
#[tokio::test]
async fn creation_timeout_after_server_commit_removes_only_the_fresh_uuid() {
    let h = Arc::new(Harness::new());
    h.terminal
        .tmux()
        .start_direct()
        .await
        .expect("owned server");
    h.terminal
        .tmux()
        .new_session("keeper", h.home.path(), Some("exec sleep 60"), &[])
        .await
        .expect("owned sibling");
    h.terminal
        .tmux()
        .run(&[
            "set-hook",
            "-g",
            "after-new-session",
            "set-buffer -b fixture-created ready; wait-for fixture-create-response",
        ])
        .await
        .expect("delay only new-session response after commit");
    let uuid = uuid::Uuid::new_v4();
    let id = format!("t{}", uuid.simple());
    let mut events = h.hub.subscribe("terminal.changed");
    let creating = h.clone();
    let request = tokio::spawn(async move {
        creating
            .terminal
            .create_internal_with_id(
                (SessionKind::Shell, uuid),
                None,
                creating.home.path().into(),
                "timed-out terminal".into(),
                "exec sleep 60".into(),
                &[],
            )
            .await
    });
    until(async || {
        h.terminal
            .tmux()
            .run(&["list-buffers", "-F", "#{buffer_name}"])
            .await
            .expect("creation marker")
            .contains("fixture-created")
    })
    .await;
    assert!(
        h.terminal
            .tmux()
            .list()
            .await
            .expect("committed owned UUID")
            .iter()
            .any(|s| s.name == id),
        "server created exact UUID before client timed out"
    );
    let error = tokio::time::timeout(Duration::from_secs(15), request)
        .await
        .expect("creation deadline")
        .expect("creation task")
        .expect_err("tmux reply times out");
    assert!(
        matches!(&error, Fail::Internal(reason) if reason.contains("timed out")),
        "retain creation timeout cause: {error:?}"
    );
    let remaining = h.terminal.tmux().list().await.expect("cleanup observation");
    assert!(
        !remaining.iter().any(|s| s.name == id),
        "server-created terminal {id} was left invisible after failed creation"
    );
    assert_eq!(
        remaining
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        ["keeper"]
    );
    assert_eq!(h.db.rev(), 0);
    assert!(
        h.terminal
            .list(ListSessions {
                project_id: None,
                after_id: None
            })
            .await
            .expect("no failed metadata")
            .items
            .is_empty()
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(20), events.recv())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn reused_creation_identity_never_removes_a_preexisting_session() {
    let h = Harness::new();
    h.terminal
        .tmux()
        .start_direct()
        .await
        .expect("owned server");
    let uuid = uuid::Uuid::new_v4();
    let id = format!("t{}", uuid.simple());
    h.terminal
        .tmux()
        .new_session(&id, h.home.path(), Some("exec sleep 60"), &[])
        .await
        .expect("preexisting fixture identity");
    assert!(
        h.terminal
            .create_internal_with_id(
                (SessionKind::Shell, uuid),
                None,
                h.home.path().into(),
                "reused identity".into(),
                "exec sleep 60".into(),
                &[]
            )
            .await
            .is_err()
    );
    assert_eq!(
        h.terminal
            .tmux()
            .list()
            .await
            .expect("preserved target")
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        [id.as_str()]
    );
    assert_eq!(h.db.rev(), 0);
}

#[cfg(unix)]
#[tokio::test]
async fn creation_and_cleanup_reply_errors_keep_the_exact_uuid_in_diagnostics() {
    use std::os::unix::fs::PermissionsExt;
    const CHILD: &str = "DEVBOX_TEST_TERMINAL_CREATE_FAILURE_CHILD";
    if std::env::var_os(CHILD).is_some() {
        cleanup_failure_child().await;
        return;
    }
    // Change PATH only in an owned test subprocess, never the test runner or
    // daemon. The wrapper performs real creation then fails its reply, and
    // refuses cleanup so the diagnostic must retain exact unresolved ownership.
    let real = std::process::Command::new("sh")
        .args(["-c", "command -v tmux"])
        .output()
        .expect("locate installed tmux");
    assert!(real.status.success());
    let real = String::from_utf8(real.stdout).expect("tmux executable path");
    let fixture = tempfile::tempdir().expect("owned wrapper directory");
    let wrapper = fixture.path().join("tmux");
    std::fs::write(
        &wrapper,
        format!(
            r#"#!/bin/sh
operation=
sibling=false
for arg in "$@"; do
    case "$arg" in
        new-session|kill-session) operation="$arg" ;;
        keeper) sibling=true ;;
    esac
done
case "$operation" in
    new-session)
        if [ "$sibling" = true ]; then exec {real} "$@"; fi
        {real} "$@" || exit $?
        echo fixture-creation-error >&2
        exit 70 ;;
    kill-session)
        echo fixture-cleanup-error >&2
        exit 71 ;;
esac
exec {real} "$@"
"#,
            real = devbox_core::sh_quote(real.trim())
        ),
    )
    .expect("wrapper script");
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700))
        .expect("owned executable");
    let path = std::env::join_paths(std::iter::once(fixture.path().to_path_buf()).chain(
        std::env::split_paths(&std::env::var_os("PATH").expect("runner PATH")),
    ))
    .expect("child-only PATH");
    let mut child = tokio::process::Command::new(std::env::current_exe().expect("test executable"));
    child
        .args([
            "--exact",
            "creation_and_cleanup_reply_errors_keep_the_exact_uuid_in_diagnostics",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .env("PATH", path)
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(10), child.output())
        .await
        .expect("isolated diagnostic fixture deadline")
        .expect("fixture subprocess");
    assert!(
        output.status.success(),
        "isolated diagnostic fixture: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[cfg(unix)]
async fn cleanup_failure_child() {
    let h = Harness::new();
    h.terminal
        .tmux()
        .start_direct()
        .await
        .expect("owned server");
    h.terminal
        .tmux()
        .new_session("keeper", h.home.path(), Some("exec sleep 60"), &[])
        .await
        .expect("owned sibling");
    let uuid = uuid::Uuid::new_v4();
    let id = format!("t{}", uuid.simple());
    let mut events = h.hub.subscribe("terminal.changed");
    let error = h
        .terminal
        .create_internal_with_id(
            (SessionKind::Shell, uuid),
            None,
            h.home.path().into(),
            "failed replies".into(),
            "exec sleep 60".into(),
            &[],
        )
        .await
        .expect_err("creation failed");
    assert!(
        matches!(&error, Fail::Internal(reason) if reason.contains(&id)
        && reason.contains("fixture-creation-error") && reason.contains("fixture-cleanup-error")
        && reason.contains("cleanup unresolved")),
        "retain ownership and both causes: {error:?}"
    );
    assert_eq!(h.db.rev(), 0);
    let remaining = h
        .terminal
        .tmux()
        .list()
        .await
        .expect("observable unresolved state");
    assert!(remaining.iter().any(|s| s.name == "keeper"));
    assert!(
        remaining.iter().any(|s| s.name == id),
        "failed cleanup must not be called absence"
    );
    assert!(
        h.terminal
            .list(ListSessions {
                project_id: None,
                after_id: None
            })
            .await
            .expect("no failed metadata")
            .items
            .is_empty()
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(20), events.recv())
            .await
            .is_err()
    );
}
