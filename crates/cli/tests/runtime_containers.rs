mod support;
use devbox_cli::client::ClientError;
use devbox_runtime::*;
use std::os::unix::fs::PermissionsExt;
use support::TestDaemon;

#[tokio::test]
async fn runtime_containers_typed_rpc_fake_path_and_actions() {
    let mut daemon = TestDaemon::fixture();
    let bin = daemon.home.path().join("bin");
    std::fs::create_dir(&bin).expect("fixture result");
    let docker = bin.join("docker");
    std::fs::write(&docker, r##"#!/bin/sh
cd -- "${0%/*}" || exit 91
printf '%s\n' "$@" > args
[ ! -e failed ] || { echo private-diagnostic >&2; exit 1; }
if [ "$1" = ps ]; then
  printf '%s\n' '{"ID":"abc","Names":"owned","Image":"fake","State":"running","Status":"Up","Ports":"","Labels":"com.docker.compose.project=owned"}'
fi
"##).expect("fixture result");
    std::fs::set_permissions(&docker, std::fs::Permissions::from_mode(0o700))
        .expect("fixture result");
    // Only the child daemon PATH changes; every Docker invocation resolves to this fake.
    daemon
        .extra_env
        .push(("PATH".into(), format!("{}:/usr/bin:/bin", bin.display())));
    daemon.launch();
    let client = daemon.client().await;
    let result = client
        .call::<RuntimeContainers>(&ContainersQuery {})
        .await
        .expect("fixture result");
    assert!(result.available && result.reason.is_none());
    assert_eq!(result.items[0].compose_project.as_deref(), Some("owned"));
    assert_eq!(
        std::fs::read_to_string(bin.join("args")).expect("fixture result"),
        "ps\n-a\n--format\njson\n"
    );
    for action in [
        ContainerAction::Start,
        ContainerAction::Stop,
        ContainerAction::Restart,
    ] {
        let result = client
            .call::<RuntimeContainerAction>(&ContainerActionParams {
                id: "abc".into(),
                action,
            })
            .await
            .expect("fixture result");
        assert_eq!(result.action, action);
        assert_eq!(result.undo.is_some(), action == ContainerAction::Stop);
        if let Some(undo) = result.undo {
            assert_eq!(undo.action, ContainerAction::Start);
        }
    }
    assert!(
        matches!(client.call::<RuntimeContainerAction>(&ContainerActionParams { id: "--help".into(), action: ContainerAction::Start }).await.expect_err("expected failure"), ClientError::Rpc { code, .. } if code == "runtime.container_invalid_id")
    );
    std::fs::write(bin.join("failed"), "").expect("fixture result");
    assert!(
        !client
            .call::<RuntimeContainers>(&ContainersQuery {})
            .await
            .expect("fixture result")
            .available
    );
    assert!(
        matches!(client.call::<RuntimeContainerAction>(&ContainerActionParams { id: "abc".into(), action: ContainerAction::Start }).await.expect_err("expected failure"), ClientError::Rpc { code, .. } if code == "runtime.container_failed")
    );
}
