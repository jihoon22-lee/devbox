use super::*;
fn control() -> ObservationControl {
    ObservationControl {
        schema: 1,
        launch_id: "launch".into(),
        nonce: "0123456789abcdef0123456789abcdef".into(),
        job_name: "Local\\Devbox.Observe.0123456789abcdef0123456789abcdef".into(),
        assigned_path: "C:\\owned\\control\\assigned.json".into(),
        grant_path: "C:\\owned\\control\\grant.json".into(),
        closed_path: "C:\\owned\\control\\closed.json".into(),
    }
}
fn assigned() -> ObservationAssigned {
    ObservationAssigned {
        schema: 1,
        run_id: "run".into(),
        launch_id: "launch".into(),
        attempt_id: "attempt".into(),
        nonce: control().nonce,
        job_name: control().job_name,
        pid: 1234,
        creation_time: "134000000000000000".into(),
        sid: "S-1-5-21-1-2-3-1000".into(),
        session_id: 1,
    }
}
#[test]
fn windows_launch_without_observation_control_is_rejected() {
    let old = r#"{"runId":"owned-run","attemptId":"owned-attempt","cwd":"C:\\owned","command":"exit 0","env":{},"ports":[]}"#;
    assert!(
        serde_json::from_str::<RunWindows>(old).is_err(),
        "live transport cannot admit an untracked launch"
    );
}
#[test]
fn windows_observation_grant_identity_requires_exact_launch_attempt_nonce_job_and_schema() {
    let id = RunIdentity {
        run_id: "run".into(),
        attempt_id: "attempt".into(),
    };
    assert!(control().accepts(&id, &assigned()));
    for field in [
        "run", "launch", "attempt", "nonce", "job", "schema", "pid", "creation", "sid",
    ] {
        let mut wrong = assigned();
        match field {
            "run" => wrong.run_id.push('x'),
            "launch" => wrong.launch_id.push('x'),
            "attempt" => wrong.attempt_id.push('x'),
            "nonce" => wrong.nonce.replace_range(..1, "f"),
            "job" => wrong.job_name.push('x'),
            "schema" => wrong.schema = 2,
            "pid" => wrong.pid = 0,
            "creation" => wrong.creation_time = "invalid".into(),
            "sid" => wrong.sid = "other".into(),
            _ => unreachable!(),
        }
        assert!(!control().accepts(&id, &wrong), "{field}");
    }
    let wire = serde_json::to_string(&assigned()).expect("assigned");
    assert_eq!(
        serde_json::from_str::<ObservationAssigned>(&wire).expect("roundtrip"),
        assigned()
    );
}
#[test]
fn windows_control_rejects_relative_reparse_namespace_alias_and_overlapping_paths() {
    for path in [
        "relative",
        "C:\\owned\\..\\grant",
        "C:\\owned\\a:stream",
        "\\\\?\\C:\\owned",
        "C:/owned",
        "C:\\owned\n",
    ] {
        let mut bad = control();
        bad.grant_path = path.into();
        assert!(!bad.valid(), "{path:?}");
    }
    let mut bad = control();
    bad.grant_path = bad.assigned_path.clone();
    assert!(!bad.valid());
    let mut bad = control();
    bad.job_name = "Global\\arbitrary".into();
    assert!(!bad.valid());
    assert!(valid_windows_path(
        "\\\\wsl.localhost\\Owned\\home\\user\\control.json"
    ));
}
#[test]
fn windows_probe_paths_have_joint_count_raw_bytes_and_typed_result_limits() {
    let paths = (0..MAX_WINDOWS_PROBE_PATHS)
        .map(|i| format!("C:\\owned\\file-{i}.rs"))
        .collect::<Vec<_>>();
    let good = WindowsProbe::Paths {
        paths: paths.clone(),
    };
    assert!(good.valid());
    let mut too_many = paths;
    too_many.push("C:\\one-more".into());
    assert!(!WindowsProbe::Paths { paths: too_many }.valid());
    assert!(
        !WindowsProbe::Paths {
            paths: vec![format!("C:\\{}", "x".repeat(14000)); 40]
        }
        .valid()
    );
    assert!(
        !WindowsProbeResult::Observation {
            state: ObservationState::Empty
        }
        .valid_for(&good)
    );
    assert!(
        !WindowsProbeResult::Observation {
            state: ObservationState::Active { count: 0 }
        }
        .valid_for(&WindowsProbe::Observation {
            assigned: assigned()
        })
    );
}
#[test]
fn windows_bootstrap_structurally_orders_receipt_and_grant_before_user_input() {
    // Structural contract only; actual Windows API/PowerShell execution remains separate acceptance.
    let script = include_str!("bootstrap.ps1");
    let associate = script.find("::AssociateFresh").expect("associate");
    let assigned = script.find("::WriteNew").expect("atomic assigned receipt");
    let grant = script.find("ReadBounded($o.grantPath)").expect("grant");
    let env = script.find("SetEnvironmentVariable").expect("user env");
    let cwd = script.find("Set-Location").expect("user cwd");
    let command = script.find("ScriptBlock]::Create").expect("user command");
    assert!(associate < assigned && assigned < grant && grant < env && env < cwd && cwd < command);
    assert!(script[grant..env].contains("Exists($o.closedPath)"));
    assert!(script.contains(&format!(
        "ElapsedMilliseconds -lt {OBSERVATION_GRANT_TIMEOUT_MS}"
    )));
    let native = include_str!("native.ps1");
    assert!(native.contains("error==183"));
    assert!(native.contains("FileMode.CreateNew"));
    assert!(native.contains("File.Move(temporary,path)"));
    assert!(!native.contains("SetInformationJobObject("));
    assert!(!native.contains("TerminateJobObject("));
    assert!(
        native.contains("Sid(),sid,StringComparison.Ordinal")
            && native.contains("Session()!=session")
    );
    assert!(!script.contains("Write-Output $assigned"));
}
