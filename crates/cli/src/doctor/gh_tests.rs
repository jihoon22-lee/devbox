use super::*;
use std::{
    os::unix::fs::PermissionsExt,
    time::{Duration, Instant},
};
fn fixture(supported: bool, status: i32, delay: bool) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().expect("owned fixture");
    let help = temp.path().join("help.txt");
    let mut text = include_str!("fixtures/gh-auth-status-old.txt").to_owned();
    if supported {
        text.push_str("\n      --no-refresh   Do not refresh authentication tokens\n");
    }
    std::fs::write(&help, text).expect("public help fixture");
    let calls = temp.path().join("calls");
    let script = temp.path().join("gh-fixture");
    let delay = if delay { "sleep 1.2\n" } else { "" };
    let content = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\n{}case \"$*\" in\n'auth status --help') cat {}; exit 0;;\n'auth status --no-refresh') printf fixture-private-auth-output; printf fixture-private-error >&2; exit {};;\n*) exit 97;;\nesac\n",
        devbox_core::sh_quote(calls.to_str().expect("path")),
        delay,
        devbox_core::sh_quote(help.to_str().expect("path")),
        status
    );
    std::fs::write(&script, content).expect("owned command fixture");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))
        .expect("executable fixture");
    (temp, script, calls)
}
#[test]
fn old_gh_without_nonrefresh_capability_is_unknown_and_never_queries_auth() {
    let (_temp, script, calls) = fixture(false, 1, false);
    assert_eq!(
        runner::run(gh_auth_status(script.to_str().expect("path"))),
        Err(ProbeIssue::UnsupportedCapability)
    );
    assert_eq!(
        std::fs::read_to_string(calls).expect("calls"),
        "auth status --help\n"
    );
}
#[test]
fn capable_gh_checks_help_first_and_returns_only_the_exit_status() {
    for (status, expected) in [
        (0, Ok(true)),
        (1, Ok(false)),
        (2, Err(ProbeIssue::Unavailable)),
    ] {
        let (_temp, script, calls) = fixture(true, status, false);
        assert_eq!(
            runner::run(gh_auth_status(script.to_str().expect("path"))),
            expected
        );
        assert_eq!(
            std::fs::read_to_string(calls).expect("calls"),
            "auth status --help\nauth status --no-refresh\n"
        );
    }
}
#[test]
fn gh_help_and_status_share_one_two_second_deadline() {
    let (_temp, script, _calls) = fixture(true, 0, true);
    let start = Instant::now();
    assert_eq!(
        runner::run(gh_auth_status(script.to_str().expect("path"))),
        Err(ProbeIssue::Timeout)
    );
    assert!(start.elapsed() < Duration::from_secs(4));
}
