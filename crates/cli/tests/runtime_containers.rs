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
  [ ! -e rows ] || exec /bin/cat rows
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
    assert_eq!(result.items[0].project_id, None);
    // The route hands the registered project folders to the listing: a container whose Compose
    // working directory is inside one belongs to that project.
    let folder = daemon.home.path().join("app");
    std::fs::create_dir(&folder).expect("fixture result");
    let project = client
        .call::<devbox_projects::ProjectsAdd>(&devbox_projects::AddProject {
            path: folder.to_string_lossy().into_owned(),
        })
        .await
        .expect("fixture result");
    let row = |id: &str, dir: &str| {
        format!(
            r#"{{"ID":"{id}","Names":"{id}","Image":"fake","State":"running","Status":"Up","Ports":"","Labels":"com.docker.compose.project=app,com.docker.compose.project.working_dir={dir},com.docker.compose.service=web"}}"#
        )
    };
    std::fs::write(
        bin.join("rows"),
        format!(
            "{}\n{}\n",
            row("inside", &format!("{}/deploy", project.path)),
            row("sibling", &format!("{}2", project.path))
        ),
    )
    .expect("fixture result");
    let result = client
        .call::<RuntimeContainers>(&ContainersQuery {})
        .await
        .expect("fixture result");
    assert_eq!(
        result.items[0].project_id.as_deref(),
        Some(project.id.as_str())
    );
    assert_eq!(result.items[1].project_id, None);
    std::fs::remove_file(bin.join("rows")).expect("fixture result");
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
