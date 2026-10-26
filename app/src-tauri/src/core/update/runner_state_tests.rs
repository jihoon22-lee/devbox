use super::*;
fn binding() -> Binding {
    Binding {
        operation: 1,
        update_generation: 2,
        quit_generation: 3,
        target_version: "1.0.1".into(),
        fixture_install: false,
    }
}
fn parent() -> ProcessIdentity {
    ProcessIdentity {
        pid: 42,
        started: 123,
        session: 1,
    }
}
#[test]
fn disarmed_runner_cannot_install_after_later_ordinary_exit() {
    let mut gate = Gate::new(binding(), parent()).unwrap();
    gate.arm(&binding(), parent()).unwrap();
    gate.disarm();
    assert_eq!(gate.commit(), Err(RunnerError::InvalidBinding));
    assert!(gate.exited(parent()).is_err());
    assert!(gate.take_install().is_err());
}
#[test]
fn prepared_or_draining_app_never_grants_installer_authority() {
    let mut gate = Gate::new(binding(), parent()).unwrap();
    assert!(gate.commit().is_err());
    assert!(gate.exited(parent()).is_err());
    assert!(gate.take_install().is_err());
    gate.arm(&binding(), parent()).unwrap();
    assert!(gate.take_install().is_err());
    gate.commit().unwrap();
    // Bridge drain has started but there is no exact process-exit evidence yet.
    assert!(gate.take_install().is_err());
    assert!(gate.finish().is_err());
}
#[test]
fn exact_exit_after_commit_allows_exactly_one_install_and_one_finish() {
    let mut gate = Gate::new(binding(), parent()).unwrap();
    gate.arm(&binding(), parent()).unwrap();
    gate.commit().unwrap();
    assert!(gate.commit().is_err());
    gate.exited(parent()).unwrap();
    gate.take_install().unwrap();
    assert!(gate.take_install().is_err());
    gate.finish().unwrap();
    assert!(gate.finish().is_err());
    gate.disarm();
    assert!(gate.commit().is_err());
    assert!(gate.take_install().is_err());
}
#[test]
fn changed_operation_quit_channel_mode_or_process_cannot_arm_or_install() {
    for change in 0..5 {
        let mut changed = binding();
        match change {
            0 => changed.operation += 1,
            1 => changed.update_generation += 1,
            2 => changed.quit_generation += 1,
            3 => changed.target_version = "1.0.2".into(),
            _ => changed.fixture_install = true,
        }
        assert!(
            Gate::new(binding(), parent())
                .unwrap()
                .arm(&changed, parent())
                .is_err()
        );
    }
    for changed in [
        ProcessIdentity {
            started: 124,
            ..parent()
        },
        ProcessIdentity {
            pid: 43,
            ..parent()
        },
        ProcessIdentity {
            session: 2,
            ..parent()
        },
    ] {
        let mut gate = Gate::new(binding(), parent()).unwrap();
        assert!(gate.arm(&binding(), changed).is_err());
        gate.arm(&binding(), parent()).unwrap();
        gate.commit().unwrap();
        assert!(gate.exited(changed).is_err());
        assert!(gate.take_install().is_err());
    }
}
#[test]
fn expiry_before_or_after_commit_cannot_be_revived_by_late_exit() {
    for committed in [false, true] {
        let mut gate = Gate::new(binding(), parent()).unwrap();
        gate.arm(&binding(), parent()).unwrap();
        if committed {
            gate.commit().unwrap();
        }
        gate.disarm();
        assert!(gate.exited(parent()).is_err());
        assert!(gate.commit().is_err());
        assert!(gate.take_install().is_err());
    }
}
#[test]
fn malformed_generation_version_or_parent_identity_is_refused() {
    for field in 0..4 {
        let mut value = binding();
        match field {
            0 => value.operation = 0,
            1 => value.update_generation = 0,
            2 => value.quit_generation = 0,
            _ => value.target_version = "not a release".into(),
        }
        assert!(Gate::new(value, parent()).is_err());
    }
    assert!(Gate::new(binding(), ProcessIdentity { pid: 0, ..parent() }).is_err());
    assert!(
        Gate::new(
            binding(),
            ProcessIdentity {
                started: 0,
                ..parent()
            }
        )
        .is_err()
    );
}
