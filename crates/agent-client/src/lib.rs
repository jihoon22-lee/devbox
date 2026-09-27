pub mod wire;
use agent_protocol::{AgentMessage, ClientMessage};
use serde_json::Value;
use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{mpsc, oneshot, Mutex},
};
pub const RETRY_DELAYS_MS: [u64; 5] = [200, 500, 1000, 2000, 4000];
#[derive(Debug, Clone, PartialEq)]
pub enum AgentError {
    Unsupported,
    Unavailable,
    Rejected(String),
    Remote(Value),
}
#[derive(Debug)]
pub enum Connect {
    Missing,
    Unavailable,
    Rejected(String),
}
pub trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}
pub struct Connected {
    pub stream: Box<dyn Io>,
    pub verify: Arc<dyn Fn() -> Result<(), AgentError> + Send + Sync>,
    /// The exact retained native process handle, never a PID/name lookup.
    pub exited: Arc<dyn Fn() -> Result<bool, AgentError> + Send + Sync>,
}
pub type ConnectFuture<'a> = Pin<Box<dyn Future<Output = Result<Connected, Connect>> + Send + 'a>>;
pub type LaunchFuture<'a> = Pin<Box<dyn Future<Output = Result<(), AgentError>> + Send + 'a>>;
pub trait Transport: Send + Sync {
    fn connect(&self) -> ConnectFuture<'_>;
    fn launch(&self) -> LaunchFuture<'_>;
    fn generation(&self) -> &str;
}
struct Outgoing {
    message: ClientMessage,
    reply: Option<oneshot::Sender<Result<Value, AgentError>>>,
}
struct Connection {
    session: String,
    outgoing: mpsc::Sender<Outgoing>,
    alive: Arc<AtomicBool>,
    driver: tokio::task::AbortHandle,
}
impl Drop for Connection {
    fn drop(&mut self) {
        self.driver.abort();
    }
}
struct Inner {
    product: String,
    transport: Option<Arc<dyn Transport>>,
    unsupported: bool,
    status: tokio::sync::watch::Sender<&'static str>,
    epoch: Arc<AtomicU64>,
    current: Mutex<Option<Arc<Connection>>>,
    next: AtomicU64,
}
#[derive(Clone)]
pub struct AgentClient(Arc<Inner>);
impl AgentClient {
    pub fn with_transport(product: &str, transport: Arc<dyn Transport>) -> Self {
        Self::new(product, Some(transport), false)
    }
    fn new(product: &str, transport: Option<Arc<dyn Transport>>, unsupported: bool) -> Self {
        Self(Arc::new(Inner {
            product: product.into(),
            transport,
            unsupported,
            status: tokio::sync::watch::channel(if unsupported {
                "unsupported"
            } else {
                "unavailable"
            })
            .0,
            epoch: Arc::new(AtomicU64::new(0)),
            current: Mutex::new(None),
            next: AtomicU64::new(1),
        }))
    }
    pub fn unsupported() -> Self {
        Self::new("", None, true)
    }
    pub fn unavailable() -> Self {
        Self::new("", None, false)
    }
    pub fn status(&self) -> &'static str {
        *self.0.status.borrow()
    }
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<&'static str> {
        self.0.status.subscribe()
    }
    pub fn supported(&self) -> bool {
        !self.0.unsupported
    }
    async fn ensure_connected(&self, session: &str) -> Result<Arc<Connection>, AgentError> {
        if self.0.unsupported {
            return Err(AgentError::Unsupported);
        }
        let transport = self.0.transport.as_ref().ok_or(AgentError::Unavailable)?;
        if session.is_empty() || session.len() > 64 {
            return Err(AgentError::Rejected("session_invalid".into()));
        }
        let mut current = self.0.current.lock().await;
        if let Some(connection) = current.as_ref() {
            if connection.session == session && connection.alive.load(Ordering::Acquire) {
                return Ok(connection.clone());
            }
        }
        let epoch = self.0.epoch.fetch_add(1, Ordering::AcqRel) + 1;
        *current = None;
        struct Starting<'a>(&'a tokio::sync::watch::Sender<&'static str>);
        impl Drop for Starting<'_> {
            fn drop(&mut self) {
                self.0.send_if_modified(|status| {
                    if matches!(*status, "starting" | "restarting") {
                        *status = "unavailable";
                        true
                    } else {
                        false
                    }
                });
            }
        }
        self.0.status.send_replace("starting");
        let _starting = Starting(&self.0.status);
        let mut launched = false;
        let mut restarted = false;
        for attempt in 0..=RETRY_DELAYS_MS.len() {
            match tokio::time::timeout(Duration::from_secs(2), transport.connect()).await {
                Ok(Ok(mut connected)) => {
                    (connected.verify)()?;
                    wire::write(
                        &mut connected.stream,
                        &ClientMessage::Hello {
                            protocol: agent_protocol::PROTOCOL_VERSION,
                            product: self.0.product.clone(),
                            session: session.into(),
                        },
                    )
                    .await
                    .map_err(|_| AgentError::Unavailable)?;
                    let welcome = tokio::time::timeout(
                        Duration::from_secs(2),
                        wire::read::<_, AgentMessage>(&mut connected.stream),
                    )
                    .await
                    .map_err(|_| AgentError::Unavailable)?
                    .map_err(|_| AgentError::Unavailable)?;
                    match welcome {
                        AgentMessage::Welcome {
                            protocol,
                            generation,
                            ..
                        } if protocol == agent_protocol::PROTOCOL_VERSION
                            && generation == transport.generation() => {}
                        AgentMessage::Rejected { reason }
                            if reason == "protocol_mismatch" && !restarted =>
                        {
                            (connected.verify)()?;
                            self.0.status.send_replace("restarting");
                            wire::write(&mut connected.stream, &ClientMessage::Shutdown {})
                                .await
                                .map_err(|_| AgentError::Unavailable)?;
                            let exited = connected.exited.clone();
                            drop(connected);
                            wait_for_retirement(transport.as_ref(), exited).await?;
                            tokio::time::timeout(Duration::from_secs(2), transport.launch())
                                .await
                                .map_err(|_| AgentError::Unavailable)??;
                            restarted = true;
                            launched = true;
                            continue;
                        }
                        AgentMessage::Rejected { reason } => {
                            return Err(AgentError::Rejected(reason))
                        }
                        _ => return Err(AgentError::Rejected("handshake_mismatch".into())),
                    }
                    (connected.verify)()?;
                    self.0.status.send_replace("connected");
                    let connection = Connection::start(
                        session.into(),
                        connected,
                        self.0.status.clone(),
                        self.0.epoch.clone(),
                        epoch,
                    );
                    *current = Some(connection.clone());
                    return Ok(connection);
                }
                Ok(Err(Connect::Missing)) => {
                    if !launched {
                        tokio::time::timeout(Duration::from_secs(2), transport.launch())
                            .await
                            .map_err(|_| AgentError::Unavailable)??;
                        launched = true;
                    }
                }
                Ok(Err(Connect::Rejected(reason))) => return Err(AgentError::Rejected(reason)),
                _ => {}
            }
            if let Some(delay) = RETRY_DELAYS_MS.get(attempt) {
                tokio::time::sleep(Duration::from_millis(*delay)).await;
            }
        }
        Err(AgentError::Unavailable)
    }
    /// Update callers never start an absent service. Missing is the only
    /// transport failure that proves it is gone; busy/unverified is fail-closed.
    pub async fn shutdown_if_running(&self, session: &str) -> Result<(), AgentError> {
        let transport = self.0.transport.as_ref().ok_or(AgentError::Unavailable)?;
        tokio::time::timeout(Duration::from_secs(10), async {
            let mut connected = match transport.connect().await {
                Ok(connection) => connection,
                Err(Connect::Missing) => return Ok(()),
                _ => return Err(AgentError::Unavailable),
            };
            (connected.verify)()?;
            wire::write(
                &mut connected.stream,
                &ClientMessage::Hello {
                    protocol: agent_protocol::PROTOCOL_VERSION,
                    product: self.0.product.clone(),
                    session: session.into(),
                },
            )
            .await
            .map_err(|_| AgentError::Unavailable)?;
            match wire::read::<_, AgentMessage>(&mut connected.stream).await {
                Ok(AgentMessage::Welcome {
                    protocol,
                    generation,
                    ..
                }) if protocol == agent_protocol::PROTOCOL_VERSION
                    && generation == transport.generation() => {}
                Ok(AgentMessage::Rejected { reason }) if reason == "protocol_mismatch" => {}
                _ => return Err(AgentError::Unavailable),
            }
            (connected.verify)()?;
            wire::write(&mut connected.stream, &ClientMessage::Shutdown {})
                .await
                .map_err(|_| AgentError::Unavailable)?;
            let exited = connected.exited.clone();
            drop(connected);
            wait_for_retirement(transport.as_ref(), exited).await
        })
        .await
        .map_err(|_| AgentError::Unavailable)?
    }
    /// Establish the native owner without executing or replaying a business call.
    pub async fn connect(&self, session: &str) -> Result<(), AgentError> {
        self.ensure_connected(session).await.map(|_| ())
    }
    pub async fn call(&self, component: &str, request: Value) -> Result<Value, AgentError> {
        if self.0.unsupported {
            return Err(AgentError::Unsupported);
        }
        if self.0.transport.is_none() {
            return Err(AgentError::Unavailable);
        }
        let session = request
            .pointer("/header/sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| AgentError::Rejected("session_invalid".into()))?;
        // Prove the full envelope fits before launching or submitting any work.
        // Reserve the largest request ID so this check remains valid at rollover.
        agent_protocol::reply::check_call(component, &request).map_err(|error| {
            AgentError::Rejected(
                if error == agent_protocol::ProtocolError::TooLarge {
                    "request_too_large"
                } else {
                    "request_invalid"
                }
                .into(),
            )
        })?;
        let connection = self.ensure_connected(session).await?;
        let id = self.0.next.fetch_add(1, Ordering::Relaxed);
        let (reply, response) = oneshot::channel();
        connection
            .outgoing
            .try_send(Outgoing {
                message: ClientMessage::Call {
                    id,
                    component: component.into(),
                    request,
                },
                reply: Some(reply),
            })
            .map_err(|_| AgentError::Unavailable)?;
        // An ambiguous failure is returned once. Never replay a submitted call.
        match tokio::time::timeout(Duration::from_secs(30), response).await {
            Ok(Ok(result)) => result,
            _ => {
                let _ = connection.outgoing.try_send(Outgoing {
                    message: ClientMessage::Cancel { id },
                    reply: None,
                });
                Err(AgentError::Unavailable)
            }
        }
    }
}
async fn wait_for_retirement(
    transport: &dyn Transport,
    exited: Arc<dyn Fn() -> Result<bool, AgentError> + Send + Sync>,
) -> Result<(), AgentError> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            tokio::time::sleep(Duration::from_millis(50)).await;
            match transport.connect().await {
                Err(Connect::Missing) if exited()? => return Ok(()),
                Err(Connect::Missing | Connect::Unavailable) => {}
                Ok(connection) => {
                    (connection.verify)()?;
                }
                Err(Connect::Rejected(reason)) => return Err(AgentError::Rejected(reason)),
            }
        }
    })
    .await
    .map_err(|_| AgentError::Unavailable)?
}
impl Connection {
    fn start(
        session: String,
        connected: Connected,
        status: tokio::sync::watch::Sender<&'static str>,
        epoch: Arc<AtomicU64>,
        own_epoch: u64,
    ) -> Arc<Self> {
        let (outgoing, mut queue) = mpsc::channel::<Outgoing>(64);
        let alive = Arc::new(AtomicBool::new(true));
        let live = alive.clone();
        let driver = tokio::spawn(async move {
            struct Status(
                tokio::sync::watch::Sender<&'static str>,
                Arc<AtomicU64>,
                u64,
            );
            impl Drop for Status {
                fn drop(&mut self) {
                    if self.1.load(Ordering::Acquire) == self.2 {
                        self.0.send_replace("unavailable");
                    }
                }
            }
            let _status = Status(status, epoch, own_epoch);
            struct Live(Arc<AtomicBool>);
            impl Drop for Live {
                fn drop(&mut self) {
                    self.0.store(false, Ordering::Release);
                }
            }
            let _live = Live(live);
            let (mut read, mut write) = tokio::io::split(connected.stream);
            let (messages, mut incoming) = mpsc::channel(1);
            let reader = tokio::spawn(async move {
                loop {
                    let result = wire::read::<_, AgentMessage>(&mut read).await;
                    let failed = result.is_err();
                    if messages.send(result).await.is_err() || failed {
                        break;
                    }
                }
            });
            struct Reader(tokio::task::JoinHandle<()>);
            impl Drop for Reader {
                fn drop(&mut self) {
                    self.0.abort();
                }
            }
            let _reader = Reader(reader);
            let mut pending = HashMap::<u64, oneshot::Sender<Result<Value, AgentError>>>::new();
            let mut replies = HashMap::<u64, agent_protocol::reply::Receiver>::new();
            loop {
                tokio::select! {
                    out = queue.recv() => {
                        let Some(out) = out else {break;};
                        if (connected.verify)().is_err() {break;}
                        if let ClientMessage::Call {id,..} = &out.message {
                            if pending.len() >= 32 {if let Some(reply)=out.reply {let _=reply.send(Err(AgentError::Unavailable));} continue;}
                            if let Some(reply) = out.reply {pending.insert(*id,reply);}
                        }
                        if wire::write(&mut write, &out.message).await.is_err() {break;}
                    }
                    message = incoming.recv() => {
                        if (connected.verify)().is_err() {break;}
                        match message {
                            Some(Ok(AgentMessage::Reply {id, response})) => {
                                if replies.contains_key(&id) { break; }
                                let Some(reply) = pending.remove(&id) else {break;};
                                let _ = reply.send(Ok(response));
                            }
                            Some(Ok(AgentMessage::Stream {stream, payload})) => {
                                if !pending.contains_key(&stream) { break; }
                                let Ok(chunk) = serde_json::from_value::<agent_protocol::reply::Chunk>(payload) else { break; };
                                if !replies.contains_key(&stream) {
                                    let reserved: usize = replies.values().map(|reply| reply.expected_bytes()).sum();
                                    if chunk.total_bytes > (128usize * 1024 * 1024).saturating_sub(reserved) as u64 { break; }
                                }
                                let Ok(cursor) = replies.entry(stream).or_default().push(chunk) else { break; };
                                // Ack only after the complete batch is retained and validated.
                                if wire::write(&mut write, &ClientMessage::Ack { stream, cursor }).await.is_err() { break; }
                            }
                            Some(Ok(AgentMessage::StreamEnd {stream, reason})) => {
                                let Some(reply) = pending.remove(&stream) else { break; };
                                let receiver = replies.remove(&stream);
                                let result = if reason == "reply_complete" {
                                    receiver.ok_or(AgentError::Unavailable).and_then(|receiver| receiver.finish().map_err(|_| AgentError::Unavailable))
                                } else { Err(AgentError::Unavailable) };
                                let invalid = reason == "reply_complete" && result.is_err()
                                    || !matches!(reason.as_str(), "reply_complete" | "cancelled" | "reply_timeout");
                                let _ = reply.send(result);
                                if invalid { break; }
                            }
                            _ => break,
                        }
                    }
                }
            }
            for (_, reply) in pending {
                let _ = reply.send(Err(AgentError::Unavailable));
            }
        });
        Arc::new(Self {
            session,
            outgoing,
            alive,
            driver: driver.abort_handle(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    struct Fake {
        attempts: AtomicUsize,
        launches: AtomicUsize,
        missing: usize,
        drop_first: AtomicBool,
    }
    impl Transport for Fake {
        fn connect(&self) -> ConnectFuture<'_> {
            Box::pin(async move {
                if self.attempts.fetch_add(1, Ordering::SeqCst) < self.missing {
                    return Err(Connect::Missing);
                }
                let disconnect = self.drop_first.swap(false, Ordering::SeqCst);
                let (client, mut server) = tokio::io::duplex(65536);
                tokio::spawn(async move {
                    let _: agent_protocol::ClientMessage = wire::read(&mut server).await.unwrap();
                    wire::write(
                        &mut server,
                        &agent_protocol::AgentMessage::Welcome {
                            protocol: 1,
                            agent_version: "0.8.1".into(),
                            generation: "fixture".into(),
                        },
                    )
                    .await
                    .unwrap();
                    while let Ok(agent_protocol::ClientMessage::Call { id, request, .. }) =
                        wire::read(&mut server).await
                    {
                        if disconnect {
                            break;
                        }
                        wire::write(
                            &mut server,
                            &agent_protocol::AgentMessage::Reply {
                                id,
                                response: request,
                            },
                        )
                        .await
                        .unwrap();
                    }
                });
                Ok(Connected {
                    stream: Box::new(client),
                    verify: Arc::new(|| Ok(())),
                    exited: Arc::new(|| Ok(false)),
                })
            })
        }
        fn launch(&self) -> LaunchFuture<'_> {
            Box::pin(async move {
                self.launches.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        }
        fn generation(&self) -> &str {
            "fixture"
        }
    }
    #[tokio::test(start_paused = true)]
    async fn launches_once_then_connects_with_backoff() {
        let transport = Arc::new(Fake {
            attempts: AtomicUsize::new(0),
            launches: AtomicUsize::new(0),
            missing: 2,
            drop_first: AtomicBool::new(false),
        });
        let client = AgentClient::with_transport("workspace", transport.clone());
        client.ensure_connected("session").await.unwrap();
        assert_eq!(transport.launches.load(Ordering::SeqCst), 1);
        assert_eq!(transport.attempts.load(Ordering::SeqCst), 3);
        let a = client.call(
            "agent.status",
            serde_json::json!({"header":{"sessionId":"session"},"n":1}),
        );
        let b = client.call(
            "agent.status",
            serde_json::json!({"header":{"sessionId":"session"},"n":2}),
        );
        let (a, b) = tokio::join!(a, b);
        assert_eq!(a.unwrap()["n"], 1);
        assert_eq!(b.unwrap()["n"], 2);
    }
    #[tokio::test(start_paused = true)]
    async fn gives_up_after_the_retry_table() {
        let transport = Arc::new(Fake {
            attempts: AtomicUsize::new(0),
            launches: AtomicUsize::new(0),
            missing: 100,
            drop_first: AtomicBool::new(false),
        });
        let client = AgentClient::with_transport("workspace", transport.clone());
        assert!(matches!(
            client.ensure_connected("session").await,
            Err(AgentError::Unavailable)
        ));
        assert_eq!(
            transport.attempts.load(Ordering::SeqCst),
            RETRY_DELAYS_MS.len() + 1
        );
        assert_eq!(transport.launches.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn disconnect_fails_the_submitted_call_once_and_only_the_next_call_reconnects() {
        let transport = Arc::new(Fake {
            attempts: AtomicUsize::new(0),
            launches: AtomicUsize::new(0),
            missing: 0,
            drop_first: AtomicBool::new(true),
        });
        let client = AgentClient::with_transport("workspace", transport.clone());
        let request = serde_json::json!({"header":{"sessionId":"session"},"mutation":"once"});
        assert_eq!(
            client.call("fixture", request.clone()).await.unwrap_err(),
            AgentError::Unavailable
        );
        assert_eq!(transport.attempts.load(Ordering::SeqCst), 1);
        assert_eq!(client.status(), "unavailable");
        assert_eq!(
            client.call("fixture", request.clone()).await.unwrap(),
            request
        );
        assert_eq!(transport.attempts.load(Ordering::SeqCst), 2);
    }
    #[tokio::test(start_paused = true)]
    async fn simultaneous_callers_share_one_launch() {
        let transport = Arc::new(Fake {
            attempts: AtomicUsize::new(0),
            launches: AtomicUsize::new(0),
            missing: 2,
            drop_first: AtomicBool::new(false),
        });
        let client = AgentClient::with_transport("workspace", transport.clone());
        let (a, b) = tokio::join!(client.ensure_connected("s"), client.ensure_connected("s"));
        assert!(Arc::ptr_eq(&a.unwrap(), &b.unwrap()));
        assert_eq!(transport.launches.load(Ordering::SeqCst), 1);
        assert_eq!(transport.attempts.load(Ordering::SeqCst), 3);
    }
    #[tokio::test]
    async fn update_shutdown_never_launches_a_missing_agent() {
        let transport = Arc::new(Fake {
            attempts: AtomicUsize::new(0),
            launches: AtomicUsize::new(0),
            missing: 100,
            drop_first: AtomicBool::new(false),
        });
        let client = AgentClient::with_transport("control-center", transport.clone());
        client.shutdown_if_running("update").await.unwrap();
        assert_eq!(transport.attempts.load(Ordering::SeqCst), 1);
        assert_eq!(transport.launches.load(Ordering::SeqCst), 0);
    }
    struct RestartFixture {
        state: Arc<AtomicUsize>,
        retired: Arc<AtomicBool>,
        stops: Arc<AtomicUsize>,
        launches: AtomicUsize,
        calls: Arc<AtomicUsize>,
        refuses: bool,
        mismatches_again: bool,
    }
    impl Transport for RestartFixture {
        fn connect(&self) -> ConnectFuture<'_> {
            Box::pin(async move {
                let current = self.state.load(Ordering::SeqCst);
                if current == 1 {
                    return Err(Connect::Missing);
                }
                let old = current == 0 || self.mismatches_again;
                let (client, mut server) = tokio::io::duplex(8192);
                let state = self.state.clone();
                let stops = self.stops.clone();
                let calls = self.calls.clone();
                let refuses = self.refuses;
                let retired = self.retired.clone();
                tokio::spawn(async move {
                    let Ok(ClientMessage::Hello { .. }) = wire::read(&mut server).await else {
                        return;
                    };
                    if old {
                        if wire::write(
                            &mut server,
                            &AgentMessage::Rejected {
                                reason: "protocol_mismatch".into(),
                            },
                        )
                        .await
                        .is_err()
                        {
                            return;
                        }
                        if matches!(
                            wire::read::<_, ClientMessage>(&mut server).await,
                            Ok(ClientMessage::Shutdown {})
                        ) {
                            stops.fetch_add(1, Ordering::SeqCst);
                            if !refuses {
                                state.store(1, Ordering::SeqCst);
                                tokio::spawn(async move {
                                    tokio::time::sleep(Duration::from_millis(200)).await;
                                    retired.store(true, Ordering::SeqCst);
                                });
                            }
                        }
                    } else {
                        if wire::write(
                            &mut server,
                            &AgentMessage::Welcome {
                                protocol: 1,
                                agent_version: "0.8.1".into(),
                                generation: "fixture".into(),
                            },
                        )
                        .await
                        .is_err()
                        {
                            return;
                        }
                        while let Ok(ClientMessage::Call { id, request, .. }) =
                            wire::read(&mut server).await
                        {
                            calls.fetch_add(1, Ordering::SeqCst);
                            if wire::write(
                                &mut server,
                                &AgentMessage::Reply {
                                    id,
                                    response: request,
                                },
                            )
                            .await
                            .is_err()
                            {
                                break;
                            }
                        }
                    }
                });
                let retired = self.retired.clone();
                Ok(Connected {
                    stream: Box::new(client),
                    verify: Arc::new(|| Ok(())),
                    exited: Arc::new(move || Ok(retired.load(Ordering::SeqCst))),
                })
            })
        }
        fn launch(&self) -> LaunchFuture<'_> {
            Box::pin(async move {
                assert_eq!(
                    self.state.load(Ordering::SeqCst),
                    1,
                    "never launch before old pipe disappearance"
                );
                assert!(
                    self.retired.load(Ordering::SeqCst),
                    "pipe disappearance alone does not prove process/writer retirement"
                );
                self.launches.fetch_add(1, Ordering::SeqCst);
                self.state.store(2, Ordering::SeqCst);
                Ok(())
            })
        }
        fn generation(&self) -> &str {
            "fixture"
        }
    }
    #[tokio::test(start_paused = true)]
    async fn protocol_mismatch_restarts_once_only_after_graceful_disappearance_and_never_replays_work(
    ) {
        for (refuses, again) in [(false, false), (true, false), (false, true)] {
            let transport = Arc::new(RestartFixture {
                state: Arc::new(AtomicUsize::new(0)),
                retired: Arc::new(AtomicBool::new(false)),
                stops: Arc::new(AtomicUsize::new(0)),
                launches: AtomicUsize::new(0),
                calls: Arc::new(AtomicUsize::new(0)),
                refuses,
                mismatches_again: again,
            });
            let client = AgentClient::with_transport("workspace", transport.clone());
            let request = serde_json::json!({"header":{"sessionId":"s"},"mutation":"once"});
            let result = client.call("fixture", request.clone()).await;
            if refuses || again {
                assert!(result.is_err());
            } else {
                assert_eq!(result.unwrap(), request);
            }
            assert_eq!(transport.stops.load(Ordering::SeqCst), 1);
            assert_eq!(
                transport.launches.load(Ordering::SeqCst),
                usize::from(!refuses)
            );
            assert_eq!(
                transport.calls.load(Ordering::SeqCst),
                usize::from(!refuses && !again)
            );
        }
    }
    #[tokio::test(start_paused = true)]
    async fn update_shutdown_waits_for_the_exact_process_after_a_protocol_mismatch_without_launching(
    ) {
        let transport = Arc::new(RestartFixture {
            state: Arc::new(AtomicUsize::new(0)),
            retired: Arc::new(AtomicBool::new(false)),
            stops: Arc::new(AtomicUsize::new(0)),
            launches: AtomicUsize::new(0),
            calls: Arc::new(AtomicUsize::new(0)),
            refuses: false,
            mismatches_again: false,
        });
        let client = AgentClient::with_transport("control-center", transport.clone());
        client.shutdown_if_running("update").await.unwrap();
        assert!(transport.retired.load(Ordering::SeqCst));
        assert_eq!(transport.stops.load(Ordering::SeqCst), 1);
        assert_eq!(transport.launches.load(Ordering::SeqCst), 0);
    }
    struct StreamTransport {
        value: Value,
        partial: bool,
    }
    impl Transport for StreamTransport {
        fn connect(&self) -> ConnectFuture<'_> {
            let value = self.value.clone();
            let partial = self.partial;
            Box::pin(async move {
                let (client, mut server) = tokio::io::duplex(128 * 1024);
                tokio::spawn(async move {
                    let _: ClientMessage = wire::read(&mut server).await.unwrap();
                    wire::write(
                        &mut server,
                        &AgentMessage::Welcome {
                            protocol: 1,
                            generation: "fixture".into(),
                            agent_version: "0.8.1".into(),
                        },
                    )
                    .await
                    .unwrap();
                    while let Ok(ClientMessage::Call { id, .. }) = wire::read(&mut server).await {
                        let mut sender = agent_protocol::reply::Sender::new(&value).unwrap();
                        while let Some(chunk) = sender.next_chunk().unwrap() {
                            if wire::write(
                                &mut server,
                                &AgentMessage::Stream {
                                    stream: id,
                                    payload: serde_json::to_value(&chunk).unwrap(),
                                },
                            )
                            .await
                            .is_err()
                            {
                                return;
                            }
                            let Ok(ClientMessage::Ack { stream, cursor }) =
                                wire::read(&mut server).await
                            else {
                                return;
                            };
                            assert_eq!(stream, id);
                            assert_eq!(cursor, chunk.offset + chunk.data.len() as u64);
                            sender.ack(cursor).unwrap();
                            if partial {
                                break;
                            }
                        }
                        if wire::write(
                            &mut server,
                            &AgentMessage::StreamEnd {
                                stream: id,
                                reason: "reply_complete".into(),
                            },
                        )
                        .await
                        .is_err()
                        {
                            return;
                        }
                    }
                });
                Ok(Connected {
                    stream: Box::new(client),
                    verify: Arc::new(|| Ok(())),
                    exited: Arc::new(|| Ok(false)),
                })
            })
        }
        fn launch(&self) -> LaunchFuture<'_> {
            Box::pin(async { panic!("existing fixture must not launch") })
        }
        fn generation(&self) -> &str {
            "fixture"
        }
    }
    #[tokio::test]
    async fn large_replies_are_acknowledged_and_only_complete_json_reaches_the_caller() {
        let value = serde_json::json!({"logs":"λ".repeat(700_000)});
        for partial in [false, true] {
            let client = AgentClient::with_transport(
                "workspace",
                Arc::new(StreamTransport {
                    value: value.clone(),
                    partial,
                }),
            );
            let result = client
                .call("fixture", serde_json::json!({"header":{"sessionId":"s"}}))
                .await;
            if partial {
                assert_eq!(result.unwrap_err(), AgentError::Unavailable);
            } else {
                assert_eq!(result.unwrap(), value);
            }
        }
    }
    #[tokio::test(start_paused = true)]
    async fn oversized_calls_fail_before_connecting_or_launching_an_owner() {
        let transport = Arc::new(Fake {
            attempts: AtomicUsize::new(0),
            launches: AtomicUsize::new(0),
            missing: 0,
            drop_first: AtomicBool::new(false),
        });
        let client = AgentClient::with_transport("workspace", transport.clone());
        let request = serde_json::json!({"header":{"sessionId":"session"},"body":"x".repeat(agent_protocol::MAX_FRAME_BYTES)});
        assert_eq!(
            client.call("fixture", request).await.unwrap_err(),
            AgentError::Rejected("request_too_large".into())
        );
        assert_eq!(transport.attempts.load(Ordering::SeqCst), 0);
        assert_eq!(transport.launches.load(Ordering::SeqCst), 0);
    }
    #[tokio::test]
    async fn portable_builds_are_unsupported() {
        assert_eq!(
            AgentClient::unsupported()
                .call("agent.status", serde_json::json!({}))
                .await
                .unwrap_err(),
            AgentError::Unsupported
        );
        assert_eq!(
            AgentClient::unavailable()
                .call("agent.status", serde_json::json!({}))
                .await
                .unwrap_err(),
            AgentError::Unavailable
        );
    }
}
