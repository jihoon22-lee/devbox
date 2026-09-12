//! One bounded pipe listener per approved installation/product. Only native peers
//! from the pinned generation reach an owner handler. No renderer PID is trusted.
use super::{
    component_scope::CapturedScope,
    peer_identity::{PipeWitness, ProcessPeer},
};
use product_contract::transport::{self, Call, Guard, Hello, Request};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::{
    future::Future,
    os::windows::io::AsRawHandle,
    pin::Pin,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions},
    task::{JoinHandle, JoinSet},
};
use windows::Win32::Foundation::HANDLE;
type Result<T> = std::result::Result<T, &'static str>;
pub(crate) type Handler = Arc<
    dyn Fn(String, Call, u64) -> Pin<Box<dyn Future<Output = Result<Value>> + Send>> + Send + Sync,
>;
const MAX_CONNECTIONS: usize = 8;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Reply {
    request_id: String,
    value: Option<Value>,
    error: Option<String>,
}

pub(crate) struct Bus {
    task: JoinHandle<()>,
    stop: tokio::sync::watch::Sender<bool>,
}
impl Bus {
    pub(crate) async fn shutdown(&mut self) {
        let _ = self.stop.send(true);
        let _ = (&mut self.task).await;
    }
    /// The caller must hold an explicit native approval of this exact capture.
    pub(crate) fn start(
        scope: Arc<CapturedScope>,
        product: &'static str,
        handler: Handler,
    ) -> Result<Self> {
        scope.revalidate()?;
        scope.member(product)?;
        let name = endpoint(&scope, product)?;
        let server = ServerOptions::new()
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .create(&name)
            .map_err(|_| "peer_owner_already_registered")?;
        let (stop, mut stopping) = tokio::sync::watch::channel(false);
        let task = tokio::spawn(async move {
            let mut next = server;
            let mut clients = JoinSet::new();
            loop {
                // Keep the current pipe alive until its successor is created, so
                // another process cannot claim an empty registration interval.
                tokio::select! {
                    _ = stopping.changed() => { break; }
                    result = next.connect() => { if result.is_err() { break; } }
                    Some(_) = clients.join_next(), if !clients.is_empty() => { continue; }
                }
                let Ok(replacement) = ServerOptions::new()
                    .reject_remote_clients(true)
                    .create(&name)
                else {
                    break;
                };
                let connection = std::mem::replace(&mut next, replacement);
                if clients.len() >= MAX_CONNECTIONS {
                    drop(connection);
                    continue;
                }
                let scope = scope.clone();
                let handler = handler.clone();
                clients.spawn(async move {
                    // Includes identity inspection, handshake, body and dispatch.
                    let _ = tokio::time::timeout(
                        Duration::from_secs(32),
                        serve(connection, scope, product, handler),
                    )
                    .await;
                });
            }
            clients.shutdown().await;
        });
        Ok(Self { task, stop })
    }
}
impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
    }
}

fn endpoint(scope: &CapturedScope, product: &str) -> Result<String> {
    if !product_contract::installation::PRODUCTS.contains(&product) {
        return Err("peer_product_invalid");
    }
    Ok(format!(
        r"\\.\pipe\devbox-v08-{}-{}-{product}",
        scope.installation_key, scope.id
    ))
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}
async fn write<T: Serialize, W: AsyncWrite + Unpin>(pipe: &mut W, value: &T) -> Result<()> {
    let frame = transport::encode(value)?;
    pipe.write_all(&frame)
        .await
        .map_err(|_| "peer_write_failed")?;
    pipe.flush().await.map_err(|_| "peer_write_failed")
}
async fn read<T: DeserializeOwned, R: AsyncRead + Unpin>(pipe: &mut R) -> Result<T> {
    let mut length = [0u8; 4];
    pipe.read_exact(&mut length)
        .await
        .map_err(|_| "peer_read_failed")?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > transport::MAX_FRAME_BYTES {
        return Err("peer_frame_limit");
    }
    let mut body = vec![0; length];
    pipe.read_exact(&mut body)
        .await
        .map_err(|_| "peer_read_failed")?;
    transport::decode(&body)
}
async fn serve(
    mut pipe: NamedPipeServer,
    scope: Arc<CapturedScope>,
    product: &str,
    handler: Handler,
) -> Result<()> {
    let handle = PipeWitness::capture(HANDLE(pipe.as_raw_handle()))?;
    let peer = tokio::task::spawn_blocking(move || ProcessPeer::from_pipe(scope, handle, true))
        .await
        .map_err(|_| "peer_identity_unavailable")??;
    let identity = peer.wire_identity()?;
    let session = uuid::Uuid::new_v4().to_string();
    let scope = peer.scope();
    let hello = Hello {
        protocol_version: transport::VERSION,
        product: product.into(),
        installation_key: scope.installation_key.clone(),
        generation: scope.id.clone(),
        session_id: session.clone(),
    };
    write(&mut pipe, &hello).await?;
    let request: Request = tokio::time::timeout(Duration::from_secs(2), read(&mut pipe))
        .await
        .map_err(|_| "peer_handshake_timeout")??;
    let mut guard = Guard::new(identity, &session)?;
    guard.authorize(&request, now())?;
    peer.revalidate()?;
    let request_id = request.request_id;
    let remaining = request.deadline_ms.saturating_sub(now());
    if remaining == 0 {
        return Err("peer_deadline_expired");
    }
    let result = tokio::time::timeout(
        Duration::from_millis(remaining),
        handler(peer.product.clone(), request.call, request.deadline_ms),
    )
    .await
    .unwrap_or(Err("peer_deadline_expired"));
    peer.revalidate()?;
    let (value, error) = match result {
        Ok(value) => (Some(value), None),
        Err(error) => (None, Some(error.into())),
    };
    write(
        &mut pipe,
        &Reply {
            request_id,
            value,
            error,
        },
    )
    .await
}

/// A call never starts another executable. Cold start requires a separate native
/// reviewed launch boundary; connection failure remains visibly unavailable.
pub(crate) async fn call(
    scope: Arc<CapturedScope>,
    product: &str,
    call: Call,
    deadline_ms: u64,
) -> Result<Value> {
    let remaining = deadline_ms.saturating_sub(now());
    if remaining == 0 || remaining > transport::MAX_DEADLINE_MS {
        return Err("peer_deadline_expired");
    }
    tokio::time::timeout(Duration::from_millis(remaining), async {
        scope.revalidate()?;
        scope.member(product)?;
        let mut pipe = ClientOptions::new()
            .open(endpoint(&scope, product)?)
            .map_err(|_| "peer_provider_unavailable")?;
        let handle = PipeWitness::capture(HANDLE(pipe.as_raw_handle()))?;
        let own_scope = scope.clone();
        let peer =
            tokio::task::spawn_blocking(move || ProcessPeer::from_pipe(own_scope, handle, false))
                .await
                .map_err(|_| "peer_identity_unavailable")??;
        if peer.product != product {
            return Err("peer_product_mismatch");
        }
        let hello: Hello = read(&mut pipe).await?;
        if hello.protocol_version != transport::VERSION
            || hello.product != product
            || hello.installation_key != scope.installation_key
            || hello.generation != scope.id
            || !product_contract::commands::opaque_id(&hello.session_id)
        {
            return Err("peer_handshake_denied");
        }
        peer.revalidate()?;
        let request_id = uuid::Uuid::new_v4().to_string();
        write(
            &mut pipe,
            &Request {
                protocol_version: transport::VERSION,
                installation_key: scope.installation_key.clone(),
                generation: scope.id.clone(),
                session_id: hello.session_id,
                request_id: request_id.clone(),
                deadline_ms,
                call,
            },
        )
        .await?;
        let reply: Reply = read(&mut pipe).await?;
        peer.revalidate()?;
        if reply.request_id != request_id {
            return Err("peer_reply_mismatch");
        }
        match (reply.value, reply.error) {
            (Some(value), None) => Ok(value),
            (None, Some(_)) => Err("peer_owner_rejected"),
            _ => Err("peer_reply_invalid"),
        }
    })
    .await
    .map_err(|_| "peer_deadline_expired")?
}
