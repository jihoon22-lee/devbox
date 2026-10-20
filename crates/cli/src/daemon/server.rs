use super::outbox::{Outbox, Outgoing};
use super::streams::Streams;
use super::{dedupe::Dedupe, routes, state::Services};
use devbox_core::HubMessage;
use devbox_protocol::frame::{FrameCodec, FrameKind};
use devbox_protocol::message::{ClientMessage, Control, ServerMessage, json_frame, parse_json};
use devbox_protocol::stream::{FLAG_FIN, parse_stream};
use devbox_protocol::{CommonError, Ctx, DispatchError, PROTOCOL_VERSION, Router};
use futures::{SinkExt, StreamExt};
use serde_json::value::RawValue;
use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::{Arc, Mutex};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::oneshot;
use tokio_util::codec::Framed;
use tokio_util::sync::CancellationToken;

struct Daemon {
    router: Router<Services>,
    services: Arc<Services>,
    topics: HashSet<&'static str>,
    dedupe: Dedupe,
}
struct Channel {
    hello: bool,
    generation: CancellationToken,
    inflight: Arc<Mutex<HashMap<u32, CancellationToken>>>,
    subscriptions: HashMap<u32, tokio::task::JoinHandle<()>>,
}
impl Default for Channel {
    fn default() -> Self {
        Self {
            hello: false,
            generation: CancellationToken::new(),
            inflight: Arc::default(),
            subscriptions: HashMap::new(),
        }
    }
}
impl Drop for Channel {
    fn drop(&mut self) {
        self.generation.cancel();
        for cancel in self.inflight.lock().expect("requests").values() {
            cancel.cancel();
        }
        for task in self.subscriptions.values() {
            task.abort();
        }
    }
}

pub async fn serve(
    listener: UnixListener,
    services: Arc<Services>,
    shutdown: CancellationToken,
) -> io::Result<()> {
    services
        .agents
        .initialize()
        .await
        .map_err(|error| io::Error::other(format!("initialize agents: {error:?}")))?;
    services.runs.initialize().await.map_err(|error| {
        io::Error::other(format!("initialize execution definitions: {error:?}"))
    })?;
    let daemon = Arc::new(Daemon {
        router: routes::router(services.clone()),
        topics: routes::declared().topics.into_iter().collect(),
        services,
        dedupe: Dedupe::default(),
    });
    let mut connections = tokio::task::JoinSet::new();
    let result = loop {
        tokio::select! {
            _ = shutdown.cancelled() => break Ok(()),
            Some(_) = connections.join_next(), if !connections.is_empty() => {},
            accepted = listener.accept() => {
                let (stream, _) = match accepted { Ok(value) => value, Err(error) => break Err(error) };
                let (daemon, shutdown) = (daemon.clone(), shutdown.clone());
                connections.spawn(async move {
                    if let Err(error) = connection(stream, daemon, shutdown).await { tracing::debug!(%error, "connection closed"); }
                });
            }
        }
    };
    shutdown.cancel();
    while connections.join_next().await.is_some() {}
    daemon.dedupe.tracker.close();
    daemon.dedupe.tracker.wait().await;
    daemon.services.agents.shutdown().await;
    daemon.services.runs.shutdown().await;
    daemon.services.observe.shutdown().await;
    result
}

fn invalid(reason: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}

async fn connection(
    stream: UnixStream,
    daemon: Arc<Daemon>,
    shutdown: CancellationToken,
) -> anyhow::Result<()> {
    let (mut sink, mut source) = Framed::new(stream, FrameCodec::default()).split();
    let tx = Arc::new(Outbox::default());
    let writer_queue = tx.clone();
    let streams = Streams::default();
    let lost = CancellationToken::new();
    let writer_lost = lost.clone();
    let writer = tokio::spawn(async move {
        while let Some(outgoing) = writer_queue.next().await {
            if outgoing.generation.is_cancelled() {
                continue;
            }
            if sink.send(outgoing.frame).await.is_err() {
                break;
            }
            if let Some(flushed) = outgoing.flushed {
                let _ = flushed.send(());
            }
        }
        writer_queue.close();
        writer_lost.cancel();
    });
    let mut channels: HashMap<u32, Channel> = HashMap::new();
    let mut next_sub = 1_u32;
    let result = async {
        loop {
            let frame = tokio::select! {
                _ = shutdown.cancelled() => break,
                _ = lost.cancelled() => break,
                frame = source.next() => match frame { Some(frame) => frame?, None => break },
            };
            let ch = frame.channel;
            match frame.kind {
                FrameKind::Control => match parse_json::<Control>(&frame)? {
                    Control::ChannelOpen => {
                        channels.remove(&ch);
                        streams.close_channel(ch);
                        tx.remove_channel(ch);
                        channels.insert(ch, Channel::default());
                    }
                    Control::ChannelClose => {
                        channels.remove(&ch);
                        streams.close_channel(ch);
                        tx.remove_channel(ch);
                    }
                    Control::StreamClose { stream } => {
                        if !channels.get(&ch).is_some_and(|state| state.hello) { return Err(invalid("hello required").into()); }
                        streams.close(ch, stream, &tx).map_err(invalid)?;
                    }
                    Control::StreamCredit { stream, bytes } => {
                        if !channels.get(&ch).is_some_and(|state| state.hello) { return Err(invalid("hello required").into()); }
                        streams.credit(ch, stream, bytes).map_err(invalid)?;
                    },
                },
                FrameKind::Stream => {
                    if !channels.get(&ch).is_some_and(|state| state.hello) { return Err(invalid("hello required").into()); }
                    let (id, flags, _) = parse_stream(&frame.body).ok_or_else(|| invalid("malformed stream frame"))?;
                    streams.input(ch, id, flags & FLAG_FIN != 0, frame.body.slice(5..)).map_err(invalid)?;
                },
                FrameKind::Json => {
                    let msg = parse_json::<ClientMessage>(&frame)?;
                    let state = channels.entry(ch).or_default();
                    tokio::select! {
                        _ = shutdown.cancelled() => break,
                        _ = lost.cancelled() => break,
                        result = handle(msg, ch, state, &daemon, &tx, &streams, &mut next_sub) => result?,
                    }
                }
            }
        }
        Ok(())
    }
    .await;
    drop(channels);
    streams.close_all();
    tx.close();
    writer.abort();
    let _ = writer.await;
    result
}

fn response(id: u32, result: Result<Box<RawValue>, DispatchError>) -> ServerMessage {
    match result {
        Ok(result) => ServerMessage::Response {
            id,
            result: Some(result),
            error: None,
            diagnostic_id: None,
        },
        Err(error) => ServerMessage::Response {
            id,
            result: None,
            error: Some(error.error),
            diagnostic_id: error.diagnostic_id,
        },
    }
}
async fn send(
    tx: &Arc<Outbox>,
    channel: u32,
    generation: &CancellationToken,
    message: ServerMessage,
) -> io::Result<()> {
    tokio::select! {
        _ = generation.cancelled() => Ok(()),
        result = tx.send(Outgoing { frame: json_frame(channel, &message), generation: generation.clone(), flushed: None }) => if result { Ok(()) } else { Err(invalid("connection closed")) },
    }
}
async fn handle(
    msg: ClientMessage,
    ch: u32,
    state: &mut Channel,
    d: &Arc<Daemon>,
    tx: &Arc<Outbox>,
    streams: &Streams,
    next_sub: &mut u32,
) -> io::Result<()> {
    match msg {
        ClientMessage::Hello {
            version,
            protocol,
            instance,
            ..
        } => {
            if state.hello || instance != d.services.paths.instance.as_str() {
                return Err(invalid("duplicate hello or instance mismatch"));
            }
            let welcome = ServerMessage::Welcome {
                version: d.services.version.into(),
                protocol: PROTOCOL_VERSION,
                daemon_id: d.services.daemon_id.clone(),
            };
            if version != d.services.version || protocol != PROTOCOL_VERSION {
                // Wait for the writer: connection teardown cancels this generation and
                // aborts the writer, so enqueueing alone can silently discard Welcome.
                let (flushed, received) = oneshot::channel();
                tokio::time::timeout(std::time::Duration::from_secs(2), async {
                    tx.send(Outgoing {
                        frame: json_frame(ch, &welcome),
                        generation: state.generation.clone(),
                        flushed: Some(flushed),
                    })
                    .await
                    .then_some(())
                    .ok_or_else(|| invalid("connection closed"))?;
                    received.await.map_err(|_| invalid("connection closed"))
                })
                .await
                .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Welcome flush deadline"))??;
                return Err(invalid("hello version or protocol mismatch"));
            }
            state.hello = true;
            send(tx, ch, &state.generation, welcome).await?;
        }
        ClientMessage::Request { id, .. } | ClientMessage::Subscribe { id, .. } if !state.hello => {
            send(
                tx,
                ch,
                &state.generation,
                response(id, Err(DispatchError::common(CommonError::HelloRequired))),
            )
            .await?;
        }
        ClientMessage::Cancel { .. } | ClientMessage::Unsubscribe { .. } if !state.hello => {
            return Err(invalid("hello required"));
        }
        ClientMessage::Request {
            id,
            method,
            params,
            client_request_id,
        } => {
            let token = CancellationToken::new();
            {
                let mut inflight = state.inflight.lock().expect("requests");
                if inflight.contains_key(&id) {
                    return Err(invalid("duplicate inflight request id"));
                }
                inflight.insert(id, token.clone());
            }
            let (d, tx, inflight, generation) = (
                d.clone(),
                tx.clone(),
                state.inflight.clone(),
                state.generation.clone(),
            );
            let opener = streams.opener(ch, generation.clone(), tx.clone());
            let tracker = d.dedupe.tracker.clone();
            tracker.spawn(async move {
                let result = d
                    .dedupe
                    .dispatch(
                        &d.router,
                        &method,
                        Ctx {
                            channel: ch,
                            cancel: token,
                            streams: Some(opener),
                        },
                        params,
                        client_request_id,
                    )
                    .await;
                // Removal follows enqueue, so a reused id cannot overtake its old reply.
                let _ = send(&tx, ch, &generation, response(id, result)).await;
                inflight.lock().expect("requests").remove(&id);
            });
        }
        ClientMessage::Cancel { id } => {
            if let Some(cancel) = state.inflight.lock().expect("requests").get(&id) {
                cancel.cancel();
            }
        }
        ClientMessage::Subscribe { id, topic, .. } => {
            if state.inflight.lock().expect("requests").contains_key(&id) {
                return Err(invalid("duplicate inflight request id"));
            }
            if !d.topics.contains(topic.as_str()) {
                return send(
                    tx,
                    ch,
                    &state.generation,
                    response(
                        id,
                        Err(DispatchError::common(CommonError::MethodNotFound {
                            method: topic,
                        })),
                    ),
                )
                .await;
            }
            let sub_id = *next_sub;
            *next_sub = next_sub
                .checked_add(1)
                .ok_or_else(|| invalid("subscription id exhausted"))?;
            let mut receiver = d.services.hub.subscribe(&topic);
            send(
                tx,
                ch,
                &state.generation,
                ServerMessage::Subscribed {
                    id,
                    sub_id,
                    rev: if topic == "observe.problems.changed" {
                        d.services.observe.problem_rev()
                    } else {
                        d.services.db.rev()
                    }
                    .to_string(),
                },
            )
            .await?;
            let (tx, generation) = (tx.clone(), state.generation.clone());
            let task = tokio::spawn(async move {
                while let Some(event) = receiver.recv().await {
                    let message = match event {
                        HubMessage::Event { rev, payload } => ServerMessage::Event {
                            sub_id,
                            rev: rev.to_string(),
                            payload: RawValue::from_string(payload.to_string()).expect("hub JSON"),
                        },
                        HubMessage::Resync => ServerMessage::Resync { sub_id },
                    };
                    if send(&tx, ch, &generation, message).await.is_err() {
                        break;
                    }
                }
            });
            state.subscriptions.insert(sub_id, task);
        }
        ClientMessage::Unsubscribe { sub_id } => {
            if let Some(task) = state.subscriptions.remove(&sub_id) {
                task.abort();
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
