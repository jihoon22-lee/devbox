//! Development-only loopback WebSocket transport.
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use bytes::BytesMut;
use devbox_protocol::frame::{FrameCodec, HEADER_LEN, MAX_JSON_BYTES};
use futures::{SinkExt, StreamExt};
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::codec::{Decoder, Encoder, Framed};

struct Gateway {
    token: String,
    origin: HeaderValue,
    socket: PathBuf,
}
#[derive(serde::Deserialize)]
struct QueryToken {
    token: Option<String>,
}

pub async fn run(port: u16, origin: String, token: Option<String>) -> anyhow::Result<()> {
    anyhow::ensure!(!origin.trim().is_empty(), "origin must not be empty");
    let origin = HeaderValue::from_str(&origin)?;
    let token = token.unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
    anyhow::ensure!(!token.trim().is_empty(), "token must not be empty");
    let state = Arc::new(Gateway {
        token,
        origin,
        socket: crate::paths()?.socket,
    });
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    let escaped_token: String = state
        .token
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                char::from(b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    println!(
        "dev-gateway ws://{}/ws?token={}",
        listener.local_addr()?,
        escaped_token
    );
    let app = axum::Router::new()
        .route("/ws", get(upgrade))
        .with_state(state);
    axum::serve(listener, app).await?;
    Ok(())
}
async fn upgrade(
    ws: WebSocketUpgrade,
    Query(query): Query<QueryToken>,
    headers: HeaderMap,
    State(state): State<Arc<Gateway>>,
) -> Response {
    if query.token.as_deref() != Some(state.token.as_str()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let mut origins = headers.get_all("origin").iter();
    if origins.next() != Some(&state.origin) || origins.next().is_some() {
        return StatusCode::FORBIDDEN.into_response();
    }
    ws.max_message_size(HEADER_LEN + MAX_JSON_BYTES)
        .max_frame_size(HEADER_LEN + MAX_JSON_BYTES)
        .on_upgrade(move |ws| async move {
            if let Err(error) = relay(ws, &state.socket).await {
                tracing::debug!(%error, "gateway relay ended");
            }
        })
}
async fn relay(ws: WebSocket, socket: &std::path::Path) -> anyhow::Result<()> {
    let unix = tokio::net::UnixStream::connect(socket).await?;
    let (mut daemon_tx, mut daemon_rx) = Framed::new(unix, FrameCodec::default()).split();
    let (mut ws_tx, mut ws_rx) = ws.split();
    let (flush_tx, mut flush_rx) = tokio::sync::mpsc::channel::<()>(1);
    let up = async {
        while let Some(message) = ws_rx.next().await {
            match message? {
                Message::Binary(raw) => {
                    let mut bytes = BytesMut::from(raw.as_ref());
                    let frame = FrameCodec::default()
                        .decode(&mut bytes)?
                        .ok_or_else(|| anyhow::anyhow!("incomplete WebSocket frame"))?;
                    anyhow::ensure!(
                        bytes.is_empty(),
                        "WebSocket message contains trailing bytes"
                    );
                    daemon_tx.send(frame).await?;
                }
                Message::Ping(_) | Message::Pong(_) => {
                    let _ = flush_tx.try_send(());
                }
                Message::Close(_) => break,
                Message::Text(_) => anyhow::bail!("unexpected text message"),
            }
        }
        anyhow::Ok(())
    };
    let down = async {
        loop {
            tokio::select! {
                Some(()) = flush_rx.recv() => ws_tx.flush().await?,
                frame = daemon_rx.next() => match frame {
                    Some(frame) => {
                        let mut bytes = BytesMut::new();
                        FrameCodec::default().encode(frame?, &mut bytes)?;
                        ws_tx.send(Message::Binary(bytes.freeze())).await?;
                    }
                    None => break,
                }
            }
        }
        anyhow::Ok(())
    };
    // Keep both directions polled during backpressure. Reading a Ping queues
    // tungstenite's automatic Pong; the sink owner flushes it independently.
    let result = {
        tokio::pin!(up, down);
        tokio::select! { r = &mut up => r, r = &mut down => r }
    };
    if result.is_ok() {
        ws_tx.close().await?;
    }
    result
}
