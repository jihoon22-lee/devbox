//! Windows-only owned-process fixtures; portable state tests do not replace these.
use super::*;
use std::{
    process::Stdio,
    time::{SystemTime, UNIX_EPOCH},
};
use windows::Win32::System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW};

#[tokio::test]
async fn retained_job_and_exact_child_identity_control_quiescence() {
    let directory = tempfile::tempdir().unwrap();
    let nonce = format!(
        "{:016x}{:016x}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64
    );
    let name = format!("Local\\Devbox.Observe.{nonce}");
    let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
    let job = own(unsafe { CreateJobObjectW(None, PCWSTR(wide.as_ptr())) }.unwrap());
    assert_ne!(
        unsafe { windows::Win32::Foundation::GetLastError() },
        windows::Win32::Foundation::ERROR_ALREADY_EXISTS
    );
    let mut child = tokio::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 60",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let proof = ChildProof::capture(&child).unwrap();
    let run = RunIdentity {
        run_id: "native-proof-run".into(),
        attempt_id: "native-proof-attempt".into(),
    };
    let control = ObservationControl {
        schema: 1,
        launch_id: "native-proof-launch".into(),
        nonce,
        job_name: name,
        assigned_path: directory
            .path()
            .join("assigned.json")
            .to_string_lossy()
            .into_owned(),
        grant_path: directory
            .path()
            .join("grant.json")
            .to_string_lossy()
            .into_owned(),
        closed_path: directory
            .path()
            .join("closed.json")
            .to_string_lossy()
            .into_owned(),
    };
    assert!(proof.capture_job(&run, &control).unwrap().is_none());
    let mut assigned = ObservationAssigned {
        schema: 1,
        run_id: run.run_id.clone(),
        attempt_id: run.attempt_id.clone(),
        launch_id: control.launch_id.clone(),
        nonce: control.nonce.clone(),
        job_name: control.job_name.clone(),
        pid: proof.pid,
        creation_time: proof.created.to_string(),
        sid: proof.sid.clone(),
        session_id: proof.session,
    };
    let write = |value: &ObservationAssigned| {
        std::fs::write(&control.assigned_path, serde_json::to_vec(value).unwrap()).unwrap()
    };
    write(&assigned);
    // A valid receipt naming an unrelated empty Job is never an exit proof.
    assert!(proof.capture_job(&run, &control).is_err());
    // Assignment uses the fixture's original owned child handle. Product proof stays query-only.
    unsafe { AssignProcessToJobObject(raw(&job), HANDLE(child.raw_handle().unwrap())) }.unwrap();
    assigned.creation_time = (proof.created + 1).to_string();
    write(&assigned);
    assert!(proof.capture_job(&run, &control).is_err());
    assigned.creation_time = proof.created.to_string();
    write(&assigned);
    let retained = proof.capture_job(&run, &control).unwrap().unwrap();
    assert_eq!(proof.exited(), Some(false));
    assert_eq!(retained.active(), Some(1));
    drop(job);
    child.kill().await.unwrap();
    child.wait().await.unwrap();
    assert_eq!(proof.exited(), Some(true));
    assert_eq!(retained.active(), Some(0));
    assert_eq!(
        crate::core::native_activity::observe(proof.exited(), retained.active()),
        crate::core::native_activity::Observation::Complete
    );
}
