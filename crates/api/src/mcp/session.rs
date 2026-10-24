//! JSON-RPC 세션(전송 공통): `initialize` → `notifications/initialized` → 화면 입력을 요청으로, 응답을 이벤트로.
//! 서버가 보낸 요청은 `ping`만 답하고 나머지는 -32601로 거절한다(sampling·roots 등 미지원). 이벤트의 JSON은
//! 이 연결에서 푼 비밀·OAuth 토큰을 가린다. 화면이 스트림을 닫으면 전송을 닫는다(stdio는 프로세스 그룹 종료).
use super::{McpEvent, McpInput, McpList, http, stdio};
use crate::ApiError;
use crate::realtime::ws::Lines;
use crate::redact::Redactor;
use bytes::Bytes;
use devbox_protocol::stream::StreamTx;
use serde_json::{Value, json};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio::time::Instant;

const MAX_ID_BYTES: usize = 128;
const MAX_ARGUMENTS_BYTES: usize = 1024 * 1024;
const MAX_TEXT_BYTES: usize = 64 * 1024;

pub(crate) enum Prepared {
    Stdio(stdio::Plan),
    Http(http::Plan),
}

pub(crate) struct Session {
    pub(crate) transport: Prepared,
    pub(crate) tx: StreamTx,
    pub(crate) input: mpsc::Receiver<Bytes>,
    pub(crate) values: Vec<String>,
    pub(crate) _permit: tokio::sync::OwnedSemaphorePermit,
}

enum Conn {
    Stdio(Box<stdio::Child>),
    Http(Box<http::Http>),
}

impl Conn {
    async fn send(&mut self, message: &Value, tx: &StreamTx) -> Result<(), ApiError> {
        let bytes = serde_json::to_vec(message).map_err(|_| ApiError::mcp("encode"))?;
        match self {
            Conn::Stdio(c) => c.send(&bytes).await,
            Conn::Http(h) => h.send(&bytes, tx).await,
        }
    }
    async fn recv(&mut self) -> Result<Option<Vec<u8>>, ApiError> {
        match self {
            Conn::Stdio(c) => c.recv().await,
            Conn::Http(h) => h.recv().await,
        }
    }
    fn token(&self) -> Option<String> {
        match self {
            Conn::Stdio(_) => None,
            Conn::Http(h) => h.token().map(str::to_owned),
        }
    }
    async fn close(self) {
        match self {
            Conn::Stdio(c) => c.shutdown().await,
            Conn::Http(h) => h.close().await,
        }
    }
}

pub(crate) async fn write(tx: &StreamTx, event: &McpEvent) -> Result<(), crate::events::Closed> {
    let mut line = serde_json::to_vec(event).map_err(|_| crate::events::Closed)?;
    line.push(b'\n');
    tx.write(&line).await.map_err(|_| crate::events::Closed)
}

fn failed(error: &ApiError) -> McpEvent {
    McpEvent::Failed {
        error: devbox_protocol::to_wire(error),
    }
}

/// 화면 입력 → (요청 메서드, params). 잘못된 인자는 보내지 않고 그 id의 오류로 돌려준다.
fn request(input: &McpInput) -> Result<(&str, Value), (i64, &'static str)> {
    let arguments = |text: &str| -> Result<Value, (i64, &'static str)> {
        if text.len() > MAX_ARGUMENTS_BYTES {
            return Err((-32602, "arguments_too_large"));
        }
        match serde_json::from_str::<Value>(text) {
            Ok(value @ Value::Object(_)) => Ok(value),
            _ => Err((-32602, "arguments_json")),
        }
    };
    let text = |t: &str| {
        if t.len() > MAX_TEXT_BYTES {
            Err((-32602, "too_long"))
        } else {
            Ok(())
        }
    };
    Ok(match input {
        McpInput::List { what, cursor, .. } => {
            let method = match what {
                McpList::Tools => "tools/list",
                McpList::Resources => "resources/list",
                McpList::Prompts => "prompts/list",
            };
            match cursor {
                Some(c) => {
                    text(c)?;
                    (method, json!({ "cursor": c }))
                }
                None => (method, json!({})),
            }
        }
        McpInput::Call {
            name,
            arguments_json,
            ..
        } => {
            text(name)?;
            (
                "tools/call",
                json!({ "name": name, "arguments": arguments(arguments_json)? }),
            )
        }
        McpInput::Read { uri, .. } => {
            text(uri)?;
            ("resources/read", json!({ "uri": uri }))
        }
        McpInput::GetPrompt {
            name,
            arguments_json,
            ..
        } => {
            text(name)?;
            (
                "prompts/get",
                json!({ "name": name, "arguments": arguments(arguments_json)? }),
            )
        }
    })
}

fn ui_id(input: &McpInput) -> &str {
    match input {
        McpInput::List { id, .. }
        | McpInput::Call { id, .. }
        | McpInput::Read { id, .. }
        | McpInput::GetPrompt { id, .. } => id,
    }
}

struct Driver<'a> {
    tx: &'a StreamTx,
    redactor: Redactor,
    values: Vec<String>,
    next: i64,
    pending: HashMap<i64, (String, Instant)>,
}

impl Driver<'_> {
    fn refresh(&mut self, conn: &Conn) {
        if let Some(token) = conn.token()
            && !self.values.contains(&token)
        {
            self.values.push(token);
            self.redactor = Redactor::new(&self.values);
        }
    }

    async fn emit(&self, event: McpEvent) -> Result<(), ApiError> {
        write(self.tx, &event)
            .await
            .map_err(|_| ApiError::network("cancelled"))
    }

    fn shown(&self, value: &Value) -> String {
        self.redactor.text(&value.to_string())
    }

    /// 서버 메시지 하나. 응답이면 `(id, 결과)`를 돌려준다.
    async fn incoming(
        &mut self,
        conn: &mut Conn,
        bytes: &[u8],
    ) -> Result<Option<(i64, Value)>, ApiError> {
        let message: Value =
            serde_json::from_slice(bytes).map_err(|_| ApiError::mcp("protocol"))?;
        if let Some(method) = message.get("method").and_then(Value::as_str) {
            if let Some(id) = message.get("id").filter(|id| !id.is_null()) {
                let reply = if method == "ping" {
                    json!({ "jsonrpc": "2.0", "id": id, "result": {} })
                } else {
                    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "method not supported" } })
                };
                conn.send(&reply, self.tx).await?;
            }
            return Ok(None);
        }
        let Some(id) = message.get("id").and_then(Value::as_i64) else {
            return Ok(None);
        };
        Ok(Some((id, message)))
    }

    async fn initialize(&mut self, conn: &mut Conn) -> Result<(), ApiError> {
        let init = json!({
            "jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": {
                "protocolVersion": super::PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "devbox", "version": env!("CARGO_PKG_VERSION") },
            },
        });
        conn.send(&init, self.tx).await?;
        self.refresh(conn);
        let deadline = Instant::now() + super::REQUEST_TIMEOUT;
        let reply = loop {
            let next = tokio::time::timeout_at(deadline, conn.recv())
                .await
                .map_err(|_| ApiError::network("timeout"))??
                .ok_or_else(|| ApiError::mcp("closed"))?;
            if let Some((0, reply)) = self.incoming(conn, &next).await? {
                break reply;
            }
        };
        let result = reply
            .get("result")
            .ok_or_else(|| ApiError::mcp("initialize"))?;
        let protocol = result["protocolVersion"].as_str().unwrap_or("").to_owned();
        if !super::SUPPORTED_VERSIONS.contains(&protocol.as_str()) {
            return Err(ApiError::mcp("protocol_version"));
        }
        if let Conn::Http(h) = conn {
            h.set_protocol(&protocol);
        }
        let text = |v: &Value| self.redactor.text(v.as_str().unwrap_or(""));
        let ready = McpEvent::Ready {
            server_name: text(&result["serverInfo"]["name"]),
            server_version: text(&result["serverInfo"]["version"]),
            protocol_version: self.redactor.text(&protocol),
            capabilities_json: self.shown(result.get("capabilities").unwrap_or(&json!({}))),
        };
        conn.send(
            &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            self.tx,
        )
        .await?;
        self.emit(ready).await
    }

    async fn input(&mut self, conn: &mut Conn, line: &[u8]) -> Result<(), ApiError> {
        let input: McpInput =
            serde_json::from_slice(line).map_err(|_| ApiError::input("input", "parse"))?;
        let id = ui_id(&input).to_owned();
        if id.is_empty() || id.len() > MAX_ID_BYTES {
            return Err(ApiError::input("input", "id"));
        }
        let refuse = |code: i64, message: &str| McpEvent::Error {
            id: id.clone(),
            code,
            message: message.into(),
        };
        if self.pending.len() >= super::MAX_PENDING {
            return self.emit(refuse(-32000, "too_many_pending")).await;
        }
        if self.pending.values().any(|(pending, _)| *pending == id) {
            return self.emit(refuse(-32600, "duplicate_id")).await;
        }
        let (method, params) = match request(&input) {
            Ok(request) => request,
            Err((code, message)) => return self.emit(refuse(code, message)).await,
        };
        self.next += 1;
        let rpc = self.next;
        self.pending
            .insert(rpc, (id, Instant::now() + super::REQUEST_TIMEOUT));
        conn.send(
            &json!({ "jsonrpc": "2.0", "id": rpc, "method": method, "params": params }),
            self.tx,
        )
        .await?;
        self.refresh(conn);
        Ok(())
    }

    async fn response(&mut self, rpc: i64, message: Value) -> Result<(), ApiError> {
        let Some((id, _)) = self.pending.remove(&rpc) else {
            return Ok(());
        };
        let event = match (message.get("result"), message.get("error")) {
            (Some(result), _) => McpEvent::Result {
                id,
                json: self.shown(result),
            },
            (None, Some(error)) => McpEvent::Error {
                id,
                code: error["code"].as_i64().unwrap_or(-32603),
                message: self.redactor.text(error["message"].as_str().unwrap_or("")),
            },
            _ => McpEvent::Error {
                id,
                code: -32603,
                message: "invalid response".into(),
            },
        };
        self.emit(event).await
    }

    async fn drive(
        &mut self,
        conn: &mut Conn,
        input: &mut mpsc::Receiver<Bytes>,
    ) -> Result<(), ApiError> {
        self.initialize(conn).await?;
        let mut lines = Lines::default();
        let mut input_open = true;
        loop {
            let deadline = self.pending.values().map(|(_, at)| *at).min();
            tokio::select! {
                chunk = input.recv(), if input_open => {
                    let Some(chunk) = chunk else {
                        input_open = false;
                        continue;
                    };
                    for line in lines.push(&chunk)? {
                        self.input(conn, &line).await?;
                    }
                }
                next = conn.recv() => {
                    let Some(next) = next? else {
                        return self.emit(McpEvent::Closed).await;
                    };
                    if let Some((rpc, message)) = self.incoming(conn, &next).await? {
                        self.response(rpc, message).await?;
                    }
                }
                _ = tokio::time::sleep_until(deadline.unwrap_or_else(Instant::now)), if deadline.is_some() => {
                    let now = Instant::now();
                    let late: Vec<i64> = self.pending.iter().filter(|(_, (_, at))| *at <= now).map(|(k, _)| *k).collect();
                    for rpc in late {
                        if let Some((id, _)) = self.pending.remove(&rpc) {
                            self.emit(McpEvent::Error { id, code: -32001, message: "timeout".into() }).await?;
                        }
                    }
                }
            }
        }
    }
}

pub(crate) async fn run(s: Session) {
    let started = std::time::Instant::now();
    let Session {
        transport,
        tx,
        mut input,
        values,
        _permit,
    } = s;
    let closed = tx.closed();
    let conn = match transport {
        Prepared::Stdio(plan) => stdio::spawn(&plan).map(|c| Conn::Stdio(Box::new(c))),
        Prepared::Http(plan) => Ok(Conn::Http(Box::new(http::Http::new(plan)))),
    };
    let mut conn = match conn {
        Ok(conn) => conn,
        Err(error) => {
            let _ = write(&tx, &failed(&error)).await;
            tx.finish().await;
            return;
        }
    };
    let mut driver = Driver {
        tx: &tx,
        redactor: Redactor::new(&values),
        values,
        next: 0,
        pending: HashMap::new(),
    };
    let outcome = tokio::select! {
        biased;
        _ = closed.cancelled() => "cancelled",
        r = driver.drive(&mut conn, &mut input) => match r {
            Ok(()) => "closed",
            Err(error) => {
                let _ = write(&tx, &failed(&error)).await;
                "failed"
            }
        },
    };
    conn.close().await;
    tx.finish().await;
    tracing::debug!(
        method = "api.mcp_connect",
        elapsed_ms = started.elapsed().as_millis() as u64,
        outcome
    );
}
