//! Credit-controlled byte streams; the lifetime is independent of channel generations.
use crate::frame::{Frame, FrameKind, MAX_STREAM_CHUNK};
use bytes::{BufMut, Bytes, BytesMut};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use tokio::sync::{Mutex, Notify, mpsc};
use tokio_util::sync::CancellationToken;

pub const STREAM_WINDOW: u32 = 256 * 1024;
pub const FLAG_FIN: u8 = 1;
#[derive(Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct StreamOpened {
    pub stream_id: u32,
}

pub fn stream_frame(channel: u32, stream: u32, flags: u8, data: &[u8]) -> Frame {
    let mut body = BytesMut::with_capacity(5 + data.len());
    body.put_u32_le(stream);
    body.put_u8(flags);
    body.put_slice(data);
    Frame {
        kind: FrameKind::Stream,
        channel,
        body: body.freeze(),
    }
}
pub fn parse_stream(body: &[u8]) -> Option<(u32, u8, &[u8])> {
    if body.len() < 5 || body.len() > MAX_STREAM_CHUNK + 5 || body[4] & !FLAG_FIN != 0 {
        return None;
    }
    Some((
        u32::from_le_bytes(body[..4].try_into().ok()?),
        body[4],
        &body[5..],
    ))
}
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StreamError {
    #[error("stream closed")]
    Closed,
    #[error("this connection cannot open streams")]
    Unsupported,
}
pub trait StreamSink: Send + Sync {
    fn push(&self, frame: Frame) -> Pin<Box<dyn Future<Output = bool> + Send + '_>>;
}
pub struct Credit {
    avail: AtomicU32,
    notify: Notify,
}
impl Credit {
    pub fn new(initial: u32) -> Self {
        Self {
            avail: AtomicU32::new(initial.min(STREAM_WINDOW)),
            notify: Notify::new(),
        }
    }
    pub fn add(&self, n: u32) {
        let _ = self
            .avail
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |old| {
                Some(old.saturating_add(n).min(STREAM_WINDOW))
            });
        self.notify.notify_waiters();
    }
    /// Reserve up to n available bytes, allowing small credit grants to make progress.
    pub async fn take(&self, n: u32, closed: &CancellationToken) -> Result<u32, StreamError> {
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if closed.is_cancelled() {
                return Err(StreamError::Closed);
            }
            let cur = self.avail.load(Ordering::Acquire);
            let take = cur.min(n);
            if take > 0
                && self
                    .avail
                    .compare_exchange(cur, cur - take, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            {
                return Ok(take);
            }
            if cur > 0 {
                continue;
            }
            tokio::select! { _ = notified => {}, _ = closed.cancelled() => return Err(StreamError::Closed) }
        }
    }
}
#[derive(Clone)]
pub struct StreamTx {
    pub id: u32,
    channel: u32,
    credit: Arc<Credit>,
    sink: Arc<dyn StreamSink>,
    closed: CancellationToken,
    // Serialize the final enqueue with finish; no data may follow FIN.
    sending: Arc<Mutex<()>>,
    finishing: Arc<AtomicBool>,
}
impl StreamTx {
    pub fn new(
        id: u32,
        channel: u32,
        credit: Arc<Credit>,
        sink: Arc<dyn StreamSink>,
        closed: CancellationToken,
    ) -> Self {
        Self {
            id,
            channel,
            credit,
            sink,
            closed,
            sending: Arc::default(),
            finishing: Arc::default(),
        }
    }
    pub async fn write(&self, mut data: &[u8]) -> Result<(), StreamError> {
        let _guard = tokio::select! {
            biased;
            _ = self.closed.cancelled() => return Err(StreamError::Closed),
            guard = self.sending.lock() => guard,
        };
        while !data.is_empty() {
            let n = self
                .credit
                .take(data.len().min(MAX_STREAM_CHUNK) as u32, &self.closed)
                .await? as usize;
            let sent = tokio::select! {
                biased;
                _ = self.closed.cancelled() => return Err(StreamError::Closed),
                sent = self.sink.push(stream_frame(self.channel, self.id, 0, &data[..n])) => sent,
            };
            if !sent {
                self.closed.cancel();
                return Err(StreamError::Closed);
            }
            data = &data[n..];
        }
        Ok(())
    }
    pub async fn finish(&self) {
        if self.closed.is_cancelled() || self.finishing.swap(true, Ordering::AcqRel) {
            return;
        }
        // Interrupt a writer waiting for credit/queue space, then place FIN after
        // any frame it already enqueued. Channel generation cancellation remains separate.
        self.closed.cancel();
        let _guard = self.sending.lock().await;
        let _ = self
            .sink
            .push(stream_frame(self.channel, self.id, FLAG_FIN, &[]))
            .await;
    }
    pub fn closed(&self) -> CancellationToken {
        self.closed.clone()
    }
}
pub struct StreamEnds {
    pub id: u32,
    pub tx: StreamTx,
    pub input: mpsc::Receiver<Bytes>,
}
pub trait StreamOpener: Send + Sync {
    fn open(&self) -> Result<StreamEnds, StreamError>;
}
#[cfg(test)]
mod tests;
