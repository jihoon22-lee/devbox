//! gRPC·MCP 스트림용 가짜 화면: 아무 스트림 메서드나 열고 NDJSON 줄을 JSON 값으로 읽는다(입력도 보낼 수 있다).
use super::api::Rpc;
use bytes::Bytes;
use devbox_api::ApiError;
use devbox_protocol::frame::Frame;
use devbox_protocol::stream::{
    Credit, FLAG_FIN, STREAM_WINDOW, StreamEnds, StreamError, StreamOpener, StreamSink, StreamTx,
    parse_stream,
};
use devbox_protocol::{Ctx, Fail, StreamOpened};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

struct Sink {
    credit: Arc<Credit>,
    tx: mpsc::UnboundedSender<Option<Vec<u8>>>,
}
impl StreamSink for Sink {
    fn push(&self, frame: Frame) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
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
static NEXT: AtomicU32 = AtomicU32::new(20_000);

pub struct Lines {
    pub stream_id: u32,
    closed: CancellationToken,
    input: Option<mpsc::Sender<Bytes>>,
    rx: mpsc::UnboundedReceiver<Option<Vec<u8>>>,
    pending: Vec<u8>,
    finished: bool,
}

fn rejected(e: Fail<ApiError>) -> Rpc {
    match e {
        Fail::Domain(e) => Rpc(devbox_protocol::to_wire(&e)),
        Fail::Internal(m) => panic!("internal failure: {m}"),
    }
}

/// `open(|ctx| api.grpc_call(ctx, p))`처럼 스트림 메서드를 연다. 검사 실패는 RPC 오류로 돌아온다.
pub async fn open<F, Fut>(f: F) -> Result<Lines, Rpc>
where
    F: FnOnce(Ctx) -> Fut,
    Fut: Future<Output = Result<StreamOpened, Fail<ApiError>>>,
{
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
    let (input_tx, input) = mpsc::channel(256);
    let ctx = Ctx {
        channel: 0,
        cancel: CancellationToken::new(),
        streams: Some(Arc::new(Opener(Mutex::new(Some(StreamEnds {
            id,
            tx: stream,
            input,
        }))))),
    };
    let opened = f(ctx).await.map_err(rejected)?;
    Ok(Lines {
        stream_id: opened.stream_id,
        closed,
        input: Some(input_tx),
        rx,
        pending: vec![],
        finished: false,
    })
}

impl Lines {
    /// 화면 `ClientStream.end()`.
    pub fn close(&self) {
        self.closed.cancel();
    }
    pub async fn send(&self, value: serde_json::Value) {
        let mut line = serde_json::to_vec(&value).expect("input json");
        line.push(b'\n');
        self.input
            .as_ref()
            .expect("input open")
            .send(Bytes::from(line))
            .await
            .expect("daemon input");
    }
    pub async fn next(&mut self, timeout: Duration) -> Option<serde_json::Value> {
        loop {
            if let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = self.pending.drain(..=end).collect();
                return Some(serde_json::from_slice(&line[..end]).expect("NDJSON line"));
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
    /// 다음 `type == kind` 이벤트(그 전 이벤트는 돌려주는 목록에 담긴다).
    pub async fn until(
        &mut self,
        kind: &str,
        timeout: Duration,
    ) -> (Option<serde_json::Value>, Vec<serde_json::Value>) {
        let mut before = vec![];
        while let Some(event) = self.next(timeout).await {
            if event["type"] == kind {
                return (Some(event), before);
            }
            before.push(event);
        }
        (None, before)
    }
    /// FIN까지 모든 줄.
    pub async fn all(&mut self, timeout: Duration) -> Vec<serde_json::Value> {
        let mut out = vec![];
        while let Some(event) = self.next(timeout).await {
            out.push(event);
        }
        assert!(self.finished, "stream must finish, got {out:?}");
        out
    }
    pub fn is_finished(&self) -> bool {
        self.finished
    }
}
