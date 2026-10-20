use crate::{
    host::ProcFs,
    processes::{ProcessSampler, parse_stat},
    *,
};
use std::{
    fs,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

pub(crate) fn stat(pid: u32, ticks: u64, start: u64) -> String {
    // Fields 3..24; comm contains both whitespace and closing parentheses.
    format!("{pid} (worker (a) b)) S 1 1 1 0 0 0 0 0 0 0 {ticks} 0 0 0 20 0 1 0 {start} 4096 3\n")
}
#[test]
fn process_stat_cpu_delta_priority_and_disappearing_entries() {
    let parsed = parse_stat(&stat(32, 100, 1234)).expect("fixture value");
    assert_eq!(parsed.name, "worker (a) b)");
    assert_eq!(parsed.start_ticks, 1234);
    assert_eq!(parsed.cpu_ticks, 100);
    assert_eq!(parsed.rss_pages, 3);
    let temp = tempfile::tempdir().expect("fixture result");
    fs::write(temp.path().join("stat"), "btime 1700000000\n").expect("fixture result");
    let now = Arc::new(AtomicU64::new(1000));
    let clock = now.clone();
    let proc = ProcFs::fixture(
        temp.path().into(),
        Arc::new(move || clock.load(Ordering::Relaxed)),
        100,
        4096,
    );
    for pid in 10..70 {
        let path = temp.path().join(pid.to_string());
        fs::create_dir(&path).expect("fixture result");
        fs::write(path.join("stat"), stat(pid, 100, 1234)).expect("fixture result");
        std::os::unix::fs::symlink(
            if pid == 69 {
                "/project/sub"
            } else {
                "/elsewhere"
            },
            path.join("cwd"),
        )
        .expect("fixture result");
    }
    fs::create_dir(temp.path().join("99")).expect("fixture result"); // exited before stat
    let mut sampler = ProcessSampler::default();
    let projects = vec![ProjectRoot {
        id: "p".into(),
        path: "/project".into(),
    }];
    let first = sampler
        .sample(&proc, &projects, &[], Some("p"))
        .expect("fixture result");
    assert_eq!(first.len(), 51);
    assert_eq!(first[0].pid, 69);
    assert_eq!(first[0].start_time_ms, 1_700_000_012_340);
    assert_eq!(first[0].rss_bytes, 12288);
    assert!(first.iter().all(|p| p.cpu_percent == 0.0));
    now.store(2000, Ordering::Relaxed);
    fs::write(temp.path().join("10/stat"), stat(10, 150, 1234)).expect("fixture result");
    let next = sampler
        .sample(&proc, &projects, &[], Some("p"))
        .expect("fixture result");
    assert_eq!(next[0].pid, 69);
    assert_eq!(next[1].pid, 10);
    assert_eq!(next[1].cpu_percent, 50.0);
    for pid in 10..70 {
        let cwd = temp.path().join(format!("{pid}/cwd"));
        fs::remove_file(&cwd).expect("fixture result");
        std::os::unix::fs::symlink("/project/sub", cwd).expect("fixture result");
    }
    assert_eq!(
        sampler
            .sample(&proc, &projects, &[], Some("p"))
            .expect("fixture result")
            .len(),
        60,
        "all project rows survive the 50 other-process limit"
    );
    assert!(
        crate::processes::project_for(Some(std::path::Path::new("/project-other")), &projects)
            .is_none()
    );
    // PID reuse resets the CPU sample, and prefix siblings are not project children.
    fs::write(temp.path().join("10/stat"), stat(10, 900, 2000)).expect("fixture result");
    now.store(3000, Ordering::Relaxed);
    assert_eq!(
        sampler
            .sample(&proc, &projects, &[], None)
            .expect("fixture result")
            .iter()
            .find(|p| p.pid == 10)
            .expect("fixture value")
            .cpu_percent,
        0.0
    );
}
#[tokio::test]
async fn kill_refuses_a_reused_pid_and_signals_only_owned_children() {
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    for signal in [KillSignal::Term, KillSignal::Kill] {
        let mut owned = Child(
            std::process::Command::new("sleep")
                .arg("60")
                .spawn()
                .expect("fixture result"),
        );
        let child = &mut owned.0;
        let pid = child.id();
        let current = crate::processes::identity(pid).expect("fixture result");
        let result = crate::processes::kill_process(KillProcess {
            pid,
            signal,
            expect: ProcessExpectation {
                start_time_ms: current.start_time_ms + 1,
            },
        });
        assert!(matches!(
            result,
            Err(devbox_protocol::Fail::Domain(ObserveError::StateChanged))
        ));
        assert!(child.try_wait().expect("fixture result").is_none());
        crate::processes::kill_process(KillProcess {
            pid,
            signal,
            expect: ProcessExpectation {
                start_time_ms: current.start_time_ms,
            },
        })
        .expect("fixture result");
        assert!(!child.wait().expect("fixture result").success());
    }
}
#[test]
fn kill_refuses_other_users_processes_and_invalid_pids() {
    for (pid, permission) in [(1, true), (0, false), (u32::MAX, false)] {
        let result = crate::processes::kill_process(KillProcess {
            pid,
            signal: KillSignal::Term,
            expect: ProcessExpectation { start_time_ms: 0 },
        });
        assert!(matches!(
            (result, permission),
            (
                Err(devbox_protocol::Fail::Domain(
                    ObserveError::PermissionDenied
                )),
                true
            ) | (
                Err(devbox_protocol::Fail::Domain(ObserveError::InvalidInput)),
                false
            )
        ));
    }
}

#[test]
fn selected_project_keeps_unit_owned_processes_even_inside_another_project() {
    let temp = tempfile::tempdir().expect("owned proc fixture");
    fs::write(temp.path().join("stat"), "btime 1700000000\n").expect("boot time");
    for pid in 10..72 {
        let path = temp.path().join(pid.to_string());
        fs::create_dir(&path).expect("process directory");
        fs::write(path.join("stat"), stat(pid, 100, 1234)).expect("process stat");
        std::os::unix::fs::symlink(
            match pid {
                70 => "/agent-worktrees/task",
                71 => "/other-project/sub",
                _ => "/absolute-cwd",
            },
            path.join("cwd"),
        )
        .expect("process cwd");
        if pid >= 70 {
            fs::write(
                path.join("cgroup"),
                "0::/user.slice/devbox-run-selected.service\n",
            )
            .expect("owned run unit");
        }
    }
    let proc = ProcFs::fixture(temp.path().into(), Arc::new(|| 0), 100, 4096);
    let rows = ProcessSampler::default()
        .sample(
            &proc,
            &[
                ProjectRoot {
                    id: "selected".into(),
                    path: "/project".into(),
                },
                ProjectRoot {
                    id: "other".into(),
                    path: "/other-project".into(),
                },
            ],
            &[UnitRun {
                unit: "devbox-run-selected.service".into(),
                run_id: "run".into(),
                project_id: "selected".into(),
            }],
            Some("selected"),
        )
        .expect("process snapshot");
    assert_eq!(
        rows.len(),
        52,
        "both unit-owned processes survive, including cwd inside another project, plus 50 global rows"
    );
    assert_eq!(
        rows[..2].iter().map(|row| row.pid).collect::<Vec<_>>(),
        [70, 71]
    );
    assert!(
        rows[..2]
            .iter()
            .all(|row| row.project_id.as_deref() == Some("selected"))
    );
}
