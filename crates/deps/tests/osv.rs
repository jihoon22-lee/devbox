use devbox_deps::{model::*, osv::*};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
#[derive(Default)]
struct Fake {
    responses: Mutex<std::collections::VecDeque<Result<Vec<u8>, AuditFailure>>>,
    requests: Mutex<Vec<(String, BatchRequest)>>,
}
impl Http for Fake {
    fn post(&self, endpoint: &str, body: Vec<u8>) -> HttpFuture {
        self.requests
            .lock()
            .expect("lock owned fixture state")
            .push((
                endpoint.into(),
                serde_json::from_slice(&body).expect("decode owned HTTP request"),
            ));
        let response = self
            .responses
            .lock()
            .expect("lock owned fixture state")
            .pop_front()
            .expect("unexpected HTTP request");
        Box::pin(async move { response })
    }
}
fn inventory(fingerprint: &str) -> DependencyInventory {
    DependencyInventory {
        fingerprint: fingerprint.into(),
        locks: vec![LockKind::Pnpm],
        dependencies: vec![Dependency {
            ecosystem: Ecosystem::Npm,
            name: "@scope/pkg".into(),
            direct: true,
            versions: vec![
                DependencyVersion {
                    version: "1.0.0".into(),
                    source: DependencySource::Registry,
                    direct: true,
                },
                DependencyVersion {
                    version: "2.0.0".into(),
                    source: DependencySource::Registry,
                    direct: false,
                },
            ],
        }],
    }
}
fn fixture(responses: Vec<Result<&str, AuditFailure>>) -> (Arc<Fake>, Arc<AtomicU64>, Auditor) {
    let http = Arc::new(Fake::default());
    http.responses
        .lock()
        .expect("lock owned fixture state")
        .extend(
            responses
                .into_iter()
                .map(|r| r.map(|s| s.as_bytes().to_vec())),
        );
    let now = Arc::new(AtomicU64::new(1_000));
    let c = now.clone();
    let auditor = Auditor::new(
        http.clone(),
        Arc::new(move || c.load(Ordering::SeqCst)),
        ENDPOINT.into(),
    );
    (http, now, auditor)
}
fn progress() -> Arc<dyn Fn(u32, u32) + Send + Sync> {
    Arc::new(|_, _| {})
}
#[tokio::test]
async fn querybatch_maps_positions_and_paginates_only_pending_packages() {
    let (http, _, a) = fixture(vec![
        Ok(
            r#"{"results":[{"vulns":[{"id":"GHSA-one","modified":"2026-01-01"}],"next_page_token":"next"},{}]}"#,
        ),
        Ok(r#"{"results":[{"vulns":[{"id":"GHSA-two","modified":"2026-01-02"}]}]}"#),
    ]);
    let (report, cached) = a
        .audit("p", &inventory("a"), progress())
        .await
        .expect("audit owned dependency inventory");
    assert!(!cached);
    assert_eq!(report.packages.len(), 2);
    assert_eq!(report.packages[0].advisories.len(), 2);
    assert!(report.packages[1].advisories.is_empty());
    let req = http.requests.lock().expect("lock owned fixture state");
    assert_eq!(req.len(), 2);
    assert_eq!(req[0].1.queries.len(), 2);
    assert_eq!(req[1].1.queries.len(), 1);
    assert_eq!(req[1].1.queries[0].version, "1.0.0");
    assert_eq!(req[1].1.queries[0].page_token.as_deref(), Some("next"));
}
#[tokio::test]
async fn partial_missing_error_or_malformed_responses_never_become_clean() {
    for body in [
        r#"{"results":[{}]}"#,
        r#"{"results":[{},null]}"#,
        r#"{"results":[{}, {"error":{"code":429}}]}"#,
        r#"{"results":[{}, {"vulns":[{"id":"../../unsafe","modified":"x"}]}]}"#,
        "bad",
    ] {
        let (_, _, a) = fixture(vec![Ok(body)]);
        assert!(
            a.audit("p", &inventory("a"), progress()).await.is_err(),
            "{body}"
        );
    }
}
#[tokio::test]
async fn cache_expires_at_thirty_minutes_and_changed_inputs_miss() {
    let clean = r#"{"results":[{},{}]}"#;
    let (http, now, a) = fixture(vec![Ok(clean), Ok(clean), Ok(clean), Ok(clean)]);
    assert!(
        !a.audit("p", &inventory("a"), progress())
            .await
            .expect("audit owned dependency inventory")
            .1
    );
    now.store(1_000 + CACHE_MS - 1, Ordering::SeqCst);
    assert!(
        a.audit("p", &inventory("a"), progress())
            .await
            .expect("audit owned dependency inventory")
            .1
    );
    now.store(1_000 + CACHE_MS, Ordering::SeqCst);
    assert!(
        !a.audit("p", &inventory("a"), progress())
            .await
            .expect("audit owned dependency inventory")
            .1
    );
    assert!(
        !a.audit("p", &inventory("changed-lock"), progress())
            .await
            .expect("audit owned dependency inventory")
            .1
    );
    assert!(
        !a.audit("other-project", &inventory("changed-lock"), progress())
            .await
            .expect("audit owned dependency inventory")
            .1
    );
    assert_eq!(
        http.requests
            .lock()
            .expect("lock owned fixture state")
            .len(),
        4
    );
}
#[tokio::test]
async fn normalized_set_hash_includes_version_and_endpoint() {
    let clean = r#"{"results":[{},{}]}"#;
    let (http, _, a) = fixture(vec![Ok(clean), Ok(clean), Ok(clean)]);
    a.audit("p", &inventory("a"), progress())
        .await
        .expect("audit owned dependency inventory");
    let mut changed = inventory("a");
    changed.dependencies[0].versions[0].version = "9.0.0".into();
    assert!(
        !a.audit("p", &changed, progress())
            .await
            .expect("audit owned dependency inventory")
            .1
    );
    let b = Auditor::new(
        http.clone(),
        Arc::new(|| 1_000),
        "https://other.invalid/v1/querybatch".into(),
    );
    b.audit("p", &inventory("a"), progress())
        .await
        .expect("audit owned dependency inventory");
    assert_eq!(
        http.requests.lock().expect("lock owned fixture state")[2].0,
        "https://other.invalid/v1/querybatch"
    );
}
#[tokio::test]
async fn network_failure_is_not_cached_as_success() {
    let (http, _, a) = fixture(vec![
        Err(AuditFailure::Network),
        Ok(r#"{"results":[{},{}]}"#),
    ]);
    assert!(matches!(
        a.audit("p", &inventory("a"), progress()).await,
        Err(AuditFailure::Network)
    ));
    assert!(
        !a.audit("p", &inventory("a"), progress())
            .await
            .expect("audit owned dependency inventory")
            .1
    );
    assert_eq!(
        http.requests
            .lock()
            .expect("lock owned fixture state")
            .len(),
        2
    );
}
#[tokio::test]
async fn non_registry_versions_are_explicitly_skipped() {
    let (http, _, a) = fixture(vec![Ok(r#"{"results":[{}]}"#)]);
    let mut inv = inventory("a");
    inv.dependencies[0].versions[1].source = DependencySource::Git;
    let (report, _) = a
        .audit("p", &inv, progress())
        .await
        .expect("audit owned dependency inventory");
    assert_eq!(report.skipped, 1);
    assert_eq!(report.packages.len(), 1);
    assert_eq!(
        http.requests.lock().expect("lock owned fixture state")[0]
            .1
            .queries
            .len(),
        1
    );
}
#[tokio::test]
async fn repeated_pagination_token_is_incomplete_and_never_cached() {
    let page = r#"{"results":[{"next_page_token":"same"},{}]}"#;
    let (_, _, a) = fixture(vec![
        Ok(page),
        Ok(r#"{"results":[{"next_page_token":"same"}]}"#),
    ]);
    assert!(matches!(
        a.audit("p", &inventory("a"), progress()).await,
        Err(AuditFailure::Incomplete)
    ));
}
#[tokio::test]
async fn normalization_deduplicates_equivalent_python_and_cargo_names() {
    let (http, _, a) = fixture(vec![Ok(r#"{"results":[{},{}]}"#)]);
    let mut inv = inventory("a");
    inv.dependencies.clear();
    for (ecosystem, name) in [
        (Ecosystem::Python, "Some_Package"),
        (Ecosystem::Python, "some.package"),
        (Ecosystem::Cargo, "CRATE_name"),
        (Ecosystem::Cargo, "crate-name"),
    ] {
        inv.dependencies.push(Dependency {
            ecosystem,
            name: name.into(),
            direct: true,
            versions: vec![DependencyVersion {
                version: "1.0.0".into(),
                source: DependencySource::Registry,
                direct: true,
            }],
        });
    }
    let (report, _) = a
        .audit("p", &inv, progress())
        .await
        .expect("audit owned dependency inventory");
    assert_eq!(report.packages.len(), 2);
    let req = http.requests.lock().expect("lock owned fixture state");
    assert_eq!(req[0].1.queries[0].package.name, "crate-name");
    assert_eq!(req[0].1.queries[0].package.ecosystem, "crates.io");
    assert_eq!(req[0].1.queries[1].package.name, "some-package");
    assert_eq!(req[0].1.queries[1].package.ecosystem, "PyPI");
}
#[tokio::test]
async fn all_non_registry_inventory_never_calls_http_or_claims_queried_packages() {
    let (http, _, a) = fixture(vec![]);
    let mut inv = inventory("a");
    for v in &mut inv.dependencies[0].versions {
        v.source = DependencySource::Local;
    }
    let (report, _) = a
        .audit("p", &inv, progress())
        .await
        .expect("audit owned dependency inventory");
    assert_eq!(report.skipped, 2);
    assert!(report.packages.is_empty());
    assert!(
        http.requests
            .lock()
            .expect("lock owned fixture state")
            .is_empty()
    );
}
