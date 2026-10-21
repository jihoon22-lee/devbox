use super::*;
use bytes::{Bytes, BytesMut};
use devbox_protocol::stream::{FLAG_FIN, STREAM_WINDOW, parse_stream, stream_frame};
use std::time::Duration;
use tokio::sync::Notify;
#[derive(Default)]
struct Buffer {
    bytes: BytesMut,
    ended: bool,
}
#[derive(Default)]
pub(super) struct StreamQueue {
    buffer: Mutex<Buffer>,
    notify: Notify,
}
impl StreamQueue {
    fn push(&self, bytes: Bytes, fin: bool) -> Result<(), ClientError> {
        let mut buffer = self.buffer.lock().expect("client stream");
        if buffer.ended {
            return Err(ClientError::Protocol("stream data after FIN"));
        }
        if buffer.bytes.len() + bytes.len() > STREAM_WINDOW as usize {
            return Err(ClientError::Protocol("stream receive window exceeded"));
        }
        if !bytes.is_empty() {
            buffer.bytes.extend_from_slice(&bytes);
        }
        buffer.ended = fin;
        self.notify.notify_waiters();
        Ok(())
    }
    pub(super) fn close(&self) {
        self.buffer.lock().expect("client stream").ended = true;
        self.notify.notify_waiters();
    }
    async fn read(&self) -> Option<Bytes> {
        loop {
            let ready = self.notify.notified();
            tokio::pin!(ready);
            ready.as_mut().enable();
            {
                let mut buffer = self.buffer.lock().expect("client stream");
                if !buffer.bytes.is_empty() {
                    return Some(buffer.bytes.split().freeze());
                }
                if buffer.ended {
                    return None;
                }
            }
            ready.await;
        }
    }
}
impl State {
    fn remove_stream(&mut self, id: u32) {
        self.streams.remove(&id);
        self.active_streams.remove(&id);
    }
    pub(super) fn receive_stream(&mut self, frame: Frame) -> Result<(), ClientError> {
        let (id, flags, _) =
            parse_stream(&frame.body).ok_or(ClientError::Protocol("malformed stream"))?;
        if !self.streams.contains_key(&id) && self.opening_streams == 0 {
            return Ok(());
        }
        if !self.streams.contains_key(&id) && self.streams.len() >= 256 {
            return Err(ClientError::Protocol("too many streams"));
        }
        self.streams
            .entry(id)
            .or_default()
            .push(frame.body.slice(5..), flags & FLAG_FIN != 0)
    }
}
pub struct ClientStream {
    pub id: u32,
    queue: Arc<StreamQueue>,
    state: Weak<Mutex<State>>,
    out: Weak<mpsc::Sender<Outgoing>>,
    shutdown: CancellationToken,
}
impl ClientStream {
    pub(crate) async fn read_chunk(&self) -> Result<Option<Bytes>, ClientError> {
        let chunk = self.queue.read().await;
        if let Some(chunk) = &chunk {
            self.credit(chunk.len() as u32).await?;
        }
        Ok(chunk)
    }
    /// Internal run helper only. A length prefix rejects disconnect/truncation before child spawn.
    pub(crate) async fn read_secret_payload(
        &self,
    ) -> Result<zeroize::Zeroizing<Vec<u8>>, ClientError> {
        let mut data = zeroize::Zeroizing::new(Vec::new());
        while let Some(chunk) = self.queue.read().await {
            if data.len() + chunk.len() > 1024 * 1024 + 4 {
                return Err(ClientError::Protocol("secret payload limit"));
            }
            data.extend_from_slice(&chunk);
            self.credit(chunk.len() as u32).await?;
        }
        if data.len() < 4 {
            return Err(ClientError::ConnectionLost);
        }
        let size = u32::from_le_bytes(data[..4].try_into().expect("length prefix")) as usize;
        if size != data.len() - 4 {
            return Err(ClientError::ConnectionLost);
        }
        Ok(zeroize::Zeroizing::new(data[4..].to_vec()))
    }

    async fn send(&self, frame: Frame) -> Result<(), ClientError> {
        let out = self.out.upgrade().ok_or(ClientError::ConnectionLost)?;
        let (sent, receipt) = oneshot::channel();
        tokio::select! { _ = self.shutdown.cancelled() => return Err(ClientError::ConnectionLost), result = out.send(Outgoing { frame, sent }) => result.map_err(|_| ClientError::ConnectionLost)? }
        tokio::select! { _ = self.shutdown.cancelled() => Err(ClientError::ConnectionLost), result = receipt => result.map_err(|_| ClientError::ConnectionLost) }
    }
    pub async fn credit(&self, bytes: u32) -> Result<(), ClientError> {
        self.send(control_frame(
            0,
            &Control::StreamCredit {
                stream: self.id,
                bytes,
            },
        ))
        .await
    }
    pub async fn write(&self, bytes: &[u8]) -> Result<(), ClientError> {
        for chunk in bytes.chunks(devbox_protocol::frame::MAX_STREAM_CHUNK) {
            self.send(stream_frame(0, self.id, 0, chunk)).await?;
        }
        Ok(())
    }
    /// Stop both directions. Unlike finish_input, this releases a credit-stalled producer.
    pub async fn close(&self) -> Result<(), ClientError> {
        let result = self
            .send(control_frame(0, &Control::StreamClose { stream: self.id }))
            .await;
        self.queue.close();
        if let Some(state) = self.state.upgrade() {
            state.lock().expect("client").remove_stream(self.id);
        }
        result
    }
    pub async fn end(&self) -> Result<(), ClientError> {
        self.close().await
    }
    pub async fn read_until(&self, pattern: &[u8], timeout: Duration) -> Vec<u8> {
        let deadline = tokio::time::Instant::now() + timeout;
        let mut bytes = Vec::new();
        if pattern.is_empty() {
            return bytes;
        }
        while let Ok(Some(chunk)) = tokio::time::timeout_at(deadline, self.queue.read()).await {
            bytes.extend_from_slice(&chunk);
            if bytes.windows(pattern.len()).any(|window| window == pattern) {
                break;
            }
        }
        bytes
    }
    pub async fn finish_input(&self) -> Result<(), ClientError> {
        self.send(stream_frame(0, self.id, FLAG_FIN, &[])).await
    }
    pub async fn read_available(&self, timeout: Duration) -> Vec<u8> {
        let deadline = tokio::time::Instant::now() + timeout;
        let mut bytes = Vec::new();
        while let Ok(Some(chunk)) = tokio::time::timeout_at(deadline, self.queue.read()).await {
            bytes.extend_from_slice(&chunk);
        }
        bytes
    }
    pub async fn wait_end(&self, timeout: Duration) -> bool {
        tokio::time::timeout(timeout, async {
            while self.queue.read().await.is_some() {}
        })
        .await
        .is_ok()
    }
}
impl Drop for ClientStream {
    fn drop(&mut self) {
        self.queue.close();
        if let Some(state) = self.state.upgrade() {
            state.lock().expect("client").remove_stream(self.id);
        }
        if let Some(out) = self.out.upgrade()
            && out
                .try_send(Outgoing {
                    frame: control_frame(0, &Control::StreamClose { stream: self.id }),
                    sent: oneshot::channel().0,
                })
                .is_err()
        {
            self.shutdown.cancel();
        }
    }
}
struct Opening<'a> {
    client: &'a Client,
    completed: bool,
}
impl Drop for Opening<'_> {
    fn drop(&mut self) {
        let mut state = self.client.state.lock().expect("client");
        state.opening_streams -= 1;
        if state.opening_streams == 0 {
            let State {
                streams,
                active_streams,
                ..
            } = &mut *state;
            streams.retain(|id, _| active_streams.contains(id));
        }
        if !self.completed {
            state.close();
            self.client.shutdown.cancel();
        }
    }
}
impl Client {
    pub async fn open_stream<P: serde::Serialize>(
        &self,
        method: &str,
        params: P,
    ) -> Result<ClientStream, ClientError> {
        self.state.lock().expect("client").opening_streams += 1;
        let mut opening = Opening {
            client: self,
            completed: false,
        };
        let opened: devbox_protocol::StreamOpened =
            serde_json::from_str(&self.raw_call(method, params, None).await?)?;
        let queue = {
            let mut state = self.state.lock().expect("client");
            if state.closed {
                return Err(ClientError::ConnectionLost);
            }
            if !state.active_streams.insert(opened.stream_id) {
                return Err(ClientError::Protocol("duplicate stream id"));
            }
            state.streams.entry(opened.stream_id).or_default().clone()
        };
        opening.completed = true;
        Ok(ClientStream {
            id: opened.stream_id,
            queue,
            state: Arc::downgrade(&self.state),
            out: Arc::downgrade(&self.out),
            shutdown: self.shutdown.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::{Future, poll_fn};
    fn fixture() -> (Client, mpsc::Receiver<Outgoing>) {
        let (out, rx) = mpsc::channel(8);
        (
            Client {
                out: Arc::new(out),
                state: Arc::default(),
                next: AtomicU32::new(1),
                shutdown: CancellationToken::new(),
            },
            rx,
        )
    }
    async fn ack_open(client: &Client, rx: &mut mpsc::Receiver<Outgoing>) {
        rx.recv()
            .await
            .expect("fixture operation succeeds")
            .sent
            .send(())
            .expect("fixture operation succeeds");
        client
            .state
            .lock()
            .expect("fixture operation succeeds")
            .receive_stream(stream_frame(0, 9, 0, b"early"))
            .expect("fixture operation succeeds");
        client
            .state
            .lock()
            .expect("fixture operation succeeds")
            .receive(ServerMessage::Response {
                id: 1,
                result: Some(
                    RawValue::from_string("{\"streamId\":9}".into())
                        .expect("fixture operation succeeds"),
                ),
                error: None,
                diagnostic_id: None,
            })
            .expect("fixture operation succeeds");
    }
    #[tokio::test]
    async fn cancelled_open_after_reply_cleans_unclaimed_stream_and_transport() {
        let (client, mut rx) = fixture();
        let mut opening =
            Box::pin(client.open_stream("debug.echo", serde_json::json!({"bytes":0})));
        poll_fn(|cx| {
            assert!(opening.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        ack_open(&client, &mut rx).await;
        assert_eq!(
            client
                .state
                .lock()
                .expect("fixture operation succeeds")
                .streams
                .len(),
            1
        );
        drop(opening);
        assert!(client.shutdown.is_cancelled());
        assert!(
            client
                .state
                .lock()
                .expect("fixture operation succeeds")
                .streams
                .is_empty()
        );
        assert!(
            client
                .state
                .lock()
                .expect("fixture operation succeeds")
                .pending
                .is_empty()
        );
    }
    #[tokio::test]
    async fn bytes_before_open_reply_are_retained_and_fin_ends_reader() {
        let (client, mut rx) = fixture();
        let mut opening =
            Box::pin(client.open_stream("debug.echo", serde_json::json!({"bytes":0})));
        poll_fn(|cx| {
            assert!(opening.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        ack_open(&client, &mut rx).await;
        let stream = opening.await.expect("fixture operation succeeds");
        client
            .state
            .lock()
            .expect("fixture operation succeeds")
            .receive_stream(stream_frame(0, 9, FLAG_FIN, &[]))
            .expect("fixture operation succeeds");
        assert_eq!(
            stream.read_available(Duration::from_secs(1)).await,
            b"early"
        );
        assert!(stream.wait_end(Duration::from_secs(1)).await);
    }
    #[tokio::test]
    async fn tiny_chunks_within_byte_window_and_fin_are_not_rejected() {
        let queue = StreamQueue::default();
        for _ in 0..300 {
            queue
                .push(Bytes::from_static(b"x"), false)
                .expect("valid byte window");
        }
        queue.push(Bytes::new(), true).expect("FIN within window");
        let mut n = 0;
        while let Some(bytes) = queue.read().await {
            n += bytes.len();
        }
        assert_eq!(n, 300);
    }
    #[tokio::test]
    async fn late_dropped_stream_bytes_are_pruned_after_another_open() {
        let (client, mut rx) = fixture();
        let mut first = Box::pin(client.open_stream("debug.echo", serde_json::json!({"bytes":0})));
        poll_fn(|cx| {
            assert!(first.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        ack_open(&client, &mut rx).await;
        let old = first.await.expect("first stream");
        let mut opening =
            Box::pin(client.open_stream("debug.echo", serde_json::json!({"bytes":0})));
        poll_fn(|cx| {
            assert!(opening.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        drop(old);
        client
            .state
            .lock()
            .expect("state")
            .receive_stream(stream_frame(0, 9, 0, b"late"))
            .expect("late frame");
        rx.recv()
            .await
            .expect("open request")
            .sent
            .send(())
            .expect("writer ACK");
        client
            .state
            .lock()
            .expect("state")
            .receive(ServerMessage::Response {
                id: 2,
                result: Some(RawValue::from_string("{\"streamId\":10}".into()).expect("result")),
                error: None,
                diagnostic_id: None,
            })
            .expect("open reply");
        let live = opening.await.expect("new stream");
        let state = client.state.lock().expect("state");
        assert_eq!(
            state.streams.len(),
            1,
            "unclaimed old-stream prebuffer must be released"
        );
        assert!(state.streams.contains_key(&live.id));
    }
}
