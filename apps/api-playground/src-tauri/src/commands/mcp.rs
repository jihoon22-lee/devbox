//! Bounded dual-era MCP Streamable HTTP client for Protocol Lab.
//!
//! Resolved credentials, endpoint details, session IDs, and raw HTTP errors
//! remain in this process-owned state. IPC returns only stable error codes and
//! redacted, bounded protocol projections.

use crate::commands::mcp_oauth::{McpOAuthState, OAuthBearer};
use crate::commands::request::{
    is_sensitive_name, resolve_template, EnvironmentVariable, Redactor, RequestHeader,
    RequestTemplate,
};
use crate::core::mcp::{
    self, Era, EraPreference, HeaderProjection, RpcError, RpcMessage, ServerProjection,
};
use crate::core::sse::SseParser;
use crate::platform::platform_sealer;
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use zeroize::Zeroizing;

const MAX_CONNECTIONS: usize = 8;
const MAX_ACTIVE_REQUESTS: usize = 4;
const MAX_TIMELINE_EVENTS: usize = 1_000;
const MAX_TIMELINE_BYTES: usize = 4 * 1024 * 1024;
const MAX_LIST_PAGES: usize = 100;
const MAX_RETAINED_LIST_BYTES: usize = 16 * 1024 * 1024;
const MAX_ENDPOINT_BYTES: usize = 8 * 1024;
const MAX_HEADERS: usize = 100;
const MAX_HEADER_NAME_BYTES: usize = 256;
const MAX_HEADER_VALUE_BYTES: usize = 64 * 1024;
const MAX_HEADER_TOTAL_BYTES: usize = 128 * 1024;
const MAX_ENVIRONMENT_TOTAL_BYTES: usize = 1024 * 1024;
const MAX_SESSION_ID_BYTES: usize = 1024;
const MIN_TIMEOUT_MS: u64 = 100;
const MAX_TIMEOUT_MS: u64 = 120_000;

const SECRET_UNAVAILABLE: &str = "mcp_secret_unavailable";
const CONNECTION_LIMIT: &str = "mcp_connection_limit";
const CONNECT_TIMEOUT: &str = "mcp_connect_timeout";
const TRANSPORT_FAILED: &str = "mcp_transport_failed";
const REDIRECT_BLOCKED: &str = "mcp_redirect_blocked";
const RESPONSE_TYPE_INVALID: &str = "mcp_response_type_invalid";
const REQUEST_LIMIT: &str = "mcp_request_limit";
const REQUEST_TIMEOUT: &str = "mcp_request_timeout";
const REQUEST_CANCELLED: &str = "mcp_request_cancelled";
const CONNECTION_STALE: &str = "mcp_connection_stale";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpHttpProfile {
    endpoint: String,
    era: EraPreference,
    #[serde(default)]
    headers: Vec<RequestHeader>,
    timeout_ms: u64,
    #[serde(default)]
    oauth_grant_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpTimelineEntry {
    pub(crate) sequence: u32,
    pub(crate) offset_ms: u64,
    pub(crate) direction: String,
    pub(crate) kind: String,
    pub(crate) method: Option<String>,
    pub(crate) request_id: Option<String>,
    pub(crate) payload: Option<Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpConnectResult {
    pub(crate) connection_id: String,
    pub(crate) server: ServerProjection,
    pub(crate) session_managed: bool,
    pub(crate) timeline: Vec<McpTimelineEntry>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpInvokeResult {
    pub(crate) result: Option<Value>,
    pub(crate) error_code: Option<String>,
    pub(crate) rpc_error_code: Option<i64>,
    pub(crate) next_cursor: Option<String>,
    pub(crate) timeline: Vec<McpTimelineEntry>,
}

#[derive(Clone)]
struct PreparedProfile {
    endpoint: reqwest::Url,
    custom_headers: HeaderMap,
    client: reqwest::Client,
    timeout: Duration,
    redactor: Arc<Redactor>,
    oauth_grant_id: Option<String>,
    oauth_token: Option<Arc<Zeroizing<String>>>,
}

impl PreparedProfile {
    fn apply_oauth_bearer(&mut self, bearer: OAuthBearer) {
        let redaction_copy = Zeroizing::new(bearer.token.to_string());
        self.redactor = Arc::new(self.redactor.with_secret(redaction_copy));
        self.oauth_token = Some(Arc::new(bearer.token));
    }
}

#[derive(Clone)]
struct ConnectionSnapshot {
    profile: PreparedProfile,
    server: ServerProjection,
    session_id: Option<Zeroizing<String>>,
    tool_schemas: BTreeMap<String, Value>,
    prompt_schemas: BTreeMap<String, BTreeMap<String, bool>>,
}

struct StoredConnection {
    snapshot: ConnectionSnapshot,
    seen: HashMap<String, BTreeSet<Vec<u8>>>,
    list_pages: HashMap<String, usize>,
    list_bytes: HashMap<String, usize>,
    list_cursors: HashMap<String, Option<String>>,
    used_cursors: HashMap<String, BTreeSet<Vec<u8>>>,
}

#[derive(Default)]
struct McpStateInner {
    connections: HashMap<String, StoredConnection>,
    active: HashMap<(String, String), watch::Sender<bool>>,
    pending_connections: usize,
}

#[derive(Default)]
pub struct McpHttpState {
    inner: Mutex<McpStateInner>,
}

struct ConnectionAttemptGuard<'a> {
    state: &'a McpHttpState,
}

impl Drop for ConnectionAttemptGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.state.inner.lock() {
            inner.pending_connections = inner.pending_connections.saturating_sub(1);
        }
    }
}

struct ActiveRequestGuard<'a> {
    state: &'a McpHttpState,
    connection_id: String,
    request_id: String,
}

impl Drop for ActiveRequestGuard<'_> {
    fn drop(&mut self) {
        self.state
            .finish_request(&self.connection_id, &self.request_id);
    }
}

impl McpHttpState {
    fn begin_connection_attempt(&self) -> Result<ConnectionAttemptGuard<'_>, &'static str> {
        let mut inner = self.inner.lock().map_err(|_| CONNECTION_STALE)?;
        if inner
            .connections
            .len()
            .saturating_add(inner.pending_connections)
            >= MAX_CONNECTIONS
        {
            return Err(CONNECTION_LIMIT);
        }
        inner.pending_connections = inner
            .pending_connections
            .checked_add(1)
            .ok_or(CONNECTION_LIMIT)?;
        Ok(ConnectionAttemptGuard { state: self })
    }

    fn insert_connection(&self, snapshot: &ConnectionSnapshot) -> Result<String, &'static str> {
        let mut inner = self.inner.lock().map_err(|_| CONNECTION_STALE)?;
        if inner.connections.len() >= MAX_CONNECTIONS {
            return Err(CONNECTION_LIMIT);
        }
        for _ in 0..4 {
            let id = random_hex_128()?;
            if !inner.connections.contains_key(&id) {
                inner.connections.insert(
                    id.clone(),
                    StoredConnection {
                        snapshot: snapshot.clone(),
                        seen: HashMap::new(),
                        list_pages: HashMap::new(),
                        list_bytes: HashMap::new(),
                        list_cursors: HashMap::new(),
                        used_cursors: HashMap::new(),
                    },
                );
                return Ok(id);
            }
        }
        Err(CONNECTION_LIMIT)
    }

    fn begin_request(
        &self,
        connection_id: &str,
        request_id: &str,
    ) -> Result<(ConnectionSnapshot, watch::Receiver<bool>), &'static str> {
        validate_connection_id(connection_id)?;
        mcp::validate_request_id(request_id)?;
        let mut inner = self.inner.lock().map_err(|_| CONNECTION_STALE)?;
        let snapshot = inner
            .connections
            .get(connection_id)
            .ok_or(CONNECTION_STALE)?
            .snapshot
            .clone();
        let active_for_connection = inner
            .active
            .keys()
            .filter(|(id, _)| id == connection_id)
            .count();
        let key = (connection_id.to_string(), request_id.to_string());
        if active_for_connection >= MAX_ACTIVE_REQUESTS || inner.active.contains_key(&key) {
            return Err(REQUEST_LIMIT);
        }
        let (sender, receiver) = watch::channel(false);
        inner.active.insert(key, sender);
        Ok((snapshot, receiver))
    }

    fn finish_request(&self, connection_id: &str, request_id: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            inner
                .active
                .remove(&(connection_id.to_string(), request_id.to_string()));
        }
    }

    fn cancel_request(&self, connection_id: &str, request_id: &str) -> Result<bool, &'static str> {
        validate_connection_id(connection_id)?;
        mcp::validate_request_id(request_id)?;
        let inner = self.inner.lock().map_err(|_| CONNECTION_STALE)?;
        let Some(sender) = inner
            .active
            .get(&(connection_id.to_string(), request_id.to_string()))
        else {
            return Ok(false);
        };
        sender.send(true).map_err(|_| CONNECTION_STALE)?;
        Ok(true)
    }

    fn take_connection(&self, connection_id: &str) -> Result<ConnectionSnapshot, &'static str> {
        validate_connection_id(connection_id)?;
        let mut inner = self.inner.lock().map_err(|_| CONNECTION_STALE)?;
        for ((id, _), sender) in inner.active.iter() {
            if id == connection_id {
                let _ = sender.send(true);
            }
        }
        inner.active.retain(|(id, _), _| id != connection_id);
        inner
            .connections
            .remove(connection_id)
            .map(|stored| stored.snapshot)
            .ok_or(CONNECTION_STALE)
    }

    fn update_list_result(
        &self,
        connection_id: &str,
        method: &str,
        result: &Value,
        requested_cursor: Option<&str>,
    ) -> Result<(), &'static str> {
        let (list_key, identity_key) = match method {
            "tools/list" => ("tools", "name"),
            "resources/list" => ("resources", "uri"),
            "resources/templates/list" => ("resourceTemplates", "uriTemplate"),
            "prompts/list" => ("prompts", "name"),
            _ => return Ok(()),
        };
        let items = result
            .get(list_key)
            .and_then(Value::as_array)
            .ok_or(mcp::MESSAGE_INVALID)?;
        let next_cursor = result
            .get("nextCursor")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let mut inner = self.inner.lock().map_err(|_| CONNECTION_STALE)?;
        let stored = inner
            .connections
            .get_mut(connection_id)
            .ok_or(CONNECTION_STALE)?;
        let current_page_count = stored.list_pages.get(method).copied().unwrap_or_default();
        if current_page_count == 0 {
            if requested_cursor.is_some() {
                return Err(mcp::CURSOR_INVALID);
            }
        } else if !matches!(
            stored.list_cursors.get(method),
            Some(Some(expected)) if requested_cursor == Some(expected.as_str())
        ) {
            return Err(mcp::CURSOR_INVALID);
        }
        let next_page_count = stored
            .list_pages
            .get(method)
            .copied()
            .unwrap_or_default()
            .checked_add(1)
            .ok_or(mcp::RESPONSE_TOO_LARGE)?;
        if next_page_count > MAX_LIST_PAGES {
            return Err(mcp::RESPONSE_TOO_LARGE);
        }
        let page_bytes = serde_json::to_vec(items)
            .map_err(|_| mcp::MESSAGE_INVALID)?
            .len();
        let next_list_bytes = stored
            .list_bytes
            .get(method)
            .copied()
            .unwrap_or_default()
            .checked_add(page_bytes)
            .ok_or(mcp::RESPONSE_TOO_LARGE)?;
        if next_list_bytes > MAX_RETAINED_LIST_BYTES {
            return Err(mcp::RESPONSE_TOO_LARGE);
        }
        let mut next_seen = stored.seen.get(method).cloned().unwrap_or_default();
        let mut next_used_cursors = stored.used_cursors.get(method).cloned().unwrap_or_default();
        if let Some(cursor) = requested_cursor {
            if !next_used_cursors.insert(cursor.as_bytes().to_vec()) {
                return Err(mcp::CURSOR_INVALID);
            }
        }
        if next_cursor.as_ref().is_some_and(|cursor| {
            requested_cursor == Some(cursor.as_str())
                || next_used_cursors.contains(cursor.as_bytes())
        }) {
            return Err(mcp::CURSOR_INVALID);
        }
        for item in items {
            let identity = item
                .get(identity_key)
                .and_then(Value::as_str)
                .ok_or(mcp::MESSAGE_INVALID)?;
            if !next_seen.insert(identity.as_bytes().to_vec()) {
                return Err(mcp::MESSAGE_INVALID);
            }
            if next_seen.len() > mcp::MAX_LIST_ITEMS {
                return Err(mcp::RESPONSE_TOO_LARGE);
            }
        }
        let mut next_tool_schemas = stored.snapshot.tool_schemas.clone();
        let mut next_prompt_schemas = stored.snapshot.prompt_schemas.clone();
        if method == "tools/list" {
            for (name, schema) in mcp::tool_schemas(result)? {
                if let Some(existing) = next_tool_schemas.get(&name) {
                    if existing != &schema {
                        return Err(mcp::MESSAGE_INVALID);
                    }
                } else {
                    next_tool_schemas.insert(name, schema);
                }
            }
        } else if method == "prompts/list" {
            for (name, schema) in mcp::prompt_schemas(result)? {
                if let Some(existing) = next_prompt_schemas.get(&name) {
                    if existing != &schema {
                        return Err(mcp::MESSAGE_INVALID);
                    }
                } else {
                    next_prompt_schemas.insert(name, schema);
                }
            }
        }
        stored.seen.insert(method.to_string(), next_seen);
        stored
            .list_pages
            .insert(method.to_string(), next_page_count);
        stored
            .list_bytes
            .insert(method.to_string(), next_list_bytes);
        stored.list_cursors.insert(method.to_string(), next_cursor);
        stored
            .used_cursors
            .insert(method.to_string(), next_used_cursors);
        stored.snapshot.tool_schemas = next_tool_schemas;
        stored.snapshot.prompt_schemas = next_prompt_schemas;
        Ok(())
    }

    fn validate_list_request(
        &self,
        connection_id: &str,
        method: &str,
        params: &Value,
    ) -> Result<(), &'static str> {
        if !matches!(
            method,
            "tools/list" | "resources/list" | "resources/templates/list" | "prompts/list"
        ) {
            return Ok(());
        }
        validate_connection_id(connection_id)?;
        let requested_cursor = params.get("cursor").and_then(Value::as_str);
        let inner = self.inner.lock().map_err(|_| CONNECTION_STALE)?;
        let stored = inner
            .connections
            .get(connection_id)
            .ok_or(CONNECTION_STALE)?;
        let pages = stored.list_pages.get(method).copied().unwrap_or_default();
        if pages == 0 {
            if requested_cursor.is_none() {
                Ok(())
            } else {
                Err(mcp::CURSOR_INVALID)
            }
        } else {
            match stored.list_cursors.get(method) {
                Some(Some(expected)) if requested_cursor == Some(expected.as_str()) => Ok(()),
                _ => Err(mcp::CURSOR_INVALID),
            }
        }
    }
}

struct TransportExchange {
    status: u16,
    messages: Vec<Value>,
    session_id: Option<Zeroizing<String>>,
}

pub(crate) struct InterpretedExchange {
    pub(crate) final_result: Result<Value, RpcError>,
    pub(crate) timeline: Vec<McpTimelineEntry>,
}

#[tauri::command]
pub async fn connect_mcp_http(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<McpHttpState>>,
    oauth: tauri::State<'_, Arc<McpOAuthState>>,
    profile: McpHttpProfile,
    environment: Vec<EnvironmentVariable>,
) -> Result<McpConnectResult, String> {
    let _attempt = state
        .begin_connection_attempt()
        .map_err(ToOwned::to_owned)?;
    connect_mcp_http_inner(
        &app,
        state.inner().as_ref(),
        oauth.inner().as_ref(),
        profile,
        environment,
    )
    .await
}

async fn connect_mcp_http_inner(
    app: &tauri::AppHandle,
    state: &McpHttpState,
    oauth: &McpOAuthState,
    profile: McpHttpProfile,
    environment: Vec<EnvironmentVariable>,
) -> Result<McpConnectResult, String> {
    let preference = profile.era;
    let mut prepared = prepare_profile(profile, environment)?;
    if let Some(grant_id) = prepared.oauth_grant_id.clone() {
        let (_sender, mut cancellation) = watch::channel(false);
        let endpoint = prepared.endpoint.to_string();
        let bearer = oauth
            .bearer_for(app, &grant_id, &endpoint, &mut cancellation)
            .await?;
        prepared.apply_oauth_bearer(bearer);
    }
    let (server, session_id, timeline) = match preference {
        EraPreference::Modern => connect_modern(&prepared, false).await?,
        EraPreference::Legacy => connect_legacy(&prepared, Vec::new()).await?,
        EraPreference::Auto => match connect_modern(&prepared, true).await {
            Ok(connected) => connected,
            Err(code) if code == "mcp_legacy_fallback" => {
                connect_legacy(&prepared, Vec::new()).await?
            }
            Err(code) if code == "mcp_legacy_version_negotiated" => {
                connect_legacy(&prepared, Vec::new()).await?
            }
            Err(code) => return Err(code),
        },
    };
    let session_managed = session_id.is_some();
    let snapshot = ConnectionSnapshot {
        profile: prepared,
        server: server.clone(),
        session_id,
        tool_schemas: BTreeMap::new(),
        prompt_schemas: BTreeMap::new(),
    };
    let connection_id = match state.insert_connection(&snapshot) {
        Ok(connection_id) => connection_id,
        Err(code) => {
            if snapshot.server.era == Era::Legacy {
                if let Some(session_id) = snapshot.session_id.as_ref() {
                    let _ = terminate_legacy_session(&snapshot.profile, session_id).await;
                }
            }
            return Err(code.to_string());
        }
    };
    Ok(McpConnectResult {
        connection_id,
        server,
        session_managed,
        timeline,
    })
}

#[tauri::command]
pub async fn invoke_mcp_http(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<McpHttpState>>,
    oauth: tauri::State<'_, Arc<McpOAuthState>>,
    connection_id: String,
    request_id: String,
    method: String,
    params: Value,
) -> Result<McpInvokeResult, String> {
    mcp::validate_operation(&method, &params).map_err(ToOwned::to_owned)?;
    state
        .validate_list_request(&connection_id, &method, &params)
        .map_err(ToOwned::to_owned)?;
    let requested_cursor = params
        .get("cursor")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let (mut connection, mut cancellation) = state
        .begin_request(&connection_id, &request_id)
        .map_err(ToOwned::to_owned)?;
    let _request = ActiveRequestGuard {
        state: state.inner().as_ref(),
        connection_id: connection_id.clone(),
        request_id: request_id.clone(),
    };
    if let Some(grant_id) = connection.profile.oauth_grant_id.clone() {
        let endpoint = connection.profile.endpoint.to_string();
        let bearer = oauth
            .bearer_for(&app, &grant_id, &endpoint, &mut cancellation)
            .await?;
        connection.profile.apply_oauth_bearer(bearer);
    }
    let outcome = invoke_inner(&connection, &request_id, &method, params, &mut cancellation).await;
    if matches!(&outcome, Err(code) if code == CONNECTION_STALE) {
        let _ = state.take_connection(&connection_id);
    }
    let outcome = match outcome {
        Ok(mut result) => (|| {
            if let Some(value) = result.result.take() {
                let value = if method == "tools/list" && connection.server.era == Era::Modern {
                    let (filtered, rejected) =
                        mcp::filter_invalid_http_tools(&value).map_err(ToOwned::to_owned)?;
                    if rejected > 0 {
                        eprintln!(
                            "mcp: excluded {rejected} tool definitions with invalid HTTP header annotations"
                        );
                    }
                    filtered
                } else {
                    value
                };
                let (value, rejected) = filter_reflected_list_definitions(
                    &value,
                    &method,
                    connection.profile.redactor.as_ref(),
                )?;
                if rejected > 0 {
                    eprintln!(
                        "mcp: excluded {rejected} actionable definitions containing reflected credentials"
                    );
                }
                result.result = Some(value);
            }
            if let Some(value) = result.result.clone() {
                state
                    .update_list_result(
                        &connection_id,
                        &method,
                        &value,
                        requested_cursor.as_deref(),
                    )
                    .map_err(ToOwned::to_owned)?;
                let projected =
                    project_result_for_ipc(connection.profile.redactor.as_ref(), &value, &method)?;
                mcp::validate_json(&projected, mcp::MAX_RESPONSE_BYTES)
                    .map_err(ToOwned::to_owned)?;
                result.result = Some(projected);
            }
            Ok(result)
        })(),
        Err(code) => Err(code),
    };
    outcome
}

#[tauri::command]
pub fn cancel_mcp_http(
    state: tauri::State<'_, Arc<McpHttpState>>,
    connection_id: String,
    request_id: String,
) -> Result<bool, String> {
    state
        .cancel_request(&connection_id, &request_id)
        .map_err(ToOwned::to_owned)
}

#[tauri::command]
pub async fn disconnect_mcp_http(
    state: tauri::State<'_, Arc<McpHttpState>>,
    connection_id: String,
) -> Result<(), String> {
    let connection = state
        .take_connection(&connection_id)
        .map_err(ToOwned::to_owned)?;
    if connection.server.era == Era::Legacy {
        if let Some(session_id) = connection.session_id.as_ref() {
            terminate_legacy_session(&connection.profile, session_id).await?;
        }
    }
    Ok(())
}

async fn connect_modern(
    profile: &PreparedProfile,
    allow_fallback: bool,
) -> Result<
    (
        ServerProjection,
        Option<Zeroizing<String>>,
        Vec<McpTimelineEntry>,
    ),
    String,
> {
    let request_id = "discover-1";
    let params = json!({});
    let request = mcp::build_modern_request(request_id, "server/discover", params.clone())
        .map_err(ToOwned::to_owned)?;
    let headers = mcp::derived_headers(
        Era::Modern,
        mcp::MODERN_VERSION,
        "server/discover",
        &params,
        None,
    )
    .map_err(ToOwned::to_owned)?;
    let (_sender, mut cancellation) = watch::channel(false);
    let started = Instant::now();
    let exchange = execute_message(profile, &headers, &request, None, &mut cancellation)
        .await
        .map_err(map_connect_error)?;
    validate_response_session(Era::Modern, None, exchange.session_id.as_ref())?;
    let status = exchange.status;
    let modern_error_evidence = exchange.messages.iter().any(mcp::has_modern_error_evidence);
    let interpreted = interpret_exchange(
        exchange.messages,
        request_id,
        Era::Modern,
        profile.redactor.as_ref(),
        started,
        "server/discover",
        &params,
    );
    match interpreted {
        Ok(interpreted) => match interpreted.final_result {
            Ok(result) if (200..300).contains(&status) => {
                let server = sanitize_server_projection(
                    profile.redactor.as_ref(),
                    mcp::project_discover(&result).map_err(ToOwned::to_owned)?,
                )?;
                Ok((server, None, interpreted.timeline))
            }
            Ok(_) => Err(TRANSPORT_FAILED.to_string()),
            Err(error) if mcp::is_recognized_modern_error(&error) => match error.code {
                -32022 => {
                    let versions = mcp::supported_versions_from_error(&error);
                    if allow_fallback
                        && !versions.iter().any(|value| value == mcp::MODERN_VERSION)
                        && versions.iter().any(|value| value == mcp::LEGACY_VERSION)
                    {
                        Err("mcp_legacy_version_negotiated".into())
                    } else {
                        Err(mcp::VERSION_UNSUPPORTED.into())
                    }
                }
                -32021 => Err(mcp::CAPABILITY_UNAVAILABLE.into()),
                _ => Err(mcp::MESSAGE_INVALID.into()),
            },
            Err(error) if status == 404 && mcp::is_modern_method_not_found(&error) => {
                Err(mcp::VERSION_UNSUPPORTED.into())
            }
            Err(_)
                if allow_fallback
                    && !modern_error_evidence
                    && matches!(status, 400 | 404 | 405) =>
            {
                Err("mcp_legacy_fallback".into())
            }
            Err(_) => Err(mcp::VERSION_UNSUPPORTED.into()),
        },
        Err(_) if allow_fallback && !modern_error_evidence && matches!(status, 400 | 404 | 405) => {
            Err("mcp_legacy_fallback".into())
        }
        Err(code) => Err(code.to_string()),
    }
}

async fn connect_legacy(
    profile: &PreparedProfile,
    mut timeline: Vec<McpTimelineEntry>,
) -> Result<
    (
        ServerProjection,
        Option<Zeroizing<String>>,
        Vec<McpTimelineEntry>,
    ),
    String,
> {
    let request_id = "initialize-1";
    let request = mcp::build_legacy_initialize(request_id).map_err(ToOwned::to_owned)?;
    let (_sender, mut cancellation) = watch::channel(false);
    let started = Instant::now();
    let exchange = execute_message(profile, &[], &request, None, &mut cancellation)
        .await
        .map_err(map_connect_error)?;
    if !(200..300).contains(&exchange.status) {
        return Err(mcp::VERSION_UNSUPPORTED.into());
    }
    let session_id = exchange.session_id;
    let outcome = async {
        let interpreted = interpret_exchange(
            exchange.messages,
            request_id,
            Era::Legacy,
            profile.redactor.as_ref(),
            started,
            "initialize",
            &json!({}),
        )
        .map_err(ToOwned::to_owned)?;
        timeline.extend(interpreted.timeline);
        let result = interpreted
            .final_result
            .map_err(|_| mcp::VERSION_UNSUPPORTED.to_string())?;
        let server = sanitize_server_projection(
            profile.redactor.as_ref(),
            mcp::project_legacy_initialize(&result).map_err(ToOwned::to_owned)?,
        )?;

        let initialized = mcp::build_legacy_initialized();
        let headers = vec![HeaderProjection {
            name: "MCP-Protocol-Version".into(),
            value: mcp::LEGACY_VERSION.into(),
        }];
        send_notification(
            profile,
            &headers,
            &initialized,
            session_id.as_ref().map(|value| value.as_str()),
        )
        .await
        .map_err(map_connect_error)?;
        timeline.push(McpTimelineEntry {
            sequence: next_sequence(&timeline)?,
            offset_ms: elapsed_ms(started),
            direction: "outgoing".into(),
            kind: "notification".into(),
            method: Some("notifications/initialized".into()),
            request_id: None,
            payload: None,
        });
        Ok((server, timeline))
    }
    .await;

    match outcome {
        Ok((server, timeline)) => Ok((server, session_id, timeline)),
        Err(code) => {
            if let Some(session_id) = session_id.as_ref() {
                let _ = terminate_legacy_session(profile, session_id).await;
            }
            Err(code)
        }
    }
}

async fn invoke_inner(
    connection: &ConnectionSnapshot,
    request_id: &str,
    method: &str,
    params: Value,
    cancellation: &mut watch::Receiver<bool>,
) -> Result<McpInvokeResult, String> {
    if !mcp::has_capability(&connection.server.capabilities, method) {
        return Err(mcp::CAPABILITY_UNAVAILABLE.into());
    }
    let era = connection.server.era;
    let request = match era {
        Era::Modern => mcp::build_modern_request(request_id, method, params.clone()),
        Era::Legacy => mcp::build_legacy_request(request_id, method, params.clone()),
    }
    .map_err(ToOwned::to_owned)?;
    let tool_schema = if method == "tools/call" {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| mcp::MESSAGE_INVALID.to_string())?;
        let schema = connection
            .tool_schemas
            .get(name)
            .ok_or_else(|| mcp::SCHEMA_UNSUPPORTED.to_string())?;
        mcp::validate_tool_arguments(
            schema,
            params
                .get("arguments")
                .ok_or_else(|| mcp::MESSAGE_INVALID.to_string())?,
        )
        .map_err(ToOwned::to_owned)?;
        Some(schema)
    } else {
        None
    };
    if method == "prompts/get" {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| mcp::MESSAGE_INVALID.to_string())?;
        let schema = connection
            .prompt_schemas
            .get(name)
            .ok_or_else(|| mcp::MESSAGE_INVALID.to_string())?;
        mcp::validate_prompt_values(schema, params.get("arguments")).map_err(ToOwned::to_owned)?;
    }
    let headers = mcp::derived_headers(
        era,
        &connection.server.protocol_version,
        method,
        &params,
        tool_schema,
    )
    .map_err(ToOwned::to_owned)?;
    let started = Instant::now();
    let exchange = execute_message(
        &connection.profile,
        &headers,
        &request,
        connection.session_id.as_ref().map(|value| value.as_str()),
        cancellation,
    )
    .await;
    let exchange = match exchange {
        Err(code) if code == REQUEST_CANCELLED && era == Era::Legacy => {
            let cancel_timeout = connection.profile.timeout.min(Duration::from_secs(2));
            let _ =
                tokio::time::timeout(cancel_timeout, send_legacy_cancel(connection, request_id))
                    .await;
            return Err(code);
        }
        result => result?,
    };
    validate_response_session(
        era,
        connection.session_id.as_ref().map(|value| value.as_str()),
        exchange.session_id.as_ref(),
    )?;
    let successful_status = (200..300).contains(&exchange.status);
    if era == Era::Legacy && connection.session_id.is_some() && exchange.status == 404 {
        return Err(CONNECTION_STALE.into());
    }
    let interpreted = interpret_exchange(
        exchange.messages,
        request_id,
        era,
        connection.profile.redactor.as_ref(),
        started,
        method,
        &params,
    )
    .map_err(ToOwned::to_owned)?;
    match interpreted.final_result {
        Ok(result) if successful_status => {
            mcp::validate_operation_result(method, &result, era).map_err(ToOwned::to_owned)?;
            let next_cursor = matches!(
                method,
                "tools/list" | "resources/list" | "resources/templates/list" | "prompts/list"
            )
            .then(|| result.get("nextCursor").and_then(Value::as_str))
            .flatten()
            .map(ToOwned::to_owned);
            Ok(McpInvokeResult {
                result: Some(result),
                error_code: None,
                rpc_error_code: None,
                next_cursor,
                timeline: interpreted.timeline,
            })
        }
        Ok(_) => Err(TRANSPORT_FAILED.into()),
        Err(error) => Ok(McpInvokeResult {
            result: None,
            error_code: Some(
                match error.code {
                    -32022 => mcp::VERSION_UNSUPPORTED,
                    -32021 | -32601 => mcp::CAPABILITY_UNAVAILABLE,
                    -32020 => mcp::MESSAGE_INVALID,
                    _ => mcp::SERVER_ERROR,
                }
                .into(),
            ),
            rpc_error_code: Some(error.code),
            next_cursor: None,
            timeline: interpreted.timeline,
        }),
    }
}

async fn send_legacy_cancel(
    connection: &ConnectionSnapshot,
    request_id: &str,
) -> Result<(), String> {
    let notification = mcp::build_legacy_cancelled(request_id).map_err(ToOwned::to_owned)?;
    let headers = vec![HeaderProjection {
        name: "MCP-Protocol-Version".into(),
        value: connection.server.protocol_version.clone(),
    }];
    send_notification(
        &connection.profile,
        &headers,
        &notification,
        connection.session_id.as_ref().map(|value| value.as_str()),
    )
    .await
}

fn prepare_profile(
    profile: McpHttpProfile,
    environment: Vec<EnvironmentVariable>,
) -> Result<PreparedProfile, String> {
    if profile.timeout_ms < MIN_TIMEOUT_MS
        || profile.timeout_ms > MAX_TIMEOUT_MS
        || profile.endpoint.len() > MAX_ENDPOINT_BYTES
        || profile.endpoint.bytes().any(|byte| byte.is_ascii_control())
        || profile.headers.len() > MAX_HEADERS
        || environment.len() > MAX_HEADERS
    {
        return Err(mcp::INVALID_PROFILE.into());
    }
    let mut raw_header_bytes = 0usize;
    for header in &profile.headers {
        if header.key.len() > MAX_HEADER_NAME_BYTES || header.value.len() > MAX_HEADER_VALUE_BYTES {
            return Err(mcp::INVALID_PROFILE.into());
        }
        raw_header_bytes = raw_header_bytes
            .checked_add(header.key.len())
            .and_then(|bytes| bytes.checked_add(header.value.len()))
            .and_then(|bytes| bytes.checked_add(4))
            .ok_or_else(|| mcp::INVALID_PROFILE.to_string())?;
        if raw_header_bytes > MAX_HEADER_TOTAL_BYTES {
            return Err(mcp::INVALID_PROFILE.into());
        }
    }
    let mut environment_bytes = 0usize;
    let mut environment_names = BTreeSet::new();
    for variable in &environment {
        if variable.key.is_empty()
            || variable.key.len() > MAX_HEADER_NAME_BYTES
            || !variable.key.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '-')
            })
            || variable.value.len() > mcp::MAX_JSON_STRING_BYTES
            || !environment_names.insert(variable.key.as_bytes().to_vec())
        {
            return Err(mcp::INVALID_PROFILE.into());
        }
        environment_bytes = environment_bytes
            .checked_add(variable.key.len())
            .and_then(|bytes| bytes.checked_add(variable.value.len()))
            .ok_or_else(|| mcp::INVALID_PROFILE.to_string())?;
        if environment_bytes > MAX_ENVIRONMENT_TOTAL_BYTES {
            return Err(mcp::INVALID_PROFILE.into());
        }
    }
    if let Some(grant_id) = profile.oauth_grant_id.as_deref() {
        crate::commands::mcp_oauth::validate_grant_id(grant_id)?;
    }
    let oauth_grant_id = profile.oauth_grant_id;
    let template = RequestTemplate {
        method: "POST".into(),
        url: profile.endpoint,
        headers: profile.headers,
        cookies: Vec::new(),
        multipart: Vec::new(),
        params: Vec::new(),
        body_kind: "none".into(),
        body: String::new(),
        auth: None,
        timeout_ms: profile.timeout_ms,
        graphql: None,
    };
    let sealer = platform_sealer();
    let (resolved, environment_secrets) =
        resolve_template(&template, &environment, sealer.as_ref())
            .map_err(|_| SECRET_UNAVAILABLE.to_string())?;
    if oauth_grant_id.is_some()
        && resolved
            .headers
            .iter()
            .any(|header| header.enabled && header.key.eq_ignore_ascii_case("authorization"))
    {
        return Err(mcp::INVALID_PROFILE.into());
    }
    if resolved.url.len() > MAX_ENDPOINT_BYTES
        || resolved.url.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(mcp::INVALID_PROFILE.into());
    }
    let endpoint =
        reqwest::Url::parse(&resolved.url).map_err(|_| mcp::INVALID_PROFILE.to_string())?;
    validate_endpoint(&endpoint)?;
    let custom_headers = validate_custom_headers(&resolved.headers)?;
    let timeout = Duration::from_millis(profile.timeout_ms);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(timeout)
        .timeout(timeout)
        .build()
        .map_err(|_| mcp::INVALID_PROFILE.to_string())?;
    let redactor = Arc::new(Redactor::for_request(&resolved, environment_secrets));
    Ok(PreparedProfile {
        endpoint,
        custom_headers,
        client,
        timeout,
        redactor,
        oauth_grant_id,
        oauth_token: None,
    })
}

fn validate_endpoint(endpoint: &reqwest::Url) -> Result<(), String> {
    if !matches!(endpoint.scheme(), "http" | "https")
        || endpoint.host_str().is_none()
        || endpoint.fragment().is_some()
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
    {
        return Err(mcp::INVALID_PROFILE.into());
    }
    if endpoint
        .query_pairs()
        .any(|(key, _)| is_sensitive_name(&key))
    {
        return Err(mcp::INVALID_PROFILE.into());
    }
    Ok(())
}

fn validate_custom_headers(headers: &[RequestHeader]) -> Result<HeaderMap, String> {
    let mut total = 0usize;
    let mut output = HeaderMap::new();
    for header in headers.iter().filter(|header| header.enabled) {
        if header.key.is_empty()
            || header.key.len() > MAX_HEADER_NAME_BYTES
            || header.value.len() > MAX_HEADER_VALUE_BYTES
        {
            return Err(mcp::INVALID_PROFILE.into());
        }
        let normalized = header.key.to_ascii_lowercase();
        if matches!(
            normalized.as_str(),
            "accept"
                | "accept-encoding"
                | "content-type"
                | "content-length"
                | "content-encoding"
                | "transfer-encoding"
                | "host"
                | "connection"
                | "keep-alive"
                | "proxy-connection"
                | "te"
                | "trailer"
                | "upgrade"
                | "expect"
                | "cookie"
                | "mcp-protocol-version"
                | "mcp-method"
                | "mcp-name"
                | "mcp-session-id"
        ) || normalized.starts_with("mcp-param-")
        {
            return Err(mcp::INVALID_PROFILE.into());
        }
        total = total
            .checked_add(header.key.len())
            .and_then(|value| value.checked_add(header.value.len()))
            .and_then(|value| value.checked_add(4))
            .ok_or_else(|| mcp::INVALID_PROFILE.to_string())?;
        if total > MAX_HEADER_TOTAL_BYTES {
            return Err(mcp::INVALID_PROFILE.into());
        }
        let name = HeaderName::from_bytes(header.key.as_bytes())
            .map_err(|_| mcp::INVALID_PROFILE.to_string())?;
        let value =
            HeaderValue::from_str(&header.value).map_err(|_| mcp::INVALID_PROFILE.to_string())?;
        output.append(name, value);
    }
    Ok(output)
}

async fn execute_message(
    profile: &PreparedProfile,
    derived_headers: &[HeaderProjection],
    message: &Value,
    session_id: Option<&str>,
    cancellation: &mut watch::Receiver<bool>,
) -> Result<TransportExchange, String> {
    let body = serde_json::to_vec(message).map_err(|_| mcp::MESSAGE_INVALID.to_string())?;
    if body.len() > mcp::MAX_REQUEST_BYTES {
        return Err(mcp::REQUEST_TOO_LARGE.into());
    }
    let mut headers = profile.custom_headers.clone();
    if let Some(token) = &profile.oauth_token {
        let value = Zeroizing::new(format!("Bearer {}", token.as_str()));
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&value).map_err(|_| mcp::INVALID_PROFILE.to_string())?,
        );
    }
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/json, text/event-stream"),
    );
    for header in derived_headers {
        let name = HeaderName::from_bytes(header.name.as_bytes())
            .map_err(|_| mcp::MESSAGE_INVALID.to_string())?;
        let value =
            HeaderValue::from_str(&header.value).map_err(|_| mcp::MESSAGE_INVALID.to_string())?;
        headers.insert(name, value);
    }
    if let Some(session_id) = session_id {
        headers.insert(
            HeaderName::from_static("mcp-session-id"),
            HeaderValue::from_str(session_id).map_err(|_| mcp::MESSAGE_INVALID.to_string())?,
        );
    }
    let request = profile
        .client
        .post(profile.endpoint.clone())
        .headers(headers)
        .body(body);
    let response = tokio::select! {
        biased;
        changed = cancellation.changed() => {
            let _ = changed;
            return Err(REQUEST_CANCELLED.into());
        }
        response = request.send() => response.map_err(map_transport_error)?,
    };
    if response.status().is_redirection() {
        return Err(REDIRECT_BLOCKED.into());
    }
    read_exchange(response, cancellation).await
}

async fn send_notification(
    profile: &PreparedProfile,
    derived_headers: &[HeaderProjection],
    message: &Value,
    session_id: Option<&str>,
) -> Result<(), String> {
    let (_sender, mut cancellation) = watch::channel(false);
    let exchange = execute_message(
        profile,
        derived_headers,
        message,
        session_id,
        &mut cancellation,
    )
    .await?;
    validate_response_session(Era::Legacy, session_id, exchange.session_id.as_ref())?;
    if exchange.status != 202 || !exchange.messages.is_empty() {
        return Err(TRANSPORT_FAILED.into());
    }
    Ok(())
}

async fn read_exchange(
    response: reqwest::Response,
    cancellation: &mut watch::Receiver<bool>,
) -> Result<TransportExchange, String> {
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(|value| value.trim().to_ascii_lowercase());
    let session_id = response
        .headers()
        .get("mcp-session-id")
        .map(|value| validate_session_id(value.as_bytes()))
        .transpose()?;
    let mut stream = response.bytes_stream();
    if content_type.as_deref() == Some("text/event-stream") {
        let mut parser = SseParser::new();
        let mut messages = Vec::new();
        let mut received_bytes = 0usize;
        while let Some(chunk) = next_chunk(&mut stream, cancellation).await? {
            received_bytes = received_bytes
                .checked_add(chunk.len())
                .ok_or_else(|| mcp::RESPONSE_TOO_LARGE.to_string())?;
            if received_bytes > mcp::MAX_RESPONSE_BYTES {
                return Err(mcp::RESPONSE_TOO_LARGE.into());
            }
            for event in parser
                .feed(&chunk)
                .map_err(|_| mcp::MESSAGE_INVALID.to_string())?
            {
                push_sse_message(&mut messages, &event.data)?;
            }
        }
        for event in parser
            .finish()
            .map_err(|_| mcp::MESSAGE_INVALID.to_string())?
        {
            push_sse_message(&mut messages, &event.data)?;
        }
        return Ok(TransportExchange {
            status,
            messages,
            session_id,
        });
    }

    let mut bytes = Vec::new();
    while let Some(chunk) = next_chunk(&mut stream, cancellation).await? {
        if bytes.len().saturating_add(chunk.len()) > mcp::MAX_RESPONSE_BYTES {
            return Err(mcp::RESPONSE_TOO_LARGE.into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let fallback_status = matches!(status, 400 | 404 | 405);
    if bytes.is_empty() && (status == 202 || fallback_status) {
        return Ok(TransportExchange {
            status,
            messages: Vec::new(),
            session_id,
        });
    }
    let content_type_is_json = content_type.as_deref() == Some("application/json")
        || content_type
            .as_deref()
            .is_some_and(|value| value.ends_with("+json"));
    if !content_type_is_json && !fallback_status {
        return Err(RESPONSE_TYPE_INVALID.into());
    }
    if fallback_status && !content_type_is_json {
        return Ok(TransportExchange {
            status,
            messages: Vec::new(),
            session_id,
        });
    }
    let value = match serde_json::from_slice::<Value>(&bytes) {
        Ok(value) => value,
        Err(_) if fallback_status => {
            return Ok(TransportExchange {
                status,
                messages: Vec::new(),
                session_id,
            })
        }
        Err(_) => return Err(mcp::MESSAGE_INVALID.into()),
    };
    mcp::validate_json(&value, mcp::MAX_RESPONSE_BYTES).map_err(ToOwned::to_owned)?;
    Ok(TransportExchange {
        status,
        messages: vec![value],
        session_id,
    })
}

async fn next_chunk<S>(
    stream: &mut S,
    cancellation: &mut watch::Receiver<bool>,
) -> Result<Option<bytes::Bytes>, String>
where
    S: futures_util::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin,
{
    tokio::select! {
        biased;
        changed = cancellation.changed() => {
            let _ = changed;
            Err(REQUEST_CANCELLED.into())
        }
        chunk = stream.next() => match chunk {
            Some(Ok(chunk)) => Ok(Some(chunk)),
            Some(Err(error)) => Err(map_transport_error(error)),
            None => Ok(None),
        }
    }
}

fn push_sse_message(messages: &mut Vec<Value>, data: &str) -> Result<(), String> {
    if data.is_empty() {
        return Ok(());
    }
    if messages.len() >= MAX_TIMELINE_EVENTS {
        return Err(mcp::RESPONSE_TOO_LARGE.into());
    }
    let value =
        serde_json::from_str::<Value>(data).map_err(|_| mcp::MESSAGE_INVALID.to_string())?;
    mcp::validate_json(&value, mcp::MAX_RESPONSE_BYTES).map_err(ToOwned::to_owned)?;
    messages.push(value);
    Ok(())
}

pub(crate) fn interpret_exchange(
    messages: Vec<Value>,
    expected_id: &str,
    era: Era,
    redactor: &Redactor,
    started: Instant,
    outgoing_method: &str,
    outgoing_params: &Value,
) -> Result<InterpretedExchange, &'static str> {
    let mut timeline = vec![McpTimelineEntry {
        sequence: 1,
        offset_ms: 0,
        direction: "outgoing".into(),
        kind: "request".into(),
        method: Some(outgoing_method.to_string()),
        request_id: Some(expected_id.to_string()),
        payload: Some(redact_value(
            redactor,
            &mcp::safe_request_projection(outgoing_method, outgoing_params),
        )),
    }];
    let mut final_result = None;
    let mut timeline_bytes = timeline_size(&timeline[0]);
    for value in messages {
        if final_result.is_some() {
            return Err(mcp::MESSAGE_INVALID);
        }
        match mcp::parse_rpc_message(value, expected_id, era)? {
            RpcMessage::Notification { method, params } => {
                let payload = params.map(|value| redact_value(redactor, &value));
                let entry = McpTimelineEntry {
                    sequence: next_sequence(&timeline)?,
                    offset_ms: elapsed_ms(started),
                    direction: "incoming".into(),
                    kind: "notification".into(),
                    method: Some(method),
                    request_id: None,
                    payload,
                };
                push_timeline(&mut timeline, &mut timeline_bytes, entry)?;
            }
            RpcMessage::Response { id, result } => {
                let payload = match &result {
                    Ok(value) => Some(redact_value(redactor, &mcp::safe_result_projection(value))),
                    Err(error) => Some(json!({ "code": error.code })),
                };
                let entry = McpTimelineEntry {
                    sequence: next_sequence(&timeline)?,
                    offset_ms: elapsed_ms(started),
                    direction: "incoming".into(),
                    kind: if result.is_ok() {
                        "response".into()
                    } else {
                        "error".into()
                    },
                    method: None,
                    request_id: Some(id),
                    payload,
                };
                push_timeline(&mut timeline, &mut timeline_bytes, entry)?;
                final_result = Some(result);
            }
        }
    }
    Ok(InterpretedExchange {
        final_result: final_result.ok_or(mcp::MESSAGE_INVALID)?,
        timeline,
    })
}

fn push_timeline(
    timeline: &mut Vec<McpTimelineEntry>,
    bytes: &mut usize,
    entry: McpTimelineEntry,
) -> Result<(), &'static str> {
    if timeline.len() >= MAX_TIMELINE_EVENTS {
        return Err(mcp::RESPONSE_TOO_LARGE);
    }
    *bytes = bytes
        .checked_add(timeline_size(&entry))
        .ok_or(mcp::RESPONSE_TOO_LARGE)?;
    if *bytes > MAX_TIMELINE_BYTES {
        return Err(mcp::RESPONSE_TOO_LARGE);
    }
    timeline.push(entry);
    Ok(())
}

fn timeline_size(entry: &McpTimelineEntry) -> usize {
    serde_json::to_vec(entry).map_or(MAX_TIMELINE_BYTES + 1, |bytes| bytes.len())
}

fn next_sequence(timeline: &[McpTimelineEntry]) -> Result<u32, &'static str> {
    u32::try_from(timeline.len() + 1).map_err(|_| mcp::RESPONSE_TOO_LARGE)
}

fn redact_value(redactor: &Redactor, value: &Value) -> Value {
    let key_safe = redact_reflected_value(redactor, value);
    let serialized = serde_json::to_string(&key_safe).unwrap_or_default();
    let redacted = redactor.redact_body(&serialized);
    serde_json::from_str(&redacted).unwrap_or_else(|_| Value::String("[REDACTED]".into()))
}

fn redact_reflected_value(redactor: &Redactor, value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut output = serde_json::Map::new();
            for (index, (key, child)) in object.iter().enumerate() {
                let redacted = redactor.redact_text(key);
                let mut safe_key = if redacted == *key {
                    key.clone()
                } else {
                    format!("[REDACTED_KEY_{index}]")
                };
                let mut suffix = 1usize;
                while output.contains_key(&safe_key) {
                    safe_key = format!("[REDACTED_KEY_{index}_{suffix}]");
                    suffix = suffix.saturating_add(1);
                }
                output.insert(safe_key, redact_reflected_value(redactor, child));
            }
            Value::Object(output)
        }
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| redact_reflected_value(redactor, value))
                .collect(),
        ),
        Value::String(value) => Value::String(redactor.redact_text(value)),
        _ => value.clone(),
    }
}

pub(crate) fn project_result_for_ipc(
    redactor: &Redactor,
    result: &Value,
    method: &str,
) -> Result<Value, String> {
    let safe_result = mcp::safe_result_projection(result);
    let mut projected = redact_value(redactor, &safe_result);
    if method != "tools/list" {
        return Ok(projected);
    }

    let source_tools = safe_result
        .get("tools")
        .and_then(Value::as_array)
        .ok_or_else(|| mcp::MESSAGE_INVALID.to_string())?;
    let projected_tools = projected
        .get_mut("tools")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| mcp::MESSAGE_INVALID.to_string())?;
    if source_tools.len() != projected_tools.len() {
        return Err(mcp::MESSAGE_INVALID.into());
    }
    for (source, projected) in source_tools.iter().zip(projected_tools) {
        let schema = source
            .get("inputSchema")
            .ok_or_else(|| mcp::MESSAGE_INVALID.to_string())?;
        if redact_reflected_value(redactor, schema) != *schema {
            return Err(mcp::MESSAGE_INVALID.into());
        }
        projected
            .as_object_mut()
            .ok_or_else(|| mcp::MESSAGE_INVALID.to_string())?
            .insert("inputSchema".into(), schema.clone());
    }
    Ok(projected)
}

pub(crate) fn filter_reflected_list_definitions(
    result: &Value,
    method: &str,
    redactor: &Redactor,
) -> Result<(Value, usize), String> {
    let (list_key, identity_key, definition_key) = match method {
        "tools/list" => ("tools", "name", Some("inputSchema")),
        "resources/list" => ("resources", "uri", None),
        "resources/templates/list" => ("resourceTemplates", "uriTemplate", None),
        "prompts/list" => ("prompts", "name", Some("arguments")),
        _ => return Ok((result.clone(), 0)),
    };
    let mut projected = result.clone();
    let object = projected
        .as_object_mut()
        .ok_or_else(|| mcp::MESSAGE_INVALID.to_string())?;
    if object
        .get("nextCursor")
        .and_then(Value::as_str)
        .is_some_and(|cursor| redactor.redact_text(cursor) != cursor)
    {
        return Err(mcp::MESSAGE_INVALID.into());
    }
    let items = object
        .get_mut(list_key)
        .and_then(Value::as_array_mut)
        .ok_or_else(|| mcp::MESSAGE_INVALID.to_string())?;
    let original_len = items.len();
    items.retain(|item| {
        let Some(object) = item.as_object() else {
            return false;
        };
        let Some(identity) = object.get(identity_key).and_then(Value::as_str) else {
            return false;
        };
        if redactor.redact_text(identity) != identity {
            return false;
        }
        definition_key
            .and_then(|key| object.get(key))
            .is_none_or(|definition| redact_reflected_value(redactor, definition) == *definition)
    });
    let rejected = original_len - items.len();
    Ok((projected, rejected))
}

pub(crate) fn sanitize_server_projection(
    redactor: &Redactor,
    mut server: ServerProjection,
) -> Result<ServerProjection, String> {
    server.server_name = redact_bounded_display(redactor, &server.server_name);
    server.server_version = redact_bounded_display(redactor, &server.server_version);
    server.capabilities = redact_value(redactor, &server.capabilities);
    mcp::validate_json(&server.capabilities, 256 * 1024).map_err(ToOwned::to_owned)?;
    if !server.capabilities.is_object() {
        return Err(mcp::MESSAGE_INVALID.into());
    }
    Ok(server)
}

fn redact_bounded_display(redactor: &Redactor, value: &str) -> String {
    let redacted = redactor.redact_text(value);
    if redacted.len() <= 512 {
        redacted
    } else {
        "[REDACTED]".into()
    }
}

fn validate_session_id(bytes: &[u8]) -> Result<Zeroizing<String>, String> {
    if bytes.is_empty()
        || bytes.len() > MAX_SESSION_ID_BYTES
        || !bytes.iter().all(|byte| matches!(byte, 0x21..=0x7e))
    {
        return Err(mcp::MESSAGE_INVALID.into());
    }
    let value = String::from_utf8(bytes.to_vec()).map_err(|_| mcp::MESSAGE_INVALID.to_string())?;
    Ok(Zeroizing::new(value))
}

fn validate_response_session(
    era: Era,
    expected: Option<&str>,
    observed: Option<&Zeroizing<String>>,
) -> Result<(), String> {
    let observed = observed.map(|value| value.as_str());
    if (era == Era::Modern && observed.is_some())
        || (era == Era::Legacy && observed.is_some() && observed != expected)
    {
        Err(mcp::MESSAGE_INVALID.into())
    } else {
        Ok(())
    }
}

async fn terminate_legacy_session(
    profile: &PreparedProfile,
    session_id: &str,
) -> Result<(), String> {
    let mut headers = profile.custom_headers.clone();
    headers.insert(
        HeaderName::from_static("mcp-protocol-version"),
        HeaderValue::from_static(mcp::LEGACY_VERSION),
    );
    headers.insert(
        HeaderName::from_static("mcp-session-id"),
        HeaderValue::from_str(session_id).map_err(|_| mcp::MESSAGE_INVALID.to_string())?,
    );
    let response = profile
        .client
        .delete(profile.endpoint.clone())
        .headers(headers)
        .timeout(profile.timeout.min(Duration::from_secs(2)))
        .send()
        .await
        .map_err(map_transport_error)?;
    if response.status().is_redirection() {
        return Err(REDIRECT_BLOCKED.into());
    }
    if response.status().as_u16() == 405 || response.status().is_success() {
        Ok(())
    } else {
        Err(TRANSPORT_FAILED.into())
    }
}

fn map_transport_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        REQUEST_TIMEOUT.to_string()
    } else {
        TRANSPORT_FAILED.to_string()
    }
}

fn map_connect_error(code: String) -> String {
    if code == REQUEST_TIMEOUT {
        CONNECT_TIMEOUT.to_string()
    } else {
        code
    }
}

fn validate_connection_id(value: &str) -> Result<(), &'static str> {
    if value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(CONNECTION_STALE)
    }
}

fn random_hex_128() -> Result<String, &'static str> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| TRANSPORT_FAILED)?;
    let mut output = String::with_capacity(32);
    for byte in bytes {
        write!(&mut output, "{byte:02x}").map_err(|_| CONNECTION_LIMIT)?;
    }
    Ok(output)
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;

    fn profile(endpoint: &str) -> McpHttpProfile {
        McpHttpProfile {
            endpoint: endpoint.into(),
            era: EraPreference::Auto,
            headers: Vec::new(),
            timeout_ms: 2_000,
            oauth_grant_id: None,
        }
    }

    fn prepared_snapshot(endpoint: &str) -> ConnectionSnapshot {
        ConnectionSnapshot {
            profile: prepare_profile(profile(endpoint), Vec::new()).unwrap(),
            server: ServerProjection {
                era: Era::Modern,
                protocol_version: mcp::MODERN_VERSION.into(),
                server_name: "fixture".into(),
                server_version: "1".into(),
                capabilities: json!({"tools": {}}),
                supported_versions: vec![mcp::MODERN_VERSION.into()],
            },
            session_id: None,
            tool_schemas: BTreeMap::new(),
            prompt_schemas: BTreeMap::new(),
        }
    }

    fn spawn_http_fixture(
        responses: Vec<String>,
    ) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (sender, receiver) = mpsc::channel();
        let handle = thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let mut chunk = [0_u8; 4096];
                loop {
                    let count = stream.read(&mut chunk).unwrap_or_default();
                    if count == 0 {
                        break;
                    }
                    request.extend_from_slice(&chunk[..count]);
                    if request.len() > mcp::MAX_REQUEST_BYTES + 16 * 1024 {
                        break;
                    }
                    let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n")
                    else {
                        continue;
                    };
                    let headers = String::from_utf8_lossy(&request[..header_end]);
                    let content_length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap_or_default();
                    if request.len() >= header_end + 4 + content_length {
                        break;
                    }
                }
                let _ = sender.send(String::from_utf8(request).unwrap());
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        (format!("http://127.0.0.1:{port}/mcp"), receiver, handle)
    }

    fn json_response(status: &str, body: &Value, extra_headers: &str) -> String {
        let body = serde_json::to_string(body).unwrap();
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{extra_headers}\r\n{body}",
            body.len()
        )
    }

    fn empty_response(status: &str) -> String {
        format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
    }

    fn spawn_stalled_http_fixture(hold: Duration) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 128\r\nConnection: close\r\n\r\n",
            );
            let _ = stream.flush();
            thread::sleep(hold);
        });
        (format!("http://127.0.0.1:{port}/mcp"), handle)
    }

    #[test]
    fn profile_rejects_credentials_and_derived_header_overrides() {
        assert_eq!(
            prepare_profile(profile("https://user:pass@example.test/mcp"), Vec::new())
                .err()
                .expect("credentials must be rejected"),
            mcp::INVALID_PROFILE
        );
        assert_eq!(
            prepare_profile(profile("https://example.test/mcp?token=x"), Vec::new())
                .err()
                .expect("sensitive query keys must be rejected"),
            mcp::INVALID_PROFILE
        );
        let mut input = profile("https://example.test/mcp");
        input.headers.push(RequestHeader {
            key: "Mcp-Method".into(),
            value: "different".into(),
            enabled: true,
        });
        assert_eq!(
            prepare_profile(input, Vec::new())
                .err()
                .expect("derived header overrides must be rejected"),
            mcp::INVALID_PROFILE
        );

        let mut oauth_with_authorization = profile("https://example.test/mcp");
        oauth_with_authorization.oauth_grant_id = Some("a".repeat(32));
        oauth_with_authorization.headers.push(RequestHeader {
            key: "Authorization".into(),
            value: "Bearer user-value".into(),
            enabled: true,
        });
        assert_eq!(
            prepare_profile(oauth_with_authorization, Vec::new())
                .err()
                .expect("OAuth must never override a user Authorization header"),
            mcp::INVALID_PROFILE
        );

        for key in [
            "Accept-Encoding",
            "Content-Encoding",
            "Keep-Alive",
            "Proxy-Connection",
            "TE",
            "Trailer",
            "Upgrade",
            "Expect",
        ] {
            let mut input = profile("https://example.test/mcp");
            input.headers.push(RequestHeader {
                key: key.into(),
                value: "invalid override".into(),
                enabled: true,
            });
            assert_eq!(
                prepare_profile(input, Vec::new())
                    .err()
                    .expect("transport-controlled headers must be rejected"),
                mcp::INVALID_PROFILE,
                "header {key} must remain transport-controlled"
            );
        }

        let environment = vec![EnvironmentVariable {
            key: "MCP_ENDPOINT".into(),
            value: format!("https://example.test/{}", "x".repeat(MAX_ENDPOINT_BYTES)),
            secret: false,
        }];
        let mut templated = profile("{{MCP_ENDPOINT}}");
        templated.endpoint = "{{MCP_ENDPOINT}}".into();
        assert_eq!(
            prepare_profile(templated, environment)
                .err()
                .expect("resolved endpoints must remain bounded"),
            mcp::INVALID_PROFILE
        );

        let duplicates = vec![
            EnvironmentVariable {
                key: "DUPLICATE".into(),
                value: "one".into(),
                secret: false,
            },
            EnvironmentVariable {
                key: "DUPLICATE".into(),
                value: "two".into(),
                secret: false,
            },
        ];
        assert_eq!(
            prepare_profile(profile("https://example.test/mcp"), duplicates)
                .err()
                .expect("duplicate environment names must fail closed"),
            mcp::INVALID_PROFILE
        );

        let mut disabled_oversized = profile("https://example.test/mcp");
        disabled_oversized.headers.push(RequestHeader {
            key: "X-Disabled".into(),
            value: "x".repeat(MAX_HEADER_VALUE_BYTES + 1),
            enabled: false,
        });
        assert_eq!(
            prepare_profile(disabled_oversized, Vec::new())
                .err()
                .expect("disabled input rows remain bounded"),
            mcp::INVALID_PROFILE
        );
    }

    #[test]
    fn state_ids_are_opaque_and_cancel_only_owned_request() {
        let state = McpHttpState::default();
        let attempts = (0..MAX_CONNECTIONS)
            .map(|_| state.begin_connection_attempt().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            state.begin_connection_attempt().err(),
            Some(CONNECTION_LIMIT)
        );
        drop(attempts);
        let id = state
            .insert_connection(&prepared_snapshot("https://example.test/mcp"))
            .unwrap();
        assert_eq!(id.len(), 32);
        let (_, receiver) = state.begin_request(&id, "request-1").unwrap();
        assert!(!*receiver.borrow());
        assert!(state.cancel_request(&id, "request-1").unwrap());
        assert!(*receiver.borrow());
        assert!(!state.cancel_request(&id, "request-2").unwrap());
        state.finish_request(&id, "request-1");
    }

    #[test]
    fn list_state_updates_are_atomic_and_bounded_by_page_count() {
        let state = McpHttpState::default();
        let id = state
            .insert_connection(&prepared_snapshot("https://example.test/mcp"))
            .unwrap();
        for index in 0..MAX_LIST_PAGES {
            let requested_cursor = index
                .checked_sub(1)
                .map(|previous| format!("cursor-{previous}"));
            state
                .update_list_result(
                    &id,
                    "tools/list",
                    &json!({
                        "tools": [{
                            "name": format!("tool-{index}"),
                            "inputSchema": {"type":"object", "properties":{}}
                        }],
                        "nextCursor": format!("cursor-{index}")
                    }),
                    requested_cursor.as_deref(),
                )
                .unwrap();
        }
        assert_eq!(
            state.update_list_result(
                &id,
                "tools/list",
                &json!({
                    "tools": [{
                        "name":"overflow",
                        "inputSchema":{"type":"object", "properties":{}}
                    }]
                }),
                Some("cursor-99"),
            ),
            Err(mcp::RESPONSE_TOO_LARGE)
        );
        let inner = state.inner.lock().unwrap();
        let stored = inner.connections.get(&id).unwrap();
        assert_eq!(stored.list_pages.get("tools/list"), Some(&MAX_LIST_PAGES));
        assert_eq!(stored.seen["tools/list"].len(), MAX_LIST_PAGES);
        assert!(!stored.snapshot.tool_schemas.contains_key("overflow"));
    }

    #[test]
    fn list_state_rejects_aggregate_bytes_without_partial_mutation() {
        let state = McpHttpState::default();
        let id = state
            .insert_connection(&prepared_snapshot("https://example.test/mcp"))
            .unwrap();
        {
            let mut inner = state.inner.lock().unwrap();
            inner
                .connections
                .get_mut(&id)
                .unwrap()
                .list_bytes
                .insert("resources/list".into(), MAX_RETAINED_LIST_BYTES - 1);
        }
        assert_eq!(
            state.update_list_result(
                &id,
                "resources/list",
                &json!({"resources":[{"uri":"fixture://resource"}]}),
                None,
            ),
            Err(mcp::RESPONSE_TOO_LARGE)
        );
        let inner = state.inner.lock().unwrap();
        let stored = inner.connections.get(&id).unwrap();
        assert_eq!(
            stored.list_bytes.get("resources/list"),
            Some(&(MAX_RETAINED_LIST_BYTES - 1))
        );
        assert!(!stored.seen.contains_key("resources/list"));
        assert!(!stored.list_pages.contains_key("resources/list"));
    }

    #[test]
    fn list_cursor_progression_is_linear_and_cycle_safe() {
        let state = McpHttpState::default();
        let id = state
            .insert_connection(&prepared_snapshot("https://example.test/mcp"))
            .unwrap();
        assert_eq!(
            state.validate_list_request(&id, "tools/list", &json!({})),
            Ok(())
        );
        assert_eq!(
            state.validate_list_request(&id, "tools/list", &json!({"cursor":"cursor-a"})),
            Err(mcp::CURSOR_INVALID)
        );
        state
            .update_list_result(
                &id,
                "tools/list",
                &json!({"tools":[], "nextCursor":"cursor-a"}),
                None,
            )
            .unwrap();
        assert_eq!(
            state.update_list_result(
                &id,
                "tools/list",
                &json!({"tools":[{
                    "name":"racing-first-page",
                    "inputSchema":{"type":"object", "properties":{}}
                }], "nextCursor":"cursor-race"}),
                None,
            ),
            Err(mcp::CURSOR_INVALID)
        );
        assert_eq!(
            state.validate_list_request(&id, "tools/list", &json!({})),
            Err(mcp::CURSOR_INVALID)
        );
        assert_eq!(
            state.validate_list_request(&id, "tools/list", &json!({"cursor":"cursor-a"})),
            Ok(())
        );
        assert_eq!(
            state.update_list_result(
                &id,
                "tools/list",
                &json!({"tools":[], "nextCursor":"cursor-a"}),
                Some("cursor-a"),
            ),
            Err(mcp::CURSOR_INVALID)
        );
        state
            .update_list_result(
                &id,
                "tools/list",
                &json!({"tools":[], "nextCursor":"cursor-b"}),
                Some("cursor-a"),
            )
            .unwrap();
        assert_eq!(
            state.update_list_result(
                &id,
                "tools/list",
                &json!({"tools":[], "nextCursor":"cursor-a"}),
                Some("cursor-b"),
            ),
            Err(mcp::CURSOR_INVALID)
        );
    }

    #[tokio::test]
    async fn modern_loopback_uses_discover_headers_and_per_request_metadata() {
        let discover = json!({
            "jsonrpc":"2.0",
            "id":"discover-1",
            "result":{
                "resultType":"complete",
                "supportedVersions":[mcp::MODERN_VERSION],
                "capabilities":{"tools":{}},
                "ttlMs":60_000,
                "cacheScope":"public",
                "_meta":{"io.modelcontextprotocol/serverInfo":{"name":"fixture", "version":"1"}}
            }
        });
        let (endpoint, requests, handle) =
            spawn_http_fixture(vec![json_response("200 OK", &discover, "")]);
        let prepared = prepare_profile(profile(&endpoint), Vec::new()).unwrap();
        let (server, session, timeline) = connect_modern(&prepared, false).await.unwrap();
        assert_eq!(server.era, Era::Modern);
        assert!(session.is_none());
        assert_eq!(timeline.len(), 2);
        let request = requests.recv_timeout(Duration::from_secs(1)).unwrap();
        let normalized = request.to_ascii_lowercase();
        assert!(normalized.contains("mcp-protocol-version: 2026-07-28"));
        assert!(normalized.contains("mcp-method: server/discover"));
        assert!(request.contains("io.modelcontextprotocol/protocolVersion"));
        handle.join().unwrap();
    }

    #[tokio::test]
    async fn oauth_bearer_is_injected_and_added_to_protocol_redaction() {
        let discover = json!({
            "jsonrpc":"2.0",
            "id":"discover-1",
            "result":{
                "resultType":"complete",
                "supportedVersions":[mcp::MODERN_VERSION],
                "capabilities":{},
                "ttlMs":60_000,
                "cacheScope":"public",
                "_meta":{"io.modelcontextprotocol/serverInfo":{
                    "name":"server leaked oauth-secret", "version":"1"
                }}
            }
        });
        let (endpoint, requests, handle) =
            spawn_http_fixture(vec![json_response("200 OK", &discover, "")]);
        let mut input = profile(&endpoint);
        input.oauth_grant_id = Some("a".repeat(32));
        let mut prepared = prepare_profile(input, Vec::new()).unwrap();
        prepared.apply_oauth_bearer(OAuthBearer {
            token: Zeroizing::new("oauth-secret".into()),
        });

        let (server, _, _) = connect_modern(&prepared, false).await.unwrap();
        assert_eq!(server.server_name, "server leaked [REDACTED]");
        let request = requests.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(request
            .to_ascii_lowercase()
            .contains("authorization: bearer oauth-secret"));
        handle.join().unwrap();
    }

    #[tokio::test]
    async fn modern_tool_call_mirrors_validated_name_and_parameter_headers() {
        let response = json!({
            "jsonrpc":"2.0",
            "id":"request-1",
            "result":{"resultType":"complete", "content":[]}
        });
        let (endpoint, requests, handle) =
            spawn_http_fixture(vec![json_response("200 OK", &response, "")]);
        let mut snapshot = prepared_snapshot(&endpoint);
        snapshot.tool_schemas.insert(
            "echo".into(),
            json!({
                "type":"object",
                "properties":{
                    "region":{"type":"string", "x-mcp-header":"Region"}
                }
            }),
        );
        let (_sender, mut cancellation) = watch::channel(false);
        let result = invoke_inner(
            &snapshot,
            "request-1",
            "tools/call",
            json!({"name":"echo", "arguments":{"region":"서울"}}),
            &mut cancellation,
        )
        .await
        .unwrap();
        assert!(result.result.is_some());
        let request = requests.recv_timeout(Duration::from_secs(1)).unwrap();
        let normalized = request.to_ascii_lowercase();
        assert!(normalized.contains("mcp-method: tools/call"));
        assert!(normalized.contains("mcp-name: echo"));
        assert!(normalized.contains("mcp-param-region: =?base64?"));
        handle.join().unwrap();
    }

    #[tokio::test]
    async fn modern_http_error_preserves_only_the_stable_rpc_classification() {
        let response = json!({
            "jsonrpc":"2.0",
            "id":"request-1",
            "error":{"code":-32020, "message":"unsafe reflected server detail"}
        });
        let (endpoint, _requests, handle) =
            spawn_http_fixture(vec![json_response("400 Bad Request", &response, "")]);
        let snapshot = prepared_snapshot(&endpoint);
        let (_sender, mut cancellation) = watch::channel(false);
        let result = invoke_inner(
            &snapshot,
            "request-1",
            "tools/list",
            json!({}),
            &mut cancellation,
        )
        .await
        .unwrap();
        assert_eq!(result.error_code.as_deref(), Some(mcp::MESSAGE_INVALID));
        assert_eq!(result.rpc_error_code, Some(-32020));
        assert!(!serde_json::to_string(&result.timeline)
            .unwrap()
            .contains("unsafe reflected server detail"));
        handle.join().unwrap();
    }

    #[tokio::test]
    async fn legacy_session_404_marks_the_connection_stale_without_replay() {
        let (endpoint, requests, handle) =
            spawn_http_fixture(vec![empty_response("404 Not Found")]);
        let mut snapshot = prepared_snapshot(&endpoint);
        snapshot.server.era = Era::Legacy;
        snapshot.server.protocol_version = mcp::LEGACY_VERSION.into();
        snapshot.server.supported_versions = vec![mcp::LEGACY_VERSION.into()];
        snapshot.session_id = Some(Zeroizing::new("expired-session".into()));
        let (_sender, mut cancellation) = watch::channel(false);
        let error = invoke_inner(
            &snapshot,
            "request-1",
            "tools/list",
            json!({}),
            &mut cancellation,
        )
        .await
        .expect_err("a 404 for a session-bound request must expire the connection");
        assert_eq!(error, CONNECTION_STALE);
        let request = requests.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(request
            .to_ascii_lowercase()
            .contains("mcp-session-id: expired-session"));
        handle.join().unwrap();
    }

    #[tokio::test]
    async fn native_tool_argument_validation_runs_before_transport() {
        let mut snapshot = prepared_snapshot("http://127.0.0.1:9/mcp");
        snapshot.tool_schemas.insert(
            "echo".into(),
            json!({
                "type":"object",
                "required":["message"],
                "properties":{"message":{"type":"string", "minLength":1}}
            }),
        );
        let (_sender, mut cancellation) = watch::channel(false);
        let error = invoke_inner(
            &snapshot,
            "request-1",
            "tools/call",
            json!({"name":"echo", "arguments":{"message":""}}),
            &mut cancellation,
        )
        .await
        .expect_err("invalid native arguments must be rejected before transport");
        assert_eq!(error, mcp::MESSAGE_INVALID);

        snapshot.server.capabilities = json!({"prompts":{}});
        snapshot
            .prompt_schemas
            .insert("draft".into(), BTreeMap::from([("topic".into(), true)]));
        let error = invoke_inner(
            &snapshot,
            "request-2",
            "prompts/get",
            json!({"name":"draft", "arguments":{}}),
            &mut cancellation,
        )
        .await
        .expect_err("invalid native prompt arguments must be rejected before transport");
        assert_eq!(error, mcp::MESSAGE_INVALID);
    }

    #[tokio::test]
    async fn legacy_loopback_preserves_initialize_session_and_initialized_sequence() {
        let initialize = json!({
            "jsonrpc":"2.0",
            "id":"initialize-1",
            "result":{
                "protocolVersion":mcp::LEGACY_VERSION,
                "capabilities":{"prompts":{}},
                "serverInfo":{"name":"legacy-fixture", "version":"1"}
            }
        });
        let responses = vec![
            json_response("200 OK", &initialize, "Mcp-Session-Id: fixture-session\r\n"),
            empty_response("202 Accepted"),
        ];
        let (endpoint, requests, handle) = spawn_http_fixture(responses);
        let prepared = prepare_profile(profile(&endpoint), Vec::new()).unwrap();
        let (server, session, timeline) = connect_legacy(&prepared, Vec::new()).await.unwrap();
        assert_eq!(server.era, Era::Legacy);
        assert_eq!(
            session.as_ref().map(|value| value.as_str()),
            Some("fixture-session")
        );
        assert_eq!(
            timeline.last().unwrap().method.as_deref(),
            Some("notifications/initialized")
        );
        let initialize_request = requests.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(initialize_request.contains("\"method\":\"initialize\""));
        assert!(!initialize_request
            .to_ascii_lowercase()
            .contains("mcp-protocol-version:"));
        let initialized_request = requests.recv_timeout(Duration::from_secs(1)).unwrap();
        let initialized_lower = initialized_request.to_ascii_lowercase();
        assert!(initialized_request.contains("notifications/initialized"));
        assert!(initialized_lower.contains("mcp-session-id: fixture-session"));
        assert!(initialized_lower.contains("mcp-protocol-version: 2025-11-25"));
        handle.join().unwrap();
    }

    #[tokio::test]
    async fn failed_legacy_handshake_deletes_an_assigned_session() {
        let initialize = json!({
            "jsonrpc":"2.0",
            "id":"initialize-1",
            "result":{
                "protocolVersion":mcp::LEGACY_VERSION,
                "capabilities":{},
                "serverInfo":{"name":"legacy-fixture", "version":"1"}
            }
        });
        let responses = vec![
            json_response("200 OK", &initialize, "Mcp-Session-Id: cleanup-session\r\n"),
            empty_response("500 Internal Server Error"),
            empty_response("200 OK"),
        ];
        let (endpoint, requests, handle) = spawn_http_fixture(responses);
        let prepared = prepare_profile(profile(&endpoint), Vec::new()).unwrap();
        let error = connect_legacy(&prepared, Vec::new())
            .await
            .expect_err("a failed initialized notification must abort the handshake");
        assert_eq!(error, RESPONSE_TYPE_INVALID);

        let _initialize = requests.recv_timeout(Duration::from_secs(1)).unwrap();
        let _initialized = requests.recv_timeout(Duration::from_secs(1)).unwrap();
        let cleanup = requests.recv_timeout(Duration::from_secs(1)).unwrap();
        let cleanup_lower = cleanup.to_ascii_lowercase();
        assert!(cleanup.starts_with("DELETE "));
        assert!(cleanup_lower.contains("mcp-session-id: cleanup-session"));
        assert!(cleanup_lower.contains("mcp-protocol-version: 2025-11-25"));
        handle.join().unwrap();
    }

    #[tokio::test]
    async fn eligible_plain_404_allows_auto_legacy_fallback_without_reflecting_body() {
        let initialize = json!({
            "jsonrpc":"2.0",
            "id":"initialize-1",
            "result":{
                "protocolVersion":mcp::LEGACY_VERSION,
                "capabilities":{},
                "serverInfo":{"name":"legacy", "version":"1"}
            }
        });
        let plain = "legacy-only internal-path=/private\n";
        let responses = vec![
            format!(
                "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{plain}",
                plain.len()
            ),
            json_response("200 OK", &initialize, ""),
            empty_response("202 Accepted"),
        ];
        let (endpoint, requests, handle) = spawn_http_fixture(responses);
        let prepared = prepare_profile(profile(&endpoint), Vec::new()).unwrap();
        let fallback = match connect_modern(&prepared, true).await {
            Ok(_) => panic!("plain 404 must be eligible for auto fallback"),
            Err(error) => error,
        };
        assert_eq!(fallback, "mcp_legacy_fallback");
        let (server, _, _) = connect_legacy(&prepared, Vec::new()).await.unwrap();
        assert_eq!(server.era, Era::Legacy);
        for _ in 0..3 {
            let request = requests.recv_timeout(Duration::from_secs(1)).unwrap();
            assert!(!request.contains("internal-path"));
        }
        handle.join().unwrap();
    }

    #[tokio::test]
    async fn auto_does_not_fallback_for_recognized_or_non_4xx_modern_errors() {
        let fixtures = [
            (
                "400 Bad Request",
                json!({
                    "jsonrpc":"2.0",
                    "id":"discover-1",
                    "error":{"code":-32020, "message":"Header mismatch"}
                }),
                mcp::MESSAGE_INVALID,
            ),
            (
                "400 Bad Request",
                json!({
                    "jsonrpc":"2.0",
                    "error":{"code":-32020, "message":"Header mismatch"}
                }),
                mcp::MESSAGE_INVALID,
            ),
            (
                "400 Bad Request",
                json!({
                    "jsonrpc":"2.0",
                    "id":"discover-1",
                    "error":{
                        "code":-32022,
                        "message":"Unsupported protocol version",
                        "data":{
                            "requested":mcp::MODERN_VERSION,
                            "supported":[mcp::MODERN_VERSION, mcp::LEGACY_VERSION]
                        }
                    }
                }),
                mcp::VERSION_UNSUPPORTED,
            ),
            (
                "404 Not Found",
                json!({
                    "jsonrpc":"2.0",
                    "id":"discover-1",
                    "error":{"code":-32601, "message":"Method not found"}
                }),
                mcp::VERSION_UNSUPPORTED,
            ),
            (
                "200 OK",
                json!({
                    "jsonrpc":"2.0",
                    "id":"discover-1",
                    "error":{"code":-32603, "message":"Internal error"}
                }),
                mcp::VERSION_UNSUPPORTED,
            ),
        ];
        for (status, body, expected) in fixtures {
            let (endpoint, _requests, handle) =
                spawn_http_fixture(vec![json_response(status, &body, "")]);
            let prepared = prepare_profile(profile(&endpoint), Vec::new()).unwrap();
            let error = match connect_modern(&prepared, true).await {
                Ok(_) => panic!("recognized modern evidence must not fall back"),
                Err(error) => error,
            };
            assert_eq!(error, expected);
            handle.join().unwrap();
        }
    }

    #[tokio::test]
    async fn advertised_legacy_version_is_an_explicit_auto_negotiation_path() {
        let body = json!({
            "jsonrpc":"2.0",
            "id":"discover-1",
            "error":{
                "code":-32022,
                "message":"Unsupported protocol version",
                "data":{
                    "requested":mcp::MODERN_VERSION,
                    "supported":[mcp::LEGACY_VERSION]
                }
            }
        });
        let (endpoint, _requests, handle) =
            spawn_http_fixture(vec![json_response("400 Bad Request", &body, "")]);
        let prepared = prepare_profile(profile(&endpoint), Vec::new()).unwrap();
        let error = connect_modern(&prepared, true)
            .await
            .expect_err("explicit legacy support must select the negotiated path");
        assert_eq!(error, "mcp_legacy_version_negotiated");
        handle.join().unwrap();
    }

    #[tokio::test]
    async fn loopback_response_stream_honors_owned_cancellation() {
        let (endpoint, handle) = spawn_stalled_http_fixture(Duration::from_millis(250));
        let prepared = prepare_profile(profile(&endpoint), Vec::new()).unwrap();
        let request = mcp::build_modern_request("request-1", "tools/list", json!({})).unwrap();
        let headers = mcp::derived_headers(
            Era::Modern,
            mcp::MODERN_VERSION,
            "tools/list",
            &json!({}),
            None,
        )
        .unwrap();
        let (sender, mut receiver) = watch::channel(false);
        let cancel = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(40)).await;
            sender.send(true).unwrap();
        });
        let error = execute_message(&prepared, &headers, &request, None, &mut receiver)
            .await
            .err()
            .expect("the owned request must be cancelled");
        assert_eq!(error, REQUEST_CANCELLED);
        cancel.await.unwrap();
        handle.join().unwrap();
    }

    #[tokio::test]
    async fn loopback_connect_timeout_maps_to_connect_contract() {
        let (endpoint, handle) = spawn_stalled_http_fixture(Duration::from_millis(250));
        let mut input = profile(&endpoint);
        input.timeout_ms = MIN_TIMEOUT_MS;
        let prepared = prepare_profile(input, Vec::new()).unwrap();
        let error = match connect_modern(&prepared, false).await {
            Ok(_) => panic!("the stalled connect must time out"),
            Err(error) => error,
        };
        assert_eq!(error, CONNECT_TIMEOUT);
        handle.join().unwrap();
    }

    #[test]
    fn interpretation_redacts_outgoing_arguments_and_rejects_trailing_messages() {
        let prepared = prepare_profile(profile("https://example.test/mcp"), Vec::new()).unwrap();
        let result = interpret_exchange(
            vec![json!({
                "jsonrpc":"2.0",
                "id":"request-1",
                "result":{"resultType":"complete", "content":[]}
            })],
            "request-1",
            Era::Modern,
            prepared.redactor.as_ref(),
            Instant::now(),
            "tools/call",
            &json!({"name":"echo", "arguments":{"password":"secret"}}),
        )
        .unwrap();
        assert_eq!(
            result.timeline[0].payload.as_ref().unwrap()["arguments"],
            "[REDACTED]"
        );
        assert_eq!(result.timeline.len(), 2);

        assert_eq!(
            interpret_exchange(
                vec![
                    json!({"jsonrpc":"2.0", "id":"request-1", "result":{"resultType":"complete"}}),
                    json!({"jsonrpc":"2.0", "method":"notifications/progress", "params":{}})
                ],
                "request-1",
                Era::Modern,
                prepared.redactor.as_ref(),
                Instant::now(),
                "tools/list",
                &json!({})
            )
            .err()
            .expect("messages after a final response must be rejected"),
            mcp::MESSAGE_INVALID
        );
    }

    #[test]
    fn server_projection_and_timeline_redact_resolved_credentials() {
        let mut input = profile("https://example.test/mcp");
        input.headers.push(RequestHeader {
            key: "Authorization".into(),
            value: "Bearer reflected-secret".into(),
            enabled: true,
        });
        let prepared = prepare_profile(input, Vec::new()).unwrap();
        let projection = sanitize_server_projection(
            prepared.redactor.as_ref(),
            ServerProjection {
                era: Era::Modern,
                protocol_version: mcp::MODERN_VERSION.into(),
                server_name: "Bearer reflected-secret".into(),
                server_version: "1".into(),
                capabilities: json!({"tools":{}, "note":"Bearer reflected-secret"}),
                supported_versions: vec![mcp::MODERN_VERSION.into()],
            },
        )
        .unwrap();
        assert!(!serde_json::to_string(&projection)
            .unwrap()
            .contains("reflected-secret"));

        let interpreted = interpret_exchange(
            vec![json!({
                "jsonrpc":"2.0",
                "id":"request-1",
                "result":{"resultType":"complete", "messages":[]}
            })],
            "request-1",
            Era::Modern,
            prepared.redactor.as_ref(),
            Instant::now(),
            "prompts/get",
            &json!({
                "name":"Bearer reflected-secret",
                "arguments":{"token":"Bearer reflected-secret"}
            }),
        )
        .unwrap();
        assert!(!serde_json::to_string(&interpreted.timeline)
            .unwrap()
            .contains("reflected-secret"));

        let (tools, rejected) = filter_reflected_list_definitions(
            &json!({
                "tools":[
                    {
                        "name":"safe",
                        "inputSchema":{
                            "type":"object",
                            "properties":{"password":{"type":"string"}}
                        }
                    },
                    {
                        "name":"Bearer reflected-secret",
                        "inputSchema":{"type":"object", "properties":{}}
                    },
                    {
                        "name":"unsafe-schema",
                        "inputSchema":{
                            "type":"object",
                            "properties":{
                                "Bearer reflected-secret":{"type":"string"}
                            }
                        }
                    }
                ]
            }),
            "tools/list",
            prepared.redactor.as_ref(),
        )
        .unwrap();
        assert_eq!(rejected, 2);
        assert_eq!(tools["tools"].as_array().unwrap().len(), 1);
        let projected =
            project_result_for_ipc(prepared.redactor.as_ref(), &tools, "tools/list").unwrap();
        assert_eq!(
            projected["tools"][0]["inputSchema"]["properties"]["password"]["type"],
            "string"
        );
        assert_eq!(
            filter_reflected_list_definitions(
                &json!({
                    "tools":[],
                    "nextCursor":"Bearer reflected-secret"
                }),
                "tools/list",
                prepared.redactor.as_ref(),
            )
            .expect_err("a reflected credential cannot become an IPC cursor"),
            mcp::MESSAGE_INVALID
        );
    }

    #[test]
    fn response_session_header_matches_the_selected_era_and_initial_session() {
        let assigned = Zeroizing::new("session-a".to_string());
        let changed = Zeroizing::new("session-b".to_string());
        assert_eq!(
            validate_response_session(Era::Modern, None, Some(&assigned)),
            Err(mcp::MESSAGE_INVALID.into())
        );
        assert_eq!(
            validate_response_session(Era::Legacy, None, Some(&assigned)),
            Err(mcp::MESSAGE_INVALID.into())
        );
        assert_eq!(
            validate_response_session(Era::Legacy, Some("session-a"), Some(&changed)),
            Err(mcp::MESSAGE_INVALID.into())
        );
        assert_eq!(
            validate_response_session(Era::Legacy, Some("session-a"), Some(&assigned)),
            Ok(())
        );
        assert_eq!(
            validate_response_session(Era::Legacy, Some("session-a"), None),
            Ok(())
        );
    }
}
