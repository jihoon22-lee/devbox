mod support;
use devbox_cli::client::Client;
use devbox_runtime::*;
use serde_json::json;
use std::{os::unix::fs::PermissionsExt, sync::Arc, time::Duration};
use support::{TestDaemon, bounded};

struct Fixture {
    daemon: TestDaemon,
    project: String,
}
impl Fixture {
    async fn new() -> Self {
        let mut daemon = TestDaemon::fixture();
        daemon.foreground = false;
        daemon.paths.ensure_dirs().expect("fixture result");
        std::fs::write(
            daemon.paths.instance_config_dir.join("env"),
            "PATH=\"/usr/bin:/bin\"\n",
        )
        .expect("fixture result");
        let bin = daemon.home.path().join("fake-bin");
        std::fs::create_dir(&bin).expect("fixture result");
        for (name, body) in [
            ("systemd-run", "printf 'start\\n' >> \"$HOME/starts\"\n"),
            (
                "systemctl",
                "case \"$2\" in\nshow) printf 'LoadState=loaded\\nActiveState=active\\nSubState=running\\nResult=success\\nNRestarts=0\\nMainPID=0\\nInvocationID=fixture\\n';;\nlist-units) printf '[]\\n';;\n*) printf '%s\\n' \"$*\" >> \"$HOME/manager-mutations\"; exit 1;;\nesac\n",
            ),
            ("bash", "printf 'PATH=/usr/bin:/bin\\0LANG=C\\0'\n"),
        ] {
            let path = bin.join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{body}")).expect("fixture result");
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
                .expect("fixture result");
        }
        // Only this daemon resolves the controlled fake manager. No host units exist.
        daemon
            .extra_env
            .push(("PATH".into(), format!("{}:/usr/bin:/bin", bin.display())));
        let repo = daemon.git_repo("project");
        daemon.launch();
        let client = daemon.client().await;
        let project = daemon.add_project(&client, &repo).await;
        for (id, dependencies) in [
            ("prerequisite", vec![]),
            ("dependent", vec!["prerequisite".into()]),
        ] {
            bounded(client.call::<RuntimeDefine>(&DefineRun {
                project_id: project.clone(),
                def: RunDef::Task(TaskDef {
                    id: id.into(),
                    command: "sleep 600".into(),
                    depends_on: dependencies,
                    ..Default::default()
                }),
                save_to: SaveTo::Project,
            }))
            .await
            .expect("fixture result");
        }
        Self { daemon, project }
    }
    async fn runs(&self, client: &Client) -> Vec<Run> {
        bounded(client.call::<RuntimeRuns>(&RunsQuery {
            project_id: Some(self.project.clone()),
            limit: 100,
        }))
        .await
        .expect("fixture result")
        .items
    }
    fn starts(&self) -> usize {
        std::fs::read_to_string(self.daemon.home.path().join("starts"))
            .unwrap_or_default()
            .lines()
            .count()
    }
    async fn pending(
        &self,
    ) -> (
        Arc<Client>,
        tokio::task::JoinHandle<Result<String, devbox_cli::client::ClientError>>,
        Run,
    ) {
        let client = Arc::new(self.daemon.client().await);
        let caller = client.clone();
        let params = json!({"projectId":self.project,"id":"dependent","worktree":null});
        let request = tokio::spawn(async move {
            caller
                .raw_call("runtime.start", params, Some("owned-start".into()))
                .await
        });
        let observer = self.daemon.client().await;
        let run = bounded(async {
            loop {
                if let Some(run) = self
                    .runs(&observer)
                    .await
                    .into_iter()
                    .find(|run| run.state == RunState::Running)
                {
                    break run;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
        assert_eq!(run.def_id, "prerequisite");
        assert_eq!(self.starts(), 1);
        (client, request, run)
    }
    fn assert_no_stop(&self) {
        assert!(
            !self.daemon.home.path().join("manager-mutations").exists(),
            "shutdown must leave started units alone"
        );
    }
}

async fn shutdown_pending(refresh: bool, service: bool) {
    let mut fixture = Fixture::new().await;
    if service {
        let client = fixture.daemon.client().await;
        bounded(client.call::<RuntimeDefine>(&DefineRun {
            project_id: fixture.project.clone(),
            def: RunDef::Service(ServiceDef {
                id: "prerequisite".into(),
                command: "sleep 600".into(),
                ports_hint: vec![1],
                ready_timeout_sec: 600,
                ..Default::default()
            }),
            save_to: SaveTo::Project,
        }))
        .await
        .expect("fixture result");
    }
    let (_caller, request, run) = fixture.pending().await;
    if refresh {
        let client = fixture.daemon.client().await;
        let response = bounded(client.raw_call("system.refresh_env", json!({}), None))
            .await
            .expect("fixture result");
        assert!(
            serde_json::from_str::<devbox_protocol::system::RefreshEnvResult>(&response)
                .expect("fixture result")
                .restarting
        );
    } else {
        use nix::{
            sys::signal::{Signal, kill},
            unistd::Pid,
        };
        kill(
            Pid::from_raw(fixture.daemon.child.as_ref().expect("fixture value").0.id() as i32),
            Signal::SIGTERM,
        )
        .expect("fixture result");
    }
    bounded(async {
        loop {
            if fixture
                .daemon
                .child
                .as_mut()
                .expect("fixture value")
                .0
                .try_wait()
                .expect("fixture result")
                .is_some()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    bounded(request)
        .await
        .expect("fixture result")
        .expect_err("shutdown closes caller");
    fixture.assert_no_stop();
    assert_eq!(
        fixture.starts(),
        1,
        "dependent must not start during shutdown"
    );
    // Restart recovers the live prerequisite from the same DB and fake unit.
    fixture.daemon.child.take();
    fixture.daemon.launch();
    let client = fixture.daemon.client().await;
    let recovered = fixture.runs(&client).await;
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].id, run.id);
    assert_eq!(recovered[0].state, RunState::Running);
    fixture.assert_no_stop();
}

#[tokio::test]
async fn sigterm_ends_pending_dependency_coordination_without_stopping_started_run() {
    shutdown_pending(false, false).await;
}
#[tokio::test]
async fn refresh_env_ends_pending_dependency_coordination_without_stopping_started_run() {
    shutdown_pending(true, false).await;
}
#[tokio::test]
async fn shutdown_ends_service_readiness_wait_without_stopping_started_run() {
    shutdown_pending(false, true).await;
}
#[tokio::test]
async fn disconnected_start_waiter_does_not_cancel_accepted_dependency_graph() {
    let fixture = Fixture::new().await;
    let (caller, request, run) = fixture.pending().await;
    caller.close_channel().await.expect("fixture result");
    bounded(request)
        .await
        .expect("fixture result")
        .expect_err("closed waiter");
    let store = exit_record::ExitStore::new(&fixture.daemon.paths.data_dir.join("runs"), &run.id)
        .expect("fixture result");
    store.begin("fixture").expect("fixture result");
    store
        .finish(&exit_record::ExitRecord {
            attempt_id: "fixture".into(),
            code: Some(0),
            signal: None,
            ended_ms: exit_record::now_ms(),
            by: exit_record::ExitSource::Wrapper,
            service_result: None,
        })
        .expect("fixture result");
    let retry = fixture.daemon.client().await;
    let response = bounded(retry.raw_call(
        "runtime.start",
        json!({"projectId":fixture.project,"id":"dependent","worktree":null}),
        Some("owned-start".into()),
    ))
    .await
    .expect("fixture result");
    let completed: Run = serde_json::from_str(&response).expect("fixture result");
    assert_eq!(completed.def_id, "dependent");
    assert_eq!(completed.state, RunState::Running);
    assert_eq!(
        fixture.starts(),
        2,
        "retry must not duplicate the accepted graph"
    );
    fixture.assert_no_stop();
}
