use crate::routes::{failure, Routes};
use agent_protocol::{AgentMessage, ClientMessage};
use serde::{de::DeserializeOwned, Serialize};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

type Result<T> = std::result::Result<T, &'static str>;
const IO_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_REQUESTS: usize = 32;

pub struct Peer {
    product: String,
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
            verify: Box::new(|| Ok(())),
        }
    }
}
// The first byte may wait while idle. Once a frame starts it must finish within
// one deadline; a slow peer cannot reset that deadline one byte at a time.
pub async fn read_message<S: AsyncRead + Unpin, T: DeserializeOwned>(stream: &mut S) -> Result<T> {
    let mut header = [0; 4];
    stream
        .read_exact(&mut header[..1])
        .await
        .map_err(|_| "connection_closed")?;
    tokio::time::timeout(IO_TIMEOUT, async {
        stream
            .read_exact(&mut header[1..])
            .await
            .map_err(|_| "connection_closed")?;
        let size = u32::from_le_bytes(header) as usize;
        if size == 0 {
            return Err("frame_empty");
        }
        if size > agent_protocol::MAX_FRAME_BYTES {
            return Err("frame_too_large");
        }
        let mut body = vec![0; size];
        stream
            .read_exact(&mut body)
            .await
            .map_err(|_| "connection_closed")?;
        agent_protocol::decode(&body).map_err(|e| e.code())
    })
    .await
    .map_err(|_| "frame_timeout")?
}
pub async fn write_message<S: AsyncWrite + Unpin, T: Serialize>(
    stream: &mut S,
    message: &T,
) -> Result<()> {
    let bytes = agent_protocol::encode(message).map_err(|e| e.code())?;
    tokio::time::timeout(IO_TIMEOUT, stream.write_all(&bytes))
        .await
        .map_err(|_| "write_timeout")?
        .map_err(|_| "connection_closed")
}

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
    loop {
        if *shutdown.borrow() {
            return Ok(());
        }
        tokio::select! {
            _ = shutdown.changed() => return Ok(()),
            message = incoming.recv() => {
                let message = message.ok_or("connection_closed")??;
                peer.revalidate()?;
                match message {
                    ClientMessage::Call {id, component, request} => {
                        if tasks.len() >= MAX_REQUESTS { return Err("request_limit"); }
                        ids.insert(id).map_err(|e| e.code())?;
                        let (cancel, cancelled) = tokio::sync::oneshot::channel::<()>();
                        pending.insert(id, cancel);
                        let routes = routes.clone();
                        let product = peer.product.clone();
                        let session = session.clone();
                        tasks.spawn(async move {
                            let response = tokio::select! {
                                _ = cancelled => failure("cancelled"),
                                result = tokio::time::timeout(Duration::from_secs(29), routes.dispatch(&product, &session, &component, request)) => result.unwrap_or_else(|_| failure("deadline_exceeded")),
                            };
                            (id, response)
                        });
                    }
                    ClientMessage::Cancel {id} => { if let Some(cancel) = pending.remove(&id) { let _ = cancel.send(()); } }
                    ClientMessage::Shutdown {} => { routes.shutdown(); return Ok(()); }
                    _ => return Err("message_unexpected"),
                }
            }
            completed = tasks.join_next(), if !tasks.is_empty() => {
                let (id, response) = completed.ok_or("request_failed")?.map_err(|_| "request_failed")?;
                peer.revalidate()?;
                pending.remove(&id);
                ids.remove(id);
                write_message(&mut write, &AgentMessage::Reply {id, response}).await?;
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
                    Ok(Ok(Ok(peer))) => Some(Peer {
                        product: peer.product_id().into(),
                        verify: Box::new(move || peer.revalidate()),
                    }),
                    _ => None,
                };
                let _ = handle_connection(stream, peer, routes).await;
            });
        }
        routes.shutdown();
        clients.abort_all();
        while clients.join_next().await.is_some() {}
        drop(listener);
        // Later owners drain their workers here, before the process writer lease
        // may be released by run(). Exit has an explicit code, bypassing UI idle.
        app.exit(0);
    });
    Ok(())
}
