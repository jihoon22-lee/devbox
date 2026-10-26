use super::*;
use devbox_core::{Env, Instance, Paths};
use std::{os::unix::fs::PermissionsExt, path::PathBuf};
struct TestEnv(PathBuf);
impl Env for TestEnv {
    fn var(&self, key: &str) -> Option<String> {
        matches!(key, "HOME" | "XDG_RUNTIME_DIR").then(|| self.0.to_string_lossy().into_owned())
    }
    fn uid(&self) -> u32 {
        1000
    }
}
fn receipt() -> ObservationAssigned {
    let nonce = "a".repeat(32);
    ObservationAssigned {
        schema: 1,
        run_id: "run".into(),
        launch_id: "launch".into(),
        attempt_id: "attempt".into(),
        job_name: format!("Local\\Devbox.Observe.{nonce}"),
        nonce,
        pid: 123,
        creation_time: "132000000000000000".into(),
        sid: "S-1-5-21-123".into(),
        session_id: 2,
    }
}
#[tokio::test]
async fn native_observer_can_reconcile_its_original_session_when_direct_interop_recovers_elsewhere()
{
    for unavailable in [
        ObservationState::IdentityMismatch,
        ObservationState::Unavailable,
    ] {
        let temp = tempfile::tempdir().expect("owned helper");
        let path = temp.path().join("probe");
        let reply = serde_json::to_string(&WindowsProbeResult::Observation { state: unavailable })
            .expect("typed response");
        std::fs::write(&path,format!("#!/usr/bin/python3\nimport sys,json\nrequest=json.load(sys.stdin)\nassert request['assigned']['sessionId']==2\nprint({reply:?})\n")).expect("owned executable");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).expect("mode");
        let paths = Paths::resolve(
            Instance::parse("test-probe-session").expect("instance"),
            &TestEnv(temp.path().into()),
        )
        .expect("owned paths");
        let mut execution = devbox_runtime::ExecutionConfig::new(paths);
        execution.windows_interop = path;
        let broker = Arc::new(WindowsBroker::default());
        let cancel = CancellationToken::new();
        configure(&mut execution, broker.clone(), cancel.clone());
        let assigned = receipt();
        let expected = assigned.clone();
        let native = async {
            let request = broker
                .probe_next(7, cancel.clone())
                .await
                .expect("original native session");
            assert_eq!(
                request.probe,
                WindowsProbe::Observation { assigned: expected }
            );
            broker
                .probe_output(
                    7,
                    WindowsProbeReply {
                        probe_id: request.probe_id,
                        result: WindowsProbeResult::Observation {
                            state: ObservationState::Empty,
                        },
                    },
                )
                .expect("exact native result");
        };
        let (observed, ()) = tokio::join!((execution.windows_observation)(assigned), native);
        assert_eq!(
            observed,
            ObservationState::Empty,
            "mismatch is retried, never interpreted as empty itself"
        );
        cancel.cancel();
    }
}
fn listener_config(
    temp: &std::path::Path,
    direct: ListenerObservation,
) -> devbox_runtime::ExecutionConfig {
    let path = temp.join("probe");
    let reply = serde_json::to_string(&WindowsProbeResult::Listeners { state: direct })
        .expect("typed response");
    std::fs::write(&path,format!("#!/usr/bin/python3\nimport sys,json\nrequest=json.load(sys.stdin)\nassert request['type']=='listeners' and request['ports']==[3000,3001]\nassert request['assigned']['sessionId']==2\nprint({reply:?})\n")).expect("owned executable");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).expect("mode");
    let paths = Paths::resolve(
        Instance::parse("test-probe-listeners").expect("instance"),
        &TestEnv(temp.into()),
    )
    .expect("owned paths");
    let mut execution = devbox_runtime::ExecutionConfig::new(paths);
    execution.windows_interop = path;
    execution
}
#[tokio::test]
async fn listener_ownership_falls_back_to_native_only_when_direct_cannot_judge_it() {
    for direct in [
        ListenerObservation::IdentityMismatch,
        ListenerObservation::Unavailable,
    ] {
        let temp = tempfile::tempdir().expect("owned helper");
        let mut execution = listener_config(temp.path(), direct);
        let broker = Arc::new(WindowsBroker::default());
        let cancel = CancellationToken::new();
        configure(&mut execution, broker.clone(), cancel.clone());
        let native = async {
            let request = broker
                .probe_next(7, cancel.clone())
                .await
                .expect("original native session");
            assert_eq!(
                request.probe,
                WindowsProbe::Listeners {
                    assigned: receipt(),
                    ports: vec![3000, 3001]
                }
            );
            broker
                .probe_output(
                    7,
                    WindowsProbeReply {
                        probe_id: request.probe_id,
                        result: WindowsProbeResult::Listeners {
                            state: ListenerObservation::Owned,
                        },
                    },
                )
                .expect("exact native result");
        };
        let (observed, ()) = tokio::join!(
            (execution.windows_listeners)(receipt(), vec![3000, 3001]),
            native
        );
        assert_eq!(observed, ListenerObservation::Owned, "{direct:?}");
        cancel.cancel();
    }
    // A definitive direct answer, including an unrelated listener, is final.
    let temp = tempfile::tempdir().expect("owned helper");
    let mut execution = listener_config(temp.path(), ListenerObservation::NotOwned);
    configure(
        &mut execution,
        Arc::new(WindowsBroker::default()),
        CancellationToken::new(),
    );
    assert_eq!(
        (execution.windows_listeners)(receipt(), vec![3000, 3001]).await,
        ListenerObservation::NotOwned
    );
}
