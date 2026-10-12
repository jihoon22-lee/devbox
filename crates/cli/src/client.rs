//! Typed daemon client; one channel (0) with bounded subscriptions.
mod queue;
use devbox_protocol::Method;
use devbox_protocol::frame::{Frame, FrameCodec, FrameKind};
use devbox_protocol::message::{
    ClientKind, ClientMessage, Control, ServerMessage, control_frame, json_frame, parse_json,
};
use futures::{SinkExt, StreamExt};
use queue::Queue;
use serde_json::value::RawValue;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, Weak};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, oneshot};
use tokio_util::{codec::Framed, sync::CancellationToken};

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("rpc error {code}")]
    Rpc {
        code: String,
        detail: Option<Box<RawValue>>,
        diagnostic_id: Option<String>,
    },
    #[error("connection lost")]
    ConnectionLost,
    #[error("protocol violation: {0}")]
    Protocol(&'static str),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("decode: {0}")]
    Decode(#[from] serde_json::Error),
}
#[derive(Debug)]
pub struct SubEvent {
    pub rev: String,
    pub payload: String,
}
pub struct Subscription {
    queue: Arc<Queue>,
    id: u32,
    state: Weak<Mutex<State>>,
    out: Weak<mpsc::Sender<Outgoing>>,
    shutdown: CancellationToken,
}
impl Subscription {
    /// Empty rev means Resync: reread domain state before consuming new events.
    pub async fn next_event(&mut self) -> Option<SubEvent> {
        self.queue.recv().await
    }
}
impl Drop for Subscription {
    fn drop(&mut self) {
        self.queue.close();
        if let Some(state) = self.state.upgrade() {
            state.lock().expect("client").subscriptions.remove(&self.id);
        }
        let Some(out) = self.out.upgrade() else {
            return;
        };
        let outgoing = Outgoing {
            frame: json_frame(0, &ClientMessage::Unsubscribe { sub_id: self.id }),
            sent: oneshot::channel().0,
        };
        if out.try_send(outgoing).is_err() {
            if let Some(state) = self.state.upgrade() {
                state.lock().expect("client").close();
            }
            self.shutdown.cancel();
        }
    }
}
struct Pending {
    reply: oneshot::Sender<ServerMessage>,
    subscription: Option<Arc<Queue>>,
}
#[derive(Default)]
struct State {
    closed: bool,
    pending: HashMap<u32, Pending>,
    subscriptions: HashMap<u32, Arc<Queue>>,
}
impl State {
    fn close(&mut self) {
        self.closed = true;
        self.pending.clear();
        for queue in self.subscriptions.values() {
            queue.close();
        }
        self.subscriptions.clear();
    }
    fn receive(&mut self, message: ServerMessage) -> Result<(), ClientError> {
        match message {
            ServerMessage::Event {
                sub_id,
                rev,
                payload,
            } => {
                if let Some(queue) = self.subscriptions.get(&sub_id) {
                    queue.push(SubEvent {
                        rev,
                        payload: payload.get().into(),
                    });
                }
            }
            ServerMessage::Resync { sub_id } => {
                if let Some(queue) = self.subscriptions.get(&sub_id) {
                    queue.push(SubEvent {
                        rev: String::new(),
                        payload: String::new(),
                    });
                }
            }
            message => {
                let id = match &message {
                    ServerMessage::Welcome { .. } => 0,
                    ServerMessage::Response {
                        id, result, error, ..
                    } => {
                        if result.is_some() == error.is_some() {
                            return Err(ClientError::Protocol(
                                "response must have one result or error",
                            ));
                        }
                        *id
                    }
                    ServerMessage::Subscribed { id, .. } => *id,
                    _ => unreachable!(),
                };
                if let Some(pending) = self.pending.remove(&id) {
                    if let ServerMessage::Subscribed { sub_id, .. } = &message {
                        let queue = pending
                            .subscription
                            .ok_or(ClientError::Protocol("unexpected subscribed"))?;
                        if self.subscriptions.insert(*sub_id, queue).is_some() {
                            return Err(ClientError::Protocol("duplicate subscription id"));
                        }
                    }
                    // Register the queue before waking the subscribe caller; the next
                    // frame may already contain its first event.
                    let _ = pending.reply.send(message);
                } else if id == 0 {
                    return Err(ClientError::Protocol("unexpected welcome"));
                }
            }
        }
        Ok(())
    }
}
struct Outgoing {
    frame: Frame,
    sent: oneshot::Sender<()>,
}
pub struct Client {
    out: Arc<mpsc::Sender<Outgoing>>,
    state: Arc<Mutex<State>>,
    next: AtomicU32,
    shutdown: CancellationToken,
}
impl Drop for Client {
    fn drop(&mut self) {
        self.state.lock().expect("client").close();
        self.shutdown.cancel();
    }
}
struct PendingGuard {
    state: Arc<Mutex<State>>,
    id: u32,
    subscription: bool,
    completed: bool,
    shutdown: CancellationToken,
}
impl Drop for PendingGuard {
    fn drop(&mut self) {
        let mut state = self.state.lock().expect("client");
        state.pending.remove(&self.id);
        // No sub id is known until the ack; closing the transport also covers an
        // ack already consumed by the reader when this future is cancelled.
        if self.subscription && !self.completed {
            state.close();
            self.shutdown.cancel();
        }
    }
}
impl Client {
    pub async fn connect(
        socket: &Path,
        kind: ClientKind,
        instance: &str,
    ) -> Result<Self, ClientError> {
        let client = Self::connect_without_hello(socket).await?;
        let hello = ClientMessage::Hello {
            version: crate::VERSION.into(),
            protocol: devbox_protocol::PROTOCOL_VERSION,
            client: kind,
            instance: instance.into(),
        };
        let reply = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.send_and_wait(0, hello, None),
        )
        .await
        .map_err(|_| ClientError::Protocol("hello timed out"))??;
        match reply {
            ServerMessage::Welcome {
                version,
                protocol,
                daemon_id,
            } if version == crate::VERSION
                && protocol == devbox_protocol::PROTOCOL_VERSION
                && !daemon_id.is_empty() =>
            {
                Ok(client)
            }
            _ => Err(ClientError::Protocol("welcome mismatch")),
        }
    }
    pub async fn connect_without_hello(socket: &Path) -> Result<Self, ClientError> {
        let stream = UnixStream::connect(socket).await?;
        let (mut sink, mut source) = Framed::new(stream, FrameCodec::default()).split();
        let (out, mut rx) = mpsc::channel::<Outgoing>(256);
        let state = Arc::new(Mutex::new(State::default()));
        let shutdown = CancellationToken::new();
        let (writer_state, writer_shutdown) = (state.clone(), shutdown.clone());
        tokio::spawn(async move {
            loop {
                let outgoing = tokio::select! { _ = writer_shutdown.cancelled() => break, outgoing = rx.recv() => match outgoing { Some(frame) => frame, None => break } };
                let result = tokio::select! { _ = writer_shutdown.cancelled() => break, result = sink.send(outgoing.frame) => result };
                if result.is_err() {
                    break;
                }
                let _ = outgoing.sent.send(());
            }
            writer_state.lock().expect("client").close();
            writer_shutdown.cancel();
        });
        let (reader_state, reader_shutdown) = (state.clone(), shutdown.clone());
        tokio::spawn(async move {
            loop {
                let frame = tokio::select! { _ = reader_shutdown.cancelled() => break, frame = source.next() => match frame { Some(Ok(frame)) => frame, _ => break } };
                if frame.kind != FrameKind::Json || frame.channel != 0 {
                    break;
                }
                let Ok(message) = parse_json::<ServerMessage>(&frame) else {
                    break;
                };
                let mut state = reader_state.lock().expect("client");
                if state.closed || state.receive(message).is_err() {
                    break;
                }
            }
            reader_state.lock().expect("client").close();
            reader_shutdown.cancel();
        });
        Ok(Self {
            out: Arc::new(out),
            state,
            shutdown,
            next: AtomicU32::new(1),
        })
    }
    fn request_id(&self) -> Result<u32, ClientError> {
        self.next
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| ClientError::Protocol("request ids exhausted"))
    }
    async fn send_frame(&self, frame: Frame) -> Result<(), ClientError> {
        let (sent, receipt) = oneshot::channel();
        tokio::select! {
            _ = self.shutdown.cancelled() => return Err(ClientError::ConnectionLost),
            result = self.out.send(Outgoing { frame, sent }) => result.map_err(|_| ClientError::ConnectionLost)?,
        }
        tokio::select! { _ = self.shutdown.cancelled() => Err(ClientError::ConnectionLost), result = receipt => result.map_err(|_| ClientError::ConnectionLost) }
    }
    async fn send_and_wait(
        &self,
        id: u32,
        msg: ClientMessage,
        subscription: Option<Arc<Queue>>,
    ) -> Result<ServerMessage, ClientError> {
        let is_subscription = subscription.is_some();
        let (reply, receiver) = oneshot::channel();
        {
            let mut state = self.state.lock().expect("client");
            if state.closed {
                return Err(ClientError::ConnectionLost);
            }
            state.pending.insert(
                id,
                Pending {
                    reply,
                    subscription,
                },
            );
        }
        let mut guard = PendingGuard {
            state: self.state.clone(),
            id,
            subscription: is_subscription,
            completed: false,
            shutdown: self.shutdown.clone(),
        };
        self.send_frame(json_frame(0, &msg)).await?;
        let reply = receiver.await.map_err(|_| ClientError::ConnectionLost)?;
        guard.completed = true;
        Ok(reply)
    }
    pub async fn raw_call<P: serde::Serialize>(
        &self,
        method: &str,
        params: P,
        client_request_id: Option<String>,
    ) -> Result<String, ClientError> {
        let id = self.request_id()?;
        let reply = self
            .send_and_wait(
                id,
                ClientMessage::Request {
                    id,
                    method: method.into(),
                    params: serde_json::value::to_raw_value(&params)?,
                    client_request_id,
                },
                None,
            )
            .await?;
        match reply {
            ServerMessage::Response {
                result: Some(result),
                error: None,
                ..
            } => Ok(result.get().into()),
            ServerMessage::Response {
                error: Some(error),
                diagnostic_id,
                ..
            } => Err(ClientError::Rpc {
                code: error.code,
                detail: error.detail,
                diagnostic_id,
            }),
            _ => Err(ClientError::Protocol("unexpected request reply")),
        }
    }
    pub async fn call<M: Method>(&self, params: &M::Params) -> Result<M::Output, ClientError>
    where
        M::Output: serde::de::DeserializeOwned,
        M::Params: serde::Serialize,
    {
        Ok(serde_json::from_str(
            &self.raw_call(M::NAME, params, None).await?,
        )?)
    }
    pub async fn subscribe(&self, topic: &str) -> Result<Subscription, ClientError> {
        let id = self.request_id()?;
        let queue = Arc::new(Queue::default());
        match self
            .send_and_wait(
                id,
                ClientMessage::Subscribe {
                    id,
                    topic: topic.into(),
                    params: None,
                },
                Some(queue.clone()),
            )
            .await?
        {
            ServerMessage::Subscribed { sub_id, .. } => Ok(Subscription {
                queue,
                id: sub_id,
                state: Arc::downgrade(&self.state),
                out: Arc::downgrade(&self.out),
                shutdown: self.shutdown.clone(),
            }),
            ServerMessage::Response {
                error: Some(error),
                diagnostic_id,
                ..
            } => Err(ClientError::Rpc {
                code: error.code,
                detail: error.detail,
                diagnostic_id,
            }),
            _ => Err(ClientError::Protocol("unexpected subscribe reply")),
        }
    }
    pub async fn close_channel(&self) -> Result<(), ClientError> {
        let result = self
            .send_frame(control_frame(0, &Control::ChannelClose))
            .await;
        self.state.lock().expect("client").close();
        self.shutdown.cancel();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::{Future, poll_fn};

    #[tokio::test]
    async fn cancelled_subscription_after_reader_ack_closes_transport_and_queue() {
        let (out, mut outbound) = mpsc::channel(256);
        let state = Arc::new(Mutex::new(State::default()));
        let shutdown = CancellationToken::new();
        let client = Client {
            out: Arc::new(out),
            state: state.clone(),
            next: AtomicU32::new(1),
            shutdown: shutdown.clone(),
        };
        let mut subscribe = Box::pin(client.subscribe("test.events"));
        poll_fn(|cx| {
            assert!(subscribe.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        // Writer ack; reader processes subscription ack before the subscriber is
        // polled again. Dropping the future must clean the already active queue.
        outbound.recv().await.unwrap().sent.send(()).unwrap();
        state
            .lock()
            .unwrap()
            .receive(ServerMessage::Subscribed {
                id: 1,
                sub_id: 2,
                rev: "0".into(),
            })
            .unwrap();
        let queue = state.lock().unwrap().subscriptions.get(&2).unwrap().clone();
        drop(subscribe);
        assert!(shutdown.is_cancelled());
        assert!(state.lock().unwrap().subscriptions.is_empty());
        assert!(queue.recv().await.is_none());
    }

    #[tokio::test]
    async fn subscription_drop_with_full_outbox_fail_closes_all_state() {
        let (out, _receiver) = mpsc::channel(1);
        let out = Arc::new(out);
        let state = Arc::new(Mutex::new(State::default()));
        let queue = Arc::new(Queue::default());
        state.lock().unwrap().subscriptions.insert(2, queue.clone());
        let shutdown = CancellationToken::new();
        out.try_send(Outgoing {
            frame: control_frame(0, &Control::ChannelOpen),
            sent: oneshot::channel().0,
        })
        .unwrap();
        let sub = Subscription {
            queue: queue.clone(),
            id: 2,
            state: Arc::downgrade(&state),
            out: Arc::downgrade(&out),
            shutdown: shutdown.clone(),
        };
        drop(sub);
        assert!(shutdown.is_cancelled());
        assert!(state.lock().unwrap().closed);
        assert!(state.lock().unwrap().subscriptions.is_empty());
        assert!(queue.recv().await.is_none());
    }
}
