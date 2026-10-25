use super::*;
use std::{fs, os::unix::fs::PermissionsExt};
fn assigned() -> ObservationAssigned {
    ObservationAssigned {
        schema: 1,
        run_id: "run".into(),
        launch_id: "launch".into(),
        attempt_id: "attempt".into(),
        nonce: "0123456789abcdef0123456789abcdef".into(),
        job_name: "Local\\Devbox.Observe.0123456789abcdef0123456789abcdef".into(),
        pid: 1234,
        creation_time: "134000000000000000".into(),
        sid: "S-1-5-21-1-2-3-1000".into(),
        session_id: 1,
    }
}
fn fake(root: &Path, body: &str) -> std::path::PathBuf {
    let file = root.join("owned-probe");
    fs::write(&file, format!("#!/usr/bin/python3\n{body}\n")).expect("fixture");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o700)).expect("mode");
    file
}
#[tokio::test]
async fn windows_observation_uses_stdin_exact_identity_and_keeps_unknown_distinct_from_empty() {
    let temp = tempfile::tempdir().expect("fixture");
    for state in [
        ObservationState::Active { count: 2 },
        ObservationState::Empty,
        ObservationState::NotFound,
        ObservationState::Unavailable,
        ObservationState::IdentityMismatch,
    ] {
        let result = WindowsProbeResult::Observation {
            state: state.clone(),
        };
        let body = format!(
            "import sys,json\nr=json.load(sys.stdin)\na=r['assigned']\nassert a['creationTime']=='134000000000000000' and a['sessionId']==1\nassert a['nonce'] not in ' '.join(sys.argv)\nassert 'QueryInformationJobObject' in sys.argv[-1]\nprint({:?})",
            serde_json::to_string(&result).expect("result")
        );
        let path = fake(temp.path(), &body);
        assert_eq!(
            query(
                &path,
                WindowsProbe::Observation {
                    assigned: assigned()
                },
                CancellationToken::new()
            )
            .await,
            result
        );
    }
}
#[tokio::test]
async fn windows_observation_rejects_wrong_kind_malformed_overflow_and_failed_helpers() {
    let temp = tempfile::tempdir().expect("fixture");
    for body in [
        "print('{\"type\":\"paths\",\"state\":\"safe\"}')",
        "print('{\"type\":\"observation\",\"state\":{\"state\":\"active\",\"count\":0}}')",
        "print('not JSON')",
        "print(' '*5000)",
        "import sys\nprint('{\"type\":\"observation\",\"state\":{\"state\":\"empty\"}}')\nsys.exit(1)",
    ] {
        let path = fake(temp.path(), body);
        assert_eq!(
            query(
                &path,
                WindowsProbe::Observation {
                    assigned: assigned()
                },
                CancellationToken::new()
            )
            .await,
            WindowsProbeResult::Observation {
                state: ObservationState::Unavailable
            }
        );
    }
    assert_eq!(
        query(
            &temp.path().join("absent"),
            WindowsProbe::Observation {
                assigned: assigned()
            },
            CancellationToken::new()
        )
        .await,
        WindowsProbeResult::Observation {
            state: ObservationState::Unavailable
        }
    );
}
#[tokio::test]
async fn windows_observation_timeout_and_dropped_caller_reap_owned_probe_without_empty_result() {
    let temp = tempfile::tempdir().expect("fixture");
    let marker = temp.path().join("pid");
    let path = fake(
        temp.path(),
        &format!(
            "import os,time\nopen({:?},'w').write(str(os.getpid()))\ntime.sleep(60)",
            marker.to_str().expect("path")
        ),
    );
    assert_eq!(
        query_with_timeout(
            &path,
            WindowsProbe::Observation {
                assigned: assigned()
            },
            CancellationToken::new(),
            Duration::from_millis(150)
        )
        .await,
        WindowsProbeResult::Observation {
            state: ObservationState::Unavailable
        }
    );
    let pid = fs::read_to_string(&marker).expect("pid");
    assert!(!Path::new(&format!("/proc/{pid}")).exists());
    fs::remove_file(&marker).expect("own marker");
    let task = tokio::spawn(async move {
        query_with_timeout(
            &path,
            WindowsProbe::Observation {
                assigned: assigned(),
            },
            CancellationToken::new(),
            Duration::from_secs(60),
        )
        .await
    });
    let pid = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Ok(pid) = fs::read_to_string(&marker)
                && pid.parse::<u32>().is_ok()
            {
                break pid;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("started");
    task.abort();
    let _ = task.await;
    tokio::time::timeout(Duration::from_secs(2), async {
        while Path::new(&format!("/proc/{pid}")).exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("reaped");
}
#[tokio::test]
async fn windows_path_batch_uses_one_helper_and_invalid_or_cancelled_queries_do_not_spawn() {
    let temp = tempfile::tempdir().expect("fixture");
    let marker = temp.path().join("calls");
    let path = fake(
        temp.path(),
        &format!(
            "import json,sys\nr=json.load(sys.stdin)\nassert len(r['paths'])==1024\nopen({:?},'a').write('one')\nprint('{{\"type\":\"paths\",\"state\":\"safe\"}}')",
            marker.to_str().expect("path")
        ),
    );
    let request = WindowsProbe::Paths {
        paths: (0..1024).map(|n| format!("C:\\owned\\{n}.rs")).collect(),
    };
    assert_eq!(
        query(&path, request.clone(), CancellationToken::new()).await,
        WindowsProbeResult::Paths {
            state: PathObservation::Safe
        }
    );
    assert_eq!(fs::read_to_string(&marker).expect("calls"), "one");
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        query(&path, request, cancel).await,
        WindowsProbeResult::Paths {
            state: PathObservation::Unavailable
        }
    );
    assert_eq!(
        query(
            &path,
            WindowsProbe::Paths {
                paths: vec!["relative".into()]
            },
            CancellationToken::new()
        )
        .await,
        WindowsProbeResult::Paths {
            state: PathObservation::Unavailable
        }
    );
    assert_eq!(fs::read_to_string(marker).expect("calls"), "one");
}
