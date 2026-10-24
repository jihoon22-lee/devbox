//! 서비스 조립과 가짜 화면 스트림(받은 바이트만큼 credit을 돌려준다).
use devbox_api::*;
use devbox_core::{Db, Env, IndexDb, Instance, Paths};
use devbox_protocol::frame::Frame;
use devbox_protocol::message::WireError;
use devbox_protocol::stream::{
    Credit, FLAG_FIN, STREAM_WINDOW, StreamEnds, StreamError, StreamOpener, StreamSink, StreamTx,
    parse_stream,
};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

struct Home(String);
impl Env for Home {
    fn var(&self, k: &str) -> Option<String> {
        (k == "HOME").then(|| self.0.clone())
    }
    fn uid(&self) -> u32 {
        1000
    }
}

pub struct Harness {
    pub dir: tempfile::TempDir,
    pub paths: Paths,
    pub db: Db,
    pub index: IndexDb,
    pub secrets: Arc<devbox_secrets::SecretsService>,
    pub projects: Arc<devbox_projects::ProjectsService>,
    pub api: ApiService,
    /// MCP 출처 파일 확인(S5-5)에 쓰는 같은 DB·프로젝트의 컬렉션 서비스.
    pub collections: CollectionsService,
}

pub fn paths(dir: &std::path::Path) -> Paths {
    let suffix = &uuid::Uuid::new_v4().simple().to_string()[..8];
    let paths = Paths::resolve(
        Instance::parse(&format!("test-{suffix}")).expect("instance"),
        &Home(dir.display().to_string()),
    )
    .expect("paths");
    paths.ensure_dirs().expect("dirs");
    paths
}

impl Harness {
    /// 소유 test 키로 연 비밀 저장소.
    pub async fn new() -> Self {
        Self::build(true, ApiOptions::default()).await
    }
    pub async fn locked() -> Self {
        Self::build(false, ApiOptions::default()).await
    }
    pub async fn with_options(options: ApiOptions) -> Self {
        Self::build(true, options).await
    }
    pub async fn build(unlocked: bool, options: ApiOptions) -> Self {
        let dir = tempfile::tempdir().expect("owned fixture");
        let paths = paths(dir.path());
        Self::open(dir, paths, unlocked, options).await
    }
    pub async fn open(
        dir: tempfile::TempDir,
        paths: Paths,
        unlocked: bool,
        options: ApiOptions,
    ) -> Self {
        let migrations = [
            devbox_projects::MIGRATIONS,
            devbox_secrets::MIGRATIONS,
            devbox_api::MIGRATIONS,
        ]
        .concat();
        let db = Db::open(&paths.db_path, &paths.backups_dir, "test", &migrations).expect("db");
        let index = IndexDb::open(&paths.index_db_path).expect("index");
        let secrets = Arc::new(if unlocked {
            devbox_secrets::SecretsService::with_key(
                db.clone(),
                paths.clone(),
                devbox_secrets::crypto::Key::from_bytes([7; 32]),
            )
            .await
            .expect("fixture key")
        } else {
            devbox_secrets::SecretsService::new(db.clone(), paths.clone())
        });
        let projects = Arc::new(devbox_projects::ProjectsService::new(
            db.clone(),
            Arc::new(devbox_core::Hub::new()),
            dir.path().into(),
            vec![],
        ));
        let api = ApiService::with_options(
            db.clone(),
            index.clone(),
            paths.clone(),
            projects.clone(),
            secrets.clone(),
            options,
        )
        .expect("api service");
        let collections = CollectionsService::new(
            db.clone(),
            Arc::new(devbox_core::Hub::new()),
            paths.clone(),
            projects.clone(),
        );
        Self {
            dir,
            paths,
            db,
            index,
            secrets,
            projects,
            api,
            collections,
        }
    }
    pub async fn secret(&self, name: &str, value: &str) {
        self.secrets
            .set(devbox_secrets::SetSecret {
                name: name.into(),
                value: value.into(),
            })
            .await
            .expect("fixture secret");
    }
    pub async fn send(&self, request: SendRequest) -> Result<Sent, Rpc> {
        send_rpc(&self.api, request).await
    }
    /// 성공 송신을 끝까지 읽는다.
    pub async fn complete(&self, request: SendRequest) -> Vec<SendEvent> {
        self.send(request)
            .await
            .unwrap_or_else(|e| panic!("send rejected: {}", e.0.code))
            .events()
            .await
    }
    pub fn history_files(&self) -> Vec<std::path::PathBuf> {
        let root = self.paths.data_dir.join("api-history");
        let mut out = vec![];
        for day in std::fs::read_dir(&root).into_iter().flatten().flatten() {
            for file in std::fs::read_dir(day.path())
                .into_iter()
                .flatten()
                .flatten()
            {
                out.push(file.path());
            }
        }
        out.sort();
        out
    }
    /// 기록 파일 원문에 각 JSON 안 Base64 본문(1MiB 이하)을 디코딩해 덧붙인다.
    /// 바이트 검색이 Base64로 감춰진 본문 누출도 잡게 하려는 것이다.
    pub fn history_bytes(&self) -> Vec<u8> {
        let mut out = vec![];
        for path in self.history_files() {
            let bytes = std::fs::read(&path).expect("history file");
            if path.extension().is_some_and(|e| e == "json") {
                let json: serde_json::Value = serde_json::from_slice(&bytes).expect("history json");
                if let Some(body) = json["bodyBase64"].as_str() {
                    out.extend(
                        data_encoding::BASE64
                            .decode(body.as_bytes())
                            .expect("history body base64"),
                    );
                }
            }
            out.extend(bytes);
        }
        out
    }
    pub async fn history_rows(&self) -> usize {
        self.api
            .history(HistoryQuery {
                before: None,
                limit: 256,
                origin: None,
            })
            .await
            .expect("history page")
            .items
            .len()
    }
    pub async fn only_history(&self) -> HistoryEntry {
        let page = self
            .api
            .history(HistoryQuery {
                before: None,
                limit: 256,
                origin: None,
            })
            .await
            .expect("history page");
        assert_eq!(page.items.len(), 1, "one history row");
        self.api
            .history_get(HistoryId {
                id: page.items[0].id.clone(),
            })
            .await
            .unwrap_or_else(|_| panic!("history entry"))
    }
}

/// RPC 오류(화면이 받는 wire 모양).
#[derive(Debug)]
pub struct Rpc(pub WireError);
impl Rpc {
    pub fn code(&self) -> &str {
        &self.0.code
    }
    pub fn detail(&self) -> serde_json::Value {
        self.0
            .detail
            .as_ref()
            .map(|raw| serde_json::from_str(raw.get()).expect("detail json"))
            .unwrap_or(serde_json::Value::Null)
    }
    pub fn detail_text(&self) -> String {
        serde_json::to_string(&self.0).expect("wire json")
    }
}

struct Sink {
    credit: Arc<Credit>,
    tx: mpsc::UnboundedSender<Option<Vec<u8>>>,
}
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
            self.credit.add(data.len() as u32);
            if !data.is_empty() {
                let _ = self.tx.send(Some(data));
            }
            if flags & FLAG_FIN != 0 {
                let _ = self.tx.send(None);
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
            .expect("opener")
            .take()
            .ok_or(StreamError::Closed)
    }
}

static NEXT: AtomicU32 = AtomicU32::new(1);

/// 열린 스트림의 화면 쪽.
pub struct Sent {
    pub stream_id: u32,
    pub closed: CancellationToken,
    rx: mpsc::UnboundedReceiver<Option<Vec<u8>>>,
    pending: Vec<u8>,
    finished: bool,
}
impl Sent {
    /// 화면 `ClientStream.end()`와 같다(데몬의 `tx.closed()` 취소).
    pub fn close(&self) {
        self.closed.cancel();
    }
    pub async fn next(&mut self, timeout: Duration) -> Option<SendEvent> {
        loop {
            if let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = self.pending.drain(..=end).collect();
                return Some(parse_event(&line[..end]));
            }
            if self.finished {
                return None;
            }
            match tokio::time::timeout(timeout, self.rx.recv()).await {
                Ok(Some(Some(data))) => self.pending.extend(data),
                Ok(Some(None)) | Ok(None) => self.finished = true,
                Err(_) => return None,
            }
        }
    }
    /// FIN까지 모든 이벤트(10초 상한).
    pub async fn events(mut self) -> Vec<SendEvent> {
        let mut out = vec![];
        while let Some(event) = self.next(Duration::from_secs(10)).await {
            out.push(event);
        }
        assert!(self.finished, "stream must finish");
        out
    }
    pub fn is_finished(&self) -> bool {
        self.finished
    }
}

/// 내부 태그 enum은 RawValue 세부를 직접 못 읽으므로 `failed`만 따로 읽는다.
fn parse_event(line: &[u8]) -> SendEvent {
    let value: serde_json::Value = serde_json::from_slice(line).expect("NDJSON event");
    if value["type"] == "failed" {
        let error = serde_json::to_string(&value["error"]).expect("error json");
        return SendEvent::Failed {
            error: serde_json::from_str(&error).expect("wire error"),
        };
    }
    serde_json::from_slice(line).expect("NDJSON event")
}

pub async fn send_rpc(api: &ApiService, request: SendRequest) -> Result<Sent, Rpc> {
    let (tx, rx) = mpsc::unbounded_channel();
    let credit = Arc::new(Credit::new(STREAM_WINDOW));
    let closed = CancellationToken::new();
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let stream = StreamTx::new(
        id,
        0,
        credit.clone(),
        Arc::new(Sink { credit, tx }),
        closed.clone(),
    );
    let (_input, input) = mpsc::channel(1);
    let ctx = devbox_protocol::Ctx {
        channel: 0,
        cancel: CancellationToken::new(),
        streams: Some(Arc::new(Opener(Mutex::new(Some(StreamEnds {
            id,
            tx: stream,
            input,
        }))))),
    };
    match api.send(ctx, request).await {
        Ok(opened) => Ok(Sent {
            stream_id: opened.stream_id,
            closed,
            rx,
            pending: vec![],
            finished: false,
        }),
        Err(devbox_protocol::Fail::Domain(e)) => Err(Rpc(devbox_protocol::to_wire(&e))),
        Err(devbox_protocol::Fail::Internal(m)) => panic!("internal failure: {m}"),
    }
}

/// 본문 이벤트를 이어 붙인 바이트.
pub fn body(events: &[SendEvent]) -> Vec<u8> {
    events
        .iter()
        .filter_map(|e| match e {
            SendEvent::Body { data_base64 } => Some(
                data_encoding::BASE64
                    .decode(data_base64.as_bytes())
                    .expect("base64 body"),
            ),
            _ => None,
        })
        .flatten()
        .collect()
}
pub fn head(events: &[SendEvent]) -> (u16, Vec<Pair>, Vec<Redirect>) {
    events
        .iter()
        .find_map(|e| match e {
            SendEvent::Head {
                status,
                headers,
                redirects,
                ..
            } => Some((*status, headers.clone(), redirects.clone())),
            _ => None,
        })
        .expect("head event")
}
pub fn failure(events: &[SendEvent]) -> WireError {
    events
        .iter()
        .find_map(|e| match e {
            SendEvent::Failed { error } => Some(error.clone()),
            _ => None,
        })
        .expect("failed event")
}
pub fn failure_kind(events: &[SendEvent]) -> String {
    let error = failure(events);
    let detail: serde_json::Value =
        serde_json::from_str(error.detail.as_ref().map(|d| d.get()).unwrap_or("null"))
            .expect("detail");
    format!("{}:{}", error.code, detail["kind"].as_str().unwrap_or(""))
}

// ── 요청 만들기 ──
pub fn snapshot(method: &str, url: &str) -> RequestSnapshot {
    RequestSnapshot {
        method: method.into(),
        url: url.into(),
        query: vec![],
        headers: vec![],
        cookies: vec![],
        body: Body::None,
        auth: Auth::None,
        tls: TlsOptions::default(),
        timeout_ms: 5_000,
        follow_redirects: true,
    }
}
pub fn request(snapshot: RequestSnapshot) -> SendRequest {
    SendRequest {
        snapshot,
        env: EnvSnapshot::default(),
        run_id: None,
        origin: None,
        cookie_jar: None,
    }
}
pub fn with_env(snapshot: RequestSnapshot, vars: &[(&str, &str, bool)]) -> SendRequest {
    SendRequest {
        env: EnvSnapshot {
            variables: vars
                .iter()
                .map(|(n, v, s)| EnvVar {
                    name: (*n).into(),
                    value: (*v).into(),
                    sensitive: *s,
                    origin: VarOrigin::Environment,
                })
                .collect(),
        },
        ..request(snapshot)
    }
}
/// 응답 캡처 출처 시험: `(이름, 값, 캡처 여부)`. 캡처 값은 화면이 sensitive로 보낸다.
pub fn with_captured(snapshot: RequestSnapshot, vars: &[(&str, &str, bool)]) -> SendRequest {
    SendRequest {
        env: EnvSnapshot {
            variables: vars
                .iter()
                .map(|(n, v, captured)| EnvVar {
                    name: (*n).into(),
                    value: (*v).into(),
                    sensitive: *captured,
                    origin: if *captured {
                        VarOrigin::Capture
                    } else {
                        VarOrigin::Environment
                    },
                })
                .collect(),
        },
        ..request(snapshot)
    }
}
pub fn pair(name: &str, value: &str) -> Pair {
    Pair {
        name: name.into(),
        value: value.into(),
    }
}
pub fn header(row: u32, name: &str, value: &str) -> HeaderRow {
    HeaderRow {
        row,
        name: name.into(),
        value: value.into(),
    }
}
pub fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len().max(1))
        .any(|w| w == needle.as_bytes())
}
