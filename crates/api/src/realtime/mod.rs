//! WebSocket·SSE 세션(`api.ws_open`·`api.sse_open`). 입력 검사는 `api.send`와 같은 치환·비밀 규칙을 쓰고,
//! 연결 뒤에는 소켓을 계속 읽어 상한 있는 큐(`buffer`)에 넣는다. 화면이 느려 큐가 차면 새 메시지를 버리고
//! 그 수를 세어 쓰기가 재개될 때 `Dropped`로 먼저 알린다(01 §5.1, 소켓 읽기를 멈추지 않는다).
pub(crate) mod buffer;
pub(crate) mod sse;
pub(crate) mod sse_parse;
pub(crate) mod ws;

use crate::events::Closed;
use crate::{ApiError, EnvSnapshot, HeaderRow, RequestOrigin, TlsOptions};
use buffer::Buffer;
use devbox_protocol::message::WireError;
use devbox_protocol::stream::StreamTx;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 동시에 열 수 있는 WebSocket·SSE 세션 수(데몬 전체).
pub const MAX_SESSIONS: usize = 32;
/// 연결(TCP·TLS·핸드셰이크 또는 SSE 응답 머리)까지의 시한. 열린 뒤의 스트림에는 시한이 없다.
pub(crate) const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WsOpen {
    /// `ws://`·`wss://` 절대 URL(변수·비밀 참조 가능).
    pub url: String,
    pub headers: Vec<HeaderRow>,
    /// `Sec-WebSocket-Protocol` 후보(RFC 9110 token, 치환하지 않음).
    pub subprotocols: Vec<String>,
    pub tls: TlsOptions,
    pub env: EnvSnapshot,
    pub origin: Option<RequestOrigin>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SseOpen {
    /// `http://`·`https://` 절대 URL.
    pub url: String,
    pub headers: Vec<HeaderRow>,
    pub tls: TlsOptions,
    pub env: EnvSnapshot,
    /// 첫 연결부터 보낼 `Last-Event-ID`(재연결은 마지막으로 받은 id를 쓴다).
    pub last_event_id: Option<String>,
    pub origin: Option<RequestOrigin>,
}

/// 화면 → 데몬 스트림 입력(NDJSON 한 줄씩).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WsInput {
    Text {
        data: String,
    },
    Binary {
        data_base64: String,
    },
    Close {
        code: Option<u16>,
        reason: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    Sent,
    Received,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum MessageKind {
    Text,
    Binary,
}

/// `api.ws_open`·`api.sse_open` 스트림의 NDJSON 한 줄. 값은 이 세션에서 푼 비밀을 가린 것이다.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RealtimeEvent {
    Open {
        protocol: Option<String>,
    },
    Message {
        direction: Direction,
        kind: MessageKind,
        data: Option<String>,
        data_base64: Option<String>,
        at_ms: i64,
    },
    SseEvent {
        id: Option<String>,
        event: Option<String>,
        data: String,
        retry_ms: Option<i64>,
        at_ms: i64,
    },
    Reconnecting {
        attempt: u32,
        last_event_id: Option<String>,
    },
    /// 화면이 느려 버린 메시지 수(쓰기가 재개될 때 큐에 남은 메시지보다 먼저 온다).
    Dropped {
        count: u64,
    },
    Closed {
        code: Option<u16>,
        reason: Option<String>,
    },
    Failed {
        error: WireError,
    },
}

/// 핸드셰이크·SSE 요청에서 사용자가 바꿀 수 없는 헤더(연결 계층이 정한다).
pub(crate) fn reserved_header(name: &str, extra: &[&str]) -> bool {
    let name = name.to_ascii_lowercase();
    matches!(
        name.as_str(),
        "host" | "connection" | "upgrade" | "content-length" | "transfer-encoding" | "te"
    ) || extra
        .iter()
        .any(|e| name == *e || (e.ends_with('*') && name.starts_with(&e[..e.len() - 1])))
}

/// 헤더 행 검사(RFC 9110, `api.send`와 같은 오류 모양) + 예약 헤더 거절.
pub(crate) fn headers(
    rows: &[HeaderRow],
    extra: &[&str],
) -> Result<Vec<(String, String)>, ApiError> {
    if rows.len() > crate::validate::MAX_ROWS {
        return Err(crate::validate::too_large(crate::validate::MAX_ROWS));
    }
    let mut out = Vec::with_capacity(rows.len());
    let mut total = 0;
    for row in rows {
        let value = crate::validate::header(row.row, &row.name, &row.value)?;
        if reserved_header(&row.name, extra) {
            return Err(ApiError::header(row.row, "name", "reserved"));
        }
        total += row.name.len() + value.len() + 4;
        out.push((row.name.clone(), value));
    }
    if total > crate::validate::MAX_HEADER_BYTES {
        return Err(crate::validate::too_large(
            crate::validate::MAX_HEADER_BYTES,
        ));
    }
    Ok(out)
}

/// 큐에서 꺼내 스트림에 쓴다. 버린 메시지가 있으면 다음 이벤트보다 먼저 `Dropped`를 쓴다.
/// 화면이 스트림을 닫거나 큐가 끝나면 멈춘다(`Closed`).
pub(crate) async fn drain(buffer: &Buffer, tx: &StreamTx) -> Result<(), Closed> {
    let closed = tx.closed();
    loop {
        let next = tokio::select! {
            biased;
            _ = closed.cancelled() => return Err(Closed),
            next = buffer.next() => next,
        };
        let Some(next) = next else {
            break;
        };
        if next.dropped > 0 {
            write(
                tx,
                &RealtimeEvent::Dropped {
                    count: next.dropped,
                },
            )
            .await?;
        }
        write(tx, &next.event).await?;
    }
    Ok(())
}

pub(crate) async fn write(tx: &StreamTx, event: &RealtimeEvent) -> Result<(), Closed> {
    let mut line = serde_json::to_vec(event).map_err(|_| Closed)?;
    line.push(b'\n');
    tx.write(&line).await.map_err(|_| Closed)
}

pub(crate) fn failed(error: &ApiError) -> RealtimeEvent {
    RealtimeEvent::Failed {
        error: devbox_protocol::to_wire(error),
    }
}
