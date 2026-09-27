use crate::routes::{failure, Routes};
use agent_protocol::{AgentMessage, ClientMessage};
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncWrite};

type Result<T> = std::result::Result<T, &'static str>;
const MAX_REQUESTS: usize = 32;
const MAX_BUFFERED_REPLIES: usize = 128 * 1024 * 1024;
const REPLY_ACK_TIMEOUT: Duration = Duration::from_secs(10);
struct StreamingReply {
    sender: agent_protocol::reply::Sender,
    deadline: tokio::time::Instant,
}
fn retire_reply(retired: &mut VecDeque<(u64, u64)>, id: u64, reply: StreamingReply) {
    if let Some(cursor) = reply.sender.awaiting_cursor() {
        if retired.len() >= MAX_REQUESTS {
            retired.pop_front();
        }
        retired.push_back((id, cursor));
    }
}
fn chunk_message(id: u64, chunk: agent_protocol::reply::Chunk) -> Result<AgentMessage> {
    Ok(AgentMessage::Stream {
        stream: id,
        payload: serde_json::to_value(chunk).map_err(|_| "response_invalid")?,
    })
}

pub struct Peer {
    product: String,
    installation_id: String,
    verify: Box<dyn Fn() -> Result<()> + Send + Sync>,
}
impl Peer {
    fn revalidate(&self) -> Result<()> {
        (self.verify)()
    }
    #[cfg(test)]
    fn for_tests(product: &str) -> Self {
        Self {
            product: product.into(),
            installation_id: "fixture".into(),
            verify: Box::new(|| Ok(())),
        }
    }
}
pub use agent_client::wire::{read as read_message, write as write_message};

pub async fn handle_connection<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    stream: S,
    peer: Option<Peer>,
    routes: Arc<Routes>,
) -> Result<()> {
    let (mut read, mut write) = tokio::io::split(stream);
    let hello: ClientMessage =
        tokio::time::timeout(Duration::from_secs(2), read_message(&mut read))
            .await
            .map_err(|_| "hello_timeout")??;
    let validation = peer.as_ref().ok_or("peer_unverified").and_then(|peer| {
        peer.revalidate()?;
        agent_protocol::check_hello(&hello, &peer.product).map_err(|e| e.code())
    });
    if let Err(reason) = validation {
        write_message(
            &mut write,
            &AgentMessage::Rejected {
                reason: reason.into(),
            },
        )
        .await?;
        // The verified peer may request only graceful shutdown after a protocol
        // mismatch. Never admit business calls or trust a mismatched product.
        if reason == "protocol_mismatch" {
            if let Some(peer) = &peer {
                let matching = matches!(&hello, ClientMessage::Hello { product, session, .. }
                    if product == &peer.product && !session.is_empty() && session.len() <= 64);
                if matching
                    && matches!(
                        tokio::time::timeout(
                            Duration::from_secs(2),
                            read_message::<_, ClientMessage>(&mut read)
                        )
                        .await,
                        Ok(Ok(ClientMessage::Shutdown {}))
                    )
                {
                    peer.revalidate()?;
                    routes.shutdown();
                }
            }
        }
        return Err(reason);
    }
    let peer = peer.ok_or("peer_unverified")?;
    let ClientMessage::Hello { session, .. } = hello else {
        return Err("hello_required");
    };
    write_message(
        &mut write,
        &AgentMessage::Welcome {
            protocol: agent_protocol::PROTOCOL_VERSION,
            agent_version: env!("CARGO_PKG_VERSION").into(),
            generation: routes.generation.clone(),
        },
    )
    .await?;
    let session_guard = routes.session(&peer.product, &peer.installation_id, &session)?;
    let mut shutdown = routes.shutdown_receiver();
    let mut pending = HashMap::new();
    let mut tasks = tokio::task::JoinSet::new();
    let mut ids = agent_protocol::RequestIds::default();
    let (messages, mut incoming) = tokio::sync::mpsc::channel(1);
    struct Reader(tokio::task::JoinHandle<()>);
    impl Drop for Reader {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _reader = Reader(tokio::spawn(async move {
        loop {
            let message = read_message::<_, ClientMessage>(&mut read).await;
            let failed = message.is_err();
            if messages.send(message).await.is_err() || failed {
                break;
            }
        }
    }));
    let mut replies = HashMap::<u64, StreamingReply>::new();
    let mut retired = VecDeque::new();
    loop {
        let ack_deadline = replies
            .values()
            .map(|reply| reply.deadline)
            .min()
            .unwrap_or_else(|| tokio::time::Instant::now() + Duration::from_secs(86400));
        if *shutdown.borrow() {
            return write_message(
                &mut write,
                &AgentMessage::Rejected {
                    reason: "agent_shutdown".into(),
                },
            )
            .await;
        }
        tokio::select! {
            _ = shutdown.changed() => return write_message(&mut write, &AgentMessage::Rejected {reason:"agent_shutdown".into()}).await,
            _ = tokio::time::sleep_until(ack_deadline), if !replies.is_empty() => {
                let now = tokio::time::Instant::now();
                let expired = replies.iter().filter(|(_, reply)| reply.deadline <= now).map(|(id, _)| *id).collect::<Vec<_>>();
                for id in expired {
                    if let Some(reply) = replies.remove(&id) { retire_reply(&mut retired, id, reply); }
                    ids.remove(id);
                    write_message(&mut write, &AgentMessage::StreamEnd { stream: id, reason: "reply_timeout".into() }).await?;
                }
            }
            message = incoming.recv() => {
                let message = message.ok_or("connection_closed")??;
                peer.revalidate()?;
                match message {
                    ClientMessage::Call {id, component, request} => {
                        if tasks.len() + replies.len() >= MAX_REQUESTS { return Err("request_limit"); }
                        ids.insert(id).map_err(|e| e.code())?;
                        retired.retain(|(old, _)| *old != id);
                        let (cancel, cancelled) = tokio::sync::oneshot::channel::<()>();
                        pending.insert(id, cancel);
                        let routes = routes.clone();
                        let product = peer.product.clone();
                        let session = session.clone();
                        let session_guard = session_guard.clone();
                        tasks.spawn(async move {
                            let response = tokio::select! {
                                _ = cancelled => failure("cancelled"),
                                result = tokio::time::timeout(Duration::from_secs(29), routes.dispatch(&product, &session, session_guard, &component, request)) => result.unwrap_or_else(|_| failure("deadline_exceeded")),
                            };
                            (id, response)
                        });
                    }
                    ClientMessage::Cancel {id} => {
                        if let Some(cancel) = pending.remove(&id) { let _ = cancel.send(()); }
                        if let Some(reply) = replies.remove(&id) {
                            retire_reply(&mut retired, id, reply);
                            ids.remove(id);
                            write_message(&mut write, &AgentMessage::StreamEnd { stream: id, reason: "cancelled".into() }).await?;
                        }
                    }
                    ClientMessage::Ack {stream, cursor} => {
                        if !replies.contains_key(&stream) {
                            if let Some(position) = retired.iter().position(|entry| *entry == (stream, cursor)) {
                                retired.remove(position);
                                continue;
                            }
                            return Err("stream_unknown");
                        }
                        let reply = replies.get_mut(&stream).ok_or("stream_unknown")?;
                        reply.sender.ack(cursor).map_err(|error| error.code())?;
                        if let Some(chunk) = reply.sender.next_chunk().map_err(|error| error.code())? {
                            write_message(&mut write, &chunk_message(stream, chunk)?).await?;
                            reply.deadline = tokio::time::Instant::now() + REPLY_ACK_TIMEOUT;
                        } else {
                            replies.remove(&stream); ids.remove(stream);
                            write_message(&mut write, &AgentMessage::StreamEnd { stream, reason: "reply_complete".into() }).await?;
                        }
                    }
                    ClientMessage::Unsubscribe {stream} => {
                        let reply = replies.remove(&stream).ok_or("stream_unknown")?;
                        retire_reply(&mut retired, stream, reply);
                        ids.remove(stream);
                        write_message(&mut write, &AgentMessage::StreamEnd { stream, reason: "cancelled".into() }).await?;
                    }
                    ClientMessage::Shutdown {} => { routes.shutdown(); return Ok(()); }
                    _ => return Err("message_unexpected"),
                }
            }
            completed = tasks.join_next(), if !tasks.is_empty() => {
                let (id, response) = completed.ok_or("request_failed")?.map_err(|_| "request_failed")?;
                peer.revalidate()?;
                pending.remove(&id);
                let written = agent_client::wire::write_reply(&mut write, id, &response).await;
                if written.is_ok() {
                    ids.remove(id);
                } else if written == Err("frame_too_large") {
                    let buffered: usize = replies.values().map(|reply| reply.sender.buffered_bytes()).sum();
                    match agent_protocol::reply::Sender::new(&response) {
                        Ok(mut sender) if sender.buffered_bytes() <= MAX_BUFFERED_REPLIES.saturating_sub(buffered) => {
                            let chunk = sender.next_chunk().map_err(|error| error.code())?.ok_or("response_invalid")?;
                            write_message(&mut write, &chunk_message(id, chunk)?).await?;
                            replies.insert(id, StreamingReply { sender, deadline: tokio::time::Instant::now() + REPLY_ACK_TIMEOUT });
                        }
                        rejected => {
                            ids.remove(id);
                            let issue = if rejected.is_ok() { "busy" } else { "response_too_large" };
                            agent_client::wire::write_reply(&mut write, id, &failure(issue)).await?;
                        }
                    }
                } else {
                    return written;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::Routes;
    use agent_protocol::{AgentMessage, ClientMessage, PROTOCOL_VERSION};

    async fn hello(client: &mut tokio::io::DuplexStream, product: &str, protocol: u32) {
        write_message(
            client,
            &ClientMessage::Hello {
                protocol,
                product: product.into(),
                session: "s".into(),
            },
        )
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn owner_shutdown_notifies_authenticated_idle_clients_before_closing() {
        let (mut client, server) = tokio::io::duplex(4096);
        let routes = Routes::for_tests();
        let server_routes = routes.clone();
        let task = tokio::spawn(handle_connection(
            server,
            Some(Peer::for_tests("workspace")),
            server_routes,
        ));
        write_message(
            &mut client,
            &ClientMessage::Hello {
                protocol: 1,
                product: "workspace".into(),
                session: "fixture".into(),
            },
        )
        .await
        .unwrap();
        let _: AgentMessage = read_message(&mut client).await.unwrap();
        routes.shutdown();
        assert_eq!(
            read_message::<_, AgentMessage>(&mut client).await.unwrap(),
            AgentMessage::Rejected {
                reason: "agent_shutdown".into()
            }
        );
        task.await.unwrap().unwrap();
    }
    #[tokio::test]
    async fn handshake_status_and_shutdown_round_trip() {
        let (mut client, server) = tokio::io::duplex(65536);
        let routes = Routes::for_tests();
        let shutdown = routes.shutdown_receiver();
        let task = tokio::spawn(async move {
            handle_connection(server, Some(Peer::for_tests("workspace")), routes).await
        });
        hello(&mut client, "workspace", PROTOCOL_VERSION).await;
        assert!(matches!(
            read_message::<_, AgentMessage>(&mut client).await.unwrap(),
            AgentMessage::Welcome { .. }
        ));
        write_message(
            &mut client,
            &ClientMessage::Call {
                id: 1,
                component: "agent.status".into(),
                request: serde_json::json!({"header":{},"method":"status","args":{}}),
            },
        )
        .await
        .unwrap();
        match read_message::<_, AgentMessage>(&mut client).await.unwrap() {
            AgentMessage::Reply { id, response } => {
                assert_eq!(id, 1);
                assert_eq!(
                    response["value"]["components"],
                    serde_json::json!(["agent.status"])
                );
            }
            other => panic!("{other:?}"),
        }
        write_message(&mut client, &ClientMessage::Shutdown {})
            .await
            .unwrap();
        task.await.unwrap().unwrap();
        assert!(*shutdown.borrow());
    }
    #[tokio::test]
    async fn protocol_recovery_accepts_only_verified_matching_peer_shutdown() {
        for (peer_product, shutdown, allowed) in [
            ("workspace", true, true),
            ("knowledge", true, false),
            ("workspace", false, false),
        ] {
            let (mut client, server) = tokio::io::duplex(4096);
            let routes = Routes::for_tests();
            let ended = routes.shutdown_receiver();
            let task = tokio::spawn(handle_connection(
                server,
                Some(Peer::for_tests(peer_product)),
                routes,
            ));
            hello(
                &mut client,
                "workspace",
                agent_protocol::PROTOCOL_VERSION + 1,
            )
            .await;
            assert!(
                matches!(read_message::<_, AgentMessage>(&mut client).await.unwrap(), AgentMessage::Rejected {reason} if reason == "protocol_mismatch")
            );
            let message = if shutdown {
                ClientMessage::Shutdown {}
            } else {
                ClientMessage::Call {
                    id: 1,
                    component: "agent.status".into(),
                    request: serde_json::json!({"method":"status","args":{}}),
                }
            };
            let _ = write_message(&mut client, &message).await;
            assert!(task.await.unwrap().is_err());
            assert_eq!(*ended.borrow(), allowed);
        }
    }
    #[tokio::test]
    async fn foreign_peers_protocols_and_claimed_products_are_rejected() {
        for (peer, protocol) in [
            (None, PROTOCOL_VERSION),
            (Some("workspace"), PROTOCOL_VERSION + 1),
            (Some("knowledge"), PROTOCOL_VERSION),
        ] {
            let (mut client, server) = tokio::io::duplex(4096);
            let peer = peer.map(Peer::for_tests);
            let task =
                tokio::spawn(
                    async move { handle_connection(server, peer, Routes::for_tests()).await },
                );
            hello(&mut client, "workspace", protocol).await;
            assert!(matches!(
                read_message::<_, AgentMessage>(&mut client).await.unwrap(),
                AgentMessage::Rejected { .. }
            ));
            assert!(task.await.unwrap().is_err());
        }
    }
    #[tokio::test]
    async fn a_reply_does_not_discard_the_next_partial_frame() {
        use tokio::io::AsyncWriteExt;
        let (mut client, server) = tokio::io::duplex(65536);
        let task = tokio::spawn(handle_connection(
            server,
            Some(Peer::for_tests("workspace")),
            Routes::for_tests(),
        ));
        hello(&mut client, "workspace", PROTOCOL_VERSION).await;
        read_message::<_, AgentMessage>(&mut client).await.unwrap();
        let call = |id| ClientMessage::Call {
            id,
            component: "agent.status".into(),
            request: serde_json::json!({"header":{},"method":"status","args":{}}),
        };
        let first = agent_protocol::encode(&call(1)).unwrap();
        let second = agent_protocol::encode(&call(2)).unwrap();
        client
            .write_all(&[first.as_slice(), &second[..2]].concat())
            .await
            .unwrap();
        assert!(matches!(
            read_message::<_, AgentMessage>(&mut client).await.unwrap(),
            AgentMessage::Reply { id: 1, .. }
        ));
        client.write_all(&second[2..]).await.unwrap();
        assert!(matches!(
            read_message::<_, AgentMessage>(&mut client).await.unwrap(),
            AgentMessage::Reply { id: 2, .. }
        ));
        drop(client);
        let _ = task.await;
    }

    #[tokio::test]
    async fn large_results_stream_in_bounded_acknowledged_batches_without_losing_fields() {
        let value = serde_json::json!({"rows":"λ\"".repeat(400_000)});
        let (mut client, server) = tokio::io::duplex(128 * 1024);
        let routes = Routes::with_reply_for_tests(value.clone());
        let task = tokio::spawn(handle_connection(
            server,
            Some(Peer::for_tests("workspace")),
            routes,
        ));
        hello(&mut client, "workspace", agent_protocol::PROTOCOL_VERSION).await;
        assert!(matches!(
            read_message::<_, AgentMessage>(&mut client).await.unwrap(),
            AgentMessage::Welcome { .. }
        ));
        write_message(
            &mut client,
            &ClientMessage::Call {
                id: 42,
                component: "agent.status".into(),
                request: serde_json::json!({"header":{},"method":"status","args":{}}),
            },
        )
        .await
        .unwrap();
        let mut collected = String::new();
        loop {
            let message: AgentMessage = read_message(&mut client).await.unwrap();
            assert!(agent_protocol::encode(&message).unwrap().len() <= 64 * 1024 + 4);
            match message {
                AgentMessage::Stream { stream, payload } => {
                    assert_eq!(stream, 42);
                    assert_eq!(payload["offset"], collected.len() as u64);
                    collected.push_str(payload["data"].as_str().unwrap());
                    write_message(
                        &mut client,
                        &ClientMessage::Ack {
                            stream,
                            cursor: collected.len() as u64,
                        },
                    )
                    .await
                    .unwrap();
                }
                AgentMessage::StreamEnd { stream, reason } => {
                    assert_eq!(stream, 42);
                    assert_eq!(reason, "reply_complete");
                    break;
                }
                other => panic!("a large read must be streamed, got {other:?}"),
            }
        }
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&collected).unwrap(),
            value
        );
        write_message(&mut client, &ClientMessage::Shutdown {})
            .await
            .unwrap();
        task.await.unwrap().unwrap();
    }
    #[tokio::test]
    async fn cancelling_or_expiring_a_reply_does_not_break_other_calls_when_its_ack_arrives_late() {
        for cancel in [true, false] {
            let value = serde_json::json!({"rows":"x".repeat(agent_protocol::MAX_FRAME_BYTES)});
            let (mut client, server) = tokio::io::duplex(128 * 1024);
            let task = tokio::spawn(handle_connection(
                server,
                Some(Peer::for_tests("workspace")),
                Routes::with_reply_for_tests(value),
            ));
            hello(&mut client, "workspace", agent_protocol::PROTOCOL_VERSION).await;
            let _: AgentMessage = read_message(&mut client).await.unwrap();
            let request = serde_json::json!({"header":{},"method":"status","args":{}});
            write_message(
                &mut client,
                &ClientMessage::Call {
                    id: 1,
                    component: "agent.status".into(),
                    request: request.clone(),
                },
            )
            .await
            .unwrap();
            let AgentMessage::Stream { payload, .. } = read_message(&mut client).await.unwrap()
            else {
                panic!("stream required");
            };
            let cursor = payload["data"].as_str().unwrap().len() as u64;
            assert!(
                tokio::time::timeout(
                    Duration::from_millis(20),
                    read_message::<_, AgentMessage>(&mut client)
                )
                .await
                .is_err(),
                "no second unacknowledged batch"
            );
            if cancel {
                write_message(&mut client, &ClientMessage::Cancel { id: 1 })
                    .await
                    .unwrap();
            }
            let end: AgentMessage = read_message(&mut client).await.unwrap();
            assert!(
                matches!(end, AgentMessage::StreamEnd {stream:1, reason} if reason == if cancel {"cancelled"} else {"reply_timeout"})
            );
            write_message(&mut client, &ClientMessage::Ack { stream: 1, cursor })
                .await
                .unwrap();
            write_message(
                &mut client,
                &ClientMessage::Call {
                    id: 2,
                    component: "agent.status".into(),
                    request,
                },
            )
            .await
            .unwrap();
            assert!(matches!(
                read_message::<_, AgentMessage>(&mut client).await.unwrap(),
                AgentMessage::Stream { stream: 2, .. }
            ));
            write_message(&mut client, &ClientMessage::Shutdown {})
                .await
                .unwrap();
            task.await.unwrap().unwrap();
        }
    }
    #[tokio::test]
    async fn oversized_frames_are_rejected_before_reading_the_body() {
        let (mut client, mut server) = tokio::io::duplex(64);
        use tokio::io::AsyncWriteExt;
        client
            .write_all(&((agent_protocol::MAX_FRAME_BYTES + 1) as u32).to_le_bytes())
            .await
            .unwrap();
        assert_eq!(
            read_message::<_, ClientMessage>(&mut server)
                .await
                .unwrap_err(),
            "frame_too_large"
        );
    }
}

#[cfg(windows)]
pub fn start(
    app: tauri::AppHandle,
    scope: Arc<suite_runtime::platform::component_scope::CapturedScope>,
    name: String,
    routes: Arc<Routes>,
) -> Result<()> {
    use std::os::windows::io::AsRawHandle;
    use suite_runtime::platform::agent_peer::{AgentPeer, Witness};
    use tokio::net::windows::named_pipe::ServerOptions;
    use windows::Win32::Foundation::HANDLE;
    let first = ServerOptions::new()
        .first_pipe_instance(true)
        .reject_remote_clients(true)
        .create(&name)
        .map_err(|_| "agent_pipe_unavailable")?;
    tauri::async_runtime::spawn(async move {
        let mut listener = first;
        let mut clients = tokio::task::JoinSet::new();
        let mut shutdown = routes.shutdown_receiver();
        loop {
            if *shutdown.borrow() {
                break;
            }
            tokio::select! {
                _ = shutdown.changed() => break,
                Some(_) = clients.join_next(), if !clients.is_empty() => continue,
                connected = listener.connect() => { if connected.is_err() { break; } }
            }
            // Preserve the registered pipe while creating its successor.
            let Ok(next) = ServerOptions::new()
                .reject_remote_clients(true)
                .create(&name)
            else {
                break;
            };
            let stream = std::mem::replace(&mut listener, next);
            if clients.len() >= 32 {
                continue;
            }
            let Ok(witness) = Witness::capture(HANDLE(stream.as_raw_handle())) else {
                continue;
            };
            let scope = scope.clone();
            let routes = routes.clone();
            clients.spawn(async move {
                let verified = tokio::time::timeout(
                    Duration::from_secs(2),
                    tokio::task::spawn_blocking(move || AgentPeer::product(scope, witness)),
                )
                .await;
                let peer = match verified {
                    Ok(Ok(Ok(peer))) => peer.installation_id().ok().map(|installation_id| Peer {
                        product: peer.product_id().into(),
                        installation_id,
                        verify: Box::new(move || peer.revalidate()),
                    }),
                    _ => None,
                };
                let _ = handle_connection(stream, peer, routes).await;
            });
        }
        routes.shutdown();
        // Let authenticated peers latch intentional shutdown before dropping
        // their pipes. Their background queries must not launch us again.
        if tokio::time::timeout(Duration::from_secs(6), async {
            while clients.join_next().await.is_some() {}
        })
        .await
        .is_err()
        {
            clients.abort_all();
            while clients.join_next().await.is_some() {}
        }
        routes.shutdown_owners().await;
        drop(listener);
        // All owners have drained before run() may release the writer lease.
        // Explicit exit bypasses the headless idle-exit prevention.
        app.exit(0);
    });
    Ok(())
}
