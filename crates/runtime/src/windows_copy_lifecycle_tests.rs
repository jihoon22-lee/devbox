use crate::{
    RuntimeError, RuntimeService, windows_copy::CopyIdentity, windows_copy_store as store,
    windows_copy_test_support::*,
};
use devbox_protocol::{Fail, windows::ObservationState};
use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

#[tokio::test]
async fn owned_markers_preserve_legacy_outputs_and_refuse_changed_source_or_foreign_owner() {
    let o = owned().await;
    let legacy = o.f.root.path().join("legacy-repo");
    std::fs::create_dir_all(legacy.join("node_modules")).expect("legacy");
    std::fs::write(legacy.join("manual"), "user content").expect("legacy file");
    o.copy.identity.ensure_owned().expect("first owner");
    std::fs::create_dir_all(o.copy.target.join("node_modules")).expect("output");
    std::fs::write(o.copy.target.join("node_modules/keep"), "output").expect("keep output");
    o.copy.identity.ensure_owned().expect("same owner");
    let other = o.f.root.path().join("other");
    std::fs::create_dir(&other).expect("other source");
    let mut moved = o.copy.identity.clone();
    moved.source = other;
    assert!(moved.ensure_owned().is_err());
    let mut foreign = o.copy.identity.clone();
    foreign.owner = "other distro".into();
    assert!(foreign.ensure_owned().is_err());
    assert_eq!(
        std::fs::read_to_string(legacy.join("manual")).expect("legacy unchanged"),
        "user content"
    );
    assert_eq!(
        std::fs::read_to_string(o.copy.target.join("node_modules/keep")).expect("output unchanged"),
        "output"
    );
    assert!(marker(&o.copy.identity.mapped_base).exists());
}
#[tokio::test]
async fn unmarked_nonempty_target_or_symlink_marker_is_not_adopted() {
    use std::os::unix::fs::symlink;
    let o = owned().await;
    let namespace = o
        .copy
        .identity
        .mapped_base
        .parent()
        .expect("project")
        .parent()
        .expect("owner")
        .parent()
        .expect("namespace");
    std::fs::create_dir_all(namespace).expect("unowned directory");
    std::fs::write(namespace.join("user.txt"), "preserve").expect("user file");
    assert!(o.copy.identity.ensure_owned().is_err());
    assert!(!marker(namespace).exists());
    std::fs::remove_file(namespace.join("user.txt")).expect("remove own fixture");
    o.copy
        .identity
        .ensure_owned()
        .expect("empty namespace can be marked");
    let real = o.f.root.path().join("real-marker");
    std::fs::rename(marker(&o.copy.identity.mapped_base), &real).expect("fixture marker");
    symlink(&real, marker(&o.copy.identity.mapped_base)).expect("fixture symlink");
    assert!(o.copy.identity.ensure_owned().is_err());
}
#[tokio::test]
async fn instance_and_effective_worktree_ids_separate_copies_without_renaming_legacy_paths() {
    let o = owned().await;
    let main = CopyIdentity::new(&o.config, &o.f.project, "main", &o.copy.source).expect("main");
    let worktree =
        CopyIdentity::new(&o.config, &o.f.project, "agent-123", &o.copy.source).expect("worktree");
    let mut config = o.config.clone();
    config.windows_owner = "other-instance-channel-distro".into();
    let other =
        CopyIdentity::new(&config, &o.f.project, "main", &o.copy.source).expect("other instance");
    assert_ne!(main.base, worktree.base);
    assert_ne!(main.base, other.base);
    assert!(main.base.contains(&o.f.project));
    assert!(main.directory().ends_with("\\main\\files"));
}
#[tokio::test]
async fn shared_consumer_does_not_resync_or_erase_windows_outputs() {
    let o = owned().await;
    let svc = service(&o);
    let first = svc
        .reserve_windows_copy(
            o.copy.clone(),
            "service".into(),
            "devbox-test-copy-service.service".into(),
        )
        .await
        .expect("first consumer");
    first.submitted().await.expect("dispatch");
    first.launched().await.expect("submission ACK");
    std::fs::write(o.copy.target.join("input.txt"), "command changed copy")
        .expect("owned command output");
    let second = svc
        .reserve_windows_copy(
            o.copy.clone(),
            "dependent".into(),
            "devbox-test-copy-dependent.service".into(),
        )
        .await
        .expect("compatible dependent");
    assert_eq!(first.reference.revision, second.reference.revision);
    assert_eq!(
        std::fs::read_to_string(o.copy.target.join("input.txt")).expect("no resync"),
        "command changed copy"
    );
    std::fs::write(o.copy.source.join("input.txt"), "source v2").expect("new source");
    assert!(matches!(
        svc.reserve_windows_copy(
            o.copy.clone(),
            "changed".into(),
            "devbox-test-copy-changed.service".into()
        )
        .await,
        Err(Fail::Domain(RuntimeError::WindowsCopyBusy))
    ));
    svc.shutdown().await;
}
struct Release(Arc<(Mutex<bool>, Condvar)>);
impl Release {
    fn release(&self) {
        let (lock, wake) = &*self.0;
        *lock.lock().expect("gate") = true;
        wake.notify_all();
    }
}
impl Drop for Release {
    fn drop(&mut self) {
        self.release();
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_caller_cannot_release_writer_until_the_actual_publisher_worker_exits() {
    let mut o = owned().await;
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let release = Release(gate.clone());
    let (entered, mut enters) = tokio::sync::mpsc::unbounded_channel();
    let visits = Arc::new(AtomicUsize::new(0));
    let count = visits.clone();
    o.config.windows_publish_hook = Some(Arc::new(move || {
        if count.fetch_add(1, Ordering::SeqCst) == 0 {
            entered.send(()).expect("entered");
            let (lock, wake) = &*gate;
            let mut done = lock.lock().expect("gate");
            while !*done {
                done = wake.wait(done).expect("gate wait");
            }
        }
    }));
    let svc = service(&o);
    let first_svc = svc.clone();
    let first_copy = o.copy.clone();
    let first = tokio::spawn(async move {
        first_svc
            .reserve_windows_copy(
                first_copy,
                "cancelled".into(),
                "devbox-test-copy-cancelled.service".into(),
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), enters.recv())
        .await
        .expect("publisher reached")
        .expect("publisher");
    first.abort();
    assert!(matches!(first.await,Err(error) if error.is_cancelled()));
    let key = o.copy.identity.key.clone();
    assert!(
        o.f.db
            .read(move |tx| Ok(store::copy(tx, &key)?.expect("copy").writer.is_some()))
            .await
            .expect("writer persisted")
    );
    let next_svc = svc.clone();
    let next_copy = o.copy.clone();
    let mut next = tokio::spawn(async move {
        next_svc
            .reserve_windows_copy(
                next_copy,
                "next".into(),
                "devbox-test-copy-next.service".into(),
            )
            .await
    });
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut next)
            .await
            .is_err(),
        "guard must remain with real worker"
    );
    release.release();
    let permit = tokio::time::timeout(Duration::from_secs(2), next)
        .await
        .expect("next waits worker")
        .expect("next task")
        .expect("next copy");
    assert_eq!(
        std::fs::read_to_string(o.copy.target.join("input.txt")).expect("successful next copy"),
        "source v1"
    );
    let launch = permit.reference.launch_id.clone();
    o.f.db
        .write(move |tx| store::cancel_reserved(tx, &launch))
        .await
        .expect("close owned pending");
    drop(permit);
    svc.shutdown().await;
}
#[tokio::test]
async fn partial_copy_failure_never_publishes_a_ready_revision() {
    let mut o = owned().await;
    let input = o.copy.source.join("input.txt");
    o.config.windows_publish_hook = Some(Arc::new(move || {
        std::fs::write(&input, "changed during copy").expect("owned concurrent edit")
    }));
    let svc = service(&o);
    assert!(
        svc.reserve_windows_copy(
            o.copy.clone(),
            "partial".into(),
            "devbox-test-copy-partial.service".into()
        )
        .await
        .is_err()
    );
    let key = o.copy.identity.key.clone();
    o.f.db
        .read(move |tx| {
            let c = store::copy(tx, &key)?.expect("copy record");
            assert!(c.revision.is_none());
            assert!(c.writer.is_none());
            assert!(store::unresolved(tx)?.is_empty());
            Ok(())
        })
        .await
        .expect("partial not ready");
    svc.shutdown().await;
}
#[tokio::test]
async fn terminal_reader_is_polled_and_late_exact_observation_allows_project_removal() {
    let mut o = owned().await;
    let live = Arc::new(AtomicUsize::new(1));
    let state = live.clone();
    o.config.windows_observation = Arc::new(move |_| {
        let state = state.clone();
        Box::pin(async move {
            if state.load(Ordering::SeqCst) > 0 {
                ObservationState::Active { count: 1 }
            } else {
                ObservationState::Empty
            }
        })
    });
    let identity = o.copy.identity.clone();
    let r = reader(&o, "terminal", "rev1");
    let a = attempt(&o, &r, "one", 1);
    let receipt = assigned(&a);
    o.f.db
        .write(move |tx| {
            store::reserve(tx, &identity, &r)?.expect("reserve");
            store::publish(tx, &r.copy)?.expect("publish");
            store::submit(tx, &r.copy)?.expect("dispatch");
            store::intent(tx, &a)?.expect("attempt");
            store::commit(tx, &a.control.nonce, &receipt)?.expect("commit");
            store::seal(tx, &r.copy.launch_id)?;
            assert!(store::needs_reconcile(tx)?);
            Ok(())
        })
        .await
        .expect("terminal live consumer");
    let svc = service(&o);
    svc.reconcile_windows_copies()
        .await
        .expect("active observation");
    assert!(
        o.f.db
            .read(store::needs_reconcile)
            .await
            .expect("poll remains active")
    );
    assert!(
        o.f.projects
            .remove(devbox_projects::RemoveProject {
                id: o.f.project.clone()
            })
            .await
            .is_err()
    );
    live.store(0, Ordering::SeqCst);
    svc.reconcile_windows_copies()
        .await
        .expect("late final child exit");
    assert!(
        !o.f.db
            .read(store::needs_reconcile)
            .await
            .expect("idle after proof")
    );
    o.f.projects
        .remove(devbox_projects::RemoveProject {
            id: o.f.project.clone(),
        })
        .await
        .expect("project deletion after exact proof");
    svc.shutdown().await;
}
#[tokio::test]
async fn daemon_reconstruction_retains_unknown_attempts_and_recovers_only_dead_reserved_writers() {
    let o = owned().await;
    let identity = o.copy.identity.clone();
    let mut r = reader(&o, "dead-writer", "rev1");
    r.owner.pid = u32::MAX;
    o.f.db
        .write(move |tx| {
            store::reserve(tx, &identity, &r)?.expect("reserve abandoned writer");
            Ok(())
        })
        .await
        .expect("persist writer");
    let svc = service(&o);
    svc.reconcile_windows_copies()
        .await
        .expect("recover dead writer");
    assert!(
        !o.f.db
            .read(store::needs_reconcile)
            .await
            .expect("no active writer")
    );
    let identity = o.copy.identity.clone();
    let r = reader(&o, "unknown", "rev2");
    let a = attempt(&o, &r, "one", 1);
    let receipt = assigned(&a);
    let launch = r.copy.launch_id.clone();
    o.f.db
        .write(move |tx| {
            store::reserve(tx, &identity, &r)?.expect("new reservation");
            store::publish(tx, &r.copy)?.expect("publish");
            store::submit(tx, &r.copy)?.expect("dispatch");
            store::intent(tx, &a)?.expect("new attempt");
            store::commit(tx, &a.control.nonce, &receipt)?.expect("commit");
            store::seal(tx, &r.copy.launch_id)?;
            Ok(())
        })
        .await
        .expect("persist unknown attempt");
    let reopened = devbox_core::Db::open(
        &o.f.root.path().join("index.db"),
        &o.f.root.path().join("backups"),
        "1.0.0",
        &[devbox_projects::MIGRATIONS, crate::MIGRATIONS].concat(),
    )
    .expect("reopen durable DB");
    let restored = RuntimeService::new(reopened.clone(), o.f.hub.clone(), o.f.projects.clone())
        .with_execution(o.config.clone());
    restored
        .reconcile_windows_copies()
        .await
        .expect("unavailable observer stays unknown");
    assert_eq!(
        reopened
            .read(move |tx| Ok(store::reader(tx, &launch)?.expect("retained").state))
            .await
            .expect("durable state"),
        store::ReaderState::Sealed
    );
    svc.shutdown().await;
    restored.shutdown().await;
}

#[tokio::test]
async fn reparse_path_probe_batches_preserve_shared_count_and_byte_admission() {
    use devbox_protocol::windows::{
        MAX_WINDOWS_PROBE_PATH_BYTES, MAX_WINDOWS_PROBE_PATHS, PathObservation, WindowsProbe,
    };
    let mut o = owned().await;
    let batches = Arc::new(Mutex::new(Vec::new()));
    let seen = batches.clone();
    o.config.windows_paths = Arc::new(move |paths| {
        let seen = seen.clone();
        Box::pin(async move {
            assert!(
                WindowsProbe::Paths {
                    paths: paths.clone()
                }
                .valid()
            );
            seen.lock()
                .expect("batches")
                .push((paths.len(), paths.iter().map(String::len).sum::<usize>()));
            PathObservation::Safe
        })
    });
    let paths = (0..1500).map(|i| format!("C:\\owned\\{i}.txt")).collect();
    crate::windows_copy_service::safe_paths(&o.config, paths)
        .await
        .expect("ordinary project >64 and >1024 files");
    let paths = (0..80)
        .map(|i| format!("C:\\owned\\{}-{i}", "x".repeat(10_000)))
        .collect();
    crate::windows_copy_service::safe_paths(&o.config, paths)
        .await
        .expect("long path byte budget");
    let batches = batches.lock().expect("batches");
    assert!(batches.len() >= 4);
    assert_eq!(batches.iter().map(|(n, _)| n).sum::<usize>(), 1580);
    assert!(
        batches
            .iter()
            .all(|(n, bytes)| *n <= MAX_WINDOWS_PROBE_PATHS
                && *bytes <= MAX_WINDOWS_PROBE_PATH_BYTES)
    );
}

#[tokio::test]
async fn first_windows_copy_captures_inputs_created_after_graph_preparation_by_wsl_prerequisite() {
    use crate::{DefineRun, RunDef, RunTarget, SaveTo, StartRun, TaskDef};
    let o = owned().await;
    let svc = service(&o);
    for def in [
        RunDef::Task(TaskDef {
            id: "generate".into(),
            command: "owned fixture prerequisite".into(),
            ..Default::default()
        }),
        RunDef::Task(TaskDef {
            id: "win".into(),
            command: "exit 0".into(),
            target: RunTarget::Windows,
            depends_on: vec!["generate".into()],
            ..Default::default()
        }),
    ] {
        svc.define(DefineRun {
            project_id: o.f.project.clone(),
            save_to: SaveTo::ThisPc,
            def,
        })
        .await
        .expect("owned definition");
    }
    let mut graph = svc
        .authorize_run_graph(&StartRun {
            project_id: o.f.project.clone(),
            id: "win".into(),
            worktree: None,
        })
        .await
        .expect("prepare before prerequisite output");
    assert_eq!(graph.ids(), vec!["generate", "win"]);
    assert!(!o.copy.target.exists());
    std::fs::write(o.copy.source.join("generated.txt"), "prerequisite output")
        .expect("owned simulated prerequisite completion");
    let copy = graph
        .windows
        .get_mut("win")
        .expect("prepared Windows identity");
    copy.identity.mapped_base = o.copy.identity.mapped_base.clone();
    copy.target = copy.identity.target().expect("owned mapping");
    let permit = svc
        .reserve_windows_copy(
            copy.clone(),
            "win".into(),
            "devbox-test-copy-win.service".into(),
        )
        .await
        .expect("first Windows consumer");
    assert_eq!(
        std::fs::read_to_string(o.copy.target.join("generated.txt")).expect("late input included"),
        "prerequisite output"
    );
    let launch = permit.reference.launch_id.clone();
    o.f.db
        .write(move |tx| store::cancel_reserved(tx, &launch))
        .await
        .expect("close owned reservation");
    drop(permit);
    svc.shutdown().await;
}

#[path = "windows_copy_lifecycle_isolation_tests.rs"]
mod isolation;
