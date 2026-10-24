//! `api.*` 서비스. 송신은 `send` 하나가 소유하고 화면이 스트림을 닫으면 요청 future를 버린다.
use crate::auth::TokenCache;
use crate::cookies::Jars;
use crate::error::mapped;
use crate::events::Events;
use crate::history::{History, sanitize};
use crate::http::stream::{Captured, Stop};
use crate::http::{self, DnsFailed, Exec, Redaction};
use crate::tls::{self, Resolver};
use crate::*;
use devbox_core::{Db, IndexDb, Paths};
use devbox_protocol::{Ctx, Done, StreamOpened};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

/// 밀리초 단위 현재 시각(시험은 가짜 시계를 넣는다).
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

/// 인증 URL은 화면·브라우저로 그대로 나가므로 비밀 참조나 `sensitive` 변수 값을 담을 수 없다.
/// `client_id`·`scope`도 같은 URL의 query로 실리므로 함께 검사한다.
/// sensitive 변수는 원본 필드가 그 변수를 `{{NAME}}`·`${NAME}`으로 참조할 때만 막는다(짧은 값의 우연한 일치는 오탐).
/// 치환은 한 번뿐이라 조립한 `{{NAME}}`은 값으로 풀리지 않으므로 원본 참조만 보면 된다.
fn reject_secret_in_authorization_url(
    original: &RequestSnapshot,
    filled: &RequestSnapshot,
    env: &EnvSnapshot,
) -> Result<(), ApiError> {
    fn parts(auth: &Auth) -> Option<[Option<&String>; 3]> {
        let Auth::Oauth2AuthCode {
            authorization_url,
            client_id,
            scope,
            ..
        } = auth
        else {
            return None;
        };
        Some([Some(authorization_url), Some(client_id), scope.as_ref()])
    }
    let (Some(filled_parts), Some(original_parts)) = (parts(&filled.auth), parts(&original.auth))
    else {
        return Ok(());
    };
    let mut refs = std::collections::BTreeSet::new();
    for part in filled_parts.iter().flatten() {
        template::secret_names(part, &mut refs)?;
    }
    let mut used = std::collections::BTreeSet::new();
    for part in original_parts.iter().flatten() {
        template::variable_refs(part, &mut used);
    }
    let sensitive = env
        .variables
        .iter()
        .any(|v| v.masked() && used.contains(&v.name));
    if refs.is_empty() && !sensitive {
        return Ok(());
    }
    Err(ApiError::input("auth", "secret_in_authorization_url"))
}

fn system_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Default)]
pub struct ApiOptions {
    clock: Option<Clock>,
    resolver: Option<Resolver>,
}

struct HostResolver(Arc<dyn Fn(&str) -> bool + Send + Sync>);
impl reqwest::dns::Resolve for HostResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let allowed = (self.0)(name.as_str());
        Box::pin(async move {
            if allowed {
                let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 0));
                Ok(Box::new(std::iter::once(addr)) as reqwest::dns::Addrs)
            } else {
                Err(Box::new(DnsFailed) as Box<dyn std::error::Error + Send + Sync>)
            }
        })
    }
}

impl ApiOptions {
    pub fn with_clock(mut self, clock: impl Fn() -> i64 + Send + Sync + 'static) -> Self {
        self.clock = Some(Arc::new(clock));
        self
    }
    /// 시험용 이름 해석: 허용한 이름은 127.0.0.1, 나머지는 실패(외부 DNS를 쓰지 않는다).
    pub fn with_resolver(mut self, allow: impl Fn(&str) -> bool + Send + Sync + 'static) -> Self {
        self.resolver = Some(Arc::new(HostResolver(Arc::new(allow))));
        self
    }
}

pub(crate) struct Inner {
    history: History,
    jars: Jars,
    tokens: TokenCache,
    pub(crate) resolver: Option<Resolver>,
    rebuild: AtomicBool,
    ready: tokio::sync::watch::Sender<bool>,
    background: OnceLock<()>,
    /// 열린 WebSocket·SSE 세션 수 상한(`MAX_SESSIONS`).
    pub(crate) sessions: Arc<tokio::sync::Semaphore>,
    pub(crate) clock: Clock,
}

pub struct ApiService {
    db: Db,
    secrets: Arc<devbox_secrets::SecretsService>,
    /// S5-3 프로젝트 컬렉션이 쓴다.
    _projects: Arc<devbox_projects::ProjectsService>,
    pub(crate) inner: Arc<Inner>,
}

struct Job {
    snapshot: RequestSnapshot,
    prepared: http::build::Prepared,
    client: reqwest::Client,
    jar: Option<Arc<crate::cookies::Jar>>,
    values: Vec<String>,
    run_id: Option<String>,
    origin: Option<RequestOrigin>,
}

impl ApiService {
    pub fn new(
        db: Db,
        index: IndexDb,
        paths: Paths,
        projects: Arc<devbox_projects::ProjectsService>,
        secrets: Arc<devbox_secrets::SecretsService>,
    ) -> Result<Self, devbox_core::CoreError> {
        Self::with_options(db, index, paths, projects, secrets, ApiOptions::default())
    }

    pub fn with_options(
        db: Db,
        index: IndexDb,
        paths: Paths,
        projects: Arc<devbox_projects::ProjectsService>,
        secrets: Arc<devbox_secrets::SecretsService>,
        options: ApiOptions,
    ) -> Result<Self, devbox_core::CoreError> {
        let clock = options.clock.unwrap_or_else(|| Arc::new(system_ms));
        let (history, rebuild) =
            History::open(paths.data_dir.join("api-history"), index, clock.clone())?;
        let service = Self {
            db,
            secrets,
            _projects: projects,
            inner: Arc::new(Inner {
                history,
                jars: Jars::new(paths.data_dir.join("api-cookies")),
                tokens: TokenCache::default(),
                resolver: options.resolver,
                rebuild: AtomicBool::new(rebuild),
                ready: tokio::sync::watch::channel(false).0,
                background: OnceLock::new(),
                sessions: Arc::new(tokio::sync::Semaphore::new(crate::realtime::MAX_SESSIONS)),
                clock,
            }),
        };
        service.start_background();
        Ok(service)
    }

    /// 색인 재구성과 30일 정리. 런타임 밖에서 만든 서비스는 첫 호출 때 시작한다.
    fn start_background(&self) {
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        self.inner.background.get_or_init(|| {
            let inner = self.inner.clone();
            tokio::spawn(async move {
                if inner.rebuild.swap(false, Ordering::AcqRel)
                    && inner.history.rebuild().await.is_err()
                {
                    tracing::warn!(
                        code = "api.history_rebuild",
                        "api history index rebuild failed"
                    );
                }
                inner.ready.send_replace(true);
                loop {
                    if inner.history.prune().await.is_err() {
                        tracing::warn!(code = "api.history_prune", "api history prune failed");
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(24 * 60 * 60)).await;
                }
            });
        });
    }

    /// 색인 재구성이 끝날 때까지 기다린다.
    pub async fn wait_ready(&self) {
        self.start_background();
        let mut ready = self.inner.ready.subscribe();
        let _ = ready.wait_for(|r| *r).await;
    }

    pub async fn prune_history(&self) -> R<()> {
        self.inner
            .history
            .prune()
            .await
            .map_err(|_| devbox_protocol::Fail::Internal("prune api history".into()))
    }

    /// 변수 치환·비밀 해석(`api.send`·`api.ws_open`·`api.sse_open` 공통). 돌려주는 값 목록은 가림 대상이다.
    /// 출처가 섞인 변수, 조립한 비밀 참조, 인증 URL의 비밀은 여기서 거절한다(연결 전).
    pub(crate) async fn resolve(
        &self,
        snapshot: &RequestSnapshot,
        env: &EnvSnapshot,
    ) -> R<(RequestSnapshot, Vec<String>)> {
        template::reject_mixed_origins(env)?;
        let (mut filled, literals) = template::substitute_marked(snapshot, env)?;
        reject_secret_in_authorization_url(snapshot, &filled, env)?;
        let names = template::secrets_of(&filled)?;
        let allowed = template::allowed_secrets(snapshot, env)?;
        if names.iter().any(|name| !allowed.contains(name)) {
            return Err(ApiError::input("secret", "composed_reference").into());
        }
        let mut values = Vec::new();
        if !names.is_empty() {
            let resolved = self.secrets.resolve(&names).await.map_err(mapped)?;
            let mut text = BTreeMap::new();
            for (name, bytes) in resolved.iter() {
                let value = String::from_utf8(bytes.to_vec())
                    .map_err(|_| ApiError::input("secret", "encoding"))?;
                values.push(value.clone());
                text.insert(name.clone(), value);
            }
            template::fill_secrets(&mut filled, &text);
        }
        // 캡처 값은 비밀 해석이 끝난 뒤 글자 그대로 넣는다(값 검사·송신은 그 뒤).
        literals.restore(&mut filled);
        values.extend(
            env.variables
                .iter()
                .filter(|v| match v.origin {
                    // 캡처 값은 참조 모양이어도 글자 그대로 보낸 값이라 가린다.
                    VarOrigin::Capture => !v.value.is_empty(),
                    VarOrigin::Environment => v.sensitive && !template::is_reference(&v.value),
                })
                .map(|v| v.value.clone()),
        );
        Ok((filled, values))
    }

    /// 송신 전 검사 ①–⑤. 여기서 실패하면 스트림을 열지 않는다.
    async fn prepare(&self, p: SendRequest) -> R<Job> {
        if p.run_id.as_ref().is_some_and(|r| r.len() > 128) {
            return Err(ApiError::input("runId", "too_long").into());
        }
        let (filled, mut values) = self.resolve(&p.snapshot, &p.env).await?;
        let prepared = http::build::prepare(&filled)?;
        values.extend(prepared.seeds.iter().cloned());
        let client = tls::client(&prepared.tls, true, self.inner.resolver.as_ref())?;
        let jar = p
            .cookie_jar
            .as_deref()
            .map(|key| self.inner.jars.jar(key))
            .transpose()?;
        Ok(Job {
            snapshot: p.snapshot,
            prepared,
            client,
            jar,
            values,
            run_id: p.run_id,
            origin: p.origin,
        })
    }

    pub async fn send(&self, ctx: Ctx, p: SendRequest) -> R<StreamOpened> {
        self.start_background();
        let job = self.prepare(p).await?;
        let ends = ctx.open_stream().map_err(|_| ApiError::StreamUnsupported)?;
        let stream_id = ends.id;
        let inner = self.inner.clone();
        tokio::spawn(async move {
            drop(ends.input);
            run(inner, job, ends.tx).await;
        });
        Ok(StreamOpened { stream_id })
    }

    /// WebSocket·SSE 세션 자리. 검사가 끝난 뒤에 잡으므로 거절된 요청은 자리를 쓰지 않는다.
    pub(crate) fn session_slot(&self) -> R<tokio::sync::OwnedSemaphorePermit> {
        self.inner
            .sessions
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                ApiError::TooManyStreams {
                    limit: crate::realtime::MAX_SESSIONS as u32,
                }
                .into()
            })
    }

    /// WebSocket 세션. 검사·비밀 해석·TLS 재료 확인을 연결 전에 끝내고, 실패는 RPC 오류다.
    /// 화면 입력(`WsInput` NDJSON)은 스트림 입력으로 받는다. 세션은 기록하지 않는다.
    pub async fn ws_open(&self, ctx: Ctx, p: WsOpen) -> R<StreamOpened> {
        let snapshot = crate::realtime::ws::snapshot(&p);
        let (filled, values) = self.resolve(&snapshot, &p.env).await?;
        let prepared = crate::realtime::ws::prepare(&filled, &p.subprotocols)?;
        let permit = self.session_slot()?;
        let ends = ctx.open_stream().map_err(|_| ApiError::StreamUnsupported)?;
        let stream_id = ends.id;
        tokio::spawn(crate::realtime::ws::run(crate::realtime::ws::Session {
            prepared,
            tx: ends.tx,
            input: ends.input,
            redactor: crate::redact::Redactor::new(&values),
            clock: self.inner.clock.clone(),
            _permit: permit,
        }));
        Ok(StreamOpened { stream_id })
    }

    /// SSE 세션. 끊기면 `retry`(없으면 3초) 뒤 최대 5회 `Last-Event-ID`로 다시 연결한다. 기록하지 않는다.
    pub async fn sse_open(&self, ctx: Ctx, p: SseOpen) -> R<StreamOpened> {
        let snapshot = crate::realtime::sse::snapshot(&p);
        let (filled, values) = self.resolve(&snapshot, &p.env).await?;
        let prepared = crate::realtime::sse::prepare(
            &filled,
            p.last_event_id.as_deref(),
            self.inner.resolver.as_ref(),
        )?;
        let permit = self.session_slot()?;
        let ends = ctx.open_stream().map_err(|_| ApiError::StreamUnsupported)?;
        let stream_id = ends.id;
        drop(ends.input);
        tokio::spawn(crate::realtime::sse::run(crate::realtime::sse::Session {
            prepared,
            tx: ends.tx,
            redactor: crate::redact::Redactor::new(&values),
            clock: self.inner.clock.clone(),
            _permit: permit,
        }));
        Ok(StreamOpened { stream_id })
    }

    pub async fn history(&self, q: HistoryQuery) -> R<HistoryPage> {
        self.start_background();
        self.inner.history.page(q).await
    }

    pub async fn history_get(&self, q: HistoryId) -> R<HistoryEntry> {
        self.start_background();
        self.inner.history.get(q).await
    }

    pub async fn cookies(&self, q: CookieJarRef) -> R<CookieList> {
        Ok(self.inner.jars.list(&q.jar)?)
    }

    pub async fn cookies_clear(&self, q: CookieJarRef) -> R<Done> {
        self.inner.jars.clear(&q.jar)?;
        Ok(Done {
            rev: self.db.rev().to_string(),
        })
    }
}

async fn run(inner: Arc<Inner>, job: Job, tx: devbox_protocol::stream::StreamTx) {
    let started = std::time::Instant::now();
    let events = Events::new(&tx);
    let id = uuid::Uuid::new_v4().simple().to_string();
    let sent_at_ms = inner.history.now();
    let mut redaction = Redaction::new(job.values.clone());
    let mut out = Captured::default();
    let closed = tx.closed();
    let result = match events
        .send(&SendEvent::Started {
            history_id: id.clone(),
            sent_at_ms,
        })
        .await
    {
        Err(_) => Err(Stop::Closed),
        Ok(()) => {
            let exec = Exec {
                prepared: &job.prepared,
                client: &job.client,
                jar: job.jar.as_deref(),
                tokens: &inner.tokens,
                events: &events,
            };
            tokio::select! {
                biased;
                _ = closed.cancelled() => Err(Stop::Closed),
                r = http::execute(exec, &mut redaction, &mut out) => r,
            }
        }
    };
    let (outcome, error) = match result {
        Ok(()) => {
            let end = SendEvent::End {
                size_bytes: out.size_bytes,
                elapsed_ms: out.elapsed_ms,
                binary: out.binary,
                history_truncated: out.truncated,
            };
            match events.send(&end).await {
                Ok(()) => (HistoryOutcome::Ok, None),
                Err(_) => (HistoryOutcome::Cancelled, None),
            }
        }
        Err(Stop::Closed) => (HistoryOutcome::Cancelled, None),
        Err(Stop::Fail(error)) => {
            let _ = events.failed(&error).await;
            (
                HistoryOutcome::Failed,
                Some(devbox_protocol::to_wire(&error)),
            )
        }
    };
    if let Some(jar) = &job.jar
        && jar.save().is_err()
    {
        tracing::warn!(code = "api.cookies_write", "api cookie jar write failed");
    }
    let redactor = &redaction.redactor;
    let response = (out.status != 0).then(|| HistoryResponse {
        status: out.status,
        reason: out.reason.clone(),
        http_version: out.http_version.clone(),
        headers: out.headers.clone(),
        redirects: out.redirects.clone(),
        elapsed_ms: out.elapsed_ms.max(out.head_ms),
        size_bytes: out.size_bytes,
        binary: out.binary,
    });
    let entry = HistoryEntry {
        id,
        sent_at_ms,
        method: job.prepared.method.to_string(),
        url: redactor.text(&sanitize::url(job.prepared.url.as_str())),
        snapshot: sanitize::snapshot(&job.snapshot, redactor),
        tls_verify: job.prepared.tls.verify,
        run_id: job.run_id,
        origin: job.origin,
        outcome,
        error,
        response,
        body_base64: String::new(),
        body_truncated: out.truncated,
    };
    if inner
        .history
        .record(entry, std::mem::take(&mut out.body))
        .await
        .is_err()
    {
        tracing::warn!(code = "api.history_write", "api history write failed");
    }
    tracing::debug!(
        method = "api.send",
        elapsed_ms = started.elapsed().as_millis() as u64,
        outcome = outcome.as_str()
    );
    tx.finish().await;
}
