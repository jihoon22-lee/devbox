//! Pure Model Context Protocol message, version, header, and result validation.
//!
//! This module deliberately owns no network, process, filesystem, secret, or
//! Tauri state. The HTTP command layer may only execute messages constructed
//! and validated here.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const MODERN_VERSION: &str = "2026-07-28";
pub const LEGACY_VERSION: &str = "2025-11-25";
pub const CLIENT_NAME: &str = "devbox-api-playground";
pub const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub const MAX_REQUEST_BYTES: usize = 1024 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_JSON_DEPTH: usize = 64;
pub const MAX_JSON_NODES: usize = 20_000;
pub const MAX_JSON_KEY_BYTES: usize = 4 * 1024;
pub const MAX_JSON_STRING_BYTES: usize = 256 * 1024;
pub const MAX_NAME_BYTES: usize = 1024;
pub const MAX_URI_BYTES: usize = 8 * 1024;
pub const MAX_CURSOR_BYTES: usize = 4 * 1024;
pub const MAX_LIST_ITEMS: usize = 10_000;
pub const MAX_SCHEMA_DEPTH: usize = 32;
pub const MAX_SCHEMA_NODES: usize = 10_000;
pub const MAX_SCHEMA_PROPERTIES: usize = 2_000;
pub const MAX_SCHEMA_ARRAY_ITEMS: usize = 100;
pub const MAX_SCHEMA_ENUM_VALUES: usize = 1_000;
pub const MAX_DERIVED_PARAMETER_HEADERS: usize = 100;
pub const MAX_DERIVED_HEADER_VALUE_BYTES: usize = 64 * 1024;
pub const MAX_DERIVED_HEADER_TOTAL_BYTES: usize = 128 * 1024;

pub const INVALID_PROFILE: &str = "mcp_invalid_profile";
pub const REQUEST_TOO_LARGE: &str = "mcp_request_too_large";
pub const RESPONSE_TOO_LARGE: &str = "mcp_response_too_large";
pub const MESSAGE_INVALID: &str = "mcp_message_invalid";
pub const VERSION_UNSUPPORTED: &str = "mcp_version_unsupported";
pub const CAPABILITY_UNAVAILABLE: &str = "mcp_capability_unavailable";
pub const SERVER_ERROR: &str = "mcp_server_error";
pub const CURSOR_INVALID: &str = "mcp_cursor_invalid";
pub const SCHEMA_UNSUPPORTED: &str = "mcp_schema_unsupported";

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EraPreference {
    Auto,
    Modern,
    Legacy,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Era {
    Modern,
    Legacy,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ServerProjection {
    pub era: Era,
    pub protocol_version: String,
    pub server_name: String,
    pub server_version: String,
    pub capabilities: Value,
    pub supported_versions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RpcMessage {
    Response {
        id: String,
        result: Result<Value, RpcError>,
    },
    Notification {
        method: String,
        params: Option<Value>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderProjection {
    pub name: String,
    pub value: String,
}

#[derive(Default)]
struct JsonBudget {
    nodes: usize,
}

#[derive(Default)]
struct SchemaBudget {
    nodes: usize,
    properties: usize,
}

pub fn validate_request_id(value: &str) -> Result<(), &'static str> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(MESSAGE_INVALID);
    }
    Ok(())
}

pub fn validate_operation(method: &str, params: &Value) -> Result<(), &'static str> {
    if !matches!(
        method,
        "server/discover"
            | "tools/list"
            | "tools/call"
            | "resources/list"
            | "resources/templates/list"
            | "resources/read"
            | "prompts/list"
            | "prompts/get"
    ) {
        return Err(CAPABILITY_UNAVAILABLE);
    }
    validate_json(params, MAX_REQUEST_BYTES)?;
    let object = params.as_object().ok_or(MESSAGE_INVALID)?;
    if object.contains_key("_meta") {
        return Err(MESSAGE_INVALID);
    }
    match method {
        "tools/list" | "resources/list" | "resources/templates/list" | "prompts/list" => {
            validate_parameter_keys(object, &["cursor"])?;
            validate_cursor(object.get("cursor"))
        }
        "tools/call" => {
            validate_parameter_keys(object, &["name", "arguments"])?;
            validate_named(object, "name", MAX_NAME_BYTES)?;
            let arguments = object.get("arguments").ok_or(MESSAGE_INVALID)?;
            if !arguments.is_object() {
                return Err(MESSAGE_INVALID);
            }
            Ok(())
        }
        "resources/read" => {
            validate_parameter_keys(object, &["uri"])?;
            validate_named(object, "uri", MAX_URI_BYTES)
        }
        "prompts/get" => {
            validate_parameter_keys(object, &["name", "arguments"])?;
            validate_named(object, "name", MAX_NAME_BYTES)?;
            if let Some(arguments) = object.get("arguments") {
                let values = arguments.as_object().ok_or(MESSAGE_INVALID)?;
                if values.len() > MAX_SCHEMA_PROPERTIES
                    || values.iter().any(|(name, value)| {
                        name.is_empty()
                            || name.len() > MAX_NAME_BYTES
                            || name.chars().any(char::is_control)
                            || !value.is_string()
                    })
                {
                    return Err(MESSAGE_INVALID);
                }
            }
            Ok(())
        }
        "server/discover" => {
            if object.is_empty() {
                Ok(())
            } else {
                Err(MESSAGE_INVALID)
            }
        }
        _ => Err(CAPABILITY_UNAVAILABLE),
    }
}

fn validate_parameter_keys(
    object: &Map<String, Value>,
    allowed: &[&str],
) -> Result<(), &'static str> {
    if object
        .keys()
        .any(|key| !allowed.iter().any(|allowed| key == allowed))
    {
        Err(MESSAGE_INVALID)
    } else {
        Ok(())
    }
}

fn validate_cursor(value: Option<&Value>) -> Result<(), &'static str> {
    let Some(value) = value else {
        return Ok(());
    };
    let cursor = value.as_str().ok_or(CURSOR_INVALID)?;
    if cursor.is_empty() || cursor.len() > MAX_CURSOR_BYTES || cursor.chars().any(char::is_control)
    {
        return Err(CURSOR_INVALID);
    }
    Ok(())
}

fn validate_named(
    object: &Map<String, Value>,
    key: &str,
    max_bytes: usize,
) -> Result<(), &'static str> {
    let value = object
        .get(key)
        .and_then(Value::as_str)
        .ok_or(MESSAGE_INVALID)?;
    if value.is_empty() || value.len() > max_bytes || value.chars().any(char::is_control) {
        return Err(MESSAGE_INVALID);
    }
    Ok(())
}

pub fn build_modern_request(id: &str, method: &str, params: Value) -> Result<Value, &'static str> {
    validate_request_id(id)?;
    validate_operation(method, &params)?;
    let mut params = params.as_object().cloned().ok_or(MESSAGE_INVALID)?;
    params.insert("_meta".into(), modern_meta());
    let request = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    validate_serialized_size(&request, MAX_REQUEST_BYTES)?;
    Ok(request)
}

pub fn build_legacy_request(id: &str, method: &str, params: Value) -> Result<Value, &'static str> {
    validate_request_id(id)?;
    validate_operation(method, &params)?;
    if method == "server/discover" {
        return Err(CAPABILITY_UNAVAILABLE);
    }
    let request = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    validate_serialized_size(&request, MAX_REQUEST_BYTES)?;
    Ok(request)
}

pub fn build_legacy_initialize(id: &str) -> Result<Value, &'static str> {
    validate_request_id(id)?;
    Ok(json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "initialize",
        "params": {
            "protocolVersion": LEGACY_VERSION,
            "capabilities": {},
            "clientInfo": {
                "name": CLIENT_NAME,
                "version": CLIENT_VERSION,
            }
        }
    }))
}

pub fn build_legacy_initialized() -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized",
        "params": {}
    })
}

pub fn build_legacy_cancelled(request_id: &str) -> Result<Value, &'static str> {
    validate_request_id(request_id)?;
    Ok(json!({
        "jsonrpc": "2.0",
        "method": "notifications/cancelled",
        "params": { "requestId": request_id, "reason": "Cancelled by user" }
    }))
}

fn modern_meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": MODERN_VERSION,
        "io.modelcontextprotocol/clientInfo": {
            "name": CLIENT_NAME,
            "version": CLIENT_VERSION,
        },
        "io.modelcontextprotocol/clientCapabilities": {},
    })
}

pub fn derived_headers(
    era: Era,
    protocol_version: &str,
    method: &str,
    params: &Value,
    tool_schema: Option<&Value>,
) -> Result<Vec<HeaderProjection>, &'static str> {
    let object = params.as_object().ok_or(MESSAGE_INVALID)?;
    let mut headers = vec![HeaderProjection {
        name: "MCP-Protocol-Version".into(),
        value: protocol_version.to_string(),
    }];
    if era == Era::Modern {
        headers.push(HeaderProjection {
            name: "Mcp-Method".into(),
            value: method.to_string(),
        });
        if let Some(name) = match method {
            "tools/call" | "prompts/get" => object.get("name").and_then(Value::as_str),
            "resources/read" => object.get("uri").and_then(Value::as_str),
            _ => None,
        } {
            headers.push(HeaderProjection {
                name: "Mcp-Name".into(),
                value: encode_header_value(name),
            });
        }
        if method == "tools/call" {
            let schema = tool_schema.ok_or(SCHEMA_UNSUPPORTED)?;
            let arguments = object
                .get("arguments")
                .and_then(Value::as_object)
                .ok_or(MESSAGE_INVALID)?;
            headers.extend(derive_tool_parameter_headers(schema, arguments)?);
        }
    }
    Ok(headers)
}

pub fn encode_header_value(value: &str) -> String {
    let plain = !value.is_empty()
        && value.trim() == value
        && value.bytes().all(|byte| matches!(byte, 0x20..=0x7e))
        && !(value.starts_with("=?base64?") && value.ends_with("?="));
    if plain {
        value.to_string()
    } else {
        format!("=?base64?{}?=", BASE64.encode(value.as_bytes()))
    }
}

fn derive_tool_parameter_headers(
    schema: &Value,
    arguments: &Map<String, Value>,
) -> Result<Vec<HeaderProjection>, &'static str> {
    validate_tool_schema(schema)?;
    let mut annotations = Vec::<(Vec<String>, String, PrimitiveKind)>::new();
    collect_header_annotations(schema, &mut Vec::new(), &mut annotations, 0)?;
    let mut headers = Vec::new();
    let mut header_bytes = 0usize;
    for (path, name, kind) in annotations {
        let Some(value) = value_at_path(arguments, &path) else {
            continue;
        };
        if value.is_null() {
            continue;
        }
        let text = match (kind, value) {
            (PrimitiveKind::String, Value::String(value)) => value.clone(),
            (PrimitiveKind::Integer, Value::Number(value))
                if value
                    .as_i64()
                    .is_some_and(|value| value.unsigned_abs() <= 9_007_199_254_740_991) =>
            {
                value.to_string()
            }
            (PrimitiveKind::Boolean, Value::Bool(value)) => value.to_string(),
            _ => return Err(MESSAGE_INVALID),
        };
        let name = format!("Mcp-Param-{name}");
        let value = encode_header_value(&text);
        if value.len() > MAX_DERIVED_HEADER_VALUE_BYTES {
            return Err(REQUEST_TOO_LARGE);
        }
        header_bytes = header_bytes
            .checked_add(name.len())
            .and_then(|bytes| bytes.checked_add(value.len()))
            .and_then(|bytes| bytes.checked_add(4))
            .ok_or(REQUEST_TOO_LARGE)?;
        if header_bytes > MAX_DERIVED_HEADER_TOTAL_BYTES {
            return Err(REQUEST_TOO_LARGE);
        }
        headers.push(HeaderProjection { name, value });
    }
    Ok(headers)
}

fn value_at_path<'a>(root: &'a Map<String, Value>, path: &[String]) -> Option<&'a Value> {
    let (first, rest) = path.split_first()?;
    let mut current = root.get(first)?;
    for part in rest {
        current = current.as_object()?.get(part)?;
    }
    Some(current)
}

#[derive(Debug, Clone, Copy)]
enum PrimitiveKind {
    String,
    Integer,
    Boolean,
}

fn collect_header_annotations(
    schema: &Value,
    path: &mut Vec<String>,
    output: &mut Vec<(Vec<String>, String, PrimitiveKind)>,
    depth: usize,
) -> Result<(), &'static str> {
    if depth > MAX_SCHEMA_DEPTH {
        return Err(SCHEMA_UNSUPPORTED);
    }
    let object = schema.as_object().ok_or(SCHEMA_UNSUPPORTED)?;
    if let Some(name) = object.get("x-mcp-header") {
        let name = name.as_str().ok_or(SCHEMA_UNSUPPORTED)?;
        if path.is_empty() || !valid_header_token(name) {
            return Err(SCHEMA_UNSUPPORTED);
        }
        let kind = match object.get("type").and_then(Value::as_str) {
            Some("string") => PrimitiveKind::String,
            Some("integer") => PrimitiveKind::Integer,
            Some("boolean") => PrimitiveKind::Boolean,
            _ => return Err(SCHEMA_UNSUPPORTED),
        };
        if output.len() >= MAX_DERIVED_PARAMETER_HEADERS {
            return Err(SCHEMA_UNSUPPORTED);
        }
        output.push((path.clone(), name.to_string(), kind));
    }
    if let Some(properties) = object.get("properties") {
        let properties = properties.as_object().ok_or(SCHEMA_UNSUPPORTED)?;
        for (name, child) in properties {
            path.push(name.clone());
            collect_header_annotations(child, path, output, depth + 1)?;
            path.pop();
        }
    }
    for (key, child) in object {
        if key != "properties" && key != "x-mcp-header" && contains_header_annotation(child) {
            return Err(SCHEMA_UNSUPPORTED);
        }
    }
    if depth == 0 {
        let mut seen = BTreeSet::new();
        for (_, name, _) in output.iter() {
            if !seen.insert(name.to_ascii_lowercase()) {
                return Err(SCHEMA_UNSUPPORTED);
            }
        }
    }
    Ok(())
}

fn contains_header_annotation(value: &Value) -> bool {
    match value {
        Value::Object(object) => {
            object.contains_key("x-mcp-header") || object.values().any(contains_header_annotation)
        }
        Value::Array(values) => values.iter().any(contains_header_annotation),
        _ => false,
    }
}

fn valid_header_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

pub fn parse_rpc_message(
    value: Value,
    expected_id: &str,
    era: Era,
) -> Result<RpcMessage, &'static str> {
    validate_json(&value, MAX_RESPONSE_BYTES)?;
    let object = value.as_object().ok_or(MESSAGE_INVALID)?;
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(MESSAGE_INVALID);
    }
    let id = object.get("id");
    let method = object.get("method");
    if let Some(method) = method {
        if id.is_some() {
            return Err(CAPABILITY_UNAVAILABLE);
        }
        if object.contains_key("result") || object.contains_key("error") {
            return Err(MESSAGE_INVALID);
        }
        let method = method.as_str().ok_or(MESSAGE_INVALID)?;
        if method.is_empty() || method.len() > 256 || method.chars().any(char::is_control) {
            return Err(MESSAGE_INVALID);
        }
        let params = object.get("params").cloned();
        if params.as_ref().is_some_and(|params| !params.is_object()) {
            return Err(MESSAGE_INVALID);
        }
        return Ok(RpcMessage::Notification {
            method: method.to_string(),
            params,
        });
    }
    if object.contains_key("params") {
        return Err(MESSAGE_INVALID);
    }
    match (object.get("result"), object.get("error")) {
        (Some(result), None) => {
            let id = id.and_then(rpc_id_string).ok_or(MESSAGE_INVALID)?;
            if id != expected_id {
                return Err(MESSAGE_INVALID);
            }
            if era == Era::Modern {
                match result.get("resultType").and_then(Value::as_str) {
                    Some("complete") => {}
                    Some("input_required") => return Err(CAPABILITY_UNAVAILABLE),
                    _ => return Err(MESSAGE_INVALID),
                }
            }
            Ok(RpcMessage::Response {
                id,
                result: Ok(result.clone()),
            })
        }
        (None, Some(error)) => {
            let error = parse_rpc_error(error)?;
            let id = match id.and_then(rpc_id_string) {
                Some(id) if id == expected_id => id,
                None if era == Era::Modern
                    && (is_recognized_modern_error(&error)
                        || is_modern_method_not_found(&error)) =>
                {
                    expected_id.to_string()
                }
                _ => return Err(MESSAGE_INVALID),
            };
            Ok(RpcMessage::Response {
                id,
                result: Err(error),
            })
        }
        _ => Err(MESSAGE_INVALID),
    }
}

fn rpc_id_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) if value.len() <= 128 => Some(value.clone()),
        Value::Number(value) if value.is_i64() || value.is_u64() => Some(value.to_string()),
        _ => None,
    }
}

fn parse_rpc_error(value: &Value) -> Result<RpcError, &'static str> {
    let object = value.as_object().ok_or(MESSAGE_INVALID)?;
    let code = object
        .get("code")
        .and_then(Value::as_i64)
        .ok_or(MESSAGE_INVALID)?;
    let message = object
        .get("message")
        .and_then(Value::as_str)
        .ok_or(MESSAGE_INVALID)?;
    if message.len() > 4 * 1024 || message.chars().any(|character| character == '\0') {
        return Err(MESSAGE_INVALID);
    }
    Ok(RpcError {
        code,
        message: message.to_string(),
        data: object.get("data").cloned(),
    })
}

pub fn is_recognized_modern_error(error: &RpcError) -> bool {
    matches!(error.code, -32022..=-32020)
}

pub fn is_modern_method_not_found(error: &RpcError) -> bool {
    error.code == -32601
}

pub fn has_modern_error_evidence(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    matches!(
        object
            .get("error")
            .and_then(Value::as_object)
            .and_then(|error| error.get("code"))
            .and_then(Value::as_i64),
        Some(-32022..=-32020 | -32601)
    )
}

pub fn supported_versions_from_error(error: &RpcError) -> Vec<String> {
    if error.code != -32022 {
        return Vec::new();
    }
    let Some(data) = error.data.as_ref() else {
        return Vec::new();
    };
    if data.get("requested").and_then(Value::as_str) != Some(MODERN_VERSION) {
        return Vec::new();
    }
    let Some(versions) = data.get("supported").and_then(Value::as_array) else {
        return Vec::new();
    };
    if versions.is_empty()
        || versions.len() > 16
        || versions.iter().any(|value| {
            value
                .as_str()
                .is_none_or(|version| !valid_protocol_version(version))
        })
    {
        return Vec::new();
    }
    let versions = versions
        .iter()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if versions.iter().collect::<BTreeSet<_>>().len() != versions.len() {
        return Vec::new();
    }
    versions
}

pub fn project_discover(result: &Value) -> Result<ServerProjection, &'static str> {
    let object = result.as_object().ok_or(MESSAGE_INVALID)?;
    if object.get("resultType").and_then(Value::as_str) != Some("complete") {
        return Err(MESSAGE_INVALID);
    }
    validate_modern_cache_fields(object)?;
    if object
        .get("instructions")
        .is_some_and(|value| !value.is_string())
    {
        return Err(MESSAGE_INVALID);
    }
    let supported_versions = object
        .get("supportedVersions")
        .and_then(Value::as_array)
        .ok_or(MESSAGE_INVALID)?
        .iter()
        .map(|value| value.as_str().ok_or(MESSAGE_INVALID))
        .collect::<Result<Vec<_>, _>>()?;
    let unique_versions = supported_versions.iter().copied().collect::<BTreeSet<_>>();
    if supported_versions.is_empty()
        || supported_versions.len() > 16
        || unique_versions.len() != supported_versions.len()
        || supported_versions
            .iter()
            .any(|version| !valid_protocol_version(version))
        || !supported_versions.contains(&MODERN_VERSION)
    {
        return Err(VERSION_UNSUPPORTED);
    }
    let capabilities = bounded_capabilities(object.get("capabilities"))?;
    let server_info = match object.get("_meta") {
        None => None,
        Some(meta) => Some(
            meta.as_object()
                .ok_or(MESSAGE_INVALID)?
                .get("io.modelcontextprotocol/serverInfo"),
        )
        .flatten(),
    };
    let (server_name, server_version) = project_server_info(server_info, false)?;
    Ok(ServerProjection {
        era: Era::Modern,
        protocol_version: MODERN_VERSION.to_string(),
        server_name,
        server_version,
        capabilities,
        supported_versions: vec![MODERN_VERSION.to_string()],
    })
}

pub fn project_legacy_initialize(result: &Value) -> Result<ServerProjection, &'static str> {
    let object = result.as_object().ok_or(MESSAGE_INVALID)?;
    let version = object
        .get("protocolVersion")
        .and_then(Value::as_str)
        .ok_or(MESSAGE_INVALID)?;
    if version != LEGACY_VERSION {
        return Err(VERSION_UNSUPPORTED);
    }
    let capabilities = bounded_capabilities(object.get("capabilities"))?;
    let (server_name, server_version) = project_server_info(object.get("serverInfo"), true)?;
    Ok(ServerProjection {
        era: Era::Legacy,
        protocol_version: version.to_string(),
        server_name,
        server_version,
        capabilities,
        supported_versions: vec![version.to_string()],
    })
}

fn bounded_capabilities(value: Option<&Value>) -> Result<Value, &'static str> {
    let value = value.ok_or(MESSAGE_INVALID)?;
    let object = value.as_object().ok_or(MESSAGE_INVALID)?;
    if ["tools", "resources", "prompts"].into_iter().any(|key| {
        object
            .get(key)
            .is_some_and(|capability| !capability.is_object())
    }) || object
        .get("extensions")
        .is_some_and(|extensions| !extensions.is_object())
    {
        return Err(MESSAGE_INVALID);
    }
    validate_json(value, 256 * 1024)?;
    Ok(value.clone())
}

fn validate_modern_cache_fields(object: &Map<String, Value>) -> Result<(), &'static str> {
    if !object
        .get("ttlMs")
        .and_then(Value::as_f64)
        .is_some_and(|value| value.is_finite() && value >= 0.0)
        || !matches!(
            object.get("cacheScope").and_then(Value::as_str),
            Some("public" | "private")
        )
    {
        return Err(MESSAGE_INVALID);
    }
    Ok(())
}

fn project_server_info(
    value: Option<&Value>,
    required: bool,
) -> Result<(String, String), &'static str> {
    let Some(value) = value else {
        if required {
            return Err(MESSAGE_INVALID);
        }
        return Ok((String::new(), String::new()));
    };
    let object = value.as_object().ok_or(MESSAGE_INVALID)?;
    let name = bounded_display(object.get("name"), true)?;
    let version = bounded_display(object.get("version"), true)?;
    Ok((name, version))
}

fn bounded_display(value: Option<&Value>, required: bool) -> Result<String, &'static str> {
    let Some(value) = value else {
        if required {
            return Err(MESSAGE_INVALID);
        }
        return Ok(String::new());
    };
    let value = value.as_str().ok_or(MESSAGE_INVALID)?;
    if value.len() > 512 || value.chars().any(char::is_control) {
        return Err(MESSAGE_INVALID);
    }
    Ok(value.to_string())
}

fn valid_protocol_version(value: &str) -> bool {
    value.len() == 10
        && value.bytes().enumerate().all(|(index, byte)| match index {
            4 | 7 => byte == b'-',
            _ => byte.is_ascii_digit(),
        })
}

pub fn validate_operation_result(
    method: &str,
    result: &Value,
    era: Era,
) -> Result<(), &'static str> {
    validate_json(result, MAX_RESPONSE_BYTES)?;
    let object = result.as_object().ok_or(MESSAGE_INVALID)?;
    if era == Era::Modern && object.get("resultType").and_then(Value::as_str) != Some("complete") {
        return Err(MESSAGE_INVALID);
    }
    let (list_key, identity_key) = match method {
        "tools/list" => (Some("tools"), Some("name")),
        "resources/list" => (Some("resources"), Some("uri")),
        "resources/templates/list" => (Some("resourceTemplates"), Some("uriTemplate")),
        "prompts/list" => (Some("prompts"), Some("name")),
        "tools/call" | "resources/read" | "prompts/get" => (None, None),
        _ => return Err(CAPABILITY_UNAVAILABLE),
    };
    if let Some(list_key) = list_key {
        if era == Era::Modern {
            validate_modern_cache_fields(object)?;
        }
        let items = object
            .get(list_key)
            .and_then(Value::as_array)
            .ok_or(MESSAGE_INVALID)?;
        if items.len() > MAX_LIST_ITEMS {
            return Err(RESPONSE_TOO_LARGE);
        }
        let mut identities = BTreeSet::new();
        for item in items {
            let item = item.as_object().ok_or(MESSAGE_INVALID)?;
            let identity = item
                .get(identity_key.unwrap_or_default())
                .and_then(Value::as_str)
                .ok_or(MESSAGE_INVALID)?;
            let max_identity_bytes =
                if matches!(method, "resources/list" | "resources/templates/list") {
                    MAX_URI_BYTES
                } else {
                    MAX_NAME_BYTES
                };
            if identity.is_empty()
                || identity.len() > max_identity_bytes
                || identity.chars().any(char::is_control)
                || !identities.insert(identity.as_bytes().to_vec())
            {
                return Err(MESSAGE_INVALID);
            }
            match method {
                "tools/list" => {
                    validate_tool_schema(item.get("inputSchema").ok_or(MESSAGE_INVALID)?)?
                }
                "resources/list" | "resources/templates/list" => {
                    validate_named(item, "name", MAX_NAME_BYTES)?
                }
                "prompts/list" => validate_prompt_arguments(item.get("arguments"))?,
                _ => return Err(CAPABILITY_UNAVAILABLE),
            }
        }
        validate_cursor(object.get("nextCursor"))?;
    } else {
        match method {
            "tools/call" => validate_tool_call_result(object)?,
            "resources/read" => {
                if era == Era::Modern {
                    validate_modern_cache_fields(object)?;
                }
                validate_resource_contents_array(object.get("contents"))?;
            }
            "prompts/get" => validate_prompt_messages(object.get("messages"))?,
            _ => return Err(CAPABILITY_UNAVAILABLE),
        }
    }
    Ok(())
}

fn validate_tool_call_result(object: &Map<String, Value>) -> Result<(), &'static str> {
    if object
        .get("isError")
        .is_some_and(|value| !value.is_boolean())
    {
        return Err(MESSAGE_INVALID);
    }
    validate_content_blocks(object.get("content"))
}

fn validate_prompt_messages(value: Option<&Value>) -> Result<(), &'static str> {
    let messages = value.and_then(Value::as_array).ok_or(MESSAGE_INVALID)?;
    if messages.len() > MAX_LIST_ITEMS {
        return Err(RESPONSE_TOO_LARGE);
    }
    for message in messages {
        let message = message.as_object().ok_or(MESSAGE_INVALID)?;
        if !matches!(
            message.get("role").and_then(Value::as_str),
            Some("user" | "assistant")
        ) {
            return Err(MESSAGE_INVALID);
        }
        validate_content_block(message.get("content").ok_or(MESSAGE_INVALID)?)?;
    }
    Ok(())
}

fn validate_content_blocks(value: Option<&Value>) -> Result<(), &'static str> {
    let blocks = value.and_then(Value::as_array).ok_or(MESSAGE_INVALID)?;
    if blocks.len() > MAX_LIST_ITEMS {
        return Err(RESPONSE_TOO_LARGE);
    }
    for block in blocks {
        validate_content_block(block)?;
    }
    Ok(())
}

fn validate_content_block(value: &Value) -> Result<(), &'static str> {
    let object = value.as_object().ok_or(MESSAGE_INVALID)?;
    match object.get("type").and_then(Value::as_str) {
        Some("text") => require_string(object, "text", MAX_JSON_STRING_BYTES, true),
        Some("image" | "audio") => {
            let data = require_string_value(object, "data", MAX_JSON_STRING_BYTES, false)?;
            BASE64.decode(data).map_err(|_| MESSAGE_INVALID)?;
            require_string(object, "mimeType", MAX_NAME_BYTES, false)
        }
        Some("resource_link") => {
            validate_named(object, "name", MAX_NAME_BYTES)?;
            validate_named(object, "uri", MAX_URI_BYTES)
        }
        Some("resource") => {
            validate_resource_contents(object.get("resource").ok_or(MESSAGE_INVALID)?)
        }
        _ => Err(MESSAGE_INVALID),
    }
}

fn validate_resource_contents_array(value: Option<&Value>) -> Result<(), &'static str> {
    let contents = value.and_then(Value::as_array).ok_or(MESSAGE_INVALID)?;
    if contents.len() > MAX_LIST_ITEMS {
        return Err(RESPONSE_TOO_LARGE);
    }
    for content in contents {
        validate_resource_contents(content)?;
    }
    Ok(())
}

fn validate_resource_contents(value: &Value) -> Result<(), &'static str> {
    let object = value.as_object().ok_or(MESSAGE_INVALID)?;
    validate_named(object, "uri", MAX_URI_BYTES)?;
    if object.get("mimeType").is_some() {
        require_string(object, "mimeType", MAX_NAME_BYTES, false)?;
    }
    match (object.get("text"), object.get("blob")) {
        (Some(_), None) => require_string(object, "text", MAX_JSON_STRING_BYTES, true),
        (None, Some(_)) => {
            let blob = require_string_value(object, "blob", MAX_JSON_STRING_BYTES, false)?;
            BASE64.decode(blob).map_err(|_| MESSAGE_INVALID)?;
            Ok(())
        }
        _ => Err(MESSAGE_INVALID),
    }
}

fn require_string(
    object: &Map<String, Value>,
    key: &str,
    max_bytes: usize,
    allow_empty: bool,
) -> Result<(), &'static str> {
    require_string_value(object, key, max_bytes, allow_empty).map(|_| ())
}

fn require_string_value<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    max_bytes: usize,
    allow_empty: bool,
) -> Result<&'a str, &'static str> {
    let value = object
        .get(key)
        .and_then(Value::as_str)
        .ok_or(MESSAGE_INVALID)?;
    if (!allow_empty && value.is_empty()) || value.len() > max_bytes {
        return Err(MESSAGE_INVALID);
    }
    Ok(value)
}

pub fn has_capability(capabilities: &Value, method: &str) -> bool {
    let key = match method {
        "tools/list" | "tools/call" => "tools",
        "resources/list" | "resources/templates/list" | "resources/read" => "resources",
        "prompts/list" | "prompts/get" => "prompts",
        _ => return false,
    };
    capabilities
        .as_object()
        .and_then(|object| object.get(key))
        .is_some_and(Value::is_object)
}

pub fn safe_request_projection(method: &str, params: &Value) -> Value {
    let mut projected = Map::new();
    if let Some(object) = params.as_object() {
        if let Some(value) = object.get("name").and_then(Value::as_str) {
            projected.insert("name".into(), Value::String(value.to_string()));
        }
        if let Some(value) = object.get("uri").and_then(Value::as_str) {
            projected.insert("uri".into(), Value::String(safe_uri_projection(value)));
        }
        if object.get("cursor").and_then(Value::as_str).is_some() {
            projected.insert("cursor".into(), Value::String("[PRESENT]".into()));
        }
        if method == "tools/call" && object.contains_key("arguments") {
            projected.insert("arguments".into(), Value::String("[REDACTED]".into()));
        }
        if method == "prompts/get" && object.contains_key("arguments") {
            projected.insert("arguments".into(), Value::String("[REDACTED]".into()));
        }
    }
    Value::Object(projected)
}

pub fn safe_result_projection(result: &Value) -> Value {
    let mut projected = result.clone();
    if let Some(object) = projected.as_object_mut() {
        if object.get("nextCursor").and_then(Value::as_str).is_some() {
            object.insert("nextCursor".into(), Value::String("[PRESENT]".into()));
        }
    }
    projected
}

fn safe_uri_projection(value: &str) -> String {
    let boundary = [value.find('?'), value.find('#')]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(value.len());
    let mut projected = value[..boundary].to_string();
    if projected.to_ascii_lowercase().starts_with("data:") {
        return "data:[REDACTED]".into();
    }
    if let Some(authority_start) = projected.find("://").map(|index| index + 3) {
        let authority_end = projected[authority_start..]
            .find('/')
            .map(|index| authority_start + index)
            .unwrap_or(projected.len());
        if let Some(at) = projected[authority_start..authority_end].rfind('@') {
            let at = authority_start + at;
            projected.replace_range(authority_start..at, "[REDACTED]");
        }
    }
    if boundary < value.len() {
        projected.push_str("?[REDACTED]");
    }
    projected
}

fn validate_prompt_arguments(value: Option<&Value>) -> Result<(), &'static str> {
    let Some(arguments) = value else {
        return Ok(());
    };
    let arguments = arguments.as_array().ok_or(MESSAGE_INVALID)?;
    if arguments.len() > MAX_SCHEMA_PROPERTIES {
        return Err(RESPONSE_TOO_LARGE);
    }
    let mut names = BTreeSet::new();
    for argument in arguments {
        let argument = argument.as_object().ok_or(MESSAGE_INVALID)?;
        let name = argument
            .get("name")
            .and_then(Value::as_str)
            .ok_or(MESSAGE_INVALID)?;
        if name.is_empty()
            || name.len() > MAX_NAME_BYTES
            || name.chars().any(char::is_control)
            || !names.insert(name.as_bytes().to_vec())
            || argument
                .get("required")
                .is_some_and(|required| !required.is_boolean())
        {
            return Err(MESSAGE_INVALID);
        }
    }
    Ok(())
}

pub fn tool_schemas(result: &Value) -> Result<BTreeMap<String, Value>, &'static str> {
    let tools = result
        .get("tools")
        .and_then(Value::as_array)
        .ok_or(MESSAGE_INVALID)?;
    let mut schemas = BTreeMap::new();
    for tool in tools {
        let object = tool.as_object().ok_or(MESSAGE_INVALID)?;
        let name = object
            .get("name")
            .and_then(Value::as_str)
            .ok_or(MESSAGE_INVALID)?;
        let schema = object.get("inputSchema").ok_or(MESSAGE_INVALID)?;
        validate_tool_schema(schema)?;
        if validate_callable_tool_schema(schema).is_err() {
            continue;
        }
        if schemas.insert(name.to_string(), schema.clone()).is_some() {
            return Err(MESSAGE_INVALID);
        }
    }
    Ok(schemas)
}

pub fn prompt_schemas(
    result: &Value,
) -> Result<BTreeMap<String, BTreeMap<String, bool>>, &'static str> {
    let prompts = result
        .get("prompts")
        .and_then(Value::as_array)
        .ok_or(MESSAGE_INVALID)?;
    let mut schemas = BTreeMap::new();
    for prompt in prompts {
        let object = prompt.as_object().ok_or(MESSAGE_INVALID)?;
        let name = object
            .get("name")
            .and_then(Value::as_str)
            .ok_or(MESSAGE_INVALID)?;
        validate_prompt_arguments(object.get("arguments"))?;
        let mut arguments = BTreeMap::new();
        if let Some(values) = object.get("arguments").and_then(Value::as_array) {
            for value in values {
                let value = value.as_object().ok_or(MESSAGE_INVALID)?;
                let argument_name = value
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or(MESSAGE_INVALID)?;
                arguments.insert(
                    argument_name.to_string(),
                    value
                        .get("required")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                );
            }
        }
        if schemas.insert(name.to_string(), arguments).is_some() {
            return Err(MESSAGE_INVALID);
        }
    }
    Ok(schemas)
}

pub fn validate_prompt_values(
    schema: &BTreeMap<String, bool>,
    arguments: Option<&Value>,
) -> Result<(), &'static str> {
    let empty = Map::new();
    let arguments = match arguments {
        Some(value) => value.as_object().ok_or(MESSAGE_INVALID)?,
        None => &empty,
    };
    if arguments
        .iter()
        .any(|(name, value)| !schema.contains_key(name) || !value.is_string())
        || schema
            .iter()
            .any(|(name, required)| *required && !arguments.contains_key(name))
    {
        return Err(MESSAGE_INVALID);
    }
    Ok(())
}

pub fn validate_callable_tool_schema(schema: &Value) -> Result<(), &'static str> {
    validate_tool_schema(schema)?;
    let mut annotations = Vec::new();
    collect_header_annotations(schema, &mut Vec::new(), &mut annotations, 0)?;
    validate_callable_schema_node(schema, 0, true)
}

pub fn validate_tool_arguments(schema: &Value, arguments: &Value) -> Result<(), &'static str> {
    validate_callable_tool_schema(schema)?;
    validate_schema_value(schema, arguments, 0)
}

fn validate_callable_schema_node(
    schema: &Value,
    depth: usize,
    root: bool,
) -> Result<(), &'static str> {
    if depth > MAX_SCHEMA_DEPTH {
        return Err(SCHEMA_UNSUPPORTED);
    }
    let object = schema.as_object().ok_or(SCHEMA_UNSUPPORTED)?;
    let schema_type = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or(SCHEMA_UNSUPPORTED)?;
    if root && schema_type != "object" {
        return Err(SCHEMA_UNSUPPORTED);
    }
    if let Some(dialect) = object.get("$schema") {
        if !root {
            return Err(SCHEMA_UNSUPPORTED);
        }
        validate_schema_display(Some(dialect))?;
    }
    if object
        .keys()
        .any(|key| !callable_schema_key_allowed(schema_type, key))
    {
        return Err(SCHEMA_UNSUPPORTED);
    }
    validate_schema_display(object.get("title"))?;
    validate_schema_display(object.get("description"))?;
    validate_schema_enum(object.get("enum"), schema_type)?;
    match schema_type {
        "object" => {
            if object
                .get("additionalProperties")
                .is_some_and(|value| value != &Value::Bool(false))
            {
                return Err(SCHEMA_UNSUPPORTED);
            }
            let properties = match object.get("properties") {
                Some(value) => value.as_object().ok_or(SCHEMA_UNSUPPORTED)?,
                None => return validate_empty_object_schema(object, depth),
            };
            if properties.len() > MAX_SCHEMA_PROPERTIES {
                return Err(SCHEMA_UNSUPPORTED);
            }
            let required = validate_required_names(object.get("required"), properties)?;
            for (name, child) in properties {
                if name.is_empty()
                    || name.len() > MAX_JSON_KEY_BYTES
                    || name.chars().any(char::is_control)
                {
                    return Err(SCHEMA_UNSUPPORTED);
                }
                validate_callable_schema_node(child, depth + 1, false)?;
            }
            if required.len() > properties.len() {
                return Err(SCHEMA_UNSUPPORTED);
            }
        }
        "array" => {
            let items = object.get("items").ok_or(SCHEMA_UNSUPPORTED)?;
            let minimum = schema_non_negative_integer(object.get("minItems"))?;
            let maximum = schema_non_negative_integer(object.get("maxItems"))?;
            validate_schema_range(minimum, maximum, MAX_SCHEMA_ARRAY_ITEMS as f64)?;
            validate_callable_schema_node(items, depth + 1, false)?;
        }
        "string" => {
            let minimum = schema_non_negative_integer(object.get("minLength"))?;
            let maximum = schema_non_negative_integer(object.get("maxLength"))?;
            validate_schema_range(minimum, maximum, MAX_JSON_STRING_BYTES as f64)?;
        }
        "integer" | "number" => {
            let minimum = schema_finite_number(object.get("minimum"))?;
            let maximum = schema_finite_number(object.get("maximum"))?;
            validate_schema_range(minimum, maximum, f64::MAX)?;
        }
        "boolean" => {}
        _ => return Err(SCHEMA_UNSUPPORTED),
    }
    if let Some(default) = object.get("default") {
        validate_schema_value(schema, default, depth + 1).map_err(|_| SCHEMA_UNSUPPORTED)?;
    }
    Ok(())
}

fn validate_empty_object_schema(
    object: &Map<String, Value>,
    depth: usize,
) -> Result<(), &'static str> {
    let empty = Map::new();
    let required = validate_required_names(object.get("required"), &empty)?;
    if !required.is_empty() {
        return Err(SCHEMA_UNSUPPORTED);
    }
    if let Some(default) = object.get("default") {
        validate_schema_value(&Value::Object(object.clone()), default, depth + 1)
            .map_err(|_| SCHEMA_UNSUPPORTED)?;
    }
    Ok(())
}

fn callable_schema_key_allowed(schema_type: &str, key: &str) -> bool {
    if matches!(
        key,
        "$schema" | "title" | "description" | "type" | "enum" | "default" | "x-mcp-header"
    ) {
        return true;
    }
    match schema_type {
        "object" => matches!(key, "properties" | "required" | "additionalProperties"),
        "array" => matches!(key, "items" | "minItems" | "maxItems"),
        "string" => matches!(key, "minLength" | "maxLength"),
        "integer" | "number" => matches!(key, "minimum" | "maximum"),
        "boolean" => false,
        _ => false,
    }
}

fn validate_schema_display(value: Option<&Value>) -> Result<(), &'static str> {
    let Some(value) = value else {
        return Ok(());
    };
    let value = value.as_str().ok_or(SCHEMA_UNSUPPORTED)?;
    if value.len() > MAX_JSON_KEY_BYTES || value.chars().any(|character| character == '\0') {
        return Err(SCHEMA_UNSUPPORTED);
    }
    Ok(())
}

fn validate_schema_enum(value: Option<&Value>, schema_type: &str) -> Result<(), &'static str> {
    let Some(values) = value else {
        return Ok(());
    };
    let values = values.as_array().ok_or(SCHEMA_UNSUPPORTED)?;
    if values.is_empty() || values.len() > MAX_SCHEMA_ENUM_VALUES {
        return Err(SCHEMA_UNSUPPORTED);
    }
    let mut seen = BTreeSet::new();
    for value in values {
        let valid = match schema_type {
            "string" => value.as_str().is_some(),
            "integer" => is_safe_json_integer(value),
            "number" => value.as_f64().is_some_and(f64::is_finite),
            "boolean" => value.is_boolean(),
            _ => false,
        };
        let encoded = serde_json::to_vec(value).map_err(|_| SCHEMA_UNSUPPORTED)?;
        if !valid || !seen.insert(encoded) {
            return Err(SCHEMA_UNSUPPORTED);
        }
    }
    Ok(())
}

fn validate_required_names(
    value: Option<&Value>,
    properties: &Map<String, Value>,
) -> Result<BTreeSet<Vec<u8>>, &'static str> {
    let Some(values) = value else {
        return Ok(BTreeSet::new());
    };
    let values = values.as_array().ok_or(SCHEMA_UNSUPPORTED)?;
    let mut required = BTreeSet::new();
    for value in values {
        let name = value.as_str().ok_or(SCHEMA_UNSUPPORTED)?;
        if !properties.contains_key(name) || !required.insert(name.as_bytes().to_vec()) {
            return Err(SCHEMA_UNSUPPORTED);
        }
    }
    Ok(required)
}

fn schema_non_negative_integer(value: Option<&Value>) -> Result<Option<f64>, &'static str> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.as_f64().ok_or(SCHEMA_UNSUPPORTED)?;
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value > 9_007_199_254_740_991.0
    {
        return Err(SCHEMA_UNSUPPORTED);
    }
    Ok(Some(value))
}

fn schema_finite_number(value: Option<&Value>) -> Result<Option<f64>, &'static str> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.as_f64().ok_or(SCHEMA_UNSUPPORTED)?;
    if !value.is_finite() {
        return Err(SCHEMA_UNSUPPORTED);
    }
    Ok(Some(value))
}

fn validate_schema_range(
    minimum: Option<f64>,
    maximum: Option<f64>,
    absolute_maximum: f64,
) -> Result<(), &'static str> {
    if minimum.is_some_and(|value| value > absolute_maximum)
        || maximum.is_some_and(|value| value > absolute_maximum)
        || matches!((minimum, maximum), (Some(minimum), Some(maximum)) if minimum > maximum)
    {
        return Err(SCHEMA_UNSUPPORTED);
    }
    Ok(())
}

fn validate_schema_value(schema: &Value, value: &Value, depth: usize) -> Result<(), &'static str> {
    if depth > MAX_SCHEMA_DEPTH {
        return Err(MESSAGE_INVALID);
    }
    let object = schema.as_object().ok_or(MESSAGE_INVALID)?;
    if let Some(values) = object.get("enum").and_then(Value::as_array) {
        let schema_type = object.get("type").and_then(Value::as_str);
        if !values
            .iter()
            .any(|candidate| schema_values_equal(schema_type, candidate, value))
        {
            return Err(MESSAGE_INVALID);
        }
    }
    match object.get("type").and_then(Value::as_str) {
        Some("object") => {
            let value = value.as_object().ok_or(MESSAGE_INVALID)?;
            let empty = Map::new();
            let properties = object
                .get("properties")
                .and_then(Value::as_object)
                .unwrap_or(&empty);
            let required = object
                .get("required")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str);
            if required.into_iter().any(|name| !value.contains_key(name)) {
                return Err(MESSAGE_INVALID);
            }
            for (name, child) in value {
                let child_schema = properties.get(name).ok_or(MESSAGE_INVALID)?;
                validate_schema_value(child_schema, child, depth + 1)?;
            }
        }
        Some("array") => {
            let values = value.as_array().ok_or(MESSAGE_INVALID)?;
            if values.len() > MAX_SCHEMA_ARRAY_ITEMS
                || object
                    .get("minItems")
                    .and_then(Value::as_f64)
                    .is_some_and(|minimum| values.len() < minimum as usize)
                || object
                    .get("maxItems")
                    .and_then(Value::as_f64)
                    .is_some_and(|maximum| values.len() > maximum as usize)
            {
                return Err(MESSAGE_INVALID);
            }
            let items = object.get("items").ok_or(MESSAGE_INVALID)?;
            for value in values {
                validate_schema_value(items, value, depth + 1)?;
            }
        }
        Some("string") => {
            let value = value.as_str().ok_or(MESSAGE_INVALID)?;
            let length = value.chars().count();
            if object
                .get("minLength")
                .and_then(Value::as_f64)
                .is_some_and(|minimum| length < minimum as usize)
                || object
                    .get("maxLength")
                    .and_then(Value::as_f64)
                    .is_some_and(|maximum| length > maximum as usize)
            {
                return Err(MESSAGE_INVALID);
            }
        }
        Some("integer") => {
            if !is_safe_json_integer(value) {
                return Err(MESSAGE_INVALID);
            }
            validate_schema_number_range(object, value.as_f64().ok_or(MESSAGE_INVALID)?)?;
        }
        Some("number") => {
            let value = value
                .as_f64()
                .filter(|value| value.is_finite())
                .ok_or(MESSAGE_INVALID)?;
            validate_schema_number_range(object, value)?;
        }
        Some("boolean") if value.is_boolean() => {}
        _ => return Err(MESSAGE_INVALID),
    }
    Ok(())
}

fn schema_values_equal(schema_type: Option<&str>, left: &Value, right: &Value) -> bool {
    match schema_type {
        Some("integer" | "number") => left.as_f64() == right.as_f64(),
        _ => left == right,
    }
}

fn is_safe_json_integer(value: &Value) -> bool {
    value
        .as_i64()
        .is_some_and(|value| value.unsigned_abs() <= 9_007_199_254_740_991)
        || value
            .as_u64()
            .is_some_and(|value| value <= 9_007_199_254_740_991)
}

fn validate_schema_number_range(
    schema: &Map<String, Value>,
    value: f64,
) -> Result<(), &'static str> {
    if schema
        .get("minimum")
        .and_then(Value::as_f64)
        .is_some_and(|minimum| value < minimum)
        || schema
            .get("maximum")
            .and_then(Value::as_f64)
            .is_some_and(|maximum| value > maximum)
    {
        return Err(MESSAGE_INVALID);
    }
    Ok(())
}

pub fn filter_invalid_http_tools(result: &Value) -> Result<(Value, usize), &'static str> {
    let mut projected = result.clone();
    let rejected = {
        let object = projected.as_object_mut().ok_or(MESSAGE_INVALID)?;
        let tools = object
            .get_mut("tools")
            .and_then(Value::as_array_mut)
            .ok_or(MESSAGE_INVALID)?;
        let original_len = tools.len();
        tools.retain(|tool| {
            tool.get("inputSchema").is_some_and(|schema| {
                let mut annotations = Vec::new();
                validate_tool_schema(schema).is_ok()
                    && collect_header_annotations(schema, &mut Vec::new(), &mut annotations, 0)
                        .is_ok()
            })
        });
        original_len - tools.len()
    };
    Ok((projected, rejected))
}

pub fn validate_tool_schema(value: &Value) -> Result<(), &'static str> {
    if value
        .as_object()
        .and_then(|object| object.get("type"))
        .and_then(Value::as_str)
        != Some("object")
    {
        return Err(SCHEMA_UNSUPPORTED);
    }
    let mut budget = SchemaBudget::default();
    validate_schema_node(value, 0, &mut budget)
}

fn validate_schema_node(
    value: &Value,
    depth: usize,
    budget: &mut SchemaBudget,
) -> Result<(), &'static str> {
    if depth > MAX_SCHEMA_DEPTH {
        return Err(SCHEMA_UNSUPPORTED);
    }
    budget.nodes = budget.nodes.checked_add(1).ok_or(SCHEMA_UNSUPPORTED)?;
    if budget.nodes > MAX_SCHEMA_NODES {
        return Err(SCHEMA_UNSUPPORTED);
    }
    match value {
        Value::Object(object) => {
            if let Some(properties) = object.get("properties") {
                let properties = properties.as_object().ok_or(SCHEMA_UNSUPPORTED)?;
                budget.properties = budget
                    .properties
                    .checked_add(properties.len())
                    .ok_or(SCHEMA_UNSUPPORTED)?;
                if budget.properties > MAX_SCHEMA_PROPERTIES {
                    return Err(SCHEMA_UNSUPPORTED);
                }
            }
            for (key, child) in object {
                if key.len() > MAX_JSON_KEY_BYTES {
                    return Err(SCHEMA_UNSUPPORTED);
                }
                validate_schema_node(child, depth + 1, budget)?;
            }
        }
        Value::Array(values) => {
            for child in values {
                validate_schema_node(child, depth + 1, budget)?;
            }
        }
        Value::String(value) if value.len() > MAX_JSON_STRING_BYTES => {
            return Err(SCHEMA_UNSUPPORTED)
        }
        _ => {}
    }
    Ok(())
}

pub fn validate_json(value: &Value, max_bytes: usize) -> Result<(), &'static str> {
    validate_serialized_size(value, max_bytes)?;
    let mut budget = JsonBudget::default();
    validate_json_node(value, 0, &mut budget)
}

fn validate_serialized_size(value: &Value, max_bytes: usize) -> Result<(), &'static str> {
    let bytes = serde_json::to_vec(value).map_err(|_| MESSAGE_INVALID)?;
    if bytes.len() > max_bytes {
        return Err(RESPONSE_TOO_LARGE);
    }
    Ok(())
}

fn validate_json_node(
    value: &Value,
    depth: usize,
    budget: &mut JsonBudget,
) -> Result<(), &'static str> {
    if depth > MAX_JSON_DEPTH {
        return Err(MESSAGE_INVALID);
    }
    budget.nodes = budget.nodes.checked_add(1).ok_or(MESSAGE_INVALID)?;
    if budget.nodes > MAX_JSON_NODES {
        return Err(RESPONSE_TOO_LARGE);
    }
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if key.len() > MAX_JSON_KEY_BYTES || key.chars().any(|character| character == '\0')
                {
                    return Err(MESSAGE_INVALID);
                }
                validate_json_node(child, depth + 1, budget)?;
            }
        }
        Value::Array(values) => {
            for child in values {
                validate_json_node(child, depth + 1, budget)?;
            }
        }
        Value::String(value)
            if value.len() > MAX_JSON_STRING_BYTES
                || value.chars().any(|character| character == '\0') =>
        {
            return Err(MESSAGE_INVALID);
        }
        Value::String(_) => {}
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modern_discover_matches_current_per_request_metadata_contract() {
        let request = build_modern_request("discover-1", "server/discover", json!({})).unwrap();
        assert_eq!(request["jsonrpc"], "2.0");
        assert_eq!(request["method"], "server/discover");
        assert_eq!(
            request["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"],
            MODERN_VERSION
        );
        assert_eq!(
            request["params"]["_meta"]["io.modelcontextprotocol/clientInfo"]["name"],
            CLIENT_NAME
        );
        let headers = derived_headers(
            Era::Modern,
            MODERN_VERSION,
            "server/discover",
            &json!({}),
            None,
        )
        .unwrap();
        assert!(headers.iter().any(|header| {
            header.name == "MCP-Protocol-Version" && header.value == MODERN_VERSION
        }));
        assert!(headers
            .iter()
            .any(|header| { header.name == "Mcp-Method" && header.value == "server/discover" }));
    }

    #[test]
    fn legacy_initialize_and_notifications_are_fixed_and_bounded() {
        let initialize = build_legacy_initialize("legacy-1").unwrap();
        assert_eq!(initialize["method"], "initialize");
        assert_eq!(initialize["params"]["protocolVersion"], LEGACY_VERSION);
        assert_eq!(
            build_legacy_initialized()["method"],
            "notifications/initialized"
        );
        assert_eq!(
            build_legacy_cancelled("legacy-2").unwrap()["params"]["requestId"],
            "legacy-2"
        );
        assert!(build_legacy_cancelled("bad request id").is_err());
    }

    #[test]
    fn tool_headers_validate_annotations_and_encode_non_ascii_values() {
        let schema = json!({
            "type": "object",
            "properties": {
                "region": { "type": "string", "x-mcp-header": "Region" },
                "nested": {
                    "type": "object",
                    "properties": {
                        "enabled": { "type": "boolean", "x-mcp-header": "Enabled" }
                    }
                }
            }
        });
        let params = json!({
            "name": "도구",
            "arguments": { "region": "서울", "nested": { "enabled": true } }
        });
        let headers = derived_headers(
            Era::Modern,
            MODERN_VERSION,
            "tools/call",
            &params,
            Some(&schema),
        )
        .unwrap();
        assert!(headers
            .iter()
            .any(|header| header.name == "Mcp-Name" && header.value.starts_with("=?base64?")));
        assert!(headers.iter().any(|header| {
            header.name == "Mcp-Param-Region" && header.value.starts_with("=?base64?")
        }));
        assert!(headers
            .iter()
            .any(|header| { header.name == "Mcp-Param-Enabled" && header.value == "true" }));

        let duplicate = json!({
            "type": "object",
            "properties": {
                "a": { "type": "string", "x-mcp-header": "Tenant" },
                "b": { "type": "string", "x-mcp-header": "tenant" }
            }
        });
        assert_eq!(
            derived_headers(
                Era::Modern,
                MODERN_VERSION,
                "tools/call",
                &json!({"name":"x", "arguments":{"a":"1", "b":"2"}}),
                Some(&duplicate)
            )
            .unwrap_err(),
            SCHEMA_UNSUPPORTED
        );

        let unsafe_integer = json!({
            "type":"object",
            "properties":{
                "count":{"type":"integer", "x-mcp-header":"Count"}
            }
        });
        assert_eq!(
            derived_headers(
                Era::Modern,
                MODERN_VERSION,
                "tools/call",
                &json!({"name":"x", "arguments":{"count":9_007_199_254_740_992_i64}}),
                Some(&unsafe_integer)
            ),
            Err(MESSAGE_INVALID)
        );

        let oversized_header_value = json!({
            "type":"object",
            "properties":{
                "value":{"type":"string", "x-mcp-header":"Value"}
            }
        });
        assert_eq!(
            derived_headers(
                Era::Modern,
                MODERN_VERSION,
                "tools/call",
                &json!({
                    "name":"x",
                    "arguments":{"value":"x".repeat(MAX_DERIVED_HEADER_VALUE_BYTES + 1)}
                }),
                Some(&oversized_header_value)
            ),
            Err(REQUEST_TOO_LARGE)
        );

        let mut properties = Map::new();
        for index in 0..=MAX_DERIVED_PARAMETER_HEADERS {
            properties.insert(
                format!("field-{index}"),
                json!({"type":"string", "x-mcp-header":format!("Field-{index}")}),
            );
        }
        let excessive_headers = json!({"type":"object", "properties":properties});
        assert_eq!(
            validate_callable_tool_schema(&excessive_headers),
            Err(SCHEMA_UNSUPPORTED)
        );
    }

    #[test]
    fn invalid_http_header_annotations_exclude_only_the_affected_tool() {
        let result = json!({
            "resultType":"complete",
            "tools":[
                {
                    "name":"valid",
                    "inputSchema":{
                        "type":"object",
                        "properties":{
                            "region":{"type":"string", "x-mcp-header":"Region"}
                        }
                    }
                },
                {
                    "name":"root-annotation",
                    "inputSchema":{"type":"object", "x-mcp-header":"Root"}
                },
                {
                    "name":"array-annotation",
                    "inputSchema":{
                        "type":"object",
                        "properties":{
                            "values":{
                                "type":"array",
                                "items":{"type":"string", "x-mcp-header":"Item"}
                            }
                        }
                    }
                }
            ]
        });
        let (projected, rejected) = filter_invalid_http_tools(&result).unwrap();
        let tools = projected["tools"].as_array().unwrap();
        assert_eq!(rejected, 2);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "valid");
    }

    #[test]
    fn callable_tool_subset_is_revalidated_at_the_native_boundary() {
        let schema = json!({
            "$schema":"https://json-schema.org/draft/2020-12/schema",
            "type":"object",
            "additionalProperties":false,
            "required":["message", "options"],
            "properties":{
                "message":{"type":"string", "minLength":1, "maxLength":20},
                "count":{"type":"integer", "minimum":1, "maximum":5},
                "options":{
                    "type":"object",
                    "required":["enabled"],
                    "properties":{"enabled":{"type":"boolean"}}
                },
                "tags":{
                    "type":"array",
                    "maxItems":2,
                    "items":{"type":"string"}
                }
            }
        });
        assert_eq!(
            validate_tool_arguments(
                &schema,
                &json!({
                    "message":"hello",
                    "count":3,
                    "options":{"enabled":true},
                    "tags":["one", "two"]
                })
            ),
            Ok(())
        );
        for invalid in [
            json!({"message":"", "options":{"enabled":true}}),
            json!({"message":"hello", "options":{}}),
            json!({"message":"hello", "options":{"enabled":true}, "extra":true}),
            json!({"message":"hello", "count":6, "options":{"enabled":true}}),
            json!({"message":"hello", "options":{"enabled":true}, "tags":["1","2","3"]}),
        ] {
            assert_eq!(
                validate_tool_arguments(&schema, &invalid),
                Err(MESSAGE_INVALID)
            );
        }

        let fractional_bounds = json!({
            "type":"object",
            "properties":{
                "label":{"type":"string", "minLength":2.0},
                "items":{"type":"array", "maxItems":2.0, "items":{"type":"boolean"}}
            }
        });
        assert_eq!(
            validate_tool_arguments(
                &fractional_bounds,
                &json!({"label":"x", "items":[true, false, true]})
            ),
            Err(MESSAGE_INVALID)
        );

        let tools = tool_schemas(&json!({
            "tools":[
                {"name":"callable", "inputSchema":schema},
                {
                    "name":"view-only",
                    "inputSchema":{
                        "type":"object",
                        "properties":{"value":{"$ref":"#/$defs/value"}}
                    }
                }
            ]
        }))
        .unwrap();
        assert!(tools.contains_key("callable"));
        assert!(!tools.contains_key("view-only"));
        assert_eq!(
            validate_tool_schema(&json!({"properties":{}})),
            Err(SCHEMA_UNSUPPORTED)
        );
        assert_eq!(
            validate_callable_tool_schema(&json!({
                "type":"object",
                "properties":{"nested":{"type":"string", "$schema":"nested"}}
            })),
            Err(SCHEMA_UNSUPPORTED)
        );
    }

    #[test]
    fn response_requires_matching_id_and_modern_result_type() {
        let message = parse_rpc_message(
            json!({
                "jsonrpc": "2.0",
                "id": "request-1",
                "result": { "resultType": "complete", "tools": [] }
            }),
            "request-1",
            Era::Modern,
        )
        .unwrap();
        assert!(matches!(
            message,
            RpcMessage::Response { result: Ok(_), .. }
        ));
        assert_eq!(
            parse_rpc_message(
                json!({"jsonrpc":"2.0", "id":"other", "result":{"resultType":"complete"}}),
                "request-1",
                Era::Modern
            )
            .unwrap_err(),
            MESSAGE_INVALID
        );
        assert_eq!(
            parse_rpc_message(
                json!({"jsonrpc":"2.0", "id":"request-1", "result":{}}),
                "request-1",
                Era::Modern
            )
            .unwrap_err(),
            MESSAGE_INVALID
        );
        assert!(parse_rpc_message(
            json!({"jsonrpc":"2.0", "id":"request-1", "result":{}}),
            "request-1",
            Era::Legacy
        )
        .is_ok());
        assert_eq!(
            parse_rpc_message(
                json!({
                    "jsonrpc":"2.0",
                    "method":"notifications/progress",
                    "params":[]
                }),
                "request-1",
                Era::Modern
            ),
            Err(MESSAGE_INVALID)
        );
        assert_eq!(
            parse_rpc_message(
                json!({
                    "jsonrpc":"2.0",
                    "method":"notifications/progress",
                    "result":{}
                }),
                "request-1",
                Era::Modern
            ),
            Err(MESSAGE_INVALID)
        );
    }

    #[test]
    fn recognized_version_error_exposes_only_bounded_versions() {
        let message = parse_rpc_message(
            json!({
                "jsonrpc": "2.0",
                "id": "discover-1",
                "error": {
                    "code": -32022,
                    "message": "Unsupported protocol version",
                    "data": {
                        "requested": MODERN_VERSION,
                        "supported": [MODERN_VERSION, LEGACY_VERSION, "2027-01-01"]
                    }
                }
            }),
            "discover-1",
            Era::Modern,
        )
        .unwrap();
        let RpcMessage::Response {
            result: Err(error), ..
        } = message
        else {
            panic!("expected error response")
        };
        assert!(is_recognized_modern_error(&error));
        assert_eq!(
            supported_versions_from_error(&error),
            vec![
                MODERN_VERSION.to_string(),
                LEGACY_VERSION.to_string(),
                "2027-01-01".to_string()
            ]
        );

        let idless = parse_rpc_message(
            json!({
                "jsonrpc":"2.0",
                "error":{
                    "code":-32022,
                    "message":"Unsupported protocol version",
                    "data":{
                        "requested":MODERN_VERSION,
                        "supported":[LEGACY_VERSION]
                    }
                }
            }),
            "discover-1",
            Era::Modern,
        )
        .unwrap();
        assert!(matches!(
            idless,
            RpcMessage::Response { id, result: Err(_) } if id == "discover-1"
        ));

        let missing_requested = RpcError {
            code: -32022,
            message: "Unsupported protocol version".into(),
            data: Some(json!({ "supported": [LEGACY_VERSION] })),
        };
        assert!(supported_versions_from_error(&missing_requested).is_empty());

        let wrong_requested = RpcError {
            code: -32022,
            message: "Unsupported protocol version".into(),
            data: Some(json!({
                "requested": LEGACY_VERSION,
                "supported": [LEGACY_VERSION]
            })),
        };
        assert!(supported_versions_from_error(&wrong_requested).is_empty());

        let malformed_supported = RpcError {
            code: -32022,
            message: "Unsupported protocol version".into(),
            data: Some(json!({
                "requested": MODERN_VERSION,
                "supported": [LEGACY_VERSION, "not-a-version"]
            })),
        };
        assert!(supported_versions_from_error(&malformed_supported).is_empty());
        assert!(has_modern_error_evidence(&json!({
            "jsonrpc":"2.0",
            "error":{"code":-32020, "message":"missing id"}
        })));
        assert!(has_modern_error_evidence(&json!({
            "error":{"code":-32020, "message":"missing jsonrpc and id"}
        })));
    }

    #[test]
    fn discover_and_legacy_projection_do_not_trust_unbounded_identity() {
        let modern = project_discover(&json!({
            "resultType": "complete",
            "supportedVersions": [MODERN_VERSION],
            "capabilities": { "tools": {}, "resources": {} },
            "ttlMs": 60_000,
            "cacheScope": "public",
            "_meta": {
                "io.modelcontextprotocol/serverInfo": { "name": "fixture", "version": "1.0.0" }
            }
        }))
        .unwrap();
        assert_eq!(modern.era, Era::Modern);
        assert_eq!(modern.server_name, "fixture");

        let legacy = project_legacy_initialize(&json!({
            "protocolVersion": LEGACY_VERSION,
            "capabilities": { "prompts": {} },
            "serverInfo": { "name": "legacy", "version": "0.1.0" }
        }))
        .unwrap();
        assert_eq!(legacy.era, Era::Legacy);
        assert_eq!(legacy.supported_versions, vec![LEGACY_VERSION]);
        assert_eq!(
            project_legacy_initialize(&json!({
                "protocolVersion":LEGACY_VERSION,
                "capabilities":{}
            })),
            Err(MESSAGE_INVALID)
        );
        assert_eq!(
            project_discover(&json!({
                "resultType":"complete",
                "supportedVersions":[MODERN_VERSION],
                "capabilities":{},
                "ttlMs":0,
                "cacheScope":"public",
                "_meta":{"io.modelcontextprotocol/serverInfo":{"name":"missing-version"}}
            })),
            Err(MESSAGE_INVALID)
        );
    }

    #[test]
    fn list_validation_rejects_duplicate_identity_and_bad_cursor() {
        let duplicate = json!({
            "resultType": "complete",
            "ttlMs": 60_000,
            "cacheScope": "private",
            "tools": [
                {"name":"same", "inputSchema":{"type":"object"}},
                {"name":"same", "inputSchema":{"type":"object"}}
            ]
        });
        assert_eq!(
            validate_operation_result("tools/list", &duplicate, Era::Modern).unwrap_err(),
            MESSAGE_INVALID
        );
        let invalid_cursor = json!({
            "resultType": "complete",
            "ttlMs": 60_000,
            "cacheScope": "private",
            "resources": [],
            "nextCursor": ""
        });
        assert_eq!(
            validate_operation_result("resources/list", &invalid_cursor, Era::Modern).unwrap_err(),
            CURSOR_INVALID
        );
    }

    #[test]
    fn operation_rejects_frontend_meta_and_unsupported_methods() {
        assert_eq!(
            build_modern_request("x", "tools/list", json!({"_meta":{"injected":true}}))
                .unwrap_err(),
            MESSAGE_INVALID
        );
        assert_eq!(
            validate_operation("custom/run", &json!({})).unwrap_err(),
            CAPABILITY_UNAVAILABLE
        );
        assert_eq!(
            validate_operation("tools/list", &json!({"unexpected":true})),
            Err(MESSAGE_INVALID)
        );
        assert_eq!(
            validate_operation(
                "prompts/get",
                &json!({"name":"prompt", "arguments":{"bad\nname":"value"}})
            ),
            Err(MESSAGE_INVALID)
        );
    }

    #[test]
    fn safe_request_projection_masks_cursor_query_and_userinfo() {
        assert_eq!(
            safe_request_projection("resources/list", &json!({"cursor":"opaque-secret-cursor"})),
            json!({"cursor":"[PRESENT]"})
        );
        assert_eq!(
            safe_request_projection(
                "resources/read",
                &json!({"uri":"https://user:password@example.test/path?token=secret#part"})
            ),
            json!({"uri":"https://[REDACTED]@example.test/path?[REDACTED]"})
        );
        assert_eq!(
            safe_request_projection("resources/read", &json!({"uri":"data:text/plain,secret"})),
            json!({"uri":"data:[REDACTED]"})
        );
        assert_eq!(
            safe_result_projection(&json!({
                "resultType":"complete",
                "tools":[],
                "nextCursor":"opaque-cursor"
            })),
            json!({
                "resultType":"complete",
                "tools":[],
                "nextCursor":"[PRESENT]"
            })
        );
    }

    #[test]
    fn prompt_projection_rejects_duplicate_or_unbounded_argument_names() {
        let duplicate = json!({
            "resultType":"complete",
            "ttlMs":60_000,
            "cacheScope":"private",
            "prompts":[{
                "name":"prompt",
                "arguments":[{"name":"topic"},{"name":"topic", "required":true}]
            }]
        });
        assert_eq!(
            validate_operation_result("prompts/list", &duplicate, Era::Modern),
            Err(MESSAGE_INVALID)
        );
        let oversized = json!({
            "resultType":"complete",
            "ttlMs":60_000,
            "cacheScope":"private",
            "prompts":[{
                "name":"prompt",
                "arguments":[{"name":"x".repeat(MAX_NAME_BYTES + 1)}]
            }]
        });
        assert_eq!(
            validate_operation_result("prompts/list", &oversized, Era::Modern),
            Err(MESSAGE_INVALID)
        );

        let schemas = prompt_schemas(&json!({
            "prompts":[{
                "name":"draft",
                "arguments":[
                    {"name":"topic", "required":true},
                    {"name":"tone"}
                ]
            }]
        }))
        .unwrap();
        let schema = &schemas["draft"];
        assert_eq!(
            validate_prompt_values(schema, Some(&json!({"topic":"release"}))),
            Ok(())
        );
        assert_eq!(
            validate_prompt_values(schema, Some(&json!({"tone":"short"}))),
            Err(MESSAGE_INVALID)
        );
        assert_eq!(
            validate_prompt_values(schema, Some(&json!({"topic":"release", "unknown":"value"}))),
            Err(MESSAGE_INVALID)
        );
    }

    #[test]
    fn operation_results_enforce_modern_cache_and_content_shapes() {
        let missing_cache = json!({
            "resultType":"complete",
            "tools":[]
        });
        assert_eq!(
            validate_operation_result("tools/list", &missing_cache, Era::Modern),
            Err(MESSAGE_INVALID)
        );
        assert_eq!(
            validate_operation_result("tools/list", &json!({"tools":[]}), Era::Legacy),
            Ok(())
        );

        let tool_result = json!({
            "resultType":"complete",
            "content":[{"type":"text", "text":"done"}],
            "isError":false
        });
        assert_eq!(
            validate_operation_result("tools/call", &tool_result, Era::Modern),
            Ok(())
        );
        assert_eq!(
            validate_operation_result("tools/call", &json!({"resultType":"complete"}), Era::Modern),
            Err(MESSAGE_INVALID)
        );

        let resource_result = json!({
            "resultType":"complete",
            "ttlMs":0,
            "cacheScope":"private",
            "contents":[{"uri":"fixture://readme", "text":"hello"}]
        });
        assert_eq!(
            validate_operation_result("resources/read", &resource_result, Era::Modern),
            Ok(())
        );
        let prompt_result = json!({
            "resultType":"complete",
            "messages":[{
                "role":"user",
                "content":{"type":"text", "text":"hello"}
            }]
        });
        assert_eq!(
            validate_operation_result("prompts/get", &prompt_result, Era::Modern),
            Ok(())
        );
    }
}
