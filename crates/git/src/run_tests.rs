use super::*;
#[cfg(target_os = "linux")]
#[tokio::test]
async fn clone_runner_allows_transfer_past_query_deadline() {
    let fixture = tempfile::tempdir().expect("owned fixture");
    let directory = std::fs::File::open(fixture.path()).expect("owned fixture");
    // An owned Git alias models a slow transfer without any network or global config.
    let result = clone_output(
        &directory,
        &["-c", "alias.fixture-clone=!sleep 31", "fixture-clone"],
    )
    .await
    .expect("clone should outlive the 30-second query deadline");
    assert_eq!(result.0, 0);
}
#[cfg(target_os = "linux")]
async fn effective_ssh_options(command: &str) {
    let ssh = ssh_with_batch(command).expect("owned fixture");
    let mut process = Command::new("sh");
    process
        .args(["-c", &format!("{ssh} -G fixture.invalid")])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let (code, output, _) = execute(process, TIMEOUT).await.expect("owned fixture");
    assert_eq!(code, 0, "SSH environment prefix broke command execution");
    assert!(output.lines().any(|line| line == "batchmode yes"));
    assert!(
        output
            .lines()
            .any(|line| line == "stricthostkeychecking true")
    );
}
#[cfg(target_os = "linux")]
#[tokio::test]
async fn clone_ssh_keeps_assignment_prefix_before_executable() {
    effective_ssh_options("SSH_AUTH_SOCK='/tmp/fixture socket' ssh -F /dev/null -i '/tmp/fixture identity' -oBatchMode=no -oStrictHostKeyChecking=no").await;
}
#[cfg(target_os = "linux")]
#[tokio::test]
async fn clone_ssh_keeps_env_wrapper_before_executable() {
    effective_ssh_options("env SSH_AUTH_SOCK='/tmp/fixture socket' ssh -F /dev/null -i '/tmp/fixture identity' -oBatchMode=no -oStrictHostKeyChecking=no").await;
}
#[cfg(target_os = "linux")]
#[tokio::test]
async fn clone_batch_options_override_conflicting_ssh_options_without_network() {
    let ssh = ssh_with_batch("ssh -F /dev/null -oBatchMode=no -oStrictHostKeyChecking=no")
        .expect("owned fixture");
    let mut command = Command::new("sh");
    command
        .args(["-c", &format!("{ssh} -G fixture.invalid")])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let (code, output, _) = execute(command, TIMEOUT).await.expect("owned fixture");
    assert_eq!(code, 0);
    assert!(
        output.lines().any(|line| line == "batchmode yes"),
        "effective batch mode was not enabled"
    );
    assert!(
        output
            .lines()
            .any(|line| line == "stricthostkeychecking true"),
        "effective strict host key checking was not enabled"
    );
}
#[cfg(target_os = "linux")]
#[test]
fn clone_credential_environment_cannot_launch_interactive_helpers() {
    let fixture = tempfile::tempdir().expect("owned fixture");
    let directory = std::fs::File::open(fixture.path()).expect("owned fixture");
    let command = clone_command(&directory, &["clone", "--", "fixture", "."]);
    let variables: std::collections::HashMap<_, _> = command.as_std().get_envs().collect();
    for key in ["GIT_ASKPASS", "SSH_ASKPASS"] {
        assert_eq!(
            variables[std::ffi::OsStr::new(key)],
            Some(std::ffi::OsStr::new(""))
        );
    }
    assert_eq!(
        variables[std::ffi::OsStr::new("SSH_ASKPASS_REQUIRE")],
        Some(std::ffi::OsStr::new("never"))
    );
    assert_eq!(variables[std::ffi::OsStr::new("GIT_SSH")], None);
    assert_eq!(
        variables[std::ffi::OsStr::new("GCM_INTERACTIVE")],
        Some(std::ffi::OsStr::new("Never"))
    );
}
#[cfg(unix)]
fn fixture(script: &str) -> Command {
    let mut command = Command::new("python3");
    command
        .args(["-c", script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command
}
#[cfg(unix)]
#[tokio::test]
async fn timed_out_command_does_not_leave_owned_descendants() {
    let fixture_dir = tempfile::tempdir().expect("owned fixture");
    let marker = fixture_dir.path().join("survived");
    let child = format!(
        "import time,pathlib; time.sleep(.3); pathlib.Path({:?}).write_text('survived')",
        marker.to_str().expect("owned fixture")
    );
    let script = format!(
        "import subprocess,time; subprocess.Popen(['python3','-c', {child:?}]); time.sleep(60)"
    );
    assert!(matches!(
        execute(fixture(&script), Duration::from_millis(100)).await,
        Err(GitError::TimedOut)
    ));
    tokio::time::sleep(Duration::from_millis(450)).await;
    assert!(
        !marker.exists(),
        "owned descendant survived its command timeout"
    );
}
#[cfg(unix)]
#[tokio::test]
async fn cancelled_command_does_not_leave_owned_descendants() {
    let fixture_dir = tempfile::tempdir().expect("owned fixture");
    let marker = fixture_dir.path().join("survived");
    let ready = fixture_dir.path().join("ready");
    let child = format!(
        "import time,pathlib; time.sleep(.3); pathlib.Path({:?}).write_text('survived')",
        marker.to_str().expect("owned fixture")
    );
    let script = format!(
        "import subprocess,time,pathlib; subprocess.Popen(['python3','-c', {child:?}]); pathlib.Path({:?}).write_text('ready'); time.sleep(60)",
        ready.to_str().expect("owned fixture")
    );
    let task = tokio::spawn(execute(fixture(&script), Duration::from_secs(30)));
    tokio::time::timeout(Duration::from_secs(2), async {
        while !ready.exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("owned fixture");
    task.abort();
    assert!(
        task.await
            .expect_err("fixture operation must fail")
            .is_cancelled()
    );
    tokio::time::sleep(Duration::from_millis(450)).await;
    assert!(!marker.exists(), "owned descendant survived cancellation");
}
#[cfg(unix)]
#[tokio::test]
async fn oversized_stream_stops_without_waiting_for_other_pipe() {
    let command = fixture("import os,time; os.write(1,b'x'*(9*1024*1024)); time.sleep(60)");
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        execute(command, Duration::from_secs(30)),
    )
    .await;
    assert!(matches!(result, Ok(Err(GitError::LimitExceeded))));
}
#[cfg(unix)]
#[tokio::test]
async fn timeout_and_invalid_utf8_are_explicit_failures() {
    assert!(matches!(
        execute(
            fixture("import time; time.sleep(60)"),
            Duration::from_millis(30)
        )
        .await,
        Err(GitError::TimedOut)
    ));
    assert!(matches!(
        execute(
            fixture("import os; os.write(1,b'\\xff')"),
            Duration::from_secs(2)
        )
        .await,
        Err(GitError::InvalidEncoding)
    ));
}
#[cfg(unix)]
#[tokio::test]
async fn stdin_is_written_while_both_outputs_are_drained() {
    let mut command = Command::new("sh");
    command
        .args([
            "-c",
            "head -c 262144 /dev/zero; head -c 262144 /dev/zero >&2; cat >/dev/null",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let (code, out, err) =
        execute_input(command, Duration::from_secs(3), Some(&vec![b'x'; 524288]))
            .await
            .expect("concurrent pipes");
    assert_eq!(code, 0);
    assert_eq!(out.len(), 262144);
    assert_eq!(err.len(), 262144);
}
