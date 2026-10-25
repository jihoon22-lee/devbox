use super::*;

#[tokio::test]
async fn concurrent_projects_can_initialize_the_same_owned_namespace_without_partial_markers() {
    let o = owned().await;
    for index in 0..24 {
        let mut a =
            CopyIdentity::new(&o.config, "project-a", "main", &o.copy.source).expect("A identity");
        let mut b =
            CopyIdentity::new(&o.config, "project-b", "main", &o.copy.source).expect("B identity");
        let namespace = o.f.root.path().join(format!("parallel-{index}"));
        a.mapped_base = namespace.join("owner/project-a/main");
        b.mapped_base = namespace.join("owner/project-b/main");
        let barrier = std::sync::Barrier::new(2);
        let (a, b) = std::thread::scope(|scope| {
            let first = scope.spawn(|| {
                barrier.wait();
                a.ensure_owned()
            });
            let second = scope.spawn(|| {
                barrier.wait();
                b.ensure_owned()
            });
            (
                first.join().expect("owned initializer A"),
                second.join().expect("owned initializer B"),
            )
        });
        assert!(
            a.is_ok() && b.is_ok(),
            "same-owner initializers must cooperate: {a:?}, {b:?}"
        );
    }
}

#[tokio::test]
async fn changed_instance_or_distro_owner_cannot_reconcile_or_revoke_foreign_reservations() {
    let mut o = owned().await;
    let identity = o.copy.identity.clone();
    let mut r = reader(&o, "foreign", "revision");
    r.owner.pid = u32::MAX;
    o.f.db
        .write(move |tx| {
            store::reserve(tx, &identity, &r)?.expect("foreign reservation");
            Ok(())
        })
        .await
        .expect("persist namespace owner");
    o.config.windows_owner = "different-instance-or-distro".into();
    let svc = service(&o);
    svc.reconcile_windows_copies()
        .await
        .expect("foreign owner is retained");
    assert!(
        o.f.db
            .read(store::needs_reconcile)
            .await
            .expect("not silently cleared")
    );
    svc.shutdown().await;
}

#[tokio::test]
async fn an_independent_copy_start_does_not_wait_for_an_unrelated_unknown_windows_observer() {
    let mut o = owned().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    o.config.windows_observation = Arc::new(move |_| {
        let observed = observed.clone();
        Box::pin(async move {
            observed.fetch_add(1, Ordering::SeqCst);
            ObservationState::Unavailable
        })
    });
    let identity = o.copy.identity.clone();
    let r = reader(&o, "unknown-other", "rev1");
    let a = attempt(&o, &r, "one", 1);
    let receipt = assigned(&a);
    o.f.db
        .write(move |tx| {
            store::reserve(tx, &identity, &r)?.expect("reserve other");
            store::publish(tx, &r.copy)?.expect("publish other");
            store::submit(tx, &r.copy)?.expect("dispatch");
            store::intent(tx, &a)?.expect("attempt");
            store::commit(tx, &a.control.nonce, &receipt)?.expect("commit");
            store::seal(tx, &r.copy.launch_id)?;
            Ok(())
        })
        .await
        .expect("persist unrelated unknown");
    let mut next = o.copy.clone();
    next.identity = CopyIdentity::new(&o.config, &o.f.project, "independent", &next.source)
        .expect("independent identity");
    next.identity.mapped_base = o
        .copy
        .identity
        .mapped_base
        .parent()
        .expect("project directory")
        .join("independent");
    next.target = next.identity.target().expect("owned target");
    next.cwd = next.identity.directory();
    let svc = service(&o);
    let permit = svc
        .reserve_windows_copy(
            next,
            "independent".into(),
            "devbox-test-copy-independent.service".into(),
        )
        .await
        .expect("independent start");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "start must not query unrelated unknown copies"
    );
    svc.reconcile_windows_copies()
        .await
        .expect("global watcher still audits all");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let launch = permit.reference.launch_id.clone();
    o.f.db
        .write(move |tx| store::cancel_reserved(tx, &launch))
        .await
        .expect("close own pending");
    drop(permit);
    svc.shutdown().await;
}
