use devbox_lsp::{adapters::DocumentGrant, servers::ServerSpec, *};
use devbox_protocol::{
    Ctx,
    frame::Frame,
    stream::{
        Credit, STREAM_WINDOW, StreamEnds, StreamError, StreamOpener, StreamSink, StreamTx,
        parse_stream,
    },
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
struct Sink(mpsc::Sender<Frame>);
impl StreamSink for Sink {
    fn push(&self, frame: Frame) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        Box::pin(async move { self.0.send(frame).await.is_ok() })
    }
}
#[derive(Default)]
struct Opener {
    next: AtomicU32,
    clients: Mutex<BTreeMap<u32, Client>>,
}
impl StreamOpener for Opener {
    fn open(&self) -> Result<StreamEnds, StreamError> {
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        let (input, receiver) = mpsc::channel(16);
        let (output, frames) = mpsc::channel(64);
        let closed = CancellationToken::new();
        let credit = Arc::new(Credit::new(STREAM_WINDOW));
        let tx = StreamTx::new(
            id,
            0,
            credit.clone(),
            Arc::new(Sink(output)),
            closed.clone(),
        );
        self.clients.lock().expect("clients").insert(
            id,
            Client {
                input,
                frames,
                closed,
                credit,
                buffer: Vec::new(),
            },
        );
        Ok(StreamEnds {
            id,
            tx,
            input: receiver,
        })
    }
}
pub struct Client {
    input: mpsc::Sender<bytes::Bytes>,
    frames: mpsc::Receiver<Frame>,
    pub closed: CancellationToken,
    credit: Arc<Credit>,
    buffer: Vec<u8>,
}
impl Client {
    pub async fn send(&self, value: Value) {
        let mut data = serde_json::to_vec(&RelayMessage {
            json: value.to_string(),
            lease_id: None,
        })
        .expect("packet");
        data.push(b'\n');
        self.input.send(data.into()).await.expect("input");
    }
    pub async fn reply(&mut self) -> Value {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if let Some(end) = self.buffer.iter().position(|b| *b == b'\n') {
                    let packet: RelayMessage =
                        serde_json::from_slice(&self.buffer[..end]).expect("packet");
                    self.buffer.drain(..end + 1);
                    return serde_json::from_str(&packet.json).expect("JSON");
                }
                let frame = self.frames.recv().await.expect("output");
                let (_, flags, data) = parse_stream(&frame.body).expect("frame");
                assert_eq!(flags, 0, "unexpected FIN");
                self.buffer.extend_from_slice(data);
                self.credit.add(data.len() as u32);
            }
        })
        .await
        .expect("response deadline")
    }
    pub fn close(&self) {
        self.closed.cancel();
    }
}
pub struct Fixture {
    pub service: LspService,
    opener: Arc<Opener>,
    root: tempfile::TempDir,
}
impl Fixture {
    pub fn new() -> Self {
        let executable = std::env::current_exe().expect("test executable");
        let examples = executable
            .parent()
            .expect("deps")
            .parent()
            .expect("debug")
            .join("examples");
        Self {
            service: LspService::with_specs(
                examples.into_os_string(),
                vec![ServerSpec {
                    language: Language::Rust,
                    command: "fake_lsp".into(),
                    args: vec![],
                    install: None,
                }],
            ),
            opener: Arc::default(),
            root: tempfile::tempdir().expect("fixture"),
        }
    }
    pub fn ctx(&self) -> Ctx {
        Ctx {
            channel: 0,
            cancel: CancellationToken::new(),
            streams: Some(self.opener.clone()),
        }
    }
    pub fn client(&self, id: u32) -> Client {
        self.opener
            .clients
            .lock()
            .expect("clients")
            .remove(&id)
            .expect("client")
    }
    pub fn project(&self) -> ProjectContext {
        ProjectContext {
            project_id: "p".into(),
            root: self.root.path().canonicalize().expect("canonical"),
            access: Arc::new(|uri, lease_id| {
                Box::pin(async move {
                    Ok(DocumentGrant {
                        uri,
                        disk_text: "disk".into(),
                        disk_rev: None,
                        lease_id,
                    })
                })
            }),
            capture: Arc::new(|_, _| Box::pin(async { Err(LspError::LimitExceeded) })),
            diagnostics: Arc::new(|_| Ok(())),
            restarted: Arc::new(|_| {}),
        }
    }
}
