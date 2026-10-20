use super::*;
use devbox_protocol::{
    frame::{HEADER_LEN, frame_channel},
    message::ClientMessage,
    windows::{WindowsEvent, WindowsOutput},
};
use serde_json::json;

fn response(id: u32, result: serde_json::Value) -> Vec<u8> {
    let body = serde_json::to_vec(&json!({"type":"response","id":id,"result":result})).unwrap();
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
        "env": {"TEST_SECRET": "fixture-secret"}
    })
}

#[test]
fn windows_native_next_uses_reserved_channel_and_id() {
    let frame = next_request();
    assert_eq!(frame_channel(&frame), Some(0));
    let ClientMessage::Request {
        id, method, params, ..
    } = serde_json::from_slice(&frame[HEADER_LEN..]).unwrap()
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
    let mut body: serde_json::Value = serde_json::from_slice(&frame[HEADER_LEN..]).unwrap();
    body["error"] = json!({"code":"windows.unavailable"});
    let body = serde_json::to_vec(&body).unwrap();
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
        } = serde_json::from_slice(&frame[HEADER_LEN..]).unwrap()
        else {
            panic!("expected output request");
        };
        assert_eq!(id, 101);
        assert_eq!(method, "windows.output");
        let decoded: WindowsEvent = serde_json::from_str(params.get()).unwrap();
        assert_eq!(decoded.run_id, event.run_id);
        assert_eq!(decoded.attempt_id, event.attempt_id);
        assert_eq!(
            serde_json::to_value(decoded.event).unwrap(),
            serde_json::to_value(event.event).unwrap()
        );
    }
}

#[test]
fn windows_native_powershell_arguments_never_contain_request_values() {
    let request = receive(&response(100, run())).unwrap();
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
    super::platform_compile::run_windows(receive(&response(100, run())).unwrap(), output, cancel)
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
