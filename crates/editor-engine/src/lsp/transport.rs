//! JSON-RPC 2.0 over stdio.
//!
//! LSP uses a byte-counted protocol rather than newline-delimited JSON.  The
//! reader therefore treats the header and body as separate bounded regions and
//! always measures the body in UTF-8 bytes.  No logging is written to the
//! protocol stream by this module.

use serde_json::{Map, Number, Value};
use std::collections::HashMap;
use std::fmt;
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{oneshot, watch, Mutex};

/// Maximum header size accepted by default.  This includes the terminating
/// `\r\n\r\n` bytes.
pub const DEFAULT_MAX_HEADER_BYTES: usize = 8 * 1024;
/// Maximum UTF-8 JSON body size accepted by default.
pub const DEFAULT_MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

const HEADER_TERMINATOR: &[u8] = b"\r\n\r\n";

/// Bounds applied independently to the framing header and JSON body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameLimits {
    pub max_header_bytes: usize,
    pub max_message_bytes: usize,
}

impl Default for FrameLimits {
    fn default() -> Self {
        Self {
            max_header_bytes: DEFAULT_MAX_HEADER_BYTES,
            max_message_bytes: DEFAULT_MAX_MESSAGE_BYTES,
        }
    }
}

impl FrameLimits {
    pub const fn new(max_header_bytes: usize, max_message_bytes: usize) -> Self {
        Self {
            max_header_bytes,
            max_message_bytes,
        }
    }
}

/// Errors which make the protocol stream unsafe to continue reading.
#[derive(Debug)]
pub enum TransportError {
    Io(io::Error),
    UnexpectedEof,
    HeaderTooLarge { limit: usize },
    BodyTooLarge { length: usize, limit: usize },
    InvalidHeader(String),
    InvalidUtf8,
    InvalidJson(String),
    InvalidMessage(String),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "stdio transport I/O failed: {error}"),
            Self::UnexpectedEof => f.write_str("stdio transport ended in a partial frame"),
            Self::HeaderTooLarge { limit } => {
                write!(f, "stdio protocol header exceeds {limit} bytes")
            }
            Self::BodyTooLarge { length, limit } => {
                write!(f, "stdio protocol body is {length} bytes; limit is {limit}")
            }
            Self::InvalidHeader(message) => write!(f, "invalid stdio protocol header: {message}"),
            Self::InvalidUtf8 => f.write_str("stdio protocol body is not valid UTF-8"),
            Self::InvalidJson(message) => write!(f, "invalid JSON-RPC JSON: {message}"),
            Self::InvalidMessage(message) => write!(f, "invalid JSON-RPC 2.0 message: {message}"),
        }
    }
}

impl std::error::Error for TransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for TransportError {
    fn from(error: io::Error) -> Self {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            Self::UnexpectedEof
        } else {
            Self::Io(error)
        }
    }
}

/// A JSON-RPC id as it appeared on the wire.
#[derive(Debug, Clone, PartialEq)]
pub enum RpcId {
    Number(Number),
    String(String),
    Null,
}

impl RpcId {
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Self::Number(number) => number.as_u64(),
            Self::String(_) | Self::Null => None,
        }
    }

    fn from_value(value: &Value) -> Result<Self, TransportError> {
        match value {
            Value::Number(number) => Ok(Self::Number(number.clone())),
            Value::String(value) => Ok(Self::String(value.clone())),
            Value::Null => Ok(Self::Null),
            _ => Err(TransportError::InvalidMessage(
                "JSON-RPC id must be a string, number, or null".into(),
            )),
        }
    }

    fn to_value(&self) -> Value {
        match self {
            Self::Number(number) => Value::Number(number.clone()),
            Self::String(value) => Value::String(value.clone()),
            Self::Null => Value::Null,
        }
    }
}

impl From<u64> for RpcId {
    fn from(value: u64) -> Self {
        Self::Number(Number::from(value))
    }
}

impl fmt::Display for RpcId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(number) => number.fmt(f),
            Self::String(value) => write!(f, "{value:?}"),
            Self::Null => f.write_str("null"),
        }
    }
}

/// A JSON-RPC error object returned by a server.
#[derive(Debug, Clone, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

impl RpcError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }

    fn from_value(value: &Value) -> Result<Self, TransportError> {
        let object = value.as_object().ok_or_else(|| {
            TransportError::InvalidMessage("JSON-RPC error must be an object".into())
        })?;
        let code = object.get("code").and_then(Value::as_i64).ok_or_else(|| {
            TransportError::InvalidMessage(
                "JSON-RPC error code is missing or not an integer".into(),
            )
        })?;
        let message = object
            .get("message")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                TransportError::InvalidMessage(
                    "JSON-RPC error message is missing or not a string".into(),
                )
            })?;
        Ok(Self {
            code,
            message: message.to_owned(),
            data: object.get("data").cloned(),
        })
    }

    fn to_value(&self) -> Value {
        let mut object = Map::new();
        object.insert("code".into(), Value::Number(Number::from(self.code)));
        object.insert("message".into(), Value::String(self.message.clone()));
        if let Some(data) = &self.data {
            object.insert("data".into(), data.clone());
        }
        Value::Object(object)
    }
}

/// Validated JSON-RPC 2.0 messages.  Parameters and results intentionally stay
/// as generic JSON values so this layer remains independent from LSP schemas.
#[derive(Debug, Clone, PartialEq)]
pub enum JsonRpcMessage {
    Request {
        id: RpcId,
        method: String,
        params: Option<Value>,
    },
    Notification {
        method: String,
        params: Option<Value>,
    },
    Response {
        id: RpcId,
        result: Value,
    },
    Error {
        id: RpcId,
        error: RpcError,
    },
}

impl JsonRpcMessage {
    pub fn request(id: impl Into<RpcId>, method: impl Into<String>, params: Option<Value>) -> Self {
        Self::Request {
            id: id.into(),
            method: method.into(),
            params,
        }
    }

    pub fn notification(method: impl Into<String>, params: Option<Value>) -> Self {
        Self::Notification {
            method: method.into(),
            params,
        }
    }

    pub fn response(id: impl Into<RpcId>, result: Value) -> Self {
        Self::Response {
            id: id.into(),
            result,
        }
    }

    pub fn error(id: impl Into<RpcId>, error: RpcError) -> Self {
        Self::Error {
            id: id.into(),
            error,
        }
    }

    pub fn method(&self) -> Option<&str> {
        match self {
            Self::Request { method, .. } | Self::Notification { method, .. } => Some(method),
            Self::Response { .. } | Self::Error { .. } => None,
        }
    }

    pub fn id(&self) -> Option<&RpcId> {
        match self {
            Self::Request { id, .. } | Self::Response { id, .. } | Self::Error { id, .. } => {
                Some(id)
            }
            Self::Notification { .. } => None,
        }
    }

    pub fn is_response(&self) -> bool {
        matches!(self, Self::Response { .. } | Self::Error { .. })
    }

    pub fn to_value(&self) -> Value {
        let mut object = Map::new();
        object.insert("jsonrpc".into(), Value::String("2.0".into()));
        match self {
            Self::Request { id, method, params } => {
                object.insert("id".into(), id.to_value());
                object.insert("method".into(), Value::String(method.clone()));
                insert_params(&mut object, params);
            }
            Self::Notification { method, params } => {
                object.insert("method".into(), Value::String(method.clone()));
                insert_params(&mut object, params);
            }
            Self::Response { id, result } => {
                object.insert("id".into(), id.to_value());
                object.insert("result".into(), result.clone());
            }
            Self::Error { id, error } => {
                object.insert("id".into(), id.to_value());
                object.insert("error".into(), error.to_value());
            }
        }
        Value::Object(object)
    }

    pub fn from_value(value: Value) -> Result<Self, TransportError> {
        let object = value.as_object().ok_or_else(|| {
            TransportError::InvalidMessage("JSON-RPC message must be an object".into())
        })?;
        match object.get("jsonrpc") {
            Some(Value::String(version)) if version == "2.0" => {}
            Some(_) => {
                return Err(TransportError::InvalidMessage(
                    "jsonrpc must be the string \"2.0\"".into(),
                ))
            }
            None => return Err(TransportError::InvalidMessage("jsonrpc is required".into())),
        }

        if let Some(method) = object.get("method") {
            if object.contains_key("result") || object.contains_key("error") {
                return Err(TransportError::InvalidMessage(
                    "request/notification cannot contain result or error".into(),
                ));
            }
            let method = method.as_str().ok_or_else(|| {
                TransportError::InvalidMessage("JSON-RPC method must be a string".into())
            })?;
            let params = parse_params(object.get("params"))?;
            return match object.get("id") {
                Some(id) => Ok(Self::Request {
                    id: RpcId::from_value(id)?,
                    method: method.to_owned(),
                    params,
                }),
                None => Ok(Self::Notification {
                    method: method.to_owned(),
                    params,
                }),
            };
        }

        let id = object.get("id").ok_or_else(|| {
            TransportError::InvalidMessage(
                "message must contain method or an id with result/error".into(),
            )
        })?;
        let id = RpcId::from_value(id)?;
        let result = object.get("result");
        let error = object.get("error");
        match (result, error) {
            (Some(_), Some(_)) => Err(TransportError::InvalidMessage(
                "response cannot contain both result and error".into(),
            )),
            (Some(result), None) => Ok(Self::Response {
                id,
                result: result.clone(),
            }),
            (None, Some(error)) => Ok(Self::Error {
                id,
                error: RpcError::from_value(error)?,
            }),
            (None, None) => Err(TransportError::InvalidMessage(
                "response must contain result or error".into(),
            )),
        }
    }
}

fn insert_params(object: &mut Map<String, Value>, params: &Option<Value>) {
    if let Some(params) = params {
        object.insert("params".into(), params.clone());
    }
}

fn parse_params(value: Option<&Value>) -> Result<Option<Value>, TransportError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if !value.is_array() && !value.is_object() {
        return Err(TransportError::InvalidMessage(
            "JSON-RPC params must be an array or object".into(),
        ));
    }
    Ok(Some(value.clone()))
}

/// Reads complete, validated JSON-RPC frames from an asynchronous byte stream.
pub struct JsonRpcReader<R> {
    reader: BufReader<R>,
    limits: FrameLimits,
}

impl<R: AsyncRead + Unpin> JsonRpcReader<R> {
    pub fn new(reader: R) -> Self {
        Self::with_limits(reader, FrameLimits::default())
    }

    pub fn with_limits(reader: R, limits: FrameLimits) -> Self {
        Self {
            reader: BufReader::new(reader),
            limits,
        }
    }

    pub fn limits(&self) -> FrameLimits {
        self.limits
    }

    /// Reads one body. `Ok(None)` is a clean EOF before the next header; EOF in
    /// a header or body is reported as a partial-frame error.
    pub async fn read_frame(&mut self) -> Result<Option<Vec<u8>>, TransportError> {
        let Some(header) = self.read_header().await? else {
            return Ok(None);
        };
        let length = parse_content_length(&header)?;
        if length > self.limits.max_message_bytes {
            return Err(TransportError::BodyTooLarge {
                length,
                limit: self.limits.max_message_bytes,
            });
        }
        let mut body = vec![0; length];
        self.reader
            .read_exact(&mut body)
            .await
            .map_err(TransportError::from)?;
        if std::str::from_utf8(&body).is_err() {
            return Err(TransportError::InvalidUtf8);
        }
        Ok(Some(body))
    }

    pub async fn read_message(&mut self) -> Result<Option<JsonRpcMessage>, TransportError> {
        let Some(body) = self.read_frame().await? else {
            return Ok(None);
        };
        let value: Value = serde_json::from_slice(&body)
            .map_err(|error| TransportError::InvalidJson(error.to_string()))?;
        JsonRpcMessage::from_value(value).map(Some)
    }

    async fn read_header(&mut self) -> Result<Option<Vec<u8>>, TransportError> {
        let mut header = Vec::with_capacity(128);
        loop {
            let mut byte = [0_u8; 1];
            let read = self
                .reader
                .read(&mut byte)
                .await
                .map_err(TransportError::from)?;
            if read == 0 {
                if header.is_empty() {
                    return Ok(None);
                }
                return Err(TransportError::UnexpectedEof);
            }
            header.push(byte[0]);
            if header.len() > self.limits.max_header_bytes {
                return Err(TransportError::HeaderTooLarge {
                    limit: self.limits.max_header_bytes,
                });
            }
            if header.ends_with(HEADER_TERMINATOR) {
                return Ok(Some(header));
            }
        }
    }
}

fn parse_content_length(header: &[u8]) -> Result<usize, TransportError> {
    if !header.ends_with(HEADER_TERMINATOR) {
        return Err(TransportError::InvalidHeader(
            "header must end with CRLF CRLF".into(),
        ));
    }
    let text = std::str::from_utf8(header)
        .map_err(|_| TransportError::InvalidHeader("header is not ASCII/UTF-8".into()))?;
    let lines = text[..text.len() - HEADER_TERMINATOR.len()].split("\r\n");
    let mut content_length = None;
    let mut content_type = None;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or_else(|| {
            TransportError::InvalidHeader(format!("header line has no colon: {line:?}"))
        })?;
        if name.is_empty() || !name.bytes().all(is_header_name_byte) {
            return Err(TransportError::InvalidHeader(format!(
                "invalid header name: {name:?}"
            )));
        }
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            if content_length.is_some() {
                return Err(TransportError::InvalidHeader(
                    "duplicate Content-Length".into(),
                ));
            }
            if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(TransportError::InvalidHeader(
                    "Content-Length must be a positive decimal integer".into(),
                ));
            }
            let parsed = value.parse::<usize>().map_err(|_| {
                TransportError::InvalidHeader("Content-Length does not fit in usize".into())
            })?;
            if parsed == 0 {
                return Err(TransportError::InvalidHeader(
                    "Content-Length must be greater than zero".into(),
                ));
            }
            content_length = Some(parsed);
        } else if name.eq_ignore_ascii_case("content-type") {
            if content_type.is_some() {
                return Err(TransportError::InvalidHeader(
                    "duplicate Content-Type".into(),
                ));
            }
            if !is_supported_content_type(value) {
                return Err(TransportError::InvalidHeader(format!(
                    "unsupported Content-Type: {value:?}"
                )));
            }
            content_type = Some(());
        }
        // Unknown headers are intentionally ignored.  They never become part
        // of the JSON-RPC object and therefore cannot affect routing.
    }
    content_length.ok_or_else(|| TransportError::InvalidHeader("missing Content-Length".into()))
}

fn is_header_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}

fn is_supported_content_type(value: &str) -> bool {
    let parts: Vec<_> = value.split(';').map(str::trim).collect();
    match parts.as_slice() {
        [mime] => mime.eq_ignore_ascii_case("application/vscode-jsonrpc"),
        [mime, charset] => {
            mime.eq_ignore_ascii_case("application/vscode-jsonrpc")
                && charset.eq_ignore_ascii_case("charset=utf-8")
        }
        _ => false,
    }
}

/// Writes validated JSON-RPC frames to an asynchronous byte stream.
pub struct JsonRpcWriter<W> {
    writer: W,
    limits: FrameLimits,
}

impl<W: AsyncWrite + Unpin> JsonRpcWriter<W> {
    pub fn new(writer: W) -> Self {
        Self::with_limits(writer, FrameLimits::default())
    }

    pub fn with_limits(writer: W, limits: FrameLimits) -> Self {
        Self { writer, limits }
    }

    pub fn limits(&self) -> FrameLimits {
        self.limits
    }

    pub async fn write_message(&mut self, message: &JsonRpcMessage) -> Result<(), TransportError> {
        let value = message.to_value();
        let body = serde_json::to_vec(&value)
            .map_err(|error| TransportError::InvalidJson(error.to_string()))?;
        self.write_frame(&body).await
    }

    /// Writes an already-validated JSON-RPC value.  This is useful for future
    /// schema adapters that need to retain fields this transport does not know.
    pub async fn write_jsonrpc_value(&mut self, value: &Value) -> Result<(), TransportError> {
        let message = JsonRpcMessage::from_value(value.clone())?;
        self.write_message(&message).await
    }

    pub async fn write_frame(&mut self, body: &[u8]) -> Result<(), TransportError> {
        if body.is_empty() {
            return Err(TransportError::InvalidHeader(
                "JSON-RPC body cannot be empty".into(),
            ));
        }
        if body.len() > self.limits.max_message_bytes {
            return Err(TransportError::BodyTooLarge {
                length: body.len(),
                limit: self.limits.max_message_bytes,
            });
        }
        if std::str::from_utf8(body).is_err() {
            return Err(TransportError::InvalidUtf8);
        }
        let value: Value = serde_json::from_slice(body)
            .map_err(|error| TransportError::InvalidJson(error.to_string()))?;
        // Constructors are intentionally ergonomic, so validate again at the
        // wire boundary in case a caller supplied scalar `params` by hand.
        JsonRpcMessage::from_value(value)?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        self.writer.write_all(header.as_bytes()).await?;
        self.writer.write_all(body).await?;
        self.writer.flush().await?;
        Ok(())
    }

    pub async fn shutdown(&mut self) -> Result<(), TransportError> {
        self.writer.shutdown().await.map_err(TransportError::from)
    }
}

/// Client-generated request id.  Wire ids remain ordinary JSON-RPC numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestId(u64);

impl RequestId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }
}

impl From<RequestId> for RpcId {
    fn from(value: RequestId) -> Self {
        Self::from(value.0)
    }
}

/// Why a pending request could not receive its response.
#[derive(Debug, Clone, PartialEq)]
pub enum PendingError {
    Cancelled,
    Disconnected,
    Transport(String),
}

impl fmt::Display for PendingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => f.write_str("request cancelled"),
            Self::Disconnected => f.write_str("language server disconnected"),
            Self::Transport(message) => write!(f, "language server transport failed: {message}"),
        }
    }
}

impl std::error::Error for PendingError {}

pub type PendingResult = Result<Value, PendingResultError>;

#[derive(Debug, Clone, PartialEq)]
pub enum PendingResultError {
    Remote(RpcError),
    Failure(PendingError),
}

/// Session-scoped request id allocator and response waiter map.
#[derive(Clone)]
pub struct PendingRequests {
    next_id: Arc<AtomicU64>,
    entries: Arc<Mutex<HashMap<RequestId, oneshot::Sender<PendingResult>>>>,
}

impl Default for PendingRequests {
    fn default() -> Self {
        Self::new()
    }
}

impl PendingRequests {
    pub fn new() -> Self {
        Self {
            next_id: Arc::new(AtomicU64::new(1)),
            entries: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn register(&self) -> (RequestId, oneshot::Receiver<PendingResult>) {
        let id = loop {
            let candidate = self.next_id.fetch_add(1, Ordering::Relaxed);
            if candidate != 0 {
                break RequestId::new(candidate);
            }
        };
        let (sender, receiver) = oneshot::channel();
        self.entries.lock().await.insert(id, sender);
        (id, receiver)
    }

    /// Routes a response/error and returns whether its numeric id was pending.
    pub async fn resolve(&self, message: &JsonRpcMessage) -> bool {
        let (id, result) = match message {
            JsonRpcMessage::Response { id, result } => (id, Ok(result.clone())),
            JsonRpcMessage::Error { id, error } => {
                (id, Err(PendingResultError::Remote(error.clone())))
            }
            JsonRpcMessage::Request { .. } | JsonRpcMessage::Notification { .. } => return false,
        };
        let Some(id) = id.as_u64().map(RequestId::new) else {
            return false;
        };
        let sender = self.entries.lock().await.remove(&id);
        sender.is_some_and(|sender| sender.send(result).is_ok())
    }

    pub async fn cancel(&self, id: RequestId) -> bool {
        self.entries.lock().await.remove(&id).is_some()
    }

    pub async fn fail_all(&self, error: PendingError) {
        let entries = std::mem::take(&mut *self.entries.lock().await);
        for sender in entries.into_values() {
            let _ = sender.send(Err(PendingResultError::Failure(error.clone())));
        }
    }

    pub async fn len(&self) -> usize {
        self.entries.lock().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.entries.lock().await.is_empty()
    }
}

/// Cooperative cancellation signal used by request callers.
///
/// A watch channel stores the cancellation state instead of using a
/// non-persistent notification.  This means a cancellation racing with the
/// check immediately before `changed().await` cannot be lost.
#[derive(Clone)]
pub struct RequestCancellation {
    state: watch::Sender<bool>,
}

impl RequestCancellation {
    pub fn new() -> Self {
        let (state, _) = watch::channel(false);
        Self { state }
    }

    pub fn cancel(&self) {
        self.state.send_modify(|state| *state = true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.state.borrow()
    }

    pub async fn cancelled(&self) {
        let mut state = self.state.subscribe();
        while !*state.borrow() {
            if state.changed().await.is_err() {
                return;
            }
        }
    }
}

impl Default for RequestCancellation {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::{duplex, AsyncWriteExt};

    #[tokio::test]
    async fn reads_partial_and_multiple_multibyte_frames_by_byte_length() {
        let (mut writer, reader) = duplex(256);
        let first =
            JsonRpcMessage::notification("window/logMessage", Some(json!({"text": "한글 🚀"})))
                .to_value();
        let second = JsonRpcMessage::response(7_u64, json!({"ok": true})).to_value();
        let first_body = serde_json::to_vec(&first).unwrap();
        let second_body = serde_json::to_vec(&second).unwrap();
        let bytes = [
            format!("Content-Length: {}\r\n\r\n", first_body.len()).into_bytes(),
            first_body,
            format!("Content-Length: {}\r\n\r\n", second_body.len()).into_bytes(),
            second_body,
        ]
        .concat();
        let task = tokio::spawn(async move {
            for chunk in bytes.chunks(3) {
                writer.write_all(chunk).await.unwrap();
                tokio::task::yield_now().await;
            }
        });
        let mut parser = JsonRpcReader::new(reader);
        assert_eq!(
            parser.read_message().await.unwrap(),
            Some(JsonRpcMessage::from_value(first).unwrap())
        );
        assert_eq!(
            parser.read_message().await.unwrap(),
            Some(JsonRpcMessage::from_value(second).unwrap())
        );
        task.await.unwrap();
    }

    #[tokio::test]
    async fn rejects_missing_duplicate_zero_and_invalid_content_length() {
        for header in [
            "X-Test: yes\r\n\r\n",
            "Content-Length: 1\r\ncontent-length: 1\r\n\r\n",
            "Content-Length: 0\r\n\r\n",
            "Content-Length: -1\r\n\r\n",
            "Content-Length: 1.5\r\n\r\n",
        ] {
            let (mut writer, reader) = duplex(128);
            writer.write_all(header.as_bytes()).await.unwrap();
            drop(writer);
            let mut parser = JsonRpcReader::new(reader);
            assert!(
                matches!(
                    parser.read_frame().await,
                    Err(TransportError::InvalidHeader(_))
                ),
                "{header:?}"
            );
        }
    }

    #[tokio::test]
    async fn rejects_lf_only_and_partial_frames() {
        let (mut writer, reader) = duplex(128);
        writer.write_all(b"Content-Length: 2\n\n{}").await.unwrap();
        drop(writer);
        let mut parser = JsonRpcReader::new(reader);
        assert!(matches!(
            parser.read_frame().await,
            Err(TransportError::UnexpectedEof)
        ));

        let (mut writer, reader) = duplex(128);
        writer
            .write_all(b"Content-Length: 2\r\n\r\n{")
            .await
            .unwrap();
        drop(writer);
        let mut parser = JsonRpcReader::new(reader);
        assert!(matches!(
            parser.read_frame().await,
            Err(TransportError::UnexpectedEof)
        ));
    }

    #[tokio::test]
    async fn rejects_oversized_header_and_body_before_allocation() {
        let limits = FrameLimits::new(24, 4);
        let (mut writer, reader) = duplex(128);
        writer
            .write_all(b"X-Header: 12345678901234567890\r\n\r\n")
            .await
            .unwrap();
        drop(writer);
        let mut parser = JsonRpcReader::with_limits(reader, limits);
        assert!(matches!(
            parser.read_frame().await,
            Err(TransportError::HeaderTooLarge { .. })
        ));

        let (mut writer, reader) = duplex(128);
        writer
            .write_all(b"Content-Length: 5\r\n\r\n")
            .await
            .unwrap();
        drop(writer);
        let mut parser = JsonRpcReader::with_limits(reader, limits);
        assert!(matches!(
            parser.read_frame().await,
            Err(TransportError::BodyTooLarge { .. })
        ));
    }

    #[tokio::test]
    async fn rejects_bad_utf8_json_and_jsonrpc_shape() {
        let (mut writer, reader) = duplex(128);
        writer
            .write_all(b"Content-Length: 1\r\n\r\n\xff")
            .await
            .unwrap();
        drop(writer);
        let mut parser = JsonRpcReader::new(reader);
        assert!(matches!(
            parser.read_message().await,
            Err(TransportError::InvalidUtf8)
        ));

        let (mut writer, reader) = duplex(128);
        writer
            .write_all(b"Content-Length: 2\r\n\r\n[]")
            .await
            .unwrap();
        drop(writer);
        let mut parser = JsonRpcReader::new(reader);
        assert!(matches!(
            parser.read_message().await,
            Err(TransportError::InvalidMessage(_))
        ));
    }

    #[tokio::test]
    async fn writer_uses_utf8_byte_length_and_content_type_is_checked() {
        let (writer, mut reader) = duplex(256);
        let mut writer = JsonRpcWriter::new(writer);
        writer
            .write_message(&JsonRpcMessage::notification(
                "log",
                Some(json!({"message": "한글"})),
            ))
            .await
            .unwrap();
        drop(writer);
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await.unwrap();
        let header_end = bytes
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap();
        let header = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
        let declared = header
            .strip_prefix("Content-Length: ")
            .unwrap()
            .parse::<usize>()
            .unwrap();
        assert_eq!(declared, bytes.len() - header_end - 4);
        assert!(
            declared > "{\"jsonrpc\":\"2.0\",\"method\":\"log\",\"params\":{\"message\":\"".len()
        );

        let (mut wire, reader) = duplex(256);
        wire.write_all(b"Content-Type: application/json\r\nContent-Length: 2\r\n\r\n{}")
            .await
            .unwrap();
        drop(wire);
        let mut parser = JsonRpcReader::new(reader);
        assert!(matches!(
            parser.read_frame().await,
            Err(TransportError::InvalidHeader(_))
        ));

        let (mut wire, reader) = duplex(256);
        let body = br#"{"jsonrpc":"2.0","method":"ok"}"#;
        wire.write_all(b"content-type: application/vscode-jsonrpc; charset=utf-8\r\n")
            .await
            .unwrap();
        wire.write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
            .await
            .unwrap();
        wire.write_all(body).await.unwrap();
        drop(wire);
        let mut parser = JsonRpcReader::new(reader);
        assert!(matches!(
            parser.read_message().await,
            Ok(Some(JsonRpcMessage::Notification { method, .. })) if method == "ok"
        ));
    }

    #[tokio::test]
    async fn routes_pending_results_errors_and_ignores_notifications() {
        let pending = PendingRequests::new();
        let (first, first_rx) = pending.register().await;
        let (second, second_rx) = pending.register().await;
        assert_eq!(first.value(), 1);
        assert_eq!(second.value(), 2);
        assert!(
            !pending
                .resolve(&JsonRpcMessage::notification("notify", None))
                .await
        );
        assert!(
            pending
                .resolve(&JsonRpcMessage::response(first, json!("ok")))
                .await
        );
        assert!(
            pending
                .resolve(&JsonRpcMessage::error(
                    second,
                    RpcError::new(-32000, "failed"),
                ))
                .await
        );
        assert_eq!(first_rx.await.unwrap().unwrap(), json!("ok"));
        assert!(matches!(
            second_rx.await.unwrap(),
            Err(PendingResultError::Remote(_))
        ));
        assert_eq!(pending.len().await, 0);
    }

    #[tokio::test]
    async fn cancellation_signal_survives_check_to_wait_races() {
        let already_cancelled = RequestCancellation::new();
        already_cancelled.cancel();
        assert!(already_cancelled.is_cancelled());
        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            already_cancelled.cancelled(),
        )
        .await
        .expect("pre-wait cancellation was lost");

        for _ in 0..256 {
            let cancellation = RequestCancellation::new();
            let waiter = cancellation.clone();
            let task = tokio::spawn(async move {
                waiter.cancelled().await;
            });
            // Give the waiter opportunities to run both before and after it
            // subscribes.  `watch` retains the value in either interleaving.
            tokio::task::yield_now().await;
            cancellation.cancel();
            tokio::time::timeout(std::time::Duration::from_millis(100), task)
                .await
                .expect("cancellation waiter lost a wakeup")
                .expect("cancellation waiter panicked");
        }
    }
}
