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
#[test]
fn windows_listener_probe_bounds_ports_and_pairs_only_with_listener_results() {
    let probe = |ports: Vec<u16>| WindowsProbe::Listeners {
        assigned: assigned(),
        ports,
    };
    assert!(probe(vec![3000, 3001]).valid());
    assert!(probe((1..=MAX_WINDOWS_PROBE_PORTS as u16).collect()).valid());
    for bad in [
        vec![],
        vec![0],
        vec![3000, 0],
        vec![3000; MAX_WINDOWS_PROBE_PORTS + 1],
    ] {
        assert!(!probe(bad.clone()).valid(), "{bad:?}");
    }
    let mut forged = assigned();
    forged.pid = 0;
    assert!(
        !WindowsProbe::Listeners {
            assigned: forged,
            ports: vec![3000]
        }
        .valid()
    );
    let request = probe(vec![3000]);
    assert_eq!(
        request.unavailable(),
        WindowsProbeResult::Listeners {
            state: ListenerObservation::Unavailable
        }
    );
    for state in [
        ListenerObservation::Owned,
        ListenerObservation::NotOwned,
        ListenerObservation::Empty,
        ListenerObservation::NotFound,
        ListenerObservation::IdentityMismatch,
        ListenerObservation::Unavailable,
    ] {
        let result = WindowsProbeResult::Listeners { state };
        assert!(result.valid_for(&request));
        assert!(!result.valid_for(&WindowsProbe::Observation {
            assigned: assigned()
        }));
        let wire = serde_json::to_string(&result).expect("result");
        assert_eq!(
            serde_json::from_str::<WindowsProbeResult>(&wire).expect("roundtrip"),
            result
        );
    }
    assert!(
        !WindowsProbeResult::Paths {
            state: PathObservation::Safe
        }
        .valid_for(&request)
    );
    let wire = serde_json::to_string(&request).expect("request");
    assert!(wire.contains(r#""type":"listeners""#) && wire.contains(r#""ports":[3000]"#));
    assert_eq!(
        serde_json::from_str::<WindowsProbe>(&wire).expect("roundtrip"),
        request
    );
    let extra = wire.replacen(r#""ports""#, r#""pid":1,"ports""#, 1);
    assert!(serde_json::from_str::<WindowsProbe>(&extra).is_err());
    assert!(
        serde_json::from_str::<WindowsProbeResult>(
            r#"{"type":"listeners","state":"owned","pid":1}"#
        )
        .is_err()
    );
}
#[test]
fn windows_ready_reply_is_typed_and_rejects_unknown_fields() {
    for value in [WindowsReadiness::Ready, WindowsReadiness::NotReady] {
        let wire = serde_json::to_string(&value).expect("reply");
        assert_eq!(
            serde_json::from_str::<WindowsReadiness>(&wire).expect("roundtrip"),
            value
        );
    }
    // A bare string leaves no room for extra fields; any other shape is rejected.
    assert_eq!(
        serde_json::to_string(&WindowsReadiness::NotReady).expect("reply"),
        r#""notReady""#
    );
    for bad in [r#"{"state":"ready"}"#, r#""owned""#, r#"{"rev":"1"}"#] {
        assert!(
            serde_json::from_str::<WindowsReadiness>(bad).is_err(),
            "{bad}"
        );
    }
}
#[test]
fn windows_listener_check_compiles_only_in_the_probe_branch_never_the_bootstrap() {
    // Structural contract only; the C# listener check is not compiled or run on Linux.
    assert!(!POWERSHELL_BOOTSTRAP.contains("GetExtendedTcpTable"));
    let probe = include_str!("probe.ps1");
    let branch = probe
        .find("$r.type -ceq 'listeners'")
        .expect("listener branch");
    let compile = probe
        .find("public static class DevboxListeners")
        .expect("type");
    let observe = probe[branch..]
        .find("::Observe(")
        .expect("receipt validation")
        + branch;
    assert!(branch < compile && compile < observe);
    assert!(probe.contains("GetExtendedTcpTable(buffer,ref size,false,family,3,0)"));
    assert!(probe.contains("IsProcessInJob(process,job,out member)"));
    assert!(probe.contains("again.Contains(pair)"));
    assert!(!probe[compile..].contains("TerminateJobObject("));
}
