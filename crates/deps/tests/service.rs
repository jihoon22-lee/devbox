use devbox_deps::{input::*, model::*, osv::*, service::*};
use std::sync::{Arc, Mutex};
struct Fake(Mutex<std::collections::VecDeque<Result<Vec<u8>, AuditFailure>>>);
impl Http for Fake {
    fn post(&self, _: &str, _: Vec<u8>) -> HttpFuture {
        let response = self
            .0
            .lock()
            .expect("lock owned fixture state")
            .pop_front()
            .expect("next owned HTTP response");
        Box::pin(async move { response })
    }
}
fn reader(files: Arc<Mutex<std::collections::BTreeMap<String, Vec<u8>>>>) -> Reader {
    Arc::new(move |path| {
        let value = files
            .lock()
            .expect("lock owned fixture state")
            .get(&path)
            .cloned();
        Box::pin(async move { Ok(value) })
    })
}
fn files() -> Arc<Mutex<std::collections::BTreeMap<String, Vec<u8>>>> {
    Arc::new(Mutex::new(std::collections::BTreeMap::from([(
        "package-lock.json".into(),
        include_bytes!("fixtures/npm.json").to_vec(),
    )])))
}
async fn finished(s: &DepsService, id: &str) -> AuditJob {
    for _ in 0..100 {
        let j = s.status("project", id).expect("read owned audit status");
        if j.state != AuditState::Running {
            return j;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    panic!("job did not finish")
}
#[tokio::test]
async fn failure_preserves_previous_success_and_events_and_retry() {
    let data = files();
    let read = reader(data.clone());
    let inv = load(read.clone())
        .await
        .expect("load owned dependency inventory");
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    let service = DepsService::new(
        Auditor::new(
            Arc::new(Fake(Mutex::new(std::collections::VecDeque::from([
                Ok(br#"{"results":[{},{}]}"#.to_vec()),
                Err(AuditFailure::Network),
                Ok(br#"{"results":[{},{}]}"#.to_vec()),
            ])))),
            Arc::new(|| 7_000),
            ENDPOINT.into(),
        ),
        Arc::new(move |j| sink.lock().expect("lock owned fixture state").push(j)),
    );
    let first = service
        .start("project".into(), inv, read.clone())
        .expect("start owned dependency audit");
    assert_eq!(first.state, AuditState::Running);
    let first = finished(&service, &first.id).await;
    assert_eq!(first.state, AuditState::Succeeded);
    assert_eq!(
        first
            .result
            .as_ref()
            .expect("owned audit report")
            .completed_at_ms,
        7_000
    );
    data.lock()
        .expect("lock owned fixture state")
        .get_mut("package-lock.json")
        .expect("exclusive owned services fixture")
        .push(b'\n');
    let inv = load(read.clone())
        .await
        .expect("load owned dependency inventory");
    let second = service
        .start("project".into(), inv.clone(), read.clone())
        .expect("start owned dependency audit");
    let second = finished(&service, &second.id).await;
    assert_eq!(second.state, AuditState::Failed);
    assert_eq!(second.error, Some(AuditFailure::Network));
    assert!(second.result.is_none());
    assert_eq!(second.last_success, first.result);
    assert!(service.status("other-project", &second.id).is_err());
    let third = service
        .start("project".into(), inv, read)
        .expect("start owned dependency audit");
    assert_eq!(
        finished(&service, &third.id).await.state,
        AuditState::Succeeded
    );
    assert!(
        events
            .lock()
            .expect("lock owned fixture state")
            .iter()
            .any(|j| j.state == AuditState::Failed)
    );
}
#[tokio::test]
async fn input_reader_hash_changes_and_unsafe_member_refuses_before_read() {
    let data = files();
    let read = reader(data.clone());
    let a = load(read.clone())
        .await
        .expect("load owned dependency inventory");
    data.lock()
        .expect("lock owned fixture state")
        .get_mut("package-lock.json")
        .expect("exclusive owned services fixture")
        .push(b'\n');
    assert_ne!(
        a.fingerprint,
        load(read)
            .await
            .expect("load owned dependency inventory")
            .fingerprint
    );
    let mut cargo = std::collections::BTreeMap::new();
    cargo.insert(
        "Cargo.lock".into(),
        include_bytes!("fixtures/Cargo.lock").to_vec(),
    );
    cargo.insert(
        "Cargo.toml".into(),
        b"[workspace]\nmembers=['../outside']".to_vec(),
    );
    assert!(matches!(
        load(reader(Arc::new(Mutex::new(cargo)))).await,
        Err(DepsError::UnsafePath)
    ));
}
#[tokio::test]
async fn lock_change_during_audit_is_failed_and_concurrent_start_is_refused() {
    struct Delayed {
        started: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    }
    impl Http for Delayed {
        fn post(&self, _: &str, _: Vec<u8>) -> HttpFuture {
            let started = self.started.clone();
            let release = self.release.clone();
            Box::pin(async move {
                started.notify_one();
                release.notified().await;
                Ok(br#"{"results":[{},{}]}"#.to_vec())
            })
        }
    }
    let data = files();
    let read = reader(data.clone());
    let inv = load(read.clone())
        .await
        .expect("load owned dependency inventory");
    let started = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let svc = DepsService::new(
        Auditor::new(
            Arc::new(Delayed {
                started: started.clone(),
                release: release.clone(),
            }),
            Arc::new(|| 1_000),
            ENDPOINT.into(),
        ),
        Arc::new(|_| {}),
    );
    let job = svc
        .start("project".into(), inv.clone(), read.clone())
        .expect("start owned dependency audit");
    started.notified().await;
    assert!(matches!(
        svc.start("project".into(), inv, read),
        Err(DepsError::Busy)
    ));
    data.lock()
        .expect("lock owned fixture state")
        .get_mut("package-lock.json")
        .expect("exclusive owned services fixture")
        .push(b'\n');
    release.notify_one();
    let job = finished(&svc, &job.id).await;
    assert_eq!(job.state, AuditState::Failed);
    assert_eq!(job.error, Some(AuditFailure::InputChanged));
    assert!(job.result.is_none());
}

struct PanicAfterSuccess(std::sync::atomic::AtomicUsize);
impl Http for PanicAfterSuccess {
    fn post(&self, _: &str, _: Vec<u8>) -> HttpFuture {
        let call = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async move {
            assert_ne!(call, 1, "owned HTTP future panic");
            Ok(br#"{"results":[{},{}]}"#.to_vec())
        })
    }
}
#[tokio::test]
async fn panicked_audit_finishes_failed_preserves_success_and_allows_retry() {
    let data = files();
    let read = reader(data.clone());
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    let service = DepsService::new(
        Auditor::new(
            Arc::new(PanicAfterSuccess(std::sync::atomic::AtomicUsize::new(0))),
            Arc::new(|| 7_000),
            ENDPOINT.into(),
        ),
        Arc::new(move |event| sink.lock().expect("owned event lock").push(event)),
    );
    let initial = load(read.clone()).await.expect("owned initial inventory");
    let first = service
        .start("project".into(), initial, read.clone())
        .expect("owned initial audit");
    let first = finished(&service, &first.id).await;
    assert_eq!(first.state, AuditState::Succeeded);
    data.lock()
        .expect("owned fixture lock")
        .get_mut("package-lock.json")
        .expect("owned lockfile")
        .push(b'\n');
    let changed = load(read.clone()).await.expect("owned changed inventory");
    let second = service
        .start("project".into(), changed.clone(), read.clone())
        .expect("owned panicking audit");
    let second = finished(&service, &second.id).await;
    assert_eq!(second.state, AuditState::Failed);
    assert_eq!(second.error, Some(AuditFailure::Network));
    assert!(second.result.is_none());
    assert_eq!(second.last_success, first.result);
    assert!(
        events
            .lock()
            .expect("owned event lock")
            .iter()
            .any(|event| event.id == second.id && event.state == AuditState::Failed)
    );
    let retry = service
        .start("project".into(), changed, read)
        .expect("retry after owned panic");
    assert_eq!(
        finished(&service, &retry.id).await.state,
        AuditState::Succeeded
    );
}
