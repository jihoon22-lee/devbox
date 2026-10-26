use super::*;
use devbox_protocol::{
    frame::{HEADER_LEN, frame_channel},
    message::ClientMessage,
    windows::{WindowsEvent, WindowsOutput},
};
use serde_json::json;

fn response(id: u32, result: serde_json::Value) -> Vec<u8> {
    let body = serde_json::to_vec(&json!({"type":"response","id":id,"result":result}))
        .expect("fixture result");
    let mut frame = ((body.len() + 5) as u32).to_le_bytes().to_vec();
    frame.push(0);
    frame.extend_from_slice(&0u32.to_le_bytes());
    frame.extend(body);
    frame
}

fn run() -> serde_json::Value {
    json!({
        "runId": "run-fixture",
        "attemptId": "attempt-fixture",
        "cwd": "C:\\dev\\a'b c",
        "command": "Write-Output $env:TEST_SECRET",
        "ports": [18432, 18433],
        "env": {"TEST_SECRET": "fixture-secret"},
        "observation": {"schema":1,"launchId":"owned-launch","nonce":"0123456789abcdef0123456789abcdef","jobName":"Local\\Devbox.Observe.0123456789abcdef0123456789abcdef","assignedPath":"C:\\owned\\control\\assigned.json","grantPath":"C:\\owned\\control\\grant.json","closedPath":"C:\\owned\\control\\closed.json"}
    })
}

#[test]
fn windows_native_next_uses_reserved_channel_and_id() {
    let frame = next_request();
    assert_eq!(frame_channel(&frame), Some(0));
    let ClientMessage::Request {
        id, method, params, ..
    } = serde_json::from_slice(&frame[HEADER_LEN..]).expect("fixture result")
    else {
        panic!("expected request");
    };
    assert_eq!(id, 100);
    assert_eq!(method, "windows.next");
    assert_eq!(params.get(), "{}");
}

#[test]
fn windows_native_receive_preserves_run_attempt_cwd_and_stdin_environment() {
    let request = receive(&response(100, run())).expect("native run request");
    assert_eq!(request.run_id, "run-fixture");
    assert_eq!(request.attempt_id, "attempt-fixture");
    assert_eq!(request.cwd, "C:\\dev\\a'b c");
    assert_eq!(request.command, "Write-Output $env:TEST_SECRET");
    assert_eq!(request.env["TEST_SECRET"], "fixture-secret");
}

#[test]
fn windows_native_rejects_renderer_channel_other_ids_and_malformed_frames() {
    let valid = response(100, run());
    let mut renderer = valid.clone();
    renderer[5..9].copy_from_slice(&1u32.to_le_bytes());
    let mut wrong_kind = valid.clone();
    wrong_kind[4] = 1;
    let mut wrong_length = valid.clone();
    wrong_length[0] ^= 1;
    let mut oversized = valid.clone();
    oversized.resize(
        devbox_protocol::windows::MAX_WINDOWS_REQUEST_BYTES + 1,
        b' ',
    );
    let size = (oversized.len() - 4) as u32;
    oversized[..4].copy_from_slice(&size.to_le_bytes());
    for invalid in [
        renderer,
        wrong_kind,
        wrong_length,
        oversized,
        response(99, run()),
        response(100, json!(null)),
        response(100, json!({"runId":"missing-fields"})),
        vec![],
        vec![0; 8],
        valid[..valid.len() - 1].to_vec(),
    ] {
        assert!(receive(&invalid).is_none());
    }
}

#[test]
fn windows_native_error_response_cannot_also_start_a_run() {
    let mut frame = response(100, run());
    let mut body: serde_json::Value =
        serde_json::from_slice(&frame[HEADER_LEN..]).expect("fixture result");
    body["error"] = json!({"code":"windows.unavailable"});
    let body = serde_json::to_vec(&body).expect("fixture result");
    frame.truncate(HEADER_LEN);
    frame[..4].copy_from_slice(&((body.len() + 5) as u32).to_le_bytes());
    frame.extend(body);
    assert!(receive(&frame).is_none());
}

#[test]
fn windows_native_output_keeps_origin_and_attempt() {
    for output in [
        WindowsOutput::Stdout {
            data: "한글".as_bytes().to_vec(),
        },
        WindowsOutput::Stderr {
            data: b"error".to_vec(),
        },
        WindowsOutput::Finished { code: 42 },
        WindowsOutput::Failed,
        WindowsOutput::Ready,
        WindowsOutput::Heartbeat,
    ] {
        let event = WindowsEvent {
            run_id: "run-fixture".into(),
            attempt_id: "attempt-fixture".into(),
            event: output,
        };
        let frame = output_request(101, &event);
        assert_eq!(frame_channel(&frame), Some(0));
        let ClientMessage::Request {
            id, method, params, ..
        } = serde_json::from_slice(&frame[HEADER_LEN..]).expect("fixture result")
        else {
            panic!("expected output request");
        };
        assert_eq!(id, 101);
        assert_eq!(method, "windows.output");
        let decoded: WindowsEvent = serde_json::from_str(params.get()).expect("fixture result");
        assert_eq!(decoded.run_id, event.run_id);
        assert_eq!(decoded.attempt_id, event.attempt_id);
        assert_eq!(
            serde_json::to_value(decoded.event).expect("fixture result"),
            serde_json::to_value(event.event).expect("fixture result")
        );
    }
}

#[test]
fn windows_native_powershell_arguments_never_contain_request_values() {
    let request = receive(&response(100, run())).expect("fixture value");
    let args = powershell_args();
    assert_eq!(args[..3], ["-NoProfile", "-NonInteractive", "-Command"]);
    assert_eq!(args[3], devbox_protocol::windows::POWERSHELL_BOOTSTRAP);
    for value in [&request.cwd, &request.command, &request.env["TEST_SECRET"]] {
        assert!(args.iter().all(|arg| !arg.contains(value)));
    }
}

#[cfg(not(windows))]
#[tokio::test]
async fn windows_native_precancelled_run_does_not_spawn() {
    let cancel = tokio_util::sync::CancellationToken::new();
    cancel.cancel();
    let (output, mut events) = tokio::sync::mpsc::channel(16);
    super::platform_compile::run_windows(
        receive(&response(100, run())).expect("fixture value"),
        output,
        cancel,
    )
    .await;
    assert!(events.recv().await.is_none());
}

#[test]
fn windows_native_ack_failure_cancels_only_matching_attempt() {
    use tokio_util::sync::CancellationToken;
    let matching = CancellationToken::new();
    let other = CancellationToken::new();
    let key = ("run".into(), "attempt-a".into());
    let other_key = ("run".into(), "attempt-b".into());
    let mut runs = std::collections::HashMap::from([
        (key.clone(), matching.clone()),
        (other_key.clone(), other.clone()),
    ]);
    finish_run(&mut runs, &key);
    assert!(matching.is_cancelled());
    assert!(!other.is_cancelled());
    assert_eq!(runs.len(), 1);
    assert!(runs.contains_key(&other_key));
    finish_run(&mut runs, &key);
    assert!(!other.is_cancelled());
}

#[test]
fn windows_native_probe_channel_and_capacity_are_independent_of_run_dispatch() {
    use devbox_protocol::windows::{
        PathObservation, WindowsProbe, WindowsProbeReply, WindowsProbeRequest, WindowsProbeResult,
    };
    let probe = WindowsProbeRequest {
        probe_id: "owned-probe".into(),
        probe: WindowsProbe::Paths {
            paths: (0..1024).map(|n| format!("C:\\owned\\{n}.rs")).collect(),
        },
    };
    let wire = response(PROBE_NEXT_ID, serde_json::to_value(&probe).expect("probe"));
    assert_eq!(receive_probe(&wire), Some(probe.clone()));
    assert!(receive(&wire).is_none());
    let mut renderer = wire.clone();
    renderer[5..9].copy_from_slice(&7u32.to_le_bytes());
    assert!(receive_probe(&renderer).is_none());
    assert!(
        receive_probe(&response(
            PROBE_OUTPUT_ID,
            serde_json::to_value(&probe).expect("probe")
        ))
        .is_none()
    );
    assert!(receive_probe(&response(PROBE_NEXT_ID, run())).is_none());
    let mut huge = wire;
    huge.resize(
        devbox_protocol::windows::MAX_WINDOWS_PROBE_BYTES + 1025,
        b' ',
    );
    let size = (huge.len() - 4) as u32;
    huge[..4].copy_from_slice(&size.to_le_bytes());
    assert!(receive_probe(&huge).is_none());
    let next = probe_next_request();
    let ClientMessage::Request { id, method, .. } =
        serde_json::from_slice(&next[HEADER_LEN..]).expect("probe next")
    else {
        panic!("request")
    };
    assert_eq!((id, method.as_str()), (PROBE_NEXT_ID, "windows.probe_next"));
    let reply = WindowsProbeReply {
        probe_id: probe.probe_id,
        result: WindowsProbeResult::Paths {
            state: PathObservation::Unavailable,
        },
    };
    let output = probe_output_request(&reply);
    let ClientMessage::Request {
        id, method, params, ..
    } = serde_json::from_slice(&output[HEADER_LEN..]).expect("reply")
    else {
        panic!("request")
    };
    assert_eq!(
        (id, method.as_str()),
        (PROBE_OUTPUT_ID, "windows.probe_output")
    );
    assert_eq!(
        serde_json::from_str::<WindowsProbeReply>(params.get()).expect("reply identity"),
        reply
    );
}
#[cfg(not(windows))]
#[tokio::test]
async fn windows_native_precancelled_probe_does_not_spawn_or_report_empty() {
    use devbox_protocol::windows::{WindowsProbe, WindowsProbeRequest};
    let cancel = tokio_util::sync::CancellationToken::new();
    cancel.cancel();
    let (output, mut events) = tokio::sync::mpsc::channel(1);
    super::platform_compile::run_probe(
        WindowsProbeRequest {
            probe_id: "probe".into(),
            probe: WindowsProbe::Paths {
                paths: vec!["C:\\owned".into()],
            },
        },
        output,
        cancel,
    )
    .await;
    assert!(events.recv().await.is_none());
}

#[cfg(not(windows))]
#[tokio::test]
async fn windows_native_invalid_observation_never_spawns_a_live_adapter() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().expect("fixture");
    let marker = temp.path().join("executed");
    let executable = temp.path().join("owned-fake-powershell");
    std::fs::write(
        &executable,
        format!(
            "#!/usr/bin/python3\nopen({:?},'w').write('executed')\n",
            marker.to_str().expect("path")
        ),
    )
    .expect("fixture");
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).expect("mode");
    let mut request = receive(&response(100, run())).expect("valid request");
    request.observation.nonce = "invalid".into();
    let (output, mut events) = tokio::sync::mpsc::channel(1);
    super::platform_compile::run_with_executable(
        request,
        output,
        tokio_util::sync::CancellationToken::new(),
        executable.as_os_str(),
    )
    .await;
    assert!(matches!(
        events.recv().await.expect("failure").event,
        WindowsOutput::Failed
    ));
    assert!(!marker.exists());
}
