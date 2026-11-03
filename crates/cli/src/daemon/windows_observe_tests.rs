use super::*;
use devbox_core::{Env, Instance, Paths};
use std::{os::unix::fs::PermissionsExt, path::PathBuf, time::Duration};
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
fn observation_config(
    temp: &std::path::Path,
    direct: ObservationState,
) -> devbox_runtime::ExecutionConfig {
    let path = temp.join("probe");
    let reply = serde_json::to_string(&WindowsProbeResult::Observation { state: direct })
        .expect("typed response");
    std::fs::write(&path,format!("#!/usr/bin/python3\nimport sys,json\nrequest=json.load(sys.stdin)\nassert request['type']=='observation'\nprint({reply:?})\n")).expect("owned executable");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).expect("mode");
    let paths = Paths::resolve(
        Instance::parse("test-probe-missing").expect("instance"),
        &TestEnv(temp.into()),
    )
    .expect("owned paths");
    let mut execution = devbox_runtime::ExecutionConfig::new(paths);
    execution.windows_interop = path;
    execution
}
#[tokio::test]
async fn a_missing_job_is_re_asked_natively_and_stands_when_native_cannot_tell() {
    // Without the holder's receipt a direct NotFound proves nothing, so the native session is
    // asked too. Only an answer about the job itself replaces the direct one.
    for (answer, expected) in [
        (
            ObservationState::Active { count: 1 },
            ObservationState::Active { count: 1 },
        ),
        (ObservationState::Empty, ObservationState::Empty),
        (ObservationState::NotFound, ObservationState::NotFound),
        (
            ObservationState::IdentityMismatch,
            ObservationState::NotFound,
        ),
        (ObservationState::Unavailable, ObservationState::NotFound),
    ] {
        let temp = tempfile::tempdir().expect("owned helper");
        let mut execution = observation_config(temp.path(), ObservationState::NotFound);
        let broker = Arc::new(WindowsBroker::default());
        let cancel = CancellationToken::new();
        configure(&mut execution, broker.clone(), cancel.clone());
        let state = answer.clone();
        let native = async {
            let request = broker
                .probe_next(7, cancel.clone())
                .await
                .expect("original native session");
            assert_eq!(
                request.probe,
                WindowsProbe::Observation {
                    assigned: receipt()
                }
            );
            broker
                .probe_output(
                    7,
                    WindowsProbeReply {
                        probe_id: request.probe_id,
                        result: WindowsProbeResult::Observation { state },
                    },
                )
                .expect("exact native result");
        };
        let both = async { tokio::join!((execution.windows_observation)(receipt()), native) };
        let (observed, ()) = tokio::time::timeout(Duration::from_secs(5), both)
            .await
            .expect("a missing job is re-asked natively");
        assert_eq!(observed, expected, "native {answer:?}");
        cancel.cancel();
    }
    // No Windows app: the direct NotFound stands, so a receipt can still release the reader.
    let temp = tempfile::tempdir().expect("owned helper");
    let mut execution = observation_config(temp.path(), ObservationState::NotFound);
    configure(
        &mut execution,
        Arc::new(WindowsBroker::default()),
        CancellationToken::new(),
    );
    assert_eq!(
        (execution.windows_observation)(receipt()).await,
        ObservationState::NotFound
    );
}
#[test]
fn the_merge_keeps_a_direct_answer_about_the_job_when_native_cannot_tell() {
    use ObservationState::*;
    let active = || Active { count: 1 };
    for (direct, native, merged) in [
        (NotFound, None, NotFound),
        (NotFound, Some(Unavailable), NotFound),
        (NotFound, Some(IdentityMismatch), NotFound),
        (NotFound, Some(Empty), Empty),
        (NotFound, Some(active()), active()),
        // After sign-out or a reboot direct interop runs in another session. Its mismatch
        // reaches the runtime, whose holder check can still release the reader without the app.
        (IdentityMismatch, None, IdentityMismatch),
        (IdentityMismatch, Some(Unavailable), IdentityMismatch),
        (IdentityMismatch, Some(IdentityMismatch), IdentityMismatch),
        (IdentityMismatch, Some(NotFound), NotFound),
        (IdentityMismatch, Some(Empty), Empty),
        (IdentityMismatch, Some(active()), active()),
        (Unavailable, None, Unavailable),
        (Unavailable, Some(Unavailable), Unavailable),
        (Unavailable, Some(IdentityMismatch), IdentityMismatch),
        (Unavailable, Some(Empty), Empty),
    ] {
        let label = format!("{direct:?} + {native:?}");
        assert_eq!(retried(direct, native), merged, "{label}");
    }
}
#[tokio::test]
async fn a_direct_mismatch_reaches_the_runtime_without_the_windows_app() {
    let temp = tempfile::tempdir().expect("owned helper");
    let mut execution = observation_config(temp.path(), ObservationState::IdentityMismatch);
    configure(
        &mut execution,
        Arc::new(WindowsBroker::default()),
        CancellationToken::new(),
    );
    assert_eq!(
        (execution.windows_observation)(receipt()).await,
        ObservationState::IdentityMismatch
    );
}
fn holder() -> HolderIdentity {
    HolderIdentity {
        nonce: "a".repeat(32),
        pid: 202,
        creation_time: "132000000000000001".into(),
        sid: "S-1-5-21-123".into(),
        session_id: 2,
    }
}
fn holder_config(
    temp: &std::path::Path,
    direct: ObservationState,
    cross_session: bool,
) -> devbox_runtime::ExecutionConfig {
    let path = temp.join("probe");
    let reply = serde_json::to_string(&WindowsProbeResult::Observation { state: direct })
        .expect("typed response");
    let cross = if cross_session { "True" } else { "False" };
    std::fs::write(&path,format!("#!/usr/bin/python3\nimport sys,json\nrequest=json.load(sys.stdin)\nassert request['type']=='holder' and request['crossSession'] is {cross}\nassert request['holder']['pid']==202 and request['holder']['sessionId']==2\nprint({reply:?})\n")).expect("owned executable");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).expect("mode");
    let paths = Paths::resolve(
        Instance::parse("test-probe-holder").expect("instance"),
        &TestEnv(temp.into()),
    )
    .expect("owned paths");
    let mut execution = devbox_runtime::ExecutionConfig::new(paths);
    execution.windows_interop = path;
    execution
}
#[tokio::test]
async fn holder_liveness_is_asked_directly_and_natively_only_when_direct_cannot_answer() {
    // A direct answer stands. A mismatch is the check's own SID or session answer, so it is
    // never re-asked in another session (that would wait for the broker and turn unavailable).
    for direct in [
        ObservationState::NotFound,
        ObservationState::Active { count: 1 },
        ObservationState::IdentityMismatch,
    ] {
        let temp = tempfile::tempdir().expect("owned helper");
        let mut execution = holder_config(temp.path(), direct.clone(), true);
        configure(
            &mut execution,
            Arc::new(WindowsBroker::default()),
            CancellationToken::new(),
        );
        let observed = tokio::time::timeout(
            Duration::from_secs(5),
            (execution.windows_holder)(holder(), true),
        )
        .await
        .expect("bounded holder check");
        assert_eq!(observed, direct);
    }
    // Direct interop unavailable: the native session runs the same holder probe.
    let temp = tempfile::tempdir().expect("owned helper");
    let mut execution = holder_config(temp.path(), ObservationState::Unavailable, false);
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
            WindowsProbe::Holder {
                holder: holder(),
                cross_session: false
            }
        );
        broker
            .probe_output(
                7,
                WindowsProbeReply {
                    probe_id: request.probe_id,
                    result: WindowsProbeResult::Observation {
                        state: ObservationState::NotFound,
                    },
                },
            )
            .expect("exact native result");
    };
    let both = async { tokio::join!((execution.windows_holder)(holder(), false), native) };
    let (observed, ()) = tokio::time::timeout(Duration::from_secs(5), both)
        .await
        .expect("native holder answer");
    assert_eq!(observed, ObservationState::NotFound);
    cancel.cancel();
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
