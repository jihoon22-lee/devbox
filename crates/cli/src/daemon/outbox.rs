//! Bounded channel-fair writer queue. Envelopes preserve channel cancellation and flush receipts.
use devbox_protocol::frame::{Frame, FrameKind};
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use tokio::sync::{Notify, oneshot};
use tokio_util::sync::CancellationToken;

pub(super) struct Outgoing {
    pub frame: Frame,
    pub generation: CancellationToken,
    pub flushed: Option<oneshot::Sender<()>>,
}
#[derive(Default)]
struct ChannelQueue {
    json: VecDeque<Outgoing>,
    stream: VecDeque<Outgoing>,
}
#[derive(Default)]
struct Queues {
    channels: HashMap<u32, ChannelQueue>,
    order: VecDeque<u32>,
    len: usize,
    stream_len: usize,
    closed: bool,
}
#[derive(Default)]
pub(super) struct Outbox {
    queue: Mutex<Queues>,
    ready: Notify,
    space: Notify,
}
const CAPACITY: usize = 256;
// Keep JSON admission available when stream producers fill their queue.
const STREAM_CAPACITY: usize = CAPACITY - 16;
impl Outbox {
    pub fn close(&self) {
        let mut q = self.queue.lock().expect("outbox");
        q.closed = true;
        q.channels.clear();
        q.order.clear();
        q.len = 0;
        q.stream_len = 0;
        self.ready.notify_waiters();
        self.space.notify_waiters();
    }
    fn enqueue(q: &mut Queues, outgoing: Outgoing) {
        let channel = outgoing.frame.channel;
        let entry = q.channels.entry(channel).or_insert_with(|| {
            q.order.push_back(channel);
            ChannelQueue::default()
        });
        if outgoing.frame.kind == FrameKind::Stream {
            entry.stream.push_back(outgoing);
            q.stream_len += 1;
        } else {
            entry.json.push_back(outgoing);
        }
        q.len += 1;
    }
    pub async fn send(&self, outgoing: Outgoing) -> bool {
        loop {
            let notified = self.space.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if outgoing.generation.is_cancelled() {
                return false;
            }
            {
                let mut q = self.queue.lock().expect("outbox");
                if q.closed {
                    return false;
                }
                if q.len < CAPACITY
                    && (outgoing.frame.kind != FrameKind::Stream || q.stream_len < STREAM_CAPACITY)
                {
                    Self::enqueue(&mut q, outgoing);
                    self.ready.notify_one();
                    return true;
                }
            }
            tokio::select! { _ = notified => {}, _ = outgoing.generation.cancelled() => return false }
        }
    }
    #[cfg(test)]
    fn try_send(&self, outgoing: Outgoing) -> bool {
        let mut q = self.queue.lock().expect("outbox");
        if q.closed || outgoing.generation.is_cancelled() || q.len >= CAPACITY {
            return false;
        }
        Self::enqueue(&mut q, outgoing);
        self.ready.notify_one();
        true
    }
    pub fn remove_channel(&self, channel: u32) {
        let mut q = self.queue.lock().expect("outbox");
        if let Some(entry) = q.channels.remove(&channel) {
            q.len -= entry.json.len() + entry.stream.len();
            q.stream_len -= entry.stream.len();
        }
        q.order.retain(|id| *id != channel);
        self.space.notify_waiters();
    }
    pub fn remove_stream(&self, channel: u32, stream: u32) {
        let mut q = self.queue.lock().expect("outbox");
        if let Some(entry) = q.channels.get_mut(&channel) {
            let before = entry.stream.len();
            entry.stream.retain(|outgoing| {
                devbox_protocol::stream::parse_stream(&outgoing.frame.body)
                    .is_none_or(|(id, _, _)| id != stream)
            });
            let removed = before - entry.stream.len();
            let empty = entry.stream.is_empty() && entry.json.is_empty();
            q.len -= removed;
            q.stream_len -= removed;
            if empty {
                q.channels.remove(&channel);
                q.order.retain(|id| *id != channel);
            }
            self.space.notify_waiters();
        }
    }
    pub async fn next(&self) -> Option<Outgoing> {
        loop {
            let notified = self.ready.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let mut q = self.queue.lock().expect("outbox");
                if q.closed {
                    return None;
                }
                while let Some(channel) = q.order.pop_front() {
                    let entry = q.channels.get_mut(&channel).expect("queued channel");
                    let outgoing = entry
                        .json
                        .pop_front()
                        .or_else(|| entry.stream.pop_front())
                        .expect("nonempty queue");
                    if entry.json.is_empty() && entry.stream.is_empty() {
                        q.channels.remove(&channel);
                    } else {
                        q.order.push_back(channel);
                    }
                    q.len -= 1;
                    if outgoing.frame.kind == FrameKind::Stream {
                        q.stream_len -= 1;
                    }
                    self.space.notify_waiters();
                    if !outgoing.generation.is_cancelled() {
                        return Some(outgoing);
                    }
                    // A discarded generation does not consume this channel's turn.
                    if q.order.back() == Some(&channel) {
                        q.order.pop_back();
                        q.order.push_front(channel);
                    }
                }
            }
            notified.await;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn frame(channel: u32, kind: FrameKind, generation: CancellationToken) -> Outgoing {
        Outgoing {
            frame: Frame {
                channel,
                kind,
                body: bytes::Bytes::new(),
            },
            generation,
            flushed: None,
        }
    }
    #[tokio::test]
    async fn round_robin_prioritizes_json_and_discards_old_generation() {
        let q = Outbox::default();
        let old = CancellationToken::new();
        let live = CancellationToken::new();
        assert!(q.try_send(frame(1, FrameKind::Stream, live.clone())));
        assert!(q.try_send(frame(1, FrameKind::Json, live.clone())));
        assert!(q.try_send(frame(2, FrameKind::Json, old.clone())));
        assert!(q.try_send(frame(2, FrameKind::Stream, live)));
        old.cancel();
        let first = q.next().await.expect("fixture operation succeeds");
        assert_eq!(
            (first.frame.channel, first.frame.kind),
            (1, FrameKind::Json)
        );
        assert_eq!(
            q.next()
                .await
                .expect("fixture operation succeeds")
                .frame
                .channel,
            2
        );
        assert_eq!(
            q.next()
                .await
                .expect("fixture operation succeeds")
                .frame
                .channel,
            1
        );
    }
    #[tokio::test]
    async fn full_queue_wakes_on_cancel_and_close() {
        let q = Outbox::default();
        let generation = CancellationToken::new();
        for _ in 0..CAPACITY {
            assert!(q.try_send(frame(0, FrameKind::Json, generation.clone())));
        }
        let cancel = CancellationToken::new();
        let blocked = q.send(frame(1, FrameKind::Json, cancel.clone()));
        tokio::pin!(blocked);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), &mut blocked)
                .await
                .is_err()
        );
        cancel.cancel();
        assert!(!blocked.await);
        let blocked = q.send(frame(1, FrameKind::Json, generation));
        tokio::pin!(blocked);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), &mut blocked)
                .await
                .is_err()
        );
        q.close();
        assert!(!blocked.await);
        assert!(q.next().await.is_none());
    }
    #[tokio::test]
    async fn small_stream_chunks_backpressure_without_blocking_json() {
        let q = std::sync::Arc::new(Outbox::default());
        let queue = q.clone();
        let generation = CancellationToken::new();
        let writer_generation = generation.clone();
        let writer = tokio::spawn(async move {
            for _ in 0..300 {
                assert!(
                    queue
                        .send(frame(1, FrameKind::Stream, writer_generation.clone()))
                        .await
                );
            }
        });
        tokio::task::yield_now().await;
        assert!(!writer.is_finished());
        assert!(
            tokio::time::timeout(
                std::time::Duration::from_millis(30),
                q.send(frame(1, FrameKind::Json, generation))
            )
            .await
            .expect("JSON must have space while stream is backpressured")
        );
        assert_eq!(
            q.next()
                .await
                .expect("fixture operation succeeds")
                .frame
                .kind,
            FrameKind::Json
        );
        for _ in 0..300 {
            assert_eq!(
                q.next()
                    .await
                    .expect("fixture operation succeeds")
                    .frame
                    .kind,
                FrameKind::Stream
            );
        }
        writer.await.expect("fixture operation succeeds");
    }
}
