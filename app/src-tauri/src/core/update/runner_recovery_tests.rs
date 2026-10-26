use super::*;
use crate::core::update::runner::files::{self, tests::staged};
fn runner() -> ProcessIdentity {
    ProcessIdentity {
        pid: 43,
        started: 124,
        session: 1,
    }
}
fn installer() -> ProcessIdentity {
    ProcessIdentity {
        pid: 44,
        started: 125,
        session: 1,
    }
}
fn recorded(
    outcome: Outcome,
    error: Option<RunnerError>,
    child: Option<ProcessIdentity>,
) -> (tempfile::TempDir, Manifest) {
    let (temp, mut stage) = staged();
    let manifest = stage.manifest.clone();
    let dir = manifest.runner_exe.parent().unwrap();
    files::write_new(
        &dir.join("spawn.json"),
        &SpawnRecord {
            nonce: manifest.nonce.clone(),
            runner: runner(),
        },
    )
    .unwrap();
    files::record(&manifest, runner(), outcome, child, error).unwrap();
    stage.keep();
    drop(stage);
    (temp, manifest)
}
fn root(manifest: &Manifest) -> &Path {
    manifest.runner_exe.parent().unwrap().parent().unwrap()
}
#[test]
fn absent_directory_is_read_only_and_has_no_recovery_notice() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("absent");
    assert_eq!(
        inspect(&path, AppChannel::Prod, |_| panic!("no process query")),
        Ok(Recovery {
            operation: None,
            outcome: RecoveryOutcome::None
        })
    );
    assert!(!path.exists());
}
#[test]
fn failed_installer_or_relaunch_is_durable_until_ack_and_only_owned_files_are_cleaned() {
    for (error, expected) in [
        (RunnerError::Installer, RecoveryOutcome::InstallFailed),
        (RunnerError::Relaunch, RecoveryOutcome::RelaunchFailed),
    ] {
        let (temp, manifest) = recorded(Outcome::Failed, Some(error), Some(installer()));
        let before = fs::read(temp.path().join("downloaded.exe")).unwrap();
        let status = inspect(root(&manifest), AppChannel::Prod, |_| Ok(false)).unwrap();
        assert_eq!(
            status,
            Recovery {
                operation: Some(7),
                outcome: expected
            }
        );
        assert_eq!(
            inspect(root(&manifest), AppChannel::Prod, |_| Ok(false)).unwrap(),
            status
        );
        acknowledge(root(&manifest), AppChannel::Prod, 7, |_| Ok(false)).unwrap();
        assert!(!manifest.runner_exe.parent().unwrap().exists());
        assert_eq!(
            fs::read(temp.path().join("downloaded.exe")).unwrap(),
            before
        );
        assert!(temp.path().join("application/Devbox.exe").exists());
    }
}
#[test]
fn completed_runner_can_relaunch_while_cleanup_waits_for_its_exact_exit() {
    let (_temp, manifest) = recorded(Outcome::Installed, None, Some(installer()));
    let status = inspect(root(&manifest), AppChannel::Prod, |process| {
        Ok(process == runner())
    })
    .unwrap();
    assert_eq!(
        status,
        Recovery {
            operation: None,
            outcome: RecoveryOutcome::Success
        }
    );
    assert!(
        !early_busy(root(&manifest), AppChannel::Prod, |process| Ok(
            process == runner()
        ))
        .unwrap()
    );
    assert!(
        acknowledge(root(&manifest), AppChannel::Prod, 7, |process| Ok(
            process == runner()
        ))
        .is_err()
    );
    assert_eq!(
        inspect(root(&manifest), AppChannel::Prod, |_| Ok(false))
            .unwrap()
            .operation,
        Some(7)
    );
    acknowledge(root(&manifest), AppChannel::Prod, 7, |_| Ok(false)).unwrap();
}
#[test]
fn committed_runner_or_live_installer_blocks_new_app_start_and_new_install() {
    for outcome in [Outcome::Committed, Outcome::Running, Outcome::Unknown] {
        let (_temp, manifest) = recorded(outcome, None, Some(installer()));
        for active in [runner(), installer()] {
            assert!(
                early_busy(root(&manifest), AppChannel::Prod, |process| Ok(
                    process == active
                ))
                .unwrap()
            );
            assert_eq!(
                inspect(root(&manifest), AppChannel::Prod, |process| Ok(
                    process == active
                ))
                .unwrap()
                .outcome,
                RecoveryOutcome::UnknownBusy
            );
            assert!(
                acknowledge(root(&manifest), AppChannel::Prod, 7, |process| Ok(
                    process == active
                ))
                .is_err()
            );
        }
    }
}
#[test]
fn timeout_or_unknown_result_never_becomes_permission_to_retry_after_processes_exit() {
    let (_temp, manifest) = recorded(
        Outcome::Unknown,
        Some(RunnerError::Unknown),
        Some(installer()),
    );
    assert_eq!(
        inspect(root(&manifest), AppChannel::Prod, |_| Ok(false))
            .unwrap()
            .outcome,
        RecoveryOutcome::UnknownBusy
    );
    assert!(acknowledge(root(&manifest), AppChannel::Prod, 7, |_| Ok(false)).is_err());
    assert!(!early_busy(root(&manifest), AppChannel::Prod, |_| Ok(false)).unwrap());
    assert!(manifest.installer.exists());
    assert!(
        inspect(root(&manifest), AppChannel::Prod, |_| Err(
            RunnerError::Unknown
        ))
        .is_err()
    );
}
#[test]
fn crash_after_installer_spawn_before_pid_record_keeps_normal_startup_blocked() {
    for (outcome, error) in [
        (Outcome::Committed, None),
        (Outcome::Running, None),
        (Outcome::Unknown, Some(RunnerError::Unknown)),
        (Outcome::Failed, Some(RunnerError::Unknown)),
    ] {
        let (_temp, manifest) = recorded(outcome, error, None);
        assert!(startup_blocked(early_busy(
            root(&manifest),
            AppChannel::Prod,
            |_| Ok(false)
        )));
    }
}
#[test]
fn owned_record_io_or_process_query_errors_fail_closed_at_the_actual_startup_policy() {
    for name in ["owner.json", "spawn.json", "outcome.json"] {
        let (_temp, manifest) = recorded(Outcome::Committed, None, None);
        let path = manifest.runner_exe.parent().unwrap().join(name);
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        let result = early_busy(root(&manifest), AppChannel::Prod, |_| Ok(false));
        assert!(result.is_err());
        assert!(startup_blocked(result));
    }
    let (_temp, manifest) = recorded(
        Outcome::Unknown,
        Some(RunnerError::Unknown),
        Some(installer()),
    );
    assert!(startup_blocked(early_busy(
        root(&manifest),
        AppChannel::Prod,
        |_| Err(RunnerError::Unknown)
    )));
    assert!(!startup_blocked(early_busy(
        root(&manifest),
        AppChannel::Prod,
        |_| Ok(false)
    )));
    assert_eq!(
        inspect(root(&manifest), AppChannel::Prod, |_| Ok(false))
            .unwrap()
            .outcome,
        RecoveryOutcome::UnknownBusy
    );
}
#[test]
fn unrelated_and_ownerless_directories_have_no_startup_or_cleanup_authority() {
    let (_temp, manifest) = recorded(Outcome::Installed, None, None);
    for name in ["install-user-not-owned", "install-other-owner"] {
        let path = root(&manifest).join(name);
        fs::create_dir(&path).unwrap();
        fs::write(path.join("user.txt"), b"preserve").unwrap();
        if name == "install-other-owner" {
            fs::write(
                path.join("owner.json"),
                br#"{"schema":2,"channel":"unrelated"}"#,
            )
            .unwrap();
        }
    }
    assert!(!startup_blocked(early_busy(
        root(&manifest),
        AppChannel::Prod,
        |_| Ok(false)
    )));
    acknowledge(root(&manifest), AppChannel::Prod, 7, |_| Ok(false)).unwrap();
    for name in ["install-user-not-owned", "install-other-owner"] {
        assert_eq!(
            fs::read(root(&manifest).join(name).join("user.txt")).unwrap(),
            b"preserve"
        );
    }
}
#[test]
fn changed_binding_or_missing_record_is_not_interpreted_as_success() {
    let (_temp, manifest) = recorded(Outcome::Installed, None, None);
    let path = manifest.runner_exe.parent().unwrap().join("outcome.json");
    let mut record = files::read_record(&path).unwrap();
    record.binding.quit_generation += 1;
    fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
    assert!(inspect(root(&manifest), AppChannel::Prod, |_| Ok(false)).is_err());
    assert!(acknowledge(root(&manifest), AppChannel::Prod, 7, |_| Ok(false)).is_err());
    fs::remove_file(path).unwrap();
    assert!(inspect(root(&manifest), AppChannel::Prod, |_| Ok(false)).is_err());
}
#[test]
fn acknowledgment_preserves_unexpected_user_entry_and_replaced_installer() {
    let (temp, manifest) = recorded(Outcome::Failed, Some(RunnerError::Installer), None);
    let dir = manifest.runner_exe.parent().unwrap();
    fs::write(dir.join("user.txt"), b"not ours").unwrap();
    assert!(acknowledge(root(&manifest), AppChannel::Prod, 7, |_| Ok(false)).is_err());
    assert_eq!(fs::read(dir.join("user.txt")).unwrap(), b"not ours");
    fs::remove_file(dir.join("user.txt")).unwrap();
    let bytes = fs::read(&manifest.installer).unwrap();
    fs::rename(&manifest.installer, temp.path().join("saved-installer.exe")).unwrap();
    fs::write(&manifest.installer, &bytes).unwrap();
    assert!(acknowledge(root(&manifest), AppChannel::Prod, 7, |_| Ok(false)).is_err());
    assert_eq!(fs::read(&manifest.installer).unwrap(), bytes);
    assert!(dir.join("owner.json").exists());
}
