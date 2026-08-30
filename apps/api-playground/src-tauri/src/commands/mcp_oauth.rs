//! Native OAuth authorization-code flow for MCP HTTP.
//!
//! Discovery and all credential lifecycle operations remain backend-owned.
//! Renderer projections contain binding/status metadata only; tokens, callback
//! parameters, discovery bodies, DPAPI envelopes, and storage paths never cross
//! IPC.

use crate::core::oauth::{self, AuthorizationServerMetadata, ProtectedResourceMetadata};
use crate::platform::platform_sealer;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use futures_util::StreamExt;
use reqwest::header::{CONTENT_TYPE, WWW_AUTHENTICATE};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
#[cfg(target_os = "windows")]
use tauri::Manager;
use tauri_plugin_opener::OpenerExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{watch, Mutex as AsyncMutex};
use zeroize::Zeroizing;

const OAUTH_REQUIRED: &str = "mcp_oauth_required";
const STORAGE_FAILED: &str = "mcp_oauth_storage_failed";
const REAUTHORIZATION_REQUIRED: &str = "mcp_oauth_reauthorization_required";
const OAUTH_CANCELLED: &str = "mcp_oauth_cancelled";
const REVOKE_FAILED: &str = "mcp_oauth_revoke_failed";
const MAX_GRANTS: usize = 32;
const MAX_STORE_BYTES: usize = 1024 * 1024;
const MAX_REQUEST_ID_BYTES: usize = 128;
const FLOW_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const NETWORK_TIMEOUT: Duration = Duration::from_secs(15);
const EXPIRY_SAFETY_MS: u64 = 60_000;
const STORE_SCHEMA: &str = "devbox.api-playground.mcp-oauth-grants";
const STORE_VERSION: u32 = 1;

#[derive(Default)]
pub struct McpOAuthState {
    active: Mutex<Option<ActiveFlow>>,
    store: AsyncMutex<Option<GrantStore>>,
    mutation: AsyncMutex<()>,
    live_expiries: Mutex<HashMap<String, Instant>>,
}

struct ActiveFlow {
    request_id: String,
    cancellation: watch::Sender<bool>,
}

struct ActiveFlowGuard<'a> {
    state: &'a McpOAuthState,
    request_id: String,
}

impl Drop for ActiveFlowGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = self.state.active.lock() {
            if active
                .as_ref()
                .is_some_and(|flow| flow.request_id == self.request_id)
            {
                active.take();
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GrantStore {
    schema: String,
    version: u32,
    grants: Vec<PersistedGrant>,
}

impl Default for GrantStore {
    fn default() -> Self {
        Self {
            schema: STORE_SCHEMA.into(),
            version: STORE_VERSION,
            grants: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PersistedGrant {
    grant_id: String,
    issuer: String,
    resource: String,
    client_id: String,
    scopes: Vec<String>,
    access_token: String,
    refresh_token: Option<String>,
    expires_at_ms: Option<u64>,
    authorization_endpoint: String,
    token_endpoint: String,
    revocation_endpoint: Option<String>,
    authorization_response_iss_parameter_supported: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpOAuthGrantProjection {
    grant_id: String,
    issuer: String,
    resource: String,
    client_id: String,
    scopes: Vec<String>,
    expires_at_ms: Option<u64>,
    status: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpOAuthRevokeResult {
    remote_revoked: bool,
    removed_local: bool,
}

pub(crate) struct OAuthBearer {
    pub(crate) token: Zeroizing<String>,
}

struct DiscoveredAuthorization {
    resource: String,
    scopes: Vec<String>,
    server: AuthorizationServerMetadata,
}

impl McpOAuthState {
    fn begin_flow(
        &self,
        request_id: &str,
    ) -> Result<(watch::Receiver<bool>, ActiveFlowGuard<'_>), String> {
        validate_request_id(request_id)?;
        let mut active = self
            .active
            .lock()
            .map_err(|_| oauth::REQUEST_INVALID.to_string())?;
        if active.is_some() {
            return Err(oauth::REQUEST_INVALID.into());
        }
        let (sender, receiver) = watch::channel(false);
        *active = Some(ActiveFlow {
            request_id: request_id.to_string(),
            cancellation: sender,
        });
        Ok((
            receiver,
            ActiveFlowGuard {
                state: self,
                request_id: request_id.to_string(),
            },
        ))
    }

    fn cancel_flow(&self, request_id: &str) -> Result<bool, String> {
        validate_request_id(request_id)?;
        let active = self
            .active
            .lock()
            .map_err(|_| oauth::REQUEST_INVALID.to_string())?;
        let Some(flow) = active.as_ref().filter(|flow| flow.request_id == request_id) else {
            return Ok(false);
        };
        flow.cancellation
            .send(true)
            .map_err(|_| OAUTH_CANCELLED.to_string())?;
        Ok(true)
    }

    async fn store_snapshot(&self, app: &tauri::AppHandle) -> Result<GrantStore, String> {
        let mut store = self.store.lock().await;
        if store.is_none() {
            let path = grant_store_path(app)?;
            let loaded = tauri::async_runtime::spawn_blocking(move || load_store_path(&path))
                .await
                .map_err(|_| STORAGE_FAILED.to_string())??;
            *store = Some(loaded);
        }
        store.clone().ok_or_else(|| STORAGE_FAILED.to_string())
    }

    async fn replace_store(&self, app: &tauri::AppHandle, next: GrantStore) -> Result<(), String> {
        validate_store(&next)?;
        let path = grant_store_path(app)?;
        let saved = next.clone();
        tauri::async_runtime::spawn_blocking(move || save_store_path(&path, &saved))
            .await
            .map_err(|_| STORAGE_FAILED.to_string())??;
        *self.store.lock().await = Some(next);
        Ok(())
    }

    fn live_expiry(&self, grant_id: &str) -> Option<Instant> {
        self.live_expiries
            .lock()
            .ok()
            .and_then(|expiries| expiries.get(grant_id).copied())
    }

    fn remember_live_expiry(&self, grant_id: &str, expires_in: Option<u64>) {
        let Ok(mut expiries) = self.live_expiries.lock() else {
            return;
        };
        match expires_in
            .and_then(|seconds| Instant::now().checked_add(Duration::from_secs(seconds)))
        {
            Some(deadline) => {
                expiries.insert(grant_id.to_string(), deadline);
            }
            None => {
                expiries.remove(grant_id);
            }
        }
    }

    pub(crate) async fn bearer_for(
        &self,
        app: &tauri::AppHandle,
        grant_id: &str,
        expected_resource: &str,
        cancellation: &mut watch::Receiver<bool>,
    ) -> Result<OAuthBearer, String> {
        validate_grant_id(grant_id)?;
        let (_, expected_resource) =
            oauth::normalize_resource(expected_resource).map_err(ToOwned::to_owned)?;
        let _mutation = self.mutation.lock().await;
        let store = self.store_snapshot(app).await?;
        let grant = store
            .grants
            .iter()
            .find(|grant| grant.grant_id == grant_id)
            .cloned()
            .ok_or_else(|| OAUTH_REQUIRED.to_string())?;
        if grant.resource != expected_resource {
            return Err(oauth::RESOURCE_MISMATCH.into());
        }
        let now = now_unix_ms()?;
        let usable = token_is_usable(
            self.live_expiry(grant_id),
            grant.expires_at_ms,
            Instant::now(),
            now,
        );
        if usable {
            return Ok(OAuthBearer {
                token: unseal_token(&grant.access_token)?,
            });
        }
        let refresh_token = grant
            .refresh_token
            .as_deref()
            .ok_or_else(|| REAUTHORIZATION_REQUIRED.to_string())
            .and_then(unseal_token)?;
        let client = oauth_client(REAUTHORIZATION_REQUIRED)?;
        let response = send_form(
            &client,
            &grant.token_endpoint,
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token.as_str()),
                ("client_id", grant.client_id.as_str()),
                ("resource", grant.resource.as_str()),
            ],
            cancellation,
            REAUTHORIZATION_REQUIRED,
        )
        .await?;
        let token = oauth::parse_token_response(&response)
            .map_err(|_| REAUTHORIZATION_REQUIRED.to_string())?;
        let next_refresh = token
            .refresh_token
            .as_ref()
            .map(|value| seal_token(value))
            .transpose()?
            .or_else(|| grant.refresh_token.clone());
        let expires_at_ms = expiry_timestamp(token.expires_in)?;
        let scopes = token.scopes.clone().unwrap_or_else(|| grant.scopes.clone());
        let mut next = store;
        let current = next
            .grants
            .iter_mut()
            .find(|stored| stored.grant_id == grant_id)
            .ok_or_else(|| OAUTH_REQUIRED.to_string())?;
        if current.issuer != grant.issuer
            || current.resource != grant.resource
            || current.client_id != grant.client_id
        {
            return Err(REAUTHORIZATION_REQUIRED.into());
        }
        current.access_token = seal_token(&token.access_token)?;
        current.refresh_token = next_refresh;
        current.expires_at_ms = expires_at_ms;
        current.scopes = scopes;
        self.replace_store(app, next).await?;
        self.remember_live_expiry(grant_id, token.expires_in);
        Ok(OAuthBearer {
            token: token.access_token,
        })
    }
}

#[tauri::command]
pub async fn authorize_mcp_http(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<McpOAuthState>>,
    request_id: String,
    endpoint: String,
    issuer: Option<String>,
    client_id: String,
    scopes: Vec<String>,
) -> Result<McpOAuthGrantProjection, String> {
    // OAuth persistence is intentionally Windows/DPAPI-only. Fail before
    // discovery or opening a browser on unsupported native builds.
    let _ = grant_store_path(&app)?;
    let state = state.inner().as_ref();
    let (mut cancellation, _flow) = state.begin_flow(&request_id)?;
    oauth::validate_client_id(&client_id).map_err(ToOwned::to_owned)?;
    let requested_scopes = oauth::validate_scopes(&scopes).map_err(ToOwned::to_owned)?;
    let requested_issuer = issuer.as_deref().filter(|value| !value.is_empty());
    let client = oauth_client(oauth::DISCOVERY_FAILED)?;
    let discovered = discover_authorization(
        &client,
        &endpoint,
        requested_issuer,
        requested_scopes,
        &mut cancellation,
    )
    .await?;

    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|_| oauth::CALLBACK_FAILED.to_string())?;
    let port = listener
        .local_addr()
        .map_err(|_| oauth::CALLBACK_FAILED.to_string())?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}{}", oauth::CALLBACK_PATH);
    let (state_value, verifier, challenge) =
        oauth::generate_state_and_pkce().map_err(|_| oauth::CALLBACK_FAILED.to_string())?;
    let authorization_url = oauth::build_authorization_url(oauth::AuthorizationUrlInput {
        endpoint: &discovered.server.authorization_endpoint,
        client_id: &client_id,
        redirect_uri: &redirect_uri,
        state: &state_value,
        challenge: &challenge,
        resource: &discovered.resource,
        scopes: &discovered.scopes,
    })
    .map_err(ToOwned::to_owned)?;
    app.opener()
        .open_url(authorization_url.as_str(), None::<&str>)
        .map_err(|_| oauth::CALLBACK_FAILED.to_string())?;
    drop(authorization_url);

    let (mut stream, peer) = tokio::select! {
        biased;
        changed = cancellation.changed() => {
            let _ = changed;
            return Err(OAUTH_CANCELLED.into());
        }
        accepted = tokio::time::timeout(FLOW_TIMEOUT, listener.accept()) => {
            accepted
                .map_err(|_| oauth::CALLBACK_FAILED.to_string())?
                .map_err(|_| oauth::CALLBACK_FAILED.to_string())?
        }
    };
    if !peer.ip().is_loopback() {
        let _ = write_callback_page(&mut stream, false).await;
        return Err(oauth::CALLBACK_FAILED.into());
    }
    let request = match read_callback(&mut stream, &mut cancellation).await {
        Ok(request) => request,
        Err(code) => {
            let _ = write_callback_page(&mut stream, false).await;
            return Err(code);
        }
    };
    let callback = match oauth::parse_callback_request(
        &request,
        &state_value,
        &discovered.server.issuer,
        discovered
            .server
            .authorization_response_iss_parameter_supported,
    ) {
        Ok(callback) => callback,
        Err(code) => {
            let _ = write_callback_page(&mut stream, false).await;
            return Err(code.into());
        }
    };
    drop(request);
    drop(state_value);
    let token_bytes = send_form(
        &client,
        &discovered.server.token_endpoint,
        &[
            ("grant_type", "authorization_code"),
            ("code", callback.code.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("client_id", client_id.as_str()),
            ("code_verifier", verifier.as_str()),
            ("resource", discovered.resource.as_str()),
        ],
        &mut cancellation,
        oauth::TOKEN_FAILED,
    )
    .await;
    drop(callback);
    drop(verifier);
    let token = match token_bytes
        .and_then(|bytes| oauth::parse_token_response(&bytes).map_err(ToOwned::to_owned))
    {
        Ok(token) => token,
        Err(code) => {
            let _ = write_callback_page(&mut stream, false).await;
            return Err(code);
        }
    };
    let projection = match persist_authorized_grant(state, &app, discovered, client_id, token).await
    {
        Ok(projection) => projection,
        Err(code) => {
            let _ = write_callback_page(&mut stream, false).await;
            return Err(code);
        }
    };
    let _ = write_callback_page(&mut stream, true).await;
    Ok(projection)
}

#[tauri::command]
pub fn cancel_mcp_oauth(
    state: tauri::State<'_, Arc<McpOAuthState>>,
    request_id: String,
) -> Result<bool, String> {
    state.cancel_flow(&request_id)
}

#[tauri::command]
pub async fn list_mcp_oauth_grants(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<McpOAuthState>>,
) -> Result<Vec<McpOAuthGrantProjection>, String> {
    let _mutation = state.mutation.lock().await;
    let store = state.store_snapshot(&app).await?;
    let now = now_unix_ms()?;
    Ok(store
        .grants
        .iter()
        .map(|grant| project_grant(grant, now))
        .collect())
}

#[tauri::command]
pub async fn revoke_mcp_oauth_grant(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<McpOAuthState>>,
    grant_id: String,
    remove_local_on_remote_failure: bool,
) -> Result<McpOAuthRevokeResult, String> {
    validate_grant_id(&grant_id)?;
    let _mutation = state.mutation.lock().await;
    let store = state.store_snapshot(&app).await?;
    let grant = store
        .grants
        .iter()
        .find(|grant| grant.grant_id == grant_id)
        .cloned()
        .ok_or_else(|| OAUTH_REQUIRED.to_string())?;
    let mut remote_revoked = false;
    if let Some(endpoint) = &grant.revocation_endpoint {
        let sealed_token = grant
            .refresh_token
            .as_deref()
            .unwrap_or(grant.access_token.as_str());
        let token = unseal_token(sealed_token)?;
        let client = oauth_client(REVOKE_FAILED)?;
        let (_sender, mut cancellation) = watch::channel(false);
        let outcome = send_form_allow_empty(
            &client,
            endpoint,
            &[
                ("token", token.as_str()),
                (
                    "token_type_hint",
                    if grant.refresh_token.is_some() {
                        "refresh_token"
                    } else {
                        "access_token"
                    },
                ),
                ("client_id", grant.client_id.as_str()),
            ],
            &mut cancellation,
            REVOKE_FAILED,
        )
        .await;
        match outcome {
            Ok(()) => remote_revoked = true,
            Err(_) if !remove_local_on_remote_failure => return Err(REVOKE_FAILED.into()),
            Err(_) => {}
        }
    } else if !remove_local_on_remote_failure {
        return Err(REVOKE_FAILED.into());
    }
    let mut next = store;
    let previous_len = next.grants.len();
    next.grants.retain(|grant| grant.grant_id != grant_id);
    if next.grants.len() == previous_len {
        return Err(OAUTH_REQUIRED.into());
    }
    state.replace_store(&app, next).await?;
    if let Ok(mut expiries) = state.live_expiries.lock() {
        expiries.remove(&grant_id);
    }
    Ok(McpOAuthRevokeResult {
        remote_revoked,
        removed_local: true,
    })
}

async fn discover_authorization(
    client: &reqwest::Client,
    endpoint: &str,
    requested_issuer: Option<&str>,
    requested_scopes: Vec<String>,
    cancellation: &mut watch::Receiver<bool>,
) -> Result<DiscoveredAuthorization, String> {
    let (resource_url, resource) =
        oauth::normalize_resource(endpoint).map_err(ToOwned::to_owned)?;
    let response = send_request(
        client.get(resource_url.clone()),
        cancellation,
        oauth::DISCOVERY_FAILED,
    )
    .await?;
    if response.status().is_redirection() {
        return Err(oauth::DISCOVERY_FAILED.into());
    }
    let challenges = response
        .headers()
        .get_all(WWW_AUTHENTICATE)
        .iter()
        .map(|value| {
            value
                .to_str()
                .map(ToOwned::to_owned)
                .map_err(|_| oauth::DISCOVERY_FAILED.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let challenge = oauth::parse_bearer_challenge(&challenges).map_err(ToOwned::to_owned)?;
    drop(response);

    let protected = if let Some(location) = challenge.resource_metadata.as_deref() {
        let metadata_url = oauth::validate_secure_url(location, true)
            .map_err(|_| oauth::DISCOVERY_FAILED.to_string())?;
        if metadata_url.origin() != resource_url.origin() {
            return Err(oauth::DISCOVERY_FAILED.into());
        }
        fetch_protected_metadata(client, metadata_url, &resource, cancellation).await?
    } else {
        let mut discovered = None;
        for candidate in
            oauth::protected_resource_candidates(&resource_url).map_err(ToOwned::to_owned)?
        {
            if let Some(metadata) =
                try_fetch_protected_metadata(client, candidate, &resource, cancellation).await?
            {
                discovered = Some(metadata);
                break;
            }
        }
        discovered.ok_or_else(|| oauth::DISCOVERY_FAILED.to_string())?
    };
    let issuer = oauth::select_issuer(&protected.authorization_servers, requested_issuer)
        .map_err(ToOwned::to_owned)?;
    let (issuer_url, _) = oauth::normalize_issuer(&issuer).map_err(ToOwned::to_owned)?;
    let mut server = None;
    for candidate in
        oauth::authorization_server_candidates(&issuer_url).map_err(ToOwned::to_owned)?
    {
        if let Some(metadata) =
            try_fetch_server_metadata(client, candidate, &issuer, cancellation).await?
        {
            server = Some(metadata);
            break;
        }
    }
    let scopes = challenge.scope.unwrap_or({
        if requested_scopes.is_empty() {
            protected.scopes_supported
        } else {
            requested_scopes
        }
    });
    Ok(DiscoveredAuthorization {
        resource,
        scopes,
        server: server.ok_or_else(|| oauth::DISCOVERY_FAILED.to_string())?,
    })
}

async fn fetch_protected_metadata(
    client: &reqwest::Client,
    url: reqwest::Url,
    resource: &str,
    cancellation: &mut watch::Receiver<bool>,
) -> Result<ProtectedResourceMetadata, String> {
    let response = send_request(client.get(url), cancellation, oauth::DISCOVERY_FAILED).await?;
    if !response.status().is_success() || response.status().is_redirection() {
        return Err(oauth::DISCOVERY_FAILED.into());
    }
    let bytes = read_json_response(
        response,
        oauth::MAX_METADATA_BYTES,
        cancellation,
        oauth::DISCOVERY_FAILED,
    )
    .await?;
    oauth::parse_protected_resource_metadata(&bytes, resource).map_err(ToOwned::to_owned)
}

async fn try_fetch_protected_metadata(
    client: &reqwest::Client,
    url: reqwest::Url,
    resource: &str,
    cancellation: &mut watch::Receiver<bool>,
) -> Result<Option<ProtectedResourceMetadata>, String> {
    let response = send_request(client.get(url), cancellation, oauth::DISCOVERY_FAILED).await?;
    if response.status().is_redirection() {
        return Err(oauth::DISCOVERY_FAILED.into());
    }
    if !response.status().is_success() {
        return Ok(None);
    }
    let bytes = read_json_response(
        response,
        oauth::MAX_METADATA_BYTES,
        cancellation,
        oauth::DISCOVERY_FAILED,
    )
    .await?;
    oauth::parse_protected_resource_metadata(&bytes, resource)
        .map(Some)
        .map_err(ToOwned::to_owned)
}

async fn try_fetch_server_metadata(
    client: &reqwest::Client,
    url: reqwest::Url,
    issuer: &str,
    cancellation: &mut watch::Receiver<bool>,
) -> Result<Option<AuthorizationServerMetadata>, String> {
    let response = send_request(client.get(url), cancellation, oauth::DISCOVERY_FAILED).await?;
    if response.status().is_redirection() {
        return Err(oauth::DISCOVERY_FAILED.into());
    }
    if !response.status().is_success() {
        return Ok(None);
    }
    let bytes = read_json_response(
        response,
        oauth::MAX_METADATA_BYTES,
        cancellation,
        oauth::DISCOVERY_FAILED,
    )
    .await?;
    oauth::parse_authorization_server_metadata(&bytes, issuer)
        .map(Some)
        .map_err(ToOwned::to_owned)
}

async fn persist_authorized_grant(
    state: &McpOAuthState,
    app: &tauri::AppHandle,
    discovered: DiscoveredAuthorization,
    client_id: String,
    token: oauth::TokenResponse,
) -> Result<McpOAuthGrantProjection, String> {
    let _mutation = state.mutation.lock().await;
    let mut store = state.store_snapshot(app).await?;
    let matching = store.grants.iter().position(|grant| {
        grant.issuer == discovered.server.issuer
            && grant.resource == discovered.resource
            && grant.client_id == client_id
    });
    if matching.is_none() && store.grants.len() >= MAX_GRANTS {
        return Err(STORAGE_FAILED.into());
    }
    let grant_id = matching
        .map(|index| store.grants[index].grant_id.clone())
        .unwrap_or(random_grant_id()?);
    let expires_at_ms = expiry_timestamp(token.expires_in)?;
    let grant = PersistedGrant {
        grant_id: grant_id.clone(),
        issuer: discovered.server.issuer,
        resource: discovered.resource,
        client_id,
        scopes: token.scopes.unwrap_or(discovered.scopes),
        access_token: seal_token(&token.access_token)?,
        refresh_token: token
            .refresh_token
            .as_ref()
            .map(|value| seal_token(value))
            .transpose()?,
        expires_at_ms,
        authorization_endpoint: discovered.server.authorization_endpoint,
        token_endpoint: discovered.server.token_endpoint,
        revocation_endpoint: discovered.server.revocation_endpoint,
        authorization_response_iss_parameter_supported: discovered
            .server
            .authorization_response_iss_parameter_supported,
    };
    if let Some(index) = matching {
        store.grants[index] = grant.clone();
    } else {
        store.grants.push(grant.clone());
    }
    state.replace_store(app, store).await?;
    state.remember_live_expiry(&grant_id, token.expires_in);
    Ok(project_grant(&grant, now_unix_ms()?))
}

fn token_is_usable(
    live_expiry: Option<Instant>,
    persisted_expiry_ms: Option<u64>,
    now: Instant,
    now_ms: u64,
) -> bool {
    if let Some(deadline) = live_expiry {
        return deadline.saturating_duration_since(now) > Duration::from_millis(EXPIRY_SAFETY_MS);
    }
    persisted_expiry_ms.is_none_or(|expires| expires > now_ms.saturating_add(EXPIRY_SAFETY_MS))
}

fn project_grant(grant: &PersistedGrant, now: u64) -> McpOAuthGrantProjection {
    McpOAuthGrantProjection {
        grant_id: grant.grant_id.clone(),
        issuer: grant.issuer.clone(),
        resource: grant.resource.clone(),
        client_id: grant.client_id.clone(),
        scopes: grant.scopes.clone(),
        expires_at_ms: grant.expires_at_ms,
        status: if grant.expires_at_ms.is_some_and(|expires| expires <= now) {
            "expired"
        } else {
            "active"
        },
    }
}

fn oauth_client(error: &'static str) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(NETWORK_TIMEOUT)
        .timeout(NETWORK_TIMEOUT)
        .build()
        .map_err(|_| error.to_string())
}

async fn send_request(
    request: reqwest::RequestBuilder,
    cancellation: &mut watch::Receiver<bool>,
    error: &'static str,
) -> Result<reqwest::Response, String> {
    if *cancellation.borrow() {
        return Err(OAUTH_CANCELLED.into());
    }
    tokio::select! {
        biased;
        changed = cancellation.changed() => {
            let _ = changed;
            Err(OAUTH_CANCELLED.into())
        }
        response = request.send() => response.map_err(|_| error.to_string()),
    }
}

async fn read_json_response(
    response: reqwest::Response,
    limit: usize,
    cancellation: &mut watch::Receiver<bool>,
    error: &'static str,
) -> Result<Zeroizing<Vec<u8>>, String> {
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .is_some_and(|value| {
            value.eq_ignore_ascii_case("application/json")
                || value.to_ascii_lowercase().ends_with("+json")
        });
    if !content_type {
        return Err(error.into());
    }
    let mut bytes = Zeroizing::new(Vec::new());
    let mut stream = response.bytes_stream();
    loop {
        let chunk = tokio::select! {
            biased;
            changed = cancellation.changed() => {
                let _ = changed;
                return Err(OAUTH_CANCELLED.into());
            }
            chunk = stream.next() => chunk,
        };
        let Some(chunk) = chunk else { break };
        let chunk = chunk.map_err(|_| error.to_string())?;
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(error.into());
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.is_empty() {
        return Err(error.into());
    }
    Ok(bytes)
}

async fn send_form(
    client: &reqwest::Client,
    endpoint: &str,
    form: &[(&str, &str)],
    cancellation: &mut watch::Receiver<bool>,
    error: &'static str,
) -> Result<Zeroizing<Vec<u8>>, String> {
    let url = oauth::validate_secure_url(endpoint, true).map_err(|_| error.to_string())?;
    let response = send_request(client.post(url).form(form), cancellation, error).await?;
    if response.status().is_redirection() || !response.status().is_success() {
        return Err(error.into());
    }
    read_json_response(
        response,
        oauth::MAX_TOKEN_RESPONSE_BYTES,
        cancellation,
        error,
    )
    .await
}

async fn send_form_allow_empty(
    client: &reqwest::Client,
    endpoint: &str,
    form: &[(&str, &str)],
    cancellation: &mut watch::Receiver<bool>,
    error: &'static str,
) -> Result<(), String> {
    let url = oauth::validate_secure_url(endpoint, true).map_err(|_| error.to_string())?;
    let response = send_request(client.post(url).form(form), cancellation, error).await?;
    if response.status().is_redirection() || !response.status().is_success() {
        return Err(error.into());
    }
    let mut stream = response.bytes_stream();
    let mut bytes = 0usize;
    while let Some(chunk) = tokio::select! {
        biased;
        changed = cancellation.changed() => {
            let _ = changed;
            return Err(OAUTH_CANCELLED.into());
        }
        chunk = stream.next() => chunk,
    } {
        let chunk = chunk.map_err(|_| error.to_string())?;
        bytes = bytes.saturating_add(chunk.len());
        if bytes > oauth::MAX_TOKEN_RESPONSE_BYTES {
            return Err(error.into());
        }
    }
    Ok(())
}

async fn read_callback(
    stream: &mut TcpStream,
    cancellation: &mut watch::Receiver<bool>,
) -> Result<Zeroizing<Vec<u8>>, String> {
    let mut request = Zeroizing::new(Vec::new());
    let mut buffer = Zeroizing::new([0_u8; 2048]);
    loop {
        let count = tokio::select! {
            biased;
            changed = cancellation.changed() => {
                let _ = changed;
                return Err(OAUTH_CANCELLED.into());
            }
            read = tokio::time::timeout(NETWORK_TIMEOUT, stream.read(&mut buffer[..])) => {
                read.map_err(|_| oauth::CALLBACK_FAILED.to_string())?
                    .map_err(|_| oauth::CALLBACK_FAILED.to_string())?
            }
        };
        if count == 0 {
            return Err(oauth::CALLBACK_FAILED.into());
        }
        request.extend_from_slice(&buffer[..count]);
        if request.len() > 16 * 1024 {
            return Err(oauth::CALLBACK_FAILED.into());
        }
        if request.ends_with(b"\r\n\r\n") {
            return Ok(request);
        }
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            return Err(oauth::CALLBACK_FAILED.into());
        }
    }
}

async fn write_callback_page(stream: &mut TcpStream, success: bool) -> Result<(), String> {
    let body = if success {
        "Authorization completed. You may close this window."
    } else {
        "Authorization failed. You may close this window."
    };
    let status = if success { "200 OK" } else { "400 Bad Request" };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .await
        .map_err(|_| oauth::CALLBACK_FAILED.to_string())?;
    stream
        .shutdown()
        .await
        .map_err(|_| oauth::CALLBACK_FAILED.to_string())
}

fn seal_token(token: &str) -> Result<String, String> {
    let sealer = platform_sealer();
    devbox_secrets::seal_v1(sealer.as_ref(), token)
        .map(|bytes| B64.encode(bytes))
        .map_err(|_| STORAGE_FAILED.to_string())
}

fn unseal_token(encoded: &str) -> Result<Zeroizing<String>, String> {
    if encoded.is_empty() || encoded.len() > oauth::MAX_TOKEN_BYTES * 4 {
        return Err(STORAGE_FAILED.into());
    }
    let bytes = B64
        .decode(encoded)
        .map_err(|_| STORAGE_FAILED.to_string())?;
    let sealer = platform_sealer();
    devbox_secrets::unseal_v1(sealer.as_ref(), &bytes).map_err(|_| STORAGE_FAILED.to_string())
}

fn grant_store_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
        Err(STORAGE_FAILED.into())
    }
    #[cfg(target_os = "windows")]
    {
        let root = app
            .path()
            .app_local_data_dir()
            .map_err(|_| STORAGE_FAILED.to_string())?;
        Ok(root.join("oauth").join("mcp-grants.json"))
    }
}

fn load_store_path(path: &Path) -> Result<GrantStore, String> {
    prepare_store_parent(path)?;
    if !path.exists() {
        return Ok(GrantStore::default());
    }
    devbox_filesystem::ensure_no_links(path).map_err(|_| STORAGE_FAILED.to_string())?;
    let identity = devbox_filesystem::filesystem_identity(path, false)
        .map_err(|_| STORAGE_FAILED.to_string())?;
    let metadata = std::fs::metadata(path).map_err(|_| STORAGE_FAILED.to_string())?;
    if metadata.len() > MAX_STORE_BYTES as u64 {
        return Err(STORAGE_FAILED.into());
    }
    let bytes = std::fs::read(path).map_err(|_| STORAGE_FAILED.to_string())?;
    if devbox_filesystem::filesystem_identity(path, false)
        .map_err(|_| STORAGE_FAILED.to_string())?
        != identity
    {
        return Err(STORAGE_FAILED.into());
    }
    decode_store(&bytes)
}

fn save_store_path(path: &Path, store: &GrantStore) -> Result<(), String> {
    prepare_store_parent(path)?;
    if path.exists() {
        devbox_filesystem::ensure_no_links(path).map_err(|_| STORAGE_FAILED.to_string())?;
    }
    let parent = path.parent().ok_or_else(|| STORAGE_FAILED.to_string())?;
    let parent_identity = devbox_filesystem::filesystem_identity(parent, true)
        .map_err(|_| STORAGE_FAILED.to_string())?;
    let bytes = Zeroizing::new(encode_store(store)?);
    devbox_filesystem::atomic_write(path, bytes.as_slice())
        .map_err(|_| STORAGE_FAILED.to_string())?;
    if devbox_filesystem::filesystem_identity(parent, true)
        .map_err(|_| STORAGE_FAILED.to_string())?
        != parent_identity
        || devbox_filesystem::filesystem_identity(path, false).is_err()
    {
        return Err(STORAGE_FAILED.into());
    }
    Ok(())
}

fn prepare_store_parent(path: &Path) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| STORAGE_FAILED.to_string())?;
    std::fs::create_dir_all(parent).map_err(|_| STORAGE_FAILED.to_string())?;
    devbox_filesystem::ensure_no_links(parent).map_err(|_| STORAGE_FAILED.to_string())
}

fn encode_store(store: &GrantStore) -> Result<Vec<u8>, String> {
    validate_store(store)?;
    let bytes = serde_json::to_vec(store).map_err(|_| STORAGE_FAILED.to_string())?;
    if bytes.len() > MAX_STORE_BYTES {
        Err(STORAGE_FAILED.into())
    } else {
        Ok(bytes)
    }
}

fn decode_store(bytes: &[u8]) -> Result<GrantStore, String> {
    let value = oauth::parse_unique_json(bytes, MAX_STORE_BYTES, STORAGE_FAILED)
        .map_err(ToOwned::to_owned)?;
    let store =
        serde_json::from_value::<GrantStore>(value).map_err(|_| STORAGE_FAILED.to_string())?;
    validate_store(&store)?;
    Ok(store)
}

fn validate_store(store: &GrantStore) -> Result<(), String> {
    if store.schema != STORE_SCHEMA
        || store.version != STORE_VERSION
        || store.grants.len() > MAX_GRANTS
    {
        return Err(STORAGE_FAILED.into());
    }
    let mut ids = BTreeSet::new();
    let mut bindings = BTreeSet::new();
    for grant in &store.grants {
        validate_grant_id(&grant.grant_id)?;
        let (_, issuer) =
            oauth::normalize_issuer(&grant.issuer).map_err(|_| STORAGE_FAILED.to_string())?;
        let (_, resource) =
            oauth::normalize_resource(&grant.resource).map_err(|_| STORAGE_FAILED.to_string())?;
        oauth::validate_client_id(&grant.client_id).map_err(|_| STORAGE_FAILED.to_string())?;
        oauth::validate_scopes(&grant.scopes).map_err(|_| STORAGE_FAILED.to_string())?;
        if issuer != grant.issuer
            || resource != grant.resource
            || !ids.insert(grant.grant_id.clone())
            || !bindings.insert((issuer, resource, grant.client_id.clone()))
            || grant.access_token.is_empty()
            || grant.access_token.len() > oauth::MAX_TOKEN_BYTES * 4
        {
            return Err(STORAGE_FAILED.into());
        }
        oauth::validate_secure_url(&grant.authorization_endpoint, true)
            .map_err(|_| STORAGE_FAILED.to_string())?;
        oauth::validate_secure_url(&grant.token_endpoint, true)
            .map_err(|_| STORAGE_FAILED.to_string())?;
        if let Some(endpoint) = &grant.revocation_endpoint {
            oauth::validate_secure_url(endpoint, true).map_err(|_| STORAGE_FAILED.to_string())?;
        }
        if grant
            .refresh_token
            .as_ref()
            .is_some_and(|value| value.is_empty() || value.len() > oauth::MAX_TOKEN_BYTES * 4)
        {
            return Err(STORAGE_FAILED.into());
        }
    }
    Ok(())
}

fn validate_request_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > MAX_REQUEST_ID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        Err(oauth::REQUEST_INVALID.into())
    } else {
        Ok(())
    }
}

pub(crate) fn validate_grant_id(value: &str) -> Result<(), String> {
    if value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        Ok(())
    } else {
        Err(OAUTH_REQUIRED.into())
    }
}

fn random_grant_id() -> Result<String, String> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| STORAGE_FAILED.to_string())?;
    let mut output = String::with_capacity(32);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").map_err(|_| STORAGE_FAILED.to_string())?;
    }
    Ok(output)
}

fn expiry_timestamp(expires_in: Option<u64>) -> Result<Option<u64>, String> {
    expires_in
        .map(|seconds| {
            now_unix_ms()?
                .checked_add(
                    seconds
                        .checked_mul(1_000)
                        .ok_or_else(|| oauth::TOKEN_FAILED.to_string())?,
                )
                .ok_or_else(|| oauth::TOKEN_FAILED.to_string())
        })
        .transpose()
}

fn now_unix_ms() -> Result<u64, String> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| STORAGE_FAILED.to_string())?
        .as_millis();
    u64::try_from(millis).map_err(|_| STORAGE_FAILED.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::thread;

    fn grant() -> PersistedGrant {
        PersistedGrant {
            grant_id: "a".repeat(32),
            issuer: "https://auth.example".into(),
            resource: "https://mcp.example/mcp".into(),
            client_id: "public-client".into(),
            scopes: vec!["read".into()],
            access_token: "sealed-access".into(),
            refresh_token: Some("sealed-refresh".into()),
            expires_at_ms: Some(1_000),
            authorization_endpoint: "https://auth.example/authorize".into(),
            token_endpoint: "https://auth.example/token".into(),
            revocation_endpoint: Some("https://auth.example/revoke".into()),
            authorization_response_iss_parameter_supported: true,
        }
    }

    #[test]
    fn store_round_trip_rejects_duplicates_and_unknown_fields() {
        let store = GrantStore {
            grants: vec![grant()],
            ..GrantStore::default()
        };
        let encoded = encode_store(&store).unwrap();
        assert_eq!(decode_store(&encoded).unwrap().grants.len(), 1);
        let duplicate = br#"{"schema":"devbox.api-playground.mcp-oauth-grants","version":1,"version":1,"grants":[]}"#;
        assert_eq!(
            decode_store(duplicate).map(|_| ()),
            Err(STORAGE_FAILED.into())
        );
        let unknown = br#"{"schema":"devbox.api-playground.mcp-oauth-grants","version":1,"grants":[],"token":"leak"}"#;
        assert_eq!(
            decode_store(unknown).map(|_| ()),
            Err(STORAGE_FAILED.into())
        );
    }

    #[test]
    fn projection_never_contains_token_material() {
        let projection = project_grant(&grant(), 2_000);
        let value = serde_json::to_value(projection).unwrap();
        let serialized = value.to_string();
        assert!(!serialized.contains("sealed-access"));
        assert!(!serialized.contains("sealed-refresh"));
        assert_eq!(value["status"], "expired");
    }

    #[test]
    fn request_and_grant_ids_are_bounded() {
        assert!(validate_request_id("oauth-request-1").is_ok());
        assert!(validate_request_id("bad request").is_err());
        assert!(validate_grant_id(&"f".repeat(32)).is_ok());
        assert!(validate_grant_id(&"F".repeat(32)).is_err());
    }

    #[test]
    fn callback_page_is_constant_and_never_accepts_reflected_text() {
        let success = "Authorization completed. You may close this window.";
        let failure = "Authorization failed. You may close this window.";
        assert!(!success.contains('?'));
        assert!(!failure.contains('?'));
    }

    #[test]
    fn live_expiry_uses_monotonic_time_before_persisted_wall_clock() {
        let now = Instant::now();
        let safely_live = now.checked_add(Duration::from_secs(61)).unwrap();
        let too_close = now.checked_add(Duration::from_secs(60)).unwrap();
        assert!(token_is_usable(Some(safely_live), Some(1), now, u64::MAX));
        assert!(!token_is_usable(Some(too_close), Some(u64::MAX), now, 0));
        assert!(token_is_usable(None, None, now, u64::MAX));
        assert!(!token_is_usable(None, Some(60_000), now, 0));
    }

    #[test]
    fn grant_store_is_atomic_bounded_and_rejects_malformed_envelopes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("oauth").join("mcp-grants.json");
        let store = GrantStore {
            grants: vec![grant()],
            ..GrantStore::default()
        };
        save_store_path(&path, &store).unwrap();
        assert_eq!(load_store_path(&path).unwrap().grants.len(), 1);

        let oversized = GrantStore {
            grants: vec![grant(); MAX_GRANTS + 1],
            ..GrantStore::default()
        };
        assert_eq!(encode_store(&oversized), Err(STORAGE_FAILED.into()));
        assert_eq!(
            unseal_token("not-base64").map(|_| ()),
            Err(STORAGE_FAILED.into())
        );
    }

    #[tokio::test]
    async fn loopback_discovery_follows_resource_then_authorization_metadata() {
        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let base = format!("http://{address}");
        let endpoint = format!("{base}/mcp");
        let issuer = format!("{base}/issuer");
        let protected = serde_json::json!({
            "resource": endpoint,
            "authorization_servers": [issuer],
            "scopes_supported": ["read"]
        })
        .to_string();
        let server = serde_json::json!({
            "issuer": issuer,
            "authorization_endpoint": format!("{base}/authorize"),
            "token_endpoint": format!("{base}/token"),
            "response_types_supported": ["code"],
            "grant_types_supported": ["authorization_code", "refresh_token"],
            "code_challenge_methods_supported": ["S256"],
            "token_endpoint_auth_methods_supported": ["none"]
        })
        .to_string();
        let responses = vec![
            (
                "/mcp",
                "401 Unauthorized",
                vec![("WWW-Authenticate", "Bearer realm=\"mcp\"")],
                String::new(),
            ),
            (
                "/.well-known/oauth-protected-resource/mcp",
                "200 OK",
                vec![("Content-Type", "application/json")],
                protected,
            ),
            (
                "/.well-known/oauth-authorization-server/issuer",
                "200 OK",
                vec![("Content-Type", "application/json")],
                server,
            ),
        ];
        let handle = thread::spawn(move || {
            for (expected_path, status, headers, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                let mut buffer = [0_u8; 1024];
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&buffer[..count]);
                }
                let request = String::from_utf8(request).unwrap();
                assert!(request.starts_with(&format!("GET {expected_path} HTTP/1.1\r\n")));
                let mut response = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
                    body.len()
                );
                for (name, value) in headers {
                    response.push_str(&format!("{name}: {value}\r\n"));
                }
                response.push_str("\r\n");
                response.push_str(&body);
                stream.write_all(response.as_bytes()).unwrap();
            }
        });

        let client = oauth_client(oauth::DISCOVERY_FAILED).unwrap();
        let (_sender, mut cancellation) = watch::channel(false);
        let discovered =
            discover_authorization(&client, &endpoint, None, Vec::new(), &mut cancellation)
                .await
                .unwrap();
        assert_eq!(discovered.resource, endpoint);
        assert_eq!(discovered.server.issuer, issuer);
        assert_eq!(discovered.scopes, vec!["read"]);
        handle.join().unwrap();
    }
}
