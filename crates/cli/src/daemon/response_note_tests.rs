//! Real daemon routing across HTTP history, export authority and vault mutation.
use super::state::Services;
use devbox_api::{CollectionRoot, HistoryId, SendRequest};
use devbox_notes::{
    AppendResponseNote, ResponseNoteAppended, ResponseNoteTarget, SelectResponseNote,
};
use devbox_protocol::frame::Frame;
use devbox_protocol::stream::{
    Credit, FLAG_FIN, STREAM_WINDOW, StreamEnds, StreamError, StreamOpener, StreamSink, StreamTx,
    parse_stream,
};
use devbox_protocol::{Ctx, Router};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

struct Sink(Arc<Credit>, mpsc::UnboundedSender<Option<Vec<u8>>>);
impl StreamSink for Sink {
    fn push(
        &self,
        frame: Frame,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + '_>> {
        let parsed = parse_stream(&frame.body).map(|(_, flags, data)| (flags, data.to_vec()));
        Box::pin(async move {
            let Some((flags, data)) = parsed else {
                return false;
            };
            self.0.add(data.len() as u32);
            if !data.is_empty() {
                let _ = self.1.send(Some(data));
            }
            if flags & FLAG_FIN != 0 {
                let _ = self.1.send(None);
            }
            true
        })
    }
}
struct Opener(Mutex<Option<StreamEnds>>);
impl StreamOpener for Opener {
    fn open(&self) -> Result<StreamEnds, StreamError> {
        self.0
            .lock()
            .expect("owned stream")
            .take()
            .ok_or(StreamError::Closed)
    }
}
fn context() -> Ctx {
    Ctx {
        channel: 1,
        cancel: CancellationToken::new(),
        streams: None,
    }
}
async fn route(
    router: &Router<Services>,
    method: &str,
    params: Value,
) -> Result<Value, devbox_protocol::router::DispatchError> {
    let raw = serde_json::value::to_raw_value(&params).expect("fixture params");
    let result = router.dispatch(method, context(), raw).await?;
    Ok(serde_json::from_str(result.get()).expect("typed response JSON"))
}
async fn fixture() -> (tempfile::TempDir, Arc<Services>) {
    let home = tempfile::tempdir().expect("owned fixture home");
    std::fs::create_dir(home.path().join("notes")).expect("owned vault");
    std::fs::write(home.path().join("notes/chosen.md"), "original note\n").expect("owned note");
    let services = super::build_services_for_test(home.path());
    services.api.wait_ready().await;
    services.notes.rescan().await.expect("index owned vault");
    (home, services)
}

async fn completed(s: &Services) -> (String, Arc<AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("owned HTTP listener");
    let address = listener.local_addr().expect("owned address");
    let count = Arc::new(AtomicUsize::new(0));
    let calls = count.clone();
    let body = format!(
        "payload-start|fixture-note-secret|{}",
        "a".repeat(70 * 1024)
    );
    let app = axum::Router::new().fallback(move || {
        calls.fetch_add(1, Ordering::SeqCst);
        let body = body.clone();
        async move { ([("content-type", "text/plain")], body) }
    });
    let stop = CancellationToken::new();
    let shutdown = stop.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await
    });
    let (tx, mut rx) = mpsc::unbounded_channel();
    let credit = Arc::new(Credit::new(STREAM_WINDOW));
    let stream = StreamTx::new(
        1,
        1,
        credit.clone(),
        Arc::new(Sink(credit, tx)),
        CancellationToken::new(),
    );
    let (_input, input) = mpsc::channel(1);
    let mut ctx = context();
    ctx.streams = Some(Arc::new(Opener(Mutex::new(Some(StreamEnds {
        id: 1,
        tx: stream,
        input,
    })))));
    let request: SendRequest = serde_json::from_value(json!({
        "snapshot": {"method":"GET", "url":format!("http://{address}/original?token=fixture-note-query"),
            "query":[], "headers":[{"row":0,"name":"Authorization","value":"Bearer fixture-note-secret"}],
            "cookies":[], "body":{"kind":"none"}, "auth":{"kind":"none"},
            "tls":{"verify":true,"caPath":null,"clientCertPath":null,"clientKeyPath":null},
            "timeoutMs":5000,"followRedirects":true},
        "env":{"variables":[]}, "runId":null, "origin":null, "cookieJar":null,
        "sourceRoot": CollectionRoot::Personal
    })).expect("owned HTTP request");
    s.api
        .send(ctx, request)
        .await
        .expect("send original response");
    let bytes = tokio::time::timeout(Duration::from_secs(10), async {
        let mut bytes = Vec::new();
        while let Some(Some(chunk)) = rx.recv().await {
            bytes.extend(chunk);
        }
        bytes
    })
    .await
    .expect("response finishes");
    stop.cancel();
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .expect("owned server stops")
        .expect("owned server task")
        .expect("owned server result");
    let events: Vec<Value> = bytes
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).expect("stream event"))
        .collect();
    assert!(events.iter().any(|e| e["type"] == "end"), "{events:?}");
    assert!(!events.iter().any(|e| e["type"] == "failed"));
    let id = events
        .iter()
        .find(|e| e["type"] == "started")
        .expect("started event")["historyId"]
        .as_str()
        .expect("history identity")
        .to_owned();
    (id, count)
}
async fn request(
    router: &Router<Services>,
    history: &str,
    target: ResponseNoteTarget,
) -> AppendResponseNote {
    let selected = route(
        router,
        "notes.select_response_target",
        serde_json::to_value(SelectResponseNote { target }).expect("select params"),
    )
    .await
    .expect("select target through daemon route");
    AppendResponseNote {
        history_id: history.into(),
        selection: selected["selection"].as_str().expect("selection").into(),
        operation_id: uuid::Uuid::new_v4().simple().to_string(),
    }
}
async fn append(router: &Router<Services>, request: &AppendResponseNote) -> ResponseNoteAppended {
    serde_json::from_value(
        route(
            router,
            "notes.append_response",
            serde_json::to_value(request).expect("append params"),
        )
        .await
        .expect("append through daemon route"),
    )
    .expect("typed append result")
}

#[tokio::test]
async fn completed_http_response_routes_to_selected_and_daily_notes_without_resending() {
    let (home, services) = fixture().await;
    let (id, calls) = completed(&services).await;
    let router = super::routes::router(services);
    for target in [
        ResponseNoteTarget::Existing {
            path: "chosen.md".into(),
        },
        ResponseNoteTarget::Daily,
    ] {
        let p = request(&router, &id, target).await;
        let first = append(&router, &p).await;
        let path = home.path().join("notes").join(&first.path);
        let saved = std::fs::read_to_string(&path).expect("saved note");
        assert!(saved.contains("payload-start|[REDACTED]|"));
        assert!(saved.contains("/original?token=[REDACTED]"));
        assert!(!saved.contains("fixture-note-secret") && !saved.contains("fixture-note-query"));
        assert!(saved.len() > 64 * 1024);
        assert_eq!(append(&router, &p).await, first);
        assert_eq!(std::fs::read_to_string(path).expect("note"), saved);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1, "append never resends HTTP");
}

#[tokio::test]
async fn committed_append_returns_before_api_lookup_after_history_is_removed() {
    let (home, services) = fixture().await;
    let (id, calls) = completed(&services).await;
    let router = super::routes::router(services.clone());
    let p = request(
        &router,
        &id,
        ResponseNoteTarget::Existing {
            path: "chosen.md".into(),
        },
    )
    .await;
    let result = append(&router, &p).await;
    let saved = std::fs::read(home.path().join("notes/chosen.md")).expect("chosen note");
    let mut removed = 0;
    for day in std::fs::read_dir(services.paths.data_dir.join("api-history")).expect("history days")
    {
        let path = day.expect("history day").path().join(format!("{id}.json"));
        if path.is_file() {
            std::fs::remove_file(path).expect("remove only owned history entry");
            removed += 1;
        }
    }
    assert_eq!(removed, 1);
    assert!(
        services
            .api
            .export_response(HistoryId { id })
            .await
            .is_err()
    );
    assert_eq!(append(&router, &p).await, result);
    assert_eq!(
        std::fs::read(home.path().join("notes/chosen.md")).expect("chosen note"),
        saved
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn unknown_history_and_renderer_body_are_rejected_without_mutation() {
    let (home, services) = fixture().await;
    let router = super::routes::router(services);
    let p = request(
        &router,
        &"f".repeat(32),
        ResponseNoteTarget::Existing {
            path: "chosen.md".into(),
        },
    )
    .await;
    let params = serde_json::to_value(&p).expect("append params");
    let error = route(&router, "notes.append_response", params.clone())
        .await
        .expect_err("no export receipt");
    assert_eq!(error.error.code, "notes.response_unavailable");
    let mut forged = params;
    forged["markdown"] = json!("untrusted renderer body");
    let error = route(&router, "notes.append_response", forged)
        .await
        .expect_err("body not admitted");
    assert_eq!(error.error.code, "invalid_params");
    assert_eq!(
        std::fs::read_to_string(home.path().join("notes/chosen.md")).expect("chosen note"),
        "original note\n"
    );
}
