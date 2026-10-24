use super::*;
use devbox_protocol::{Ctx, Method};
async fn call<M: Method>(router: &Router<Services>, params: &M::Params) -> M::Output
where
    M::Params: serde::Serialize,
    M::Output: serde::de::DeserializeOwned,
{
    let raw =
        serde_json::value::RawValue::from_string(serde_json::to_string(params).unwrap()).unwrap();
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
        .unwrap();
    serde_json::from_str(result.get()).unwrap()
}
#[tokio::test]
async fn source_deps_routes_owned_inventory_and_fake_audit() {
    struct Fake;
    impl devbox_deps::osv::Http for Fake {
        fn post(&self, _: &str, _: Vec<u8>) -> devbox_deps::osv::HttpFuture {
            Box::pin(async { Ok(br#"{"results":[{},{}]}"#.to_vec()) })
        }
    }
    let home = tempfile::tempdir().unwrap();
    let repo = home.path().join("owned");
    std::fs::create_dir(&repo).unwrap();
    std::fs::write(
        repo.join("package-lock.json"),
        include_bytes!("../../../deps/tests/fixtures/npm.json"),
    )
    .unwrap();
    let mut services = crate::daemon::build_services_for_test(home.path());
    Arc::get_mut(&mut services).unwrap().deps = DepsService::new(
        Auditor::new(Arc::new(Fake), Arc::new(|| 1_000), ENDPOINT.into()),
        Arc::new(|_| {}),
    );
    let project = services
        .projects
        .add(devbox_projects::AddProject {
            path: repo.display().to_string(),
        })
        .await
        .unwrap();
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
            assert_eq!(job.result.unwrap().packages.len(), 2);
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    panic!("audit did not complete");
}
#[tokio::test]
async fn source_deps_rejects_unregistered_and_external_symlink() {
    let home = tempfile::tempdir().unwrap();
    let repo = home.path().join("owned");
    std::fs::create_dir(&repo).unwrap();
    let services = crate::daemon::build_services_for_test(home.path());
    assert!(reader(&services, "missing").await.is_err());
    let project = services
        .projects
        .add(devbox_projects::AddProject {
            path: repo.display().to_string(),
        })
        .await
        .unwrap();
    std::fs::write(
        home.path().join("outside.json"),
        include_bytes!("../../../deps/tests/fixtures/npm.json"),
    )
    .unwrap();
    std::os::unix::fs::symlink(
        home.path().join("outside.json"),
        repo.join("package-lock.json"),
    )
    .unwrap();
    assert!(matches!(
        input::load(reader(&services, &project.id).await.unwrap()).await,
        Err(DepsError::UnsafePath)
    ));
}
