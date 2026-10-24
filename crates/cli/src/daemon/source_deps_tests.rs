use super::*;
use devbox_protocol::{Ctx, Method};
async fn call<M: Method>(router: &Router<Services>, params: &M::Params) -> M::Output
where
    M::Params: serde::Serialize,
    M::Output: serde::de::DeserializeOwned,
{
    let raw = serde_json::value::RawValue::from_string(
        serde_json::to_string(params).expect("encode owned RPC request"),
    )
    .expect("encode owned RPC request");
    let result = router
        .dispatch(
            M::NAME,
            Ctx {
                channel: 1,
                cancel: tokio_util::sync::CancellationToken::new(),
                streams: None,
            },
            raw,
        )
        .await
        .expect("owned RPC response");
    serde_json::from_str(result.get()).expect("owned fixture entry")
}
#[tokio::test]
async fn source_deps_routes_owned_inventory_and_fake_audit() {
    struct Fake;
    impl devbox_deps::osv::Http for Fake {
        fn post(&self, _: &str, _: Vec<u8>) -> devbox_deps::osv::HttpFuture {
            Box::pin(async { Ok(br#"{"results":[{},{}]}"#.to_vec()) })
        }
    }
    let home = tempfile::tempdir().expect("create owned temporary directory");
    let repo = home.path().join("owned");
    std::fs::create_dir(&repo).expect("create owned fixture directory");
    std::fs::write(
        repo.join("package-lock.json"),
        include_bytes!("../../../deps/tests/fixtures/npm.json"),
    )
    .expect("write owned fixture");
    let mut services = crate::daemon::build_services_for_test(home.path());
    Arc::get_mut(&mut services)
        .expect("exclusive owned services fixture")
        .deps = DepsService::new(
        Auditor::new(Arc::new(Fake), Arc::new(|| 1_000), ENDPOINT.into()),
        Arc::new(|_| {}),
    );
    let project = services
        .projects
        .add(devbox_projects::AddProject {
            path: repo.display().to_string(),
        })
        .await
        .expect("encode owned RPC request");
    let router = crate::daemon::routes::router(services);
    let query = ProjectQuery {
        project_id: project.id,
    };
    let inventory = call::<DepsList>(&router, &query).await;
    assert_eq!(inventory.dependencies[0].versions.len(), 2);
    let job = call::<DepsAudit>(&router, &query).await;
    let params = JobQuery {
        project_id: query.project_id,
        id: job.id,
    };
    for _ in 0..100 {
        let job = call::<DepsAuditStatus>(&router, &params).await;
        if job.state == AuditState::Succeeded {
            assert_eq!(job.result.expect("owned RPC response").packages.len(), 2);
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    panic!("audit did not complete");
}
#[tokio::test]
async fn source_deps_rejects_unregistered_and_external_symlink() {
    let home = tempfile::tempdir().expect("create owned temporary directory");
    let repo = home.path().join("owned");
    std::fs::create_dir(&repo).expect("create owned fixture directory");
    let services = crate::daemon::build_services_for_test(home.path());
    assert!(reader(&services, "missing").await.is_err());
    let project = services
        .projects
        .add(devbox_projects::AddProject {
            path: repo.display().to_string(),
        })
        .await
        .expect("encode owned RPC request");
    std::fs::write(
        home.path().join("outside.json"),
        include_bytes!("../../../deps/tests/fixtures/npm.json"),
    )
    .expect("write owned fixture");
    std::os::unix::fs::symlink(
        home.path().join("outside.json"),
        repo.join("package-lock.json"),
    )
    .expect("owned RPC response");
    assert!(matches!(
        input::load(
            reader(&services, &project.id)
                .await
                .expect("read owned project dependencies")
        )
        .await,
        Err(DepsError::UnsafePath)
    ));
}
