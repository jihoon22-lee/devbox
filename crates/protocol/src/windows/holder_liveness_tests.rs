//! Rulings 34/35 structural contract: holder identity, the holder liveness check and the two
//! stored control shapes. Real Windows API behaviour is S7-14 W1 acceptance.
use super::*;
const PROBE: &str = include_str!("probe.ps1");
const HOLDER_CHECK: &str = "public static DevboxJobState Holder(uint pid,string creation,string sid,uint session,bool crossSession) {";
fn holder() -> HolderIdentity {
    HolderIdentity {
        nonce: assigned().nonce,
        pid: 4242,
        creation_time: "134000000000000001".into(),
        sid: assigned().sid,
        session_id: assigned().session_id,
    }
}
#[test]
fn holder_ready_marker_carries_the_holder_identity() {
    let hold = at(HOLDER, "public static void Hold(");
    assert!(after(HOLDER, hold, "WriteNew(ready,Identity(nonce))") > hold);
    let identity = member(HOLDER, "static string Identity(string nonce) {");
    // The same identity sources and FILETIME text as the bootstrap's assigned receipt.
    for source in [
        "GetProcessTimes(GetCurrentProcess(),out created,out exited,out kernel,out user)",
        "uint pid=GetCurrentProcessId(), session;",
        "ProcessIdToSessionId(pid,out session)",
        "using(var current=WindowsIdentity.GetCurrent()) { sid=current.User.Value; }",
        "(((ulong)created.High << 32) | created.Low).ToString(CultureInfo.InvariantCulture)",
    ] {
        assert!(identity.contains(source), "{source}");
    }
    assert!(NATIVE.contains(
        "return (((ulong)created.High << 32) | created.Low).ToString(CultureInfo.InvariantCulture);"
    ));
    // The marker is exactly the daemon's HolderIdentity wire shape.
    assert!(identity.contains(
        r#"return "{\"nonce\":\""+nonce+"\",\"pid\":"+pid.ToString(CultureInfo.InvariantCulture)+",\"creationTime\":\""+creation+"\",\"sid\":\""+sid+"\",\"sessionId\":"+session.ToString(CultureInfo.InvariantCulture)+"}";"#
    ));
    let marker = format!(
        r#"{{"nonce":"{}","pid":4242,"creationTime":"134000000000000001","sid":"{}","sessionId":1}}"#,
        assigned().nonce,
        assigned().sid
    );
    let parsed: HolderIdentity = serde_json::from_str(&marker).expect("holder marker");
    assert_eq!(parsed, holder());
    assert!(parsed.valid());
}
#[test]
fn holder_keeps_its_handle_after_a_failed_query() {
    // A holder that leaves without a receipt now releases the reader, so after assignment it
    // leaves only with the receipt. A failed query keeps polling with the handle held.
    let hold = member(HOLDER, "public static void Hold(");
    assert!(!hold.contains("while(QueryInformationJobObject("));
    let poll = at(hold, "while(true) {");
    let query = after(
        hold,
        poll,
        "if(QueryInformationJobObject(job,1,out account,size,IntPtr.Zero)) {",
    );
    let sleep = after(hold, query, "System.Threading.Thread.Sleep(500);");
    let returns: Vec<_> = hold[poll..sleep].match_indices("return;").collect();
    assert_eq!(
        returns.len(),
        2,
        "only the receipt and the idle no-process exit"
    );
    assert!(hold[poll..sleep].contains("{ WriteNew(receipt,body); return; }"));
    assert!(
        hold[poll..sleep]
            .contains("if(account.Total==0 && timer.ElapsedMilliseconds>=60000) return;")
    );
}
#[test]
fn holder_liveness_checks_process_identity_without_a_job_or_session_gate() {
    let check = member(NATIVE, HOLDER_CHECK);
    // Never a job lookup: pid + creation time are global, Local\ job names are per session.
    for banned in [
        "OpenJobObject",
        "IsProcessInJob",
        "QueryInformationJobObject",
        "Session()!=session",
    ] {
        assert!(!check.contains(banned), "{banned}");
    }
    let user = at(
        check,
        "if(!String.Equals(Sid(),sid,StringComparison.Ordinal)) return new DevboxJobState(\"identityMismatch\");",
    );
    let session = at(
        check,
        "if(crossSession && Session()==session) return new DevboxJobState(\"identityMismatch\");",
    );
    let open = at(check, "process=OpenProcess(0x1000,false,pid);");
    assert!(user < session && session < open);
    // Gone: no such PID (87), a reused PID, or an exit code. Alive: STILL_ACTIVE (259).
    for rule in [
        "return new DevboxJobState(Marshal.GetLastWin32Error()==87 ? \"notFound\" : \"unavailable\");",
        "if(!String.Equals(Creation(process),creation,StringComparison.Ordinal)) return new DevboxJobState(\"notFound\");",
        "if(!GetExitCodeProcess(process,out code)) return new DevboxJobState(\"unavailable\");",
        "return code==259 ? new DevboxJobState(\"active\",1) : new DevboxJobState(\"notFound\");",
        "} catch { return new DevboxJobState(\"unavailable\"); }",
        "finally { if(process!=IntPtr.Zero) CloseHandle(process); }",
    ] {
        assert!(check.contains(rule), "{rule}");
    }
}
#[test]
fn observe_keeps_its_session_gate_so_a_listener_probe_from_another_session_is_rejected() {
    let observe = member(NATIVE, "public static DevboxJobState Observe(");
    // The gate is still the first statement, before any process or job is opened.
    assert!(observe.contains(
        "try {\n            if(!String.Equals(Sid(),sid,StringComparison.Ordinal) || Session()!=session)\n                return new DevboxJobState(\"identityMismatch\");\n            process=OpenProcess(0x1000,false,pid);"
    ));
    let listeners = at(PROBE, "$r.type -ceq 'listeners'");
    let end = after(PROBE, listeners, "} else { exit 1 }");
    let branch = &PROBE[listeners..end];
    let observed = at(
        branch,
        "$s=[DevboxObservation]::Observe([string]$a.jobName,[uint32]$a.pid,[string]$a.creationTime,[string]$a.sid,[uint32]$a.sessionId)",
    );
    let check = at(
        branch,
        "if($state -ceq 'active') { $state=[DevboxListeners]::Check(",
    );
    assert!(
        observed < check,
        "another session never reaches the listener check"
    );
    // Neither the job observation nor the listener branch uses the holder check.
    let observation = at(PROBE, "if($r.type -ceq 'observation') {");
    let holder = after(PROBE, observation, "} elseif($r.type -ceq 'holder') {");
    for script in [&PROBE[observation..holder], branch] {
        assert!(!script.contains("::Holder("));
    }
}
#[test]
fn holder_probe_branch_answers_with_observation_states() {
    let start = at(PROBE, "} elseif($r.type -ceq 'holder') {");
    let end = after(PROBE, start, "} elseif($r.type -ceq 'paths') {");
    let branch = &PROBE[start..end];
    assert!(branch.contains(
        "$s=[DevboxObservation]::Holder([uint32]$h.pid,[string]$h.creationTime,[string]$h.sid,[uint32]$h.sessionId,[bool]$r.crossSession)"
    ));
    assert!(branch.contains("@{type='observation';state=$state}"));
    assert!(!branch.contains("::Observe(") && !branch.contains("OpenJobObject"));
    assert!(
        !BOOTSTRAP.contains("::Holder("),
        "the bootstrap never asks about a holder"
    );
}
#[test]
fn holder_probe_wire_shape_and_validation() {
    let probe = WindowsProbe::Holder {
        holder: holder(),
        cross_session: true,
    };
    assert_eq!(
        serde_json::to_string(&probe).expect("probe"),
        format!(
            r#"{{"type":"holder","holder":{{"nonce":"{}","pid":4242,"creationTime":"134000000000000001","sid":"{}","sessionId":1}},"crossSession":true}}"#,
            assigned().nonce,
            assigned().sid
        )
    );
    assert!(probe.valid());
    assert_eq!(
        probe.unavailable(),
        WindowsProbeResult::Observation {
            state: ObservationState::Unavailable
        }
    );
    for (state, ok) in [
        (ObservationState::NotFound, true),
        (ObservationState::Active { count: 1 }, true),
        (ObservationState::Active { count: 0 }, false),
        (ObservationState::Unavailable, true),
        (ObservationState::IdentityMismatch, true),
    ] {
        assert_eq!(
            WindowsProbeResult::Observation { state }.valid_for(&probe),
            ok
        );
    }
    let paths = WindowsProbeResult::Paths {
        state: PathObservation::Unavailable,
    };
    assert!(!paths.valid_for(&probe));
    for broken in [
        HolderIdentity { pid: 0, ..holder() },
        HolderIdentity {
            creation_time: "0".into(),
            ..holder()
        },
        HolderIdentity {
            creation_time: "later".into(),
            ..holder()
        },
        HolderIdentity {
            sid: "admin".into(),
            ..holder()
        },
        HolderIdentity {
            nonce: "short".into(),
            ..holder()
        },
    ] {
        let probe = WindowsProbe::Holder {
            holder: broken,
            cross_session: false,
        };
        assert!(!probe.valid(), "{probe:?}");
    }
}
#[test]
fn holder_identity_belongs_only_to_its_own_attempt() {
    assert!(holder().belongs_to(&assigned()));
    for other in [
        HolderIdentity {
            nonce: "f".repeat(32),
            ..holder()
        },
        HolderIdentity {
            session_id: 2,
            ..holder()
        },
        HolderIdentity {
            sid: "S-1-5-21-9".into(),
            ..holder()
        },
        HolderIdentity { pid: 0, ..holder() },
    ] {
        assert!(!other.belongs_to(&assigned()), "{other:?}");
    }
}
#[test]
fn a_new_control_always_carries_the_holder_paths() {
    let nonce = control().nonce;
    let created = ObservationControl::new("launch".into(), nonce.clone(), "C:\\owned\\control");
    let path = |suffix: &str| format!("C:\\owned\\control\\{nonce}.{suffix}");
    assert_eq!(created.job_name, format!("Local\\Devbox.Observe.{nonce}"));
    assert_eq!(created.assigned_path, path("assigned.json"));
    assert_eq!(created.grant_path, path("grant.json"));
    assert_eq!(created.closed_path, path("closed.json"));
    assert_eq!(created.holder_ready_path, path("holder-ready"));
    assert_eq!(created.emptied_path, path("emptied.json"));
    assert_eq!(created.shape(), Some(ControlShape::Holder));
    assert!(created.valid());
}
#[test]
fn partial_or_malformed_holder_fields_are_invalid_not_rc1() {
    assert_eq!(control().shape(), Some(ControlShape::Holder));
    let run = RunIdentity {
        run_id: "run".into(),
        attempt_id: "attempt".into(),
    };
    assert_eq!(
        control().observes(&run, &assigned()),
        Some(ControlShape::Holder)
    );
    let mut ready_only = control();
    ready_only.emptied_path.clear();
    let mut emptied_only = control();
    emptied_only.holder_ready_path.clear();
    let mut elsewhere = control();
    elsewhere.emptied_path = "C:\\other\\emptied.json".into();
    let mut duplicate = control();
    duplicate.holder_ready_path = duplicate.closed_path.clone();
    for broken in [ready_only, emptied_only, elsewhere, duplicate] {
        assert_eq!(broken.shape(), None, "{broken:?}");
        assert_eq!(broken.observes(&run, &assigned()), None);
        assert!(!broken.valid());
    }
    // A matching receipt is still required for either shape.
    let other = RunIdentity {
        run_id: "other".into(),
        attempt_id: "attempt".into(),
    };
    assert_eq!(control().observes(&other, &assigned()), None);
}
#[test]
fn csharp_blocks_avoid_csharp6_syntax() {
    // Windows PowerShell 5.1 compiles Add-Type with the .NET Framework compiler: C# 5 only.
    let listeners = at(PROBE, "public static class DevboxListeners");
    let blocks = [
        &NATIVE[at(NATIVE, "Add-Type -TypeDefinition @'")..],
        &HOLDER[at(HOLDER, "Add-Type -TypeDefinition '")..],
        &PROBE[listeners..after(PROBE, listeners, "\n'@\n")],
    ];
    for block in blocks {
        for token in [
            "$\"",
            "?.",
            "?[",
            "nameof(",
            "out var",
            "=>",
            " is var",
            "default;",
            "default)",
            "{ get; } =",
            "using static",
            ") when (",
            "?? throw",
        ] {
            assert!(!block.contains(token), "{token}");
        }
    }
}
