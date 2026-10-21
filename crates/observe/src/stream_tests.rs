use crate::*;
use devbox_protocol::{
    Ctx,
    frame::Frame,
    stream::{
        Credit, FLAG_FIN, StreamEnds, StreamError, StreamOpener, StreamSink, StreamTx, parse_stream,
    },
};
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;
#[derive(Default)]
struct Sink(Mutex<Vec<Frame>>);
impl StreamSink for Sink {
    fn push(&self, frame: Frame) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        self.0.lock().expect("fixture result").push(frame);
        Box::pin(async { true })
    }
}
pub(crate) struct Open {
    sink: Arc<Sink>,
    pub credit: Arc<Credit>,
    pub closed: CancellationToken,
}
impl Open {
    pub fn new(credit: u32) -> Arc<Self> {
        Arc::new(Self {
            sink: Arc::default(),
            credit: Arc::new(Credit::new(credit)),
            closed: CancellationToken::new(),
        })
    }
    pub fn ctx(self: &Arc<Self>) -> Ctx {
        Ctx {
            channel: 0,
            cancel: CancellationToken::new(),
            streams: Some(self.clone()),
        }
    }
    pub fn data(&self) -> Vec<u8> {
        self.sink
            .0
            .lock()
            .expect("fixture result")
            .iter()
            .flat_map(|f| parse_stream(&f.body).expect("fixture value").2.to_vec())
            .collect()
    }
    pub fn finished(&self) -> bool {
        self.sink
            .0
            .lock()
            .expect("fixture result")
            .iter()
            .any(|f| parse_stream(&f.body).expect("fixture value").1 == FLAG_FIN)
    }
}
impl StreamOpener for Open {
    fn open(&self) -> Result<StreamEnds, StreamError> {
        let (_, input) = tokio::sync::mpsc::channel(1);
        Ok(StreamEnds {
            id: 1,
            tx: StreamTx::new(
                1,
                0,
                self.credit.clone(),
                self.sink.clone(),
                self.closed.clone(),
            ),
            input,
        })
    }
}
pub(crate) async fn until(mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("condition deadline");
}
#[tokio::test]
async fn open_returns_with_zero_credit_and_does_not_prefetch_more_heads() {
    let temp = tempfile::tempdir().expect("fixture result");
    let path = temp.path().join("log");
    std::fs::write(&path, ("x".repeat(100 * 1024) + "\n").repeat(100)).expect("fixture result");
    let service = ObserveService::default();
    let open = Open::new(0);
    let source = LogSource::File {
        path: path.display().to_string(),
    };
    let result = tokio::time::timeout(
        Duration::from_secs(1),
        service.open(
            open.ctx(),
            OpenLogs {
                sources: vec![source.clone()],
                from: LogFrom::Offset { offset: 0 },
                filter: None,
            },
            vec![ResolvedSource {
                source,
                path: Some(path),
            }],
            vec![temp.path().into()],
        ),
    )
    .await
    .expect("fixture result")
    .expect("fixture result");
    assert_eq!(result.stream_id, 1);
    until(|| service.file_bytes_read() > 0).await;
    tokio::time::sleep(Duration::from_millis(25)).await;
    let read = service.file_bytes_read();
    assert!(read <= 108 * 1024, "{read}");
    assert!(open.data().is_empty());
    tokio::time::sleep(Duration::from_millis(25)).await;
    assert_eq!(service.file_bytes_read(), read);
    open.closed.cancel();
    service.shutdown().await;
}

#[tokio::test]
async fn follows_allowed_symlink_target_append_and_replaces_watch_parent() {
    use std::io::Write;
    let temp = tempfile::tempdir().expect("fixture result");
    let a = temp.path().join("a");
    let b = temp.path().join("b");
    std::fs::create_dir(&a).expect("fixture result");
    std::fs::create_dir(&b).expect("fixture result");
    let first = a.join("log");
    let second = b.join("log");
    let link = temp.path().join("link");
    std::fs::write(&first, b"first\n").expect("fixture result");
    std::fs::write(&second, b"second\n").expect("fixture result");
    std::os::unix::fs::symlink(&first, &link).expect("fixture result");
    let service = ObserveService::default();
    let open = Open::new(256 * 1024);
    let source = LogSource::File {
        path: link.display().to_string(),
    };
    service
        .open(
            open.ctx(),
            OpenLogs {
                sources: vec![source.clone()],
                from: LogFrom::Offset { offset: 0 },
                filter: None,
            },
            vec![ResolvedSource {
                source,
                path: Some(link.clone()),
            }],
            vec![temp.path().into()],
        )
        .await
        .expect("fixture result");
    until(|| String::from_utf8_lossy(&open.data()).contains("first")).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    std::fs::OpenOptions::new()
        .append(true)
        .open(&first)
        .expect("fixture result")
        .write_all(b"first-append\n")
        .expect("fixture result");
    until(|| String::from_utf8_lossy(&open.data()).contains("first-append")).await;
    std::fs::remove_file(&link).expect("fixture result");
    std::os::unix::fs::symlink(&second, &link).expect("fixture result");
    until(|| String::from_utf8_lossy(&open.data()).contains("second")).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    std::fs::OpenOptions::new()
        .append(true)
        .open(&second)
        .expect("fixture result")
        .write_all(b"second-append\n")
        .expect("fixture result");
    until(|| String::from_utf8_lossy(&open.data()).contains("second-append")).await;
    open.closed.cancel();
    service.shutdown().await;
}
