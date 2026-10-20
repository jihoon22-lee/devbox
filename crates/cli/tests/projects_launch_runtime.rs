mod support;
use devbox_cli::client::{Client, ClientError};
use devbox_projects::{LaunchProject, LaunchResult, ProjectsLaunch};
use serde::Deserialize;
use std::time::Duration;
use support::runtime::RuntimeFixture;

async fn configured(fixture: &RuntimeFixture, client: &Client, config: &str) -> String {
    let project = fixture.project(client).await;
    let path = fixture
        .daemon
        .home
        .path()
        .join("project/.devbox/devbox.toml");
    std::fs::create_dir_all(path.parent().expect("parent")).expect("owned config");
    std::fs::write(path, config).expect("write configuration");
    fixture.approve(client, &project).await;
    project
}
#[derive(Debug, Deserialize)]
struct PartialLaunch {
    services: u32,
    run_ids: Vec<String>,
}
fn partial(error: ClientError) -> PartialLaunch {
    match error {
        ClientError::Rpc {
            code,
            detail: Some(detail),
            ..
        } => {
            assert_eq!(code, "projects.launch_failed");
            serde_json::from_str(detail.get()).expect("partial launch result")
        }
        error => panic!("expected typed partial result: {error:?}"),
    }
}
#[tokio::test]
#[ignore = "needs systemd user"]
async fn launch_shared_service_dependencies_are_started_once_and_counted() {
    let fixture = RuntimeFixture::new();
    let client = fixture.daemon.client().await;
    let project = configured(&fixture, &client, "[[service]]\nid='db'\ncommand='sleep 60'\n[[service]]\nid='web'\ncommand='sleep 60'\ndepends_on=['db']\n[launch]\nservices=['db','web']\n").await;
    let result: LaunchResult = client
        .call::<ProjectsLaunch>(&LaunchProject {
            id: project.clone(),
        })
        .await
        .expect("launch services");
    assert_eq!(result.services, 2);
    assert_eq!(result.run_ids.len(), 2);
    let runs = fixture.runs(&client, &project).await;
    assert_eq!(runs.len(), 2);
    assert_eq!(runs.iter().filter(|run| run.def_id == "db").count(), 1);
    assert_eq!(fixture.units().len(), 2);
}
#[tokio::test]
#[ignore = "needs systemd user"]
async fn launch_readiness_failure_reports_the_service_already_created() {
    let fixture = RuntimeFixture::new();
    let client = fixture.daemon.client().await;
    let port = std::net::TcpListener::bind(("127.0.0.1", 0))
        .expect("port")
        .local_addr()
        .expect("address")
        .port();
    let project = configured(&fixture, &client, &format!("[[service]]\nid='unready'\ncommand='sleep 60'\nports_hint=[{port}]\nready_timeout_sec=2\n[launch]\nservices=['unready']\n")).await;
    let result = partial(
        tokio::time::timeout(
            Duration::from_secs(5),
            client.call::<ProjectsLaunch>(&LaunchProject {
                id: project.clone(),
            }),
        )
        .await
        .expect("deadline")
        .expect_err("not ready"),
    );
    assert_eq!(result.services, 1);
    assert_eq!(result.run_ids.len(), 1);
    assert_eq!(
        fixture.runs(&client, &project).await[0].id,
        result.run_ids[0]
    );
}
#[tokio::test]
#[ignore = "needs systemd user"]
async fn launch_dependency_failure_reports_task_and_missing_environment_preflights_all() {
    let fixture = RuntimeFixture::new();
    let client = fixture.daemon.client().await;
    let project = configured(&fixture, &client, "[[task]]\nid='fail'\ncommand='exit 1'\n[[service]]\nid='web'\ncommand='sleep 60'\ndepends_on=['fail']\n[launch]\nservices=['web']\n").await;
    let result = partial(
        client
            .call::<ProjectsLaunch>(&LaunchProject {
                id: project.clone(),
            })
            .await
            .expect_err("dependency fails"),
    );
    assert_eq!(result.services, 0);
    assert_eq!(result.run_ids.len(), 1);
    assert_eq!(fixture.runs(&client, &project).await.len(), 1);
    std::fs::remove_file(fixture.daemon.paths.instance_config_dir.join("env"))
        .expect("remove owned env");
    let result = partial(
        client
            .call::<ProjectsLaunch>(&LaunchProject {
                id: project.clone(),
            })
            .await
            .expect_err("environment preflight"),
    );
    assert_eq!(result.services, 0);
    assert!(result.run_ids.is_empty());
    assert_eq!(fixture.runs(&client, &project).await.len(), 1);
}
