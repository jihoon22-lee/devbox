#![cfg(all(target_os = "linux", feature = "helper"))]
use std::{
    io::Write,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
#[test]
fn document_mode_rejects_unknown_messages_and_bounds_an_unfinished_stdin() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_devbox-workspace-wsl"))
        .arg("--document-operation")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"version":1,"method":"taskExecute","paths":[]}"#)
        .unwrap();
    assert_eq!(child.wait().unwrap().code(), Some(2));
    let start = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_devbox-workspace-wsl"))
        .arg("--document-operation")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _open_input = child.stdin.take().unwrap();
    assert_eq!(child.wait().unwrap().code(), Some(124));
    assert!(start.elapsed() < Duration::from_secs(15));
}
