//! WebSocket·SSE 시험 fixture: 127.0.0.1:0 소유 tungstenite 서버(평문·TLS)와 원시 TCP SSE 서버,
//! credit을 붙잡아 둘 수 있는 가짜 화면 스트림(입력 NDJSON을 보낼 수 있다).
use super::api::Rpc;
use super::tls::ServerTls;
use bytes::Bytes;
use devbox_api::*;
use devbox_protocol::frame::Frame;
use devbox_protocol::stream::{
    Credit, FLAG_FIN, STREAM_WINDOW, StreamEnds, StreamError, StreamOpener, StreamSink, StreamTx,
    parse_stream,
};
use futures::{SinkExt, StreamExt};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{Notify, mpsc};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

// ── 가짜 화면 스트림 ──

/// credit 반환을 멈출 수 있는 화면(받은 바이트만큼 돌려주는 대신 `release` 때까지 쌓아 둔다).
struct Gate {
    credit: Arc<Credit>,
    hold: AtomicBool,
    owed: AtomicU32,
}
struct Sink {
    gate: Arc<Gate>,
    tx: mpsc::UnboundedSender<Option<Vec<u8>>>,
}
impl StreamSink for Sink {
    fn push(&self, frame: Frame) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        let parsed = parse_stream(&frame.body).map(|(_, flags, data)| (flags, data.to_vec()));
        Box::pin(async move {
            let Some((flags, data)) = parsed else {
                return false;
            };
            if self.gate.hold.load(Ordering::SeqCst) {
                self.gate
                    .owed
                    .fetch_add(data.len() as u32, Ordering::SeqCst);
            } else {
                self.gate.credit.add(data.len() as u32);
            }
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
static NEXT: AtomicU32 = AtomicU32::new(10_000);

pub struct Io {
    pub stream_id: u32,
    closed: CancellationToken,
    input: Option<mpsc::Sender<Bytes>>,
    rx: mpsc::UnboundedReceiver<Option<Vec<u8>>>,
    pending: Vec<u8>,
    finished: bool,
    gate: Arc<Gate>,
}

/// 화면 credit 설정: `initial` 바이트로 시작, `hold`면 `release` 전까지 돌려주지 않는다.
#[derive(Clone, Copy)]
pub struct Screen {
    pub initial: u32,
    pub hold: bool,
}
impl Default for Screen {
    fn default() -> Self {
        Self {
            initial: STREAM_WINDOW,
            hold: false,
        }
    }
}

fn ctx(screen: Screen) -> (devbox_protocol::Ctx, Io) {
    let (tx, rx) = mpsc::unbounded_channel();
    let credit = Arc::new(Credit::new(screen.initial));
    let gate = Arc::new(Gate {
        credit: credit.clone(),
        hold: AtomicBool::new(screen.hold),
        owed: AtomicU32::new(0),
    });
    let closed = CancellationToken::new();
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let stream = StreamTx::new(
        id,
        0,
        credit,
        Arc::new(Sink {
            gate: gate.clone(),
            tx,
        }),
        closed.clone(),
    );
    // 데몬 실제 값과 같은 입력 큐(256 조각).
    let (input_tx, input) = mpsc::channel(256);
    let ctx = devbox_protocol::Ctx {
        channel: 0,
        cancel: CancellationToken::new(),
        streams: Some(Arc::new(Opener(Mutex::new(Some(StreamEnds {
            id,
            tx: stream,
            input,
        }))))),
    };
    let io = Io {
        stream_id: id,
        closed,
        input: Some(input_tx),
        rx,
        pending: vec![],
        finished: false,
        gate,
    };
    (ctx, io)
}

fn rejected(e: devbox_protocol::Fail<ApiError>) -> Rpc {
    match e {
        devbox_protocol::Fail::Domain(e) => Rpc(devbox_protocol::to_wire(&e)),
        devbox_protocol::Fail::Internal(m) => panic!("internal failure: {m}"),
    }
}

pub async fn open_ws(api: &ApiService, p: WsOpen) -> Result<Io, Rpc> {
    open_ws_with(api, p, Screen::default()).await
}
pub async fn open_ws_with(api: &ApiService, p: WsOpen, screen: Screen) -> Result<Io, Rpc> {
    let (ctx, mut io) = ctx(screen);
    let opened = api.ws_open(ctx, p).await.map_err(rejected)?;
    io.stream_id = opened.stream_id;
    Ok(io)
}
pub async fn open_sse(api: &ApiService, p: SseOpen) -> Result<Io, Rpc> {
    let (ctx, mut io) = ctx(Screen::default());
    let opened = api.sse_open(ctx, p).await.map_err(rejected)?;
    io.stream_id = opened.stream_id;
    Ok(io)
}

/// `Failed`의 RawValue 세부는 내부 태그 enum으로 바로 못 읽어 따로 읽는다.
fn parse(line: &[u8]) -> RealtimeEvent {
    let value: serde_json::Value = serde_json::from_slice(line).expect("NDJSON event");
    if value["type"] == "failed" {
        let error = serde_json::to_string(&value["error"]).expect("error json");
        return RealtimeEvent::Failed {
            error: serde_json::from_str(&error).expect("wire error"),
        };
    }
    serde_json::from_slice(line).expect("NDJSON event")
}

impl Io {
    /// 화면 `ClientStream.end()`(데몬 `tx.closed()` 취소).
    pub fn close(&self) {
        self.closed.cancel();
    }
    /// 붙잡아 둔 credit을 돌려주고 이후로는 받은 만큼 바로 돌려준다.
    pub fn release(&self) {
        self.gate.hold.store(false, Ordering::SeqCst);
        let owed = self.gate.owed.swap(0, Ordering::SeqCst);
        self.gate.credit.add(owed.max(STREAM_WINDOW));
    }
    pub async fn send(&self, input: WsInput) {
        let mut line = serde_json::to_vec(&input).expect("input json");
        line.push(b'\n');
        self.send_raw(&line).await;
    }
    /// 64KiB 조각으로 나눠 보낸다(데몬 입력 조각 상한).
    pub async fn send_raw(&self, bytes: &[u8]) {
        let input = self.input.as_ref().expect("input open");
        for chunk in bytes.chunks(64 * 1024) {
            input
                .send(Bytes::copy_from_slice(chunk))
                .await
                .expect("daemon input");
        }
    }
    pub fn end_input(&mut self) {
        self.input = None;
    }
    pub async fn next_event(&mut self, timeout: Duration) -> Option<RealtimeEvent> {
        loop {
            if let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = self.pending.drain(..=end).collect();
                return Some(parse(&line[..end]));
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
    /// FIN까지(시한 안에 끝나지 않으면 실패).
    pub async fn events(&mut self, timeout: Duration) -> Vec<RealtimeEvent> {
        let mut out = vec![];
        while let Some(event) = self.next_event(timeout).await {
            out.push(event);
        }
        assert!(self.finished, "stream must finish, got {out:?}");
        out
    }
    pub fn is_finished(&self) -> bool {
        self.finished
    }
}

pub fn received_text(data: &str) -> RealtimeEvent {
    RealtimeEvent::Message {
        direction: Direction::Received,
        kind: MessageKind::Text,
        data: Some(data.into()),
        data_base64: None,
        at_ms: 0,
    }
}
/// 시각을 뺀 비교용 모양.
pub fn shape(event: &RealtimeEvent) -> String {
    match event {
        RealtimeEvent::Message {
            direction,
            kind,
            data,
            data_base64,
            ..
        } => format!("message:{direction:?}:{kind:?}:{data:?}:{data_base64:?}"),
        RealtimeEvent::SseEvent {
            id, event, data, ..
        } => format!("sse:{id:?}:{event:?}:{data}"),
        RealtimeEvent::Failed { error } => format!("failed:{}", error.code),
        other => format!("{other:?}"),
    }
}

// ── WebSocket 서버 ──

pub type WsSocket = tokio_tungstenite::WebSocketStream<Box<dyn TlsOrPlain>>;
pub trait TlsOrPlain: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> TlsOrPlain for T {}

pub type WsBehavior =
    Arc<dyn Fn(WsSocket, Arc<WsState>) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

#[derive(Default)]
pub struct WsState {
    pub accepted: AtomicUsize,
    pub upgraded: AtomicUsize,
    pub ended: AtomicUsize,
    pub requests: Mutex<Vec<Vec<(String, String)>>>,
    pub client_cert: AtomicBool,
    pub flood_done: AtomicBool,
    pub changed: Notify,
}

pub struct WsServer {
    pub addr: std::net::SocketAddr,
    pub scheme: &'static str,
    pub state: Arc<WsState>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for WsServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub fn behavior<F, Fut>(f: F) -> WsBehavior
where
    F: Fn(WsSocket, Arc<WsState>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    Arc::new(move |socket, state| Box::pin(f(socket, state)))
}

/// 받은 텍스트는 `pong:<text>`, 이진은 그대로, 닫기는 tungstenite가 응답한다.
pub fn echo() -> WsBehavior {
    behavior(|mut socket: WsSocket, _| async move {
        while let Some(Ok(message)) = socket.next().await {
            let reply = match message {
                Message::Text(text) => Message::text(format!("pong:{text}")),
                Message::Binary(data) => Message::Binary(data),
                // 닫기 응답은 다음 읽기에서 나가므로 끝(None)까지 계속 읽는다.
                _ => continue,
            };
            if socket.send(reply).await.is_err() {
                break;
            }
        }
        let _ = socket.close(None).await;
    })
}

impl WsServer {
    pub async fn start(behavior: WsBehavior) -> Self {
        Self::spawn(behavior, None, None).await
    }
    /// `subprotocol`: 서버가 고르는 값(요청에 없어도 돌려준다).
    pub async fn spawn(
        behavior: WsBehavior,
        tls: Option<ServerTls>,
        subprotocol: Option<&'static str>,
    ) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind owned fixture");
        let addr = listener.local_addr().expect("fixture address");
        let state = Arc::new(WsState::default());
        let scheme = if tls.is_some() { "wss" } else { "ws" };
        let acceptor = tls.map(super::tls::acceptor);
        let task_state = state.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                task_state.accepted.fetch_add(1, Ordering::SeqCst);
                task_state.changed.notify_waiters();
                let (state, behavior, acceptor) =
                    (task_state.clone(), behavior.clone(), acceptor.clone());
                tokio::spawn(async move {
                    let io: Option<Box<dyn TlsOrPlain>> = match acceptor {
                        None => Some(Box::new(stream)),
                        Some(acceptor) => match acceptor.accept(stream).await {
                            Ok(tls) => {
                                if tls.get_ref().1.peer_certificates().is_some() {
                                    state.client_cert.store(true, Ordering::SeqCst);
                                }
                                Some(Box::new(tls))
                            }
                            Err(_) => None,
                        },
                    };
                    if let Some(io) = io {
                        let seen = state.clone();
                        // tungstenite `Callback`의 오류 응답 형식이 정해져 있다.
                        #[allow(clippy::result_large_err)]
                        let callback = move |request: &tokio_tungstenite::tungstenite::handshake::server::Request,
                                             mut response: tokio_tungstenite::tungstenite::handshake::server::Response| {
                            seen.requests.lock().expect("requests").push(
                                request
                                    .headers()
                                    .iter()
                                    .map(|(n, v)| {
                                        (n.as_str().to_owned(), String::from_utf8_lossy(v.as_bytes()).into_owned())
                                    })
                                    .collect(),
                            );
                            if let Some(protocol) = subprotocol {
                                response.headers_mut().insert(
                                    "sec-websocket-protocol",
                                    protocol.parse().expect("protocol header"),
                                );
                            }
                            Ok(response)
                        };
                        if let Ok(socket) = tokio_tungstenite::accept_hdr_async(io, callback).await
                        {
                            state.upgraded.fetch_add(1, Ordering::SeqCst);
                            state.changed.notify_waiters();
                            behavior(socket, state.clone()).await;
                        }
                    }
                    state.ended.fetch_add(1, Ordering::SeqCst);
                    state.changed.notify_waiters();
                });
            }
        });
        Self {
            addr,
            scheme,
            state,
            task,
        }
    }
    pub fn url(&self, path: &str) -> String {
        format!("{}://{}{}", self.scheme, self.addr, path)
    }
    pub fn accepted(&self) -> usize {
        self.state.accepted.load(Ordering::SeqCst)
    }
    pub fn ended(&self) -> usize {
        self.state.ended.load(Ordering::SeqCst)
    }
    pub fn requests(&self) -> Vec<Vec<(String, String)>> {
        self.state.requests.lock().expect("requests").clone()
    }
    pub async fn wait(&self, done: impl Fn(&Self) -> bool, timeout: Duration) -> bool {
        wait(&self.state.changed, || done(self), timeout).await
    }
}

async fn wait(changed: &Notify, done: impl Fn() -> bool, timeout: Duration) -> bool {
    tokio::time::timeout(timeout, async {
        loop {
            let notified = changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if done() {
                return;
            }
            notified.await;
        }
    })
    .await
    .is_ok()
}

pub fn seen_header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

// ── SSE 서버(원시 TCP: 연결마다 대본 하나, Content-Length 없이 끝은 연결 종료) ──

#[derive(Clone)]
pub enum SseStep {
    /// 상태 줄·헤더(기본 200 `text/event-stream`).
    Head(u16, &'static str),
    Write(String),
    Sleep(Duration),
    /// 화면이 끊을 때까지 연결을 붙잡는다.
    Hold,
}

#[derive(Default)]
pub struct SseState {
    pub accepted: AtomicUsize,
    pub ended: AtomicUsize,
    pub requests: Mutex<Vec<Vec<(String, String)>>>,
    pub changed: Notify,
}

pub struct SseServer {
    pub addr: std::net::SocketAddr,
    pub state: Arc<SseState>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for SseServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl SseServer {
    /// `scripts[i]`는 i번째 연결의 대본(넘으면 마지막 대본을 되풀이).
    pub async fn start(scripts: Vec<Vec<SseStep>>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind owned fixture");
        let addr = listener.local_addr().expect("fixture address");
        let state = Arc::new(SseState::default());
        let task_state = state.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let index = task_state.accepted.fetch_add(1, Ordering::SeqCst);
                let script = scripts[index.min(scripts.len() - 1)].clone();
                let state = task_state.clone();
                tokio::spawn(async move {
                    let mut head = vec![];
                    let mut byte = [0u8; 1];
                    while !head.ends_with(b"\r\n\r\n") && head.len() < 64 * 1024 {
                        if stream.read(&mut byte).await.unwrap_or(0) == 0 {
                            break;
                        }
                        head.push(byte[0]);
                    }
                    let text = String::from_utf8_lossy(&head).into_owned();
                    state.requests.lock().expect("requests").push(
                        text.lines()
                            .skip(1)
                            .filter_map(|l| l.split_once(':'))
                            .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_owned()))
                            .collect(),
                    );
                    state.changed.notify_waiters();
                    let mut wrote_head = false;
                    for step in script {
                        let ok = match step {
                            SseStep::Head(status, content_type) => {
                                wrote_head = true;
                                let head = format!(
                                    "HTTP/1.1 {status} X\r\ncontent-type: {content_type}\r\nconnection: close\r\n\r\n"
                                );
                                stream.write_all(head.as_bytes()).await.is_ok()
                            }
                            SseStep::Write(text) => {
                                if !wrote_head {
                                    wrote_head = true;
                                    let _ = stream
                                        .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n")
                                        .await;
                                }
                                stream.write_all(text.as_bytes()).await.is_ok()
                                    && stream.flush().await.is_ok()
                            }
                            SseStep::Sleep(d) => {
                                tokio::time::sleep(d).await;
                                true
                            }
                            SseStep::Hold => {
                                let mut buf = [0u8; 64];
                                while stream.read(&mut buf).await.unwrap_or(0) > 0 {}
                                true
                            }
                        };
                        if !ok {
                            break;
                        }
                    }
                    let _ = stream.shutdown().await;
                    state.ended.fetch_add(1, Ordering::SeqCst);
                    state.changed.notify_waiters();
                });
            }
        });
        Self { addr, state, task }
    }
    pub fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }
    pub fn accepted(&self) -> usize {
        self.state.accepted.load(Ordering::SeqCst)
    }
    pub fn ended(&self) -> usize {
        self.state.ended.load(Ordering::SeqCst)
    }
    pub fn requests(&self) -> Vec<Vec<(String, String)>> {
        self.state.requests.lock().expect("requests").clone()
    }
    pub async fn wait(&self, done: impl Fn(&Self) -> bool, timeout: Duration) -> bool {
        wait(&self.state.changed, || done(self), timeout).await
    }
}

// ── 공통 열기 도우미 ──

pub fn ws(url: &str) -> WsOpen {
    WsOpen {
        url: url.into(),
        headers: vec![],
        subprotocols: vec![],
        tls: TlsOptions::default(),
        env: EnvSnapshot::default(),
        origin: None,
    }
}

/// 첫 이벤트가 `Open`인지 확인한다.
pub async fn first_open(io: &mut Io) -> RealtimeEvent {
    let event = io
        .next_event(Duration::from_secs(10))
        .await
        .expect("open event");
    assert!(matches!(event, RealtimeEvent::Open { .. }), "{event:?}");
    event
}
