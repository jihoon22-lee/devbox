//! Per-connection stream registry; globally unique IDs and channel-scoped ownership.
use super::outbox::{Outbox, Outgoing};
use bytes::Bytes;
use devbox_protocol::frame::Frame;
use devbox_protocol::stream::{
    Credit, STREAM_WINDOW, StreamEnds, StreamError, StreamOpener, StreamSink, StreamTx,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
static NEXT_STREAM: AtomicU32 = AtomicU32::new(1);
struct Entry {
    channel: u32,
    credit: Arc<Credit>,
    input: Option<mpsc::Sender<Bytes>>,
    generation: CancellationToken,
    tx: StreamTx,
}
#[derive(Clone, Default)]
pub(super) struct Streams(Arc<Mutex<HashMap<u32, Entry>>>);
impl Streams {
    pub fn opener(
        &self,
        channel: u32,
        generation: CancellationToken,
        outbox: Arc<Outbox>,
    ) -> Arc<dyn StreamOpener> {
        Arc::new(Opener {
            streams: self.clone(),
            channel,
            generation,
            outbox,
        })
    }
    pub fn input(&self, channel: u32, id: u32, fin: bool, data: Bytes) -> Result<(), &'static str> {
        if data.len() > devbox_protocol::frame::MAX_STREAM_CHUNK {
            return Err("stream input chunk too large");
        }
        let mut map = self.0.lock().expect("streams");
        let Some(entry) = map.get_mut(&id) else {
            return Ok(());
        };
        if entry.channel != channel {
            return Err("stream belongs to another channel");
        }
        if !data.is_empty() {
            let Some(input) = &entry.input else {
                return Err("input after FIN");
            };
            if input.try_send(data).is_err() {
                let tx = entry.tx.clone();
                map.remove(&id);
                tokio::spawn(async move {
                    tx.finish().await;
                });
                return Ok(());
            }
        }
        if fin {
            entry.input = None;
        }
        Ok(())
    }
    pub fn credit(&self, channel: u32, id: u32, bytes: u32) -> Result<(), &'static str> {
        if let Some(entry) = self.0.lock().expect("streams").get(&id) {
            if entry.channel != channel {
                return Err("stream belongs to another channel");
            }
            entry.credit.add(bytes);
        }
        Ok(())
    }
    /// Full close discards queued output and interrupts producers; input FIN is only a half-close.
    pub fn close(&self, channel: u32, id: u32, outbox: &Outbox) -> Result<(), &'static str> {
        let mut map = self.0.lock().expect("streams");
        if let Some(entry) = map.get(&id) {
            if entry.channel != channel {
                return Err("stream belongs to another channel");
            }
        }
        if let Some(entry) = map.remove(&id) {
            entry.generation.cancel();
        }
        drop(map);
        outbox.remove_stream(channel, id);
        Ok(())
    }
    pub fn close_channel(&self, channel: u32) {
        self.0.lock().expect("streams").retain(|_, entry| {
            if entry.channel == channel {
                entry.generation.cancel();
                false
            } else {
                true
            }
        });
    }
    pub fn close_all(&self) {
        for (_, entry) in self.0.lock().expect("streams").drain() {
            entry.generation.cancel();
        }
    }
}
struct ChannelSink {
    outbox: Arc<Outbox>,
    generation: CancellationToken,
}
impl StreamSink for ChannelSink {
    fn push(
        &self,
        frame: Frame,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + '_>> {
        Box::pin(self.outbox.send(Outgoing {
            frame,
            generation: self.generation.clone(),
            flushed: None,
        }))
    }
}
struct Opener {
    streams: Streams,
    channel: u32,
    generation: CancellationToken,
    outbox: Arc<Outbox>,
}
impl StreamOpener for Opener {
    fn owns(&self, id: u32) -> bool {
        !self.generation.is_cancelled()
            && self
                .streams
                .0
                .lock()
                .expect("streams")
                .get(&id)
                .is_some_and(|entry| {
                    entry.channel == self.channel && !entry.generation.is_cancelled()
                })
    }
    fn open(&self) -> Result<StreamEnds, StreamError> {
        let mut entries = self.streams.0.lock().expect("streams");
        if self.generation.is_cancelled() {
            return Err(StreamError::Closed);
        }
        let id = NEXT_STREAM
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| StreamError::Closed)?;
        let credit = Arc::new(Credit::new(STREAM_WINDOW));
        let generation = self.generation.child_token();
        let closed = generation.child_token();
        // At most 256 chunks of <=64 KiB, independent of the output credit window.
        let (input, receiver) = mpsc::channel(256);
        let sink = Arc::new(ChannelSink {
            outbox: self.outbox.clone(),
            generation: generation.clone(),
        });
        let tx = StreamTx::new(id, self.channel, credit.clone(), sink, closed.clone());
        entries.insert(
            id,
            Entry {
                channel: self.channel,
                credit,
                input: Some(input),
                generation,
                tx: tx.clone(),
            },
        );
        let registry = self.streams.clone();
        tokio::spawn(async move {
            closed.cancelled().await;
            registry.0.lock().expect("streams").remove(&id);
        });
        Ok(StreamEnds {
            id,
            tx,
            input: receiver,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn dropping_client_stream_releases_credit_stalled_server_producer() {
        use devbox_protocol::frame::{FrameCodec, FrameKind};
        use devbox_protocol::message::{ClientMessage, ServerMessage, json_frame, parse_json};
        use futures::{SinkExt, StreamExt};
        use tokio_util::codec::Framed;
        let temp = tempfile::tempdir().expect("fixture directory");
        let socket = temp.path().join("stream.sock");
        let listener = tokio::net::UnixListener::bind(&socket).expect("fixture socket");
        let registry = Streams::default();
        let queue = Arc::new(Outbox::default());
        let generation = CancellationToken::new();
        let ends = registry
            .opener(0, generation.clone(), queue.clone())
            .open()
            .expect("open stream");
        let closed = ends.tx.closed();
        let (input_seen, input_received) = tokio::sync::oneshot::channel();
        let server_registry = registry.clone();
        let server_queue = queue.clone();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.expect("accept");
            let (mut sink, mut source) = Framed::new(socket, FrameCodec::default()).split();
            let request = source.next().await.expect("request").expect("frame");
            let ClientMessage::Request { id, .. } = parse_json(&request).expect("request JSON")
            else {
                panic!("expected request")
            };
            sink.send(json_frame(
                0,
                &ServerMessage::Response {
                    id,
                    result: Some(
                        serde_json::value::to_raw_value(&devbox_protocol::StreamOpened {
                            stream_id: ends.id,
                        })
                        .expect("result"),
                    ),
                    error: None,
                    diagnostic_id: None,
                },
            ))
            .await
            .expect("response");
            let writer = tokio::spawn(async move {
                while let Some(frame) = queue.next().await {
                    if sink.send(frame.frame).await.is_err() {
                        break;
                    }
                }
            });
            let producer = tokio::spawn(async move {
                let _input = ends.input;
                ends.tx.write(&vec![b'x'; STREAM_WINDOW as usize + 1]).await
            });
            let mut input_seen = Some(input_seen);
            while let Some(Ok(frame)) = source.next().await {
                if frame.kind == FrameKind::Stream {
                    let (id, flags, _) =
                        devbox_protocol::stream::parse_stream(&frame.body).expect("stream body");
                    server_registry
                        .input(
                            0,
                            id,
                            flags & devbox_protocol::stream::FLAG_FIN != 0,
                            frame.body.slice(5..),
                        )
                        .expect("input");
                    if let Some(seen) = input_seen.take() {
                        let _ = seen.send(());
                    }
                } else if frame.kind == FrameKind::Control {
                    let control: devbox_protocol::message::Control =
                        parse_json(&frame).expect("control JSON");
                    match control {
                        devbox_protocol::message::Control::StreamClose { stream } => {
                            server_registry
                                .close(frame.channel, stream, &server_queue)
                                .expect("close owned stream");
                            if let Some(seen) = input_seen.take() {
                                let _ = seen.send(());
                            }
                        }
                        _ => panic!("unexpected control"),
                    }
                } else {
                    break;
                }
            }
            generation.cancel();
            server_registry.close_all();
            producer
                .await
                .expect("producer")
                .expect_err("closed producer");
            writer.abort();
        });
        let client = crate::client::Client::connect_without_hello(&socket)
            .await
            .expect("client");
        let stream = client
            .open_stream(
                "debug.echo",
                serde_json::json!({"bytes": STREAM_WINDOW + 1}),
            )
            .await
            .expect("stream");
        assert_eq!(
            stream
                .read_available(std::time::Duration::from_millis(100))
                .await
                .len(),
            STREAM_WINDOW as usize
        );
        drop(stream);
        let _ = tokio::time::timeout(std::time::Duration::from_millis(100), input_received).await;
        let released =
            tokio::time::timeout(std::time::Duration::from_millis(100), closed.cancelled())
                .await
                .is_ok();
        drop(client);
        tokio::time::timeout(std::time::Duration::from_secs(2), server)
            .await
            .expect("server cleanup")
            .expect("server");
        assert!(
            released,
            "dropping a stream must release its stalled producer while Client remains alive"
        );
    }
    #[tokio::test]
    async fn stream_ownership_and_channel_generation_are_enforced() {
        let registry = Streams::default();
        let q = Arc::new(Outbox::default());
        let generation = CancellationToken::new();
        let opener = registry.opener(1, generation.clone(), q.clone());
        let mut stream = opener.open().expect("fixture operation succeeds");
        assert!(opener.owns(stream.id));
        assert!(
            registry
                .opener(1, generation.clone(), q.clone())
                .owns(stream.id)
        );
        assert!(
            !registry
                .opener(2, generation.clone(), q.clone())
                .owns(stream.id)
        );
        assert!(
            !Streams::default()
                .opener(1, generation.clone(), q.clone())
                .owns(stream.id)
        );
        assert!(
            registry
                .input(2, stream.id, false, Bytes::from_static(b"bad"))
                .is_err()
        );
        assert!(registry.credit(2, stream.id, 1).is_err());
        assert!(registry.close(2, stream.id, &q).is_err());
        registry
            .input(1, stream.id, false, Bytes::from_static(b"ok"))
            .expect("fixture operation succeeds");
        assert_eq!(
            stream
                .input
                .recv()
                .await
                .expect("fixture operation succeeds"),
            &b"ok"[..]
        );
        generation.cancel();
        registry.close_channel(1);
        assert!(opener.open().is_err());
        assert!(!opener.owns(stream.id));
        assert_eq!(stream.tx.write(b"late").await, Err(StreamError::Closed));
        assert!(stream.input.recv().await.is_none());
        let new = registry
            .opener(1, CancellationToken::new(), q)
            .open()
            .expect("fixture operation succeeds");
        assert_ne!(new.id, stream.id);
        registry.close_all();
    }
    #[tokio::test]
    async fn fin_does_not_cancel_already_queued_output() {
        let registry = Streams::default();
        let q = Arc::new(Outbox::default());
        let stream = registry
            .opener(1, CancellationToken::new(), q.clone())
            .open()
            .expect("fixture operation succeeds");
        stream
            .tx
            .write(b"last")
            .await
            .expect("fixture operation succeeds");
        stream.tx.finish().await;
        let data = q.next().await.expect("fixture operation succeeds");
        assert_eq!(
            devbox_protocol::stream::parse_stream(&data.frame.body)
                .expect("fixture operation succeeds")
                .2,
            b"last"
        );
        let fin = q.next().await.expect("fixture operation succeeds");
        assert_eq!(
            devbox_protocol::stream::parse_stream(&fin.frame.body)
                .expect("fixture operation succeeds")
                .1,
            devbox_protocol::stream::FLAG_FIN
        );
    }
    #[tokio::test]
    async fn input_overflow_sends_fin_without_closing_other_streams() {
        let registry = Streams::default();
        let q = Arc::new(Outbox::default());
        let opener = registry.opener(1, CancellationToken::new(), q.clone());
        let stream = opener.open().expect("fixture operation succeeds");
        for _ in 0..257 {
            registry
                .input(1, stream.id, false, Bytes::from_static(b"x"))
                .expect("fixture operation succeeds");
        }
        let fin = tokio::time::timeout(std::time::Duration::from_secs(2), q.next())
            .await
            .expect("fixture operation succeeds")
            .expect("fixture operation succeeds");
        assert_eq!(
            devbox_protocol::stream::parse_stream(&fin.frame.body)
                .expect("fixture operation succeeds"),
            (stream.id, devbox_protocol::stream::FLAG_FIN, &[][..])
        );
        let other = opener.open().expect("fixture operation succeeds");
        other
            .tx
            .write(b"alive")
            .await
            .expect("fixture operation succeeds");
        assert_eq!(
            devbox_protocol::stream::parse_stream(
                &q.next()
                    .await
                    .expect("fixture operation succeeds")
                    .frame
                    .body
            )
            .expect("fixture operation succeeds")
            .2,
            b"alive"
        );
        registry.close_all();
    }
    #[tokio::test]
    async fn finishing_interrupts_backpressured_writer_and_preserves_fin_order() {
        let registry = Streams::default();
        let q = Arc::new(Outbox::default());
        let stream = registry
            .opener(1, CancellationToken::new(), q.clone())
            .open()
            .expect("fixture operation succeeds");
        for _ in 0..240 {
            stream
                .tx
                .write(b"x")
                .await
                .expect("fixture operation succeeds");
        }
        let tx = stream.tx.clone();
        let writer = tokio::spawn(async move { tx.write(b"blocked").await });
        tokio::task::yield_now().await;
        assert!(!writer.is_finished());
        let tx = stream.tx.clone();
        let finish = tokio::spawn(async move { tx.finish().await });
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(2), writer)
                .await
                .expect("fixture operation succeeds")
                .expect("fixture operation succeeds"),
            Err(StreamError::Closed)
        );
        for _ in 0..240 {
            assert_eq!(
                devbox_protocol::stream::parse_stream(
                    &q.next()
                        .await
                        .expect("fixture operation succeeds")
                        .frame
                        .body
                )
                .expect("fixture operation succeeds")
                .1,
                0
            );
        }
        finish.await.expect("fixture operation succeeds");
        assert_eq!(
            devbox_protocol::stream::parse_stream(
                &q.next()
                    .await
                    .expect("fixture operation succeeds")
                    .frame
                    .body
            )
            .expect("fixture operation succeeds")
            .1,
            devbox_protocol::stream::FLAG_FIN
        );
    }
    #[tokio::test]
    async fn full_close_removes_only_owned_stream_output_and_wakes_producer() {
        let registry = Streams::default();
        let q = Arc::new(Outbox::default());
        let opener = registry.opener(1, CancellationToken::new(), q.clone());
        let mut stream = opener.open().expect("open stream");
        let other = opener.open().expect("open sibling stream");
        stream.tx.write(b"discard").await.expect("queue data");
        other.tx.write(b"keep").await.expect("queue sibling data");
        registry
            .close(1, stream.id, &q)
            .expect("close owned stream");
        assert!(stream.tx.closed().is_cancelled());
        assert!(stream.input.recv().await.is_none());
        assert!(
            !registry
                .0
                .lock()
                .expect("registry")
                .contains_key(&stream.id)
        );
        let frame = q.next().await.expect("sibling output remains");
        assert_eq!(
            devbox_protocol::stream::parse_stream(&frame.frame.body).expect("stream payload"),
            (other.id, 0, &b"keep"[..])
        );
        assert!(!other.tx.closed().is_cancelled());
        registry
            .close(1, stream.id, &q)
            .expect("duplicate close is harmless");
        registry.close_all();
    }
}
