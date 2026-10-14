mod support;
use std::process::Stdio;
use support::{ChildGuard, TestDaemon, exits};

#[test]
fn absent_agent_is_a_noop_without_reading_stdin() {
    let d = TestDaemon::fixture();
    let mut command = d.environment(env!("CARGO_BIN_EXE_devbox"));
    command
        .args(["agent", "event", "--from", "claude"])
        .env_remove("DEVBOX_AGENT_ID")
        .stdin(Stdio::piped());
    let child = ChildGuard(command.spawn().expect("spawn hook"));
    assert!(exits(child).success());
}

#[test]
fn unavailable_daemon_does_not_fail_tool_hook() {
    let d = TestDaemon::fixture();
    let mut command = d.environment(env!("CARGO_BIN_EXE_devbox"));
    command
        .args(["agent", "event", "--from", "setup", "--ok"])
        .env("DEVBOX_AGENT_ID", "owned-agent");
    assert!(exits(ChildGuard(command.spawn().expect("spawn hook"))).success());
}

#[test]
fn blocked_stdin_is_bounded_even_when_agent_is_set() {
    let d = TestDaemon::fixture();
    let mut command = d.environment(env!("CARGO_BIN_EXE_devbox"));
    command
        .args(["agent", "event", "--from", "claude"])
        .env("DEVBOX_AGENT_ID", "owned-agent")
        .stdin(Stdio::piped());
    assert!(exits(ChildGuard(command.spawn().expect("spawn hook"))).success());
}
