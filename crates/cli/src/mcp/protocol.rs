//! External MCP wire types. Raw JSON is retained only for IDs and passthrough fields.
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::value::RawValue;
use std::collections::BTreeMap;

#[derive(Default)]
pub enum RequestId {
    #[default]
    Missing,
    Present(Box<RawValue>),
}
fn request_id<'de, D: Deserializer<'de>>(de: D) -> Result<RequestId, D::Error> {
    let raw = Box::<RawValue>::deserialize(de)?;
    let text = raw.get().trim();
    let integer = text.strip_prefix('-').unwrap_or(text);
    if !text.starts_with('"')
        && (integer.is_empty() || !integer.bytes().all(|c| c.is_ascii_digit()))
    {
        return Err(serde::de::Error::custom("id must be an integer or string"));
    }
    Ok(RequestId::Present(raw))
}
fn present_raw<'de, D: Deserializer<'de>>(de: D) -> Result<Option<Box<RawValue>>, D::Error> {
    Box::<RawValue>::deserialize(de).map(Some)
}
#[derive(Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    #[serde(default, deserialize_with = "request_id")]
    pub id: RequestId,
    pub method: String,
    #[serde(default, deserialize_with = "present_raw")]
    pub params: Option<Box<RawValue>>,
}

#[derive(Clone, Copy)]
pub enum ErrorKind {
    Parse,
    InvalidRequest,
    MethodNotFound,
    InvalidParams,
    Internal,
}
#[derive(Serialize)]
pub struct RpcError {
    code: i32,
    message: &'static str,
}
impl From<ErrorKind> for RpcError {
    fn from(kind: ErrorKind) -> Self {
        let (code, message) = match kind {
            ErrorKind::Parse => (-32700, "JSON 요청을 읽을 수 없습니다."),
            ErrorKind::InvalidRequest => (-32600, "JSON-RPC 요청 형식이 맞지 않습니다."),
            ErrorKind::MethodNotFound => (-32601, "이 기능을 사용할 수 없습니다."),
            ErrorKind::InvalidParams => (-32602, "요청의 입력값이 올바르지 않습니다."),
            ErrorKind::Internal => (-32603, "예상하지 못한 문제가 생겼습니다."),
        };
        Self { code, message }
    }
}
#[derive(Serialize)]
#[serde(untagged)]
pub enum Reply {
    Success {
        jsonrpc: &'static str,
        id: Box<RawValue>,
        result: Box<RawValue>,
    },
    Error {
        jsonrpc: &'static str,
        id: Option<Box<RawValue>>,
        error: RpcError,
    },
}
impl Reply {
    pub fn result(id: Box<RawValue>, result: Result<Box<RawValue>, RpcError>) -> Self {
        match result {
            Ok(result) => Self::Success {
                jsonrpc: "2.0",
                id,
                result,
            },
            Err(error) => Self::error(Some(id), error),
        }
    }
    pub fn error(id: Option<Box<RawValue>>, error: RpcError) -> Self {
        Self::Error {
            jsonrpc: "2.0",
            id,
            error,
        }
    }
}

pub type Object = BTreeMap<String, Box<RawValue>>;
fn object_field<'de, D: Deserializer<'de>, T: serde::de::DeserializeOwned>(
    de: D,
) -> Result<T, D::Error> {
    let raw = Box::<RawValue>::deserialize(de)?;
    params(Some(&raw)).map_err(|_| serde::de::Error::custom("field must be a valid object"))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Initialize {
    pub protocol_version: String,
    pub capabilities: Object,
    #[serde(deserialize_with = "object_field")]
    pub client_info: ClientInfo,
}
#[derive(Deserialize)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Initialized {
    pub protocol_version: String,
    pub capabilities: Capabilities,
    pub server_info: ServerInfo,
}
#[derive(Serialize)]
pub struct Capabilities {
    pub tools: Empty,
}
#[derive(Serialize)]
pub struct ServerInfo {
    pub name: &'static str,
    pub version: &'static str,
}
#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Empty {}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    #[serde(rename = "_meta", default)]
    pub _meta: Option<Object>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallParams {
    pub name: String,
    #[serde(default, deserialize_with = "present_raw")]
    pub arguments: Option<Box<RawValue>>,
    #[serde(rename = "_meta", default)]
    pub _meta: Option<Object>,
}

#[derive(Serialize)]
pub struct ToolList {
    pub tools: Vec<Tool>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: InputSchema,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputSchema {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub properties: BTreeMap<&'static str, PropertySchema>,
    pub required: Vec<&'static str>,
    pub additional_properties: bool,
}
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PropertySchema {
    String,
    Boolean,
    Integer { minimum: u32, maximum: u32 },
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
    pub content: [TextContent; 1],
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub is_error: bool,
}
#[derive(Serialize)]
pub struct TextContent {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub text: String,
}
impl ToolResult {
    pub fn error(message: &str) -> Self {
        Self::text(message.into(), true)
    }
    pub fn json(result: impl Serialize) -> Result<Self, RpcError> {
        Ok(Self::text(
            serde_json::to_string(&result).map_err(|_| RpcError::from(ErrorKind::Internal))?,
            false,
        ))
    }
    fn text(text: String, is_error: bool) -> Self {
        Self {
            content: [TextContent { kind: "text", text }],
            is_error,
        }
    }
}

pub fn params<T: serde::de::DeserializeOwned>(raw: Option<&RawValue>) -> Result<T, RpcError> {
    let raw = raw.ok_or_else(|| RpcError::from(ErrorKind::InvalidParams))?;
    // Serde structs accept positional arrays; MCP params/arguments must be objects.
    if !raw.get().trim_start().starts_with('{') {
        return Err(ErrorKind::InvalidParams.into());
    }
    serde_json::from_str(raw.get()).map_err(|_| ErrorKind::InvalidParams.into())
}
pub fn encode(result: impl Serialize) -> Result<Box<RawValue>, RpcError> {
    serde_json::value::to_raw_value(&result).map_err(|_| ErrorKind::Internal.into())
}
