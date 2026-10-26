use super::*;
use devbox_protocol::windows::ObservationControl;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    sync::{Arc, Mutex},
};
fn request() -> RunWindows {
    RunWindows {
        run_id: "tracking-run".into(),
        attempt_id: "tracking-attempt".into(),
        cwd: "C:\\owned".into(),
        command: "fixture".into(),
        env: Default::default(),
        ports: vec![],
        observation: ObservationControl {
            schema: 1,
            launch_id: "tracking-launch".into(),
            nonce: "0123456789abcdef0123456789abcdef".into(),
            job_name: "Local\\Devbox.Observe.0123456789abcdef0123456789abcdef".into(),
            assigned_path: "C:\\owned\\assigned.json".into(),
            grant_path: "C:\\owned\\grant.json".into(),
            closed_path: "C:\\owned\\closed.json".into(),
        },
    }
}
fn fake(path: &Path, body: &str) {
    fs::write(path, format!("#!/usr/bin/python3\n{body}\n")).expect("fake executable");
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).expect("fake mode");
}
#[tokio::test]
async fn proven_pre_spawn_failures_report_no_child_once() {
    let fixture = tempfile::tempdir().expect("owned fixture");
    let absent = fixture.path().join("absent");
    for case in 0..4 {
        let (output, events) = tokio::sync::mpsc::channel(4);
        let cancel = CancellationToken::new();
        let mut request = request();
        if case == 0 {
            cancel.cancel();
        }
        let events = if case == 1 {
            drop(events);
            None
        } else {
            Some(events)
        };
        if case == 2 {
            request.observation.nonce.clear();
        }
        let called = Arc::new(Mutex::new(Vec::new()));
        let observed = called.clone();
        run_observed(request, output, cancel, absent.as_os_str(), move |child| {
            observed.lock().expect("observed").push(child.is_some());
            true
        })
        .await;
        assert_eq!(*called.lock().expect("called"), [false]);
        drop(events);
    }
}
#[tokio::test]
async fn exact_child_is_reported_before_any_payload_and_not_cleared_on_finished() {
    let fixture = tempfile::tempdir().expect("owned fixture");
    let executable = fixture.path().join("child");
    let marker = fixture.path().join("received");
    fake(
        &executable,
        &format!(
            "import sys,json\nfrom pathlib import Path\nr=json.load(sys.stdin)\nPath({:?}).write_text(r['runId'])",
            marker.to_str().expect("utf-8 marker")
        ),
    );
    let (output, mut events) = tokio::sync::mpsc::channel(4);
    let called = Arc::new(Mutex::new(Vec::new()));
    let observed = called.clone();
    let before_input = marker.clone();
    run_observed(
        request(),
        output,
        CancellationToken::new(),
        executable.as_os_str(),
        move |child| {
            assert!(!before_input.exists());
            assert!(child.and_then(tokio::process::Child::id).is_some());
            observed.lock().expect("observed").push(child.is_some());
            true
        },
    )
    .await;
    assert_eq!(fs::read_to_string(marker).expect("marker"), "tracking-run");
    assert!(matches!(
        events.recv().await.expect("event").event,
        WindowsOutput::Finished { code: 0 }
    ));
    assert_eq!(*called.lock().expect("called"), [true]);
}
#[tokio::test]
async fn failed_tracking_cannot_send_a_user_payload() {
    let fixture = tempfile::tempdir().expect("owned fixture");
    let executable = fixture.path().join("child");
    let marker = fixture.path().join("received");
    fake(
        &executable,
        &format!(
            "import sys\nfrom pathlib import Path\nr=sys.stdin.read()\nif r: Path({:?}).write_text(r)",
            marker.to_str().expect("utf-8 marker")
        ),
    );
    let (output, mut events) = tokio::sync::mpsc::channel(4);
    run_observed(
        request(),
        output,
        CancellationToken::new(),
        executable.as_os_str(),
        |child| {
            assert!(child.is_some());
            false
        },
    )
    .await;
    assert!(!marker.exists());
    assert!(matches!(
        events.recv().await.expect("event").event,
        WindowsOutput::Failed
    ));
}
#[tokio::test]
async fn cancellation_after_spawn_never_reports_no_spawn() {
    let fixture = tempfile::tempdir().expect("owned fixture");
    let executable = fixture.path().join("child");
    fake(&executable, "import time\ntime.sleep(30)");
    let (output, _events) = tokio::sync::mpsc::channel(4);
    let cancel = CancellationToken::new();
    let token = cancel.clone();
    let called = Arc::new(Mutex::new(Vec::new()));
    let observed = called.clone();
    run_observed(
        request(),
        output,
        cancel,
        executable.as_os_str(),
        move |child| {
            observed.lock().expect("observed").push(child.is_some());
            token.cancel();
            true
        },
    )
    .await;
    assert_eq!(*called.lock().expect("called"), [true]);
}
