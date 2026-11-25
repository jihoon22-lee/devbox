use super::*;
use crate::doctor::OsProbe;
use devbox_tools::doctor::Probe;
use std::os::unix::fs::PermissionsExt;

fn caller(reason: Reason) -> Lookup {
    Lookup::Caller(reason)
}

#[test]
fn a_usable_env_file_is_used_and_only_its_allowlisted_variables() {
    let temp = tempfile::tempdir().expect("owned fixture");
    let file = temp.path().join("env");
    std::fs::write(
        &file,
        "LANG=\"C.UTF-8\"\nFIXTURE_TOKEN=\"private\"\nPATH=\"/owned/a:/owned/b\"\nSHELL=\"/bin/bash\"\n",
    )
    .expect("owned environment");
    assert_eq!(
        select(&file, false),
        Lookup::Instance(vec![
            ("LANG".into(), "C.UTF-8".into()),
            ("PATH".into(), "/owned/a:/owned/b".into()),
            ("SHELL".into(), "/bin/bash".into()),
        ])
    );
    // The app's start-up check never reads it.
    assert_eq!(select(&file, true), caller(Reason::Startup));
}

#[test]
fn an_absent_malformed_or_oversized_env_file_falls_back_to_the_caller() {
    let temp = tempfile::tempdir().expect("owned fixture");
    let file = temp.path().join("env");
    assert_eq!(select(&file, false), caller(Reason::Missing));
    let oversized = format!("PATH=\"/usr/bin\"\n{}", "#".repeat(runner::LIMIT));
    for text in [
        "",
        "LANG=\"C\"\n",
        "PATH=\"\"\n",
        "PATH=\"relative\"\n",
        "PATH=/usr/bin\n",
        "PATH=\"/usr/bin\"trailing\n",
        // The last `PATH` line is the one systemd keeps.
        "PATH=\"/usr/bin\"\nPATH=\"relative\"\n",
        oversized.as_str(),
    ] {
        std::fs::write(&file, text).expect("owned environment");
        assert_eq!(select(&file, false), caller(Reason::Invalid), "{text:.40}");
    }
    std::fs::write(&file, [b'P', 0xff, b'\n']).expect("owned environment");
    assert_eq!(select(&file, false), caller(Reason::Invalid));
}

#[test]
fn a_linked_or_non_regular_env_file_is_not_used_and_is_named_as_such() {
    let temp = tempfile::tempdir().expect("owned fixture");
    let target = temp.path().join("target");
    std::fs::write(&target, "PATH=\"/usr/bin\"\n").expect("owned environment");
    let file = temp.path().join("env");
    std::os::unix::fs::symlink(&target, &file).expect("owned link");
    assert_eq!(select(&file, false), caller(Reason::NotRegular));
    std::fs::remove_file(&file).expect("owned link");
    std::os::unix::fs::symlink(temp.path().join("gone"), &file).expect("owned dangling link");
    assert_eq!(select(&file, false), caller(Reason::NotRegular));
    std::fs::remove_file(&file).expect("owned link");
    std::fs::create_dir(&file).expect("owned folder");
    assert_eq!(select(&file, false), caller(Reason::NotRegular));
}

#[test]
fn a_tool_that_is_only_in_the_selected_path_is_found_and_started_from_there() {
    let temp = tempfile::tempdir().expect("owned fixture");
    let tools = temp.path().join("tools");
    std::fs::create_dir(&tools).expect("owned tool folder");
    for (name, body) in [
        ("tmux", "printf '%s\\n' \"tmux 9.9 ${LANG}\""),
        ("devbox-fixture-tool", "exit 0"),
    ] {
        let path = tools.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("owned tool");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .expect("owned tool mode");
    }
    let file = temp.path().join("env");
    std::fs::write(
        &file,
        format!("LANG=\"fixture-lang\"\nPATH=\"{}\"\n", tools.display()),
    )
    .expect("owned environment");

    let probe = OsProbe::new(file.clone(), temp.path().join("unused-hook"))
        .looking_in(select(&file, false));
    assert_eq!(probe.daemon_path(), tools.display().to_string());
    assert_eq!(
        probe.which("devbox-fixture-tool", &probe.daemon_path()),
        Ok(true)
    );
    // The command is found by name in the file's `PATH` and gets the file's variables.
    assert_eq!(
        probe.tmux_version(),
        Ok(Some("tmux 9.9 fixture-lang".into()))
    );

    // Without a selected environment the probe is the daemon's: the process environment.
    let own =
        OsProbe::new(file, temp.path().join("unused-hook")).looking_in(caller(Reason::Missing));
    assert_eq!(own.daemon_path(), std::env::var("PATH").unwrap_or_default());
    assert_eq!(
        own.which("devbox-fixture-tool", &own.daemon_path()),
        Ok(false)
    );
}

#[test]
fn every_report_names_its_environment_in_text_and_json() {
    let file = Path::new("/owned/config/env");
    let instance = Used::of(&Lookup::Instance(Vec::new()), file);
    assert_eq!(
        instance.line(),
        "점검 환경: 데몬 환경 파일 /owned/config/env의 PATH로 도구를 찾았습니다(에이전트 CLI의 셸 조회는 이 셸에서 합니다)"
    );
    assert_eq!(
        serde_json::to_value(&instance).expect("JSON"),
        serde_json::json!({ "source": "instance", "file": "/owned/config/env" })
    );
    for (reason, name, said) in [
        (Reason::Startup, "startup", "앱이 시작할 때 하는 점검"),
        (Reason::Missing, "missing", "데몬 환경 파일이 없습니다"),
        (Reason::Invalid, "invalid", "쓸 수 있는 PATH가 없거나"),
        (Reason::NotRegular, "notRegular", "일반 파일이 아닙니다"),
        (Reason::Unreadable, "unreadable", "읽지 못했습니다"),
    ] {
        let used = Used::of(&caller(reason), file);
        let line = used.line();
        assert!(
            line.starts_with("점검 환경: 이 셸의 PATH로 도구를 찾았습니다("),
            "{line}"
        );
        assert!(line.contains(said), "{line}");
        // Only an unreadable regular file is about its permissions.
        assert_eq!(
            line.contains("권한"),
            reason == Reason::Unreadable,
            "{line}"
        );
        // A fallback says what to do about it; the start-up check is not a fallback.
        assert_eq!(
            line.contains("devbox setup") && line.contains("/owned/config/env"),
            reason != Reason::Startup,
            "{line}"
        );
        assert_eq!(
            serde_json::to_value(&used).expect("JSON"),
            serde_json::json!({ "source": "caller", "file": "/owned/config/env", "reason": name })
        );
    }
    let report = devbox_protocol::system::DoctorReport { checks: Vec::new() };
    let output = Output {
        environment: &instance,
        report: &report,
    };
    assert_eq!(
        serde_json::to_value(&output).expect("JSON"),
        serde_json::json!({
            "environment": { "source": "instance", "file": "/owned/config/env" },
            "checks": []
        })
    );
}
