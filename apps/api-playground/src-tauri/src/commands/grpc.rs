//! Bounded dynamic gRPC client for Protocol Lab.
//!
//! The backend owns native source selections, descriptor-derived method paths,
//! channels, cancellation, TLS material, and summary export construction. IPC
//! exposes stable codes and bounded projections only.

use crate::commands::grpc_credentials::{GrpcCredentialState, PreparedTlsCredential};
use crate::commands::grpc_selection::{
    pick_grpc_selection, random_hex_128, validate_opaque_id, GrpcNativeSelection,
    GrpcSelectionKind, GrpcSelectionState,
};
use crate::core::grpc::{self, GrpcRootMode, GrpcRpcKind};
use prost::Message;
use prost_reflect::{DescriptorPool, DynamicMessage, MethodDescriptor};
use prost_types::{FileDescriptorProto, FileDescriptorSet};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::{mpsc, watch};
use tokio_stream::wrappers::ReceiverStream;
use tonic::codegen::http::uri::PathAndQuery;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Endpoint, Identity};
use tonic::{Code, Request, Status};

const MAX_CONNECTIONS: usize = 8;
const MAX_ACTIVE_REQUESTS: usize = 4;
const MAX_REQUEST_ID_BYTES: usize = 128;
const MIN_CONNECT_TIMEOUT_MS: u64 = 100;
const MAX_CONNECT_TIMEOUT_MS: u64 = 30_000;
const MIN_RPC_TIMEOUT_MS: u64 = 100;
const MAX_RPC_TIMEOUT_MS: u64 = 300_000;
const COMBINED_CONNECT_CEILING: Duration = Duration::from_secs(120);
const MAX_EXPORT_BYTES: usize = 64 * 1024;
const MAX_JS_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_ECMASCRIPT_DATE_MS: u64 = 8_640_000_000_000_000;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GrpcConnectProfile {
    endpoint: String,
    source: GrpcSchemaSource,
    tls: GrpcTlsProfile,
    connect_timeout_ms: u64,
    rpc_timeout_ms: u64,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum GrpcSchemaSource {
    LocalProto {
        proto_selection_id: String,
        #[serde(default)]
        import_root_selection_id: Option<String>,
    },
    Reflection,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GrpcTlsProfile {
    root_mode: GrpcRootMode,
    #[serde(default)]
    server_name: Option<String>,
    #[serde(default)]
    credential_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrpcSourceProjection {
    kind: String,
    label: Option<String>,
    descriptor_file_count: usize,
    service_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrpcTlsProjection {
    mode: String,
    encrypted: bool,
    credential_used: bool,
    server_name_overridden: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrpcConnectResult {
    connection_id: String,
    authority: String,
    source: GrpcSourceProjection,
    tls: GrpcTlsProjection,
    methods: Vec<grpc::GrpcMethodProjection>,
    rpc_timeout_ms: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrpcInvokeResult {
    ok: bool,
    status: String,
    responses: Vec<Value>,
    request_message_count: usize,
    response_message_count: usize,
    started_at_ms: u64,
    elapsed_ms: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GrpcExchangeSummary {
    source_kind: String,
    service: String,
    method: String,
    rpc_kind: GrpcRpcKind,
    request_message_count: usize,
    response_message_count: usize,
    started_at_ms: u64,
    elapsed_ms: u64,
    status: String,
    tls_mode: String,
    credential_used: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GrpcExchangeExport<'a> {
    schema: &'static str,
    version: u32,
    exchange: &'a GrpcExchangeSummary,
}

#[derive(Clone)]
struct ConnectionSnapshot {
    channel: Channel,
    _pool: Arc<DescriptorPool>,
    methods: Arc<HashMap<String, MethodDescriptor>>,
    rpc_timeout: Duration,
}

struct StoredConnection {
    snapshot: ConnectionSnapshot,
}

#[derive(Default)]
struct GrpcStateInner {
    connections: HashMap<String, StoredConnection>,
    active: HashMap<(String, String), watch::Sender<bool>>,
    pending_connections: usize,
}

#[derive(Default)]
pub struct GrpcState {
    inner: Mutex<GrpcStateInner>,
}

struct ConnectionAttemptGuard<'a> {
    state: &'a GrpcState,
}

impl Drop for ConnectionAttemptGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.state.inner.lock() {
            inner.pending_connections = inner.pending_connections.saturating_sub(1);
        }
    }
}

struct ActiveRequestGuard<'a> {
    state: &'a GrpcState,
    connection_id: String,
    request_id: String,
}

impl Drop for ActiveRequestGuard<'_> {
    fn drop(&mut self) {
        self.state
            .finish_request(&self.connection_id, &self.request_id);
    }
}

impl GrpcState {
    fn begin_connection_attempt(&self) -> Result<ConnectionAttemptGuard<'_>, &'static str> {
        let mut inner = self.inner.lock().map_err(|_| grpc::CONNECTION_STALE)?;
        if inner
            .connections
            .len()
            .saturating_add(inner.pending_connections)
            >= MAX_CONNECTIONS
        {
            return Err(grpc::CONNECTION_LIMIT);
        }
        inner.pending_connections = inner
            .pending_connections
            .checked_add(1)
            .ok_or(grpc::CONNECTION_LIMIT)?;
        Ok(ConnectionAttemptGuard { state: self })
    }

    fn insert_connection(&self, snapshot: ConnectionSnapshot) -> Result<String, &'static str> {
        let mut inner = self.inner.lock().map_err(|_| grpc::CONNECTION_STALE)?;
        if inner.connections.len() >= MAX_CONNECTIONS {
            return Err(grpc::CONNECTION_LIMIT);
        }
        for _ in 0..4 {
            let id = random_hex_128().map_err(|_| grpc::CONNECTION_LIMIT)?;
            if !inner.connections.contains_key(&id) {
                inner
                    .connections
                    .insert(id.clone(), StoredConnection { snapshot });
                return Ok(id);
            }
        }
        Err(grpc::CONNECTION_LIMIT)
    }

    fn begin_request(
        &self,
        connection_id: &str,
        request_id: &str,
    ) -> Result<(ConnectionSnapshot, watch::Receiver<bool>), &'static str> {
        validate_connection_id(connection_id)?;
        validate_request_id(request_id)?;
        let mut inner = self.inner.lock().map_err(|_| grpc::CONNECTION_STALE)?;
        let snapshot = inner
            .connections
            .get(connection_id)
            .ok_or(grpc::CONNECTION_STALE)?
            .snapshot
            .clone();
        let active_for_connection = inner
            .active
            .keys()
            .filter(|(owner, _)| owner == connection_id)
            .count();
        let key = (connection_id.to_string(), request_id.to_string());
        if active_for_connection >= MAX_ACTIVE_REQUESTS || inner.active.contains_key(&key) {
            return Err(grpc::REQUEST_LIMIT);
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
        validate_request_id(request_id)?;
        let inner = self.inner.lock().map_err(|_| grpc::CONNECTION_STALE)?;
        if !inner.connections.contains_key(connection_id) {
            return Err(grpc::CONNECTION_STALE);
        }
        let Some(sender) = inner
            .active
            .get(&(connection_id.to_string(), request_id.to_string()))
        else {
            return Ok(false);
        };
        sender.send(true).map_err(|_| grpc::REQUEST_CANCELLED)?;
        Ok(true)
    }

    fn disconnect(&self, connection_id: &str) -> Result<(), &'static str> {
        validate_connection_id(connection_id)?;
        let mut inner = self.inner.lock().map_err(|_| grpc::CONNECTION_STALE)?;
        if !inner.connections.contains_key(connection_id) {
            return Err(grpc::CONNECTION_STALE);
        }
        for ((owner, _), sender) in &inner.active {
            if owner == connection_id {
                let _ = sender.send(true);
            }
        }
        inner.connections.remove(connection_id);
        Ok(())
    }
}

struct PreparedSource {
    pool: DescriptorPool,
    kind: String,
    label: Option<String>,
    consumed: Vec<(String, GrpcSelectionKind)>,
}

#[tauri::command]
pub async fn pick_grpc_proto(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<GrpcSelectionState>>,
) -> Result<Option<GrpcNativeSelection>, String> {
    pick_grpc_selection(app, state.inner().as_ref(), GrpcSelectionKind::Proto).await
}

#[tauri::command]
pub async fn pick_grpc_import_root(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<GrpcSelectionState>>,
) -> Result<Option<GrpcNativeSelection>, String> {
    pick_grpc_selection(app, state.inner().as_ref(), GrpcSelectionKind::ImportRoot).await
}

#[tauri::command]
pub async fn connect_grpc(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<GrpcState>>,
    selections: tauri::State<'_, Arc<GrpcSelectionState>>,
    credentials: tauri::State<'_, Arc<GrpcCredentialState>>,
    profile: GrpcConnectProfile,
) -> Result<GrpcConnectResult, String> {
    let _attempt = state
        .begin_connection_attempt()
        .map_err(ToOwned::to_owned)?;
    tokio::time::timeout(
        COMBINED_CONNECT_CEILING,
        connect_grpc_inner(
            &app,
            state.inner().as_ref(),
            selections.inner().as_ref(),
            credentials.inner().as_ref(),
            profile,
        ),
    )
    .await
    .map_err(|_| grpc::CONNECT_TIMEOUT.to_string())?
}

async fn connect_grpc_inner(
    app: &tauri::AppHandle,
    state: &GrpcState,
    selections: &GrpcSelectionState,
    credentials: &GrpcCredentialState,
    profile: GrpcConnectProfile,
) -> Result<GrpcConnectResult, String> {
    validate_timeout(
        profile.connect_timeout_ms,
        MIN_CONNECT_TIMEOUT_MS,
        MAX_CONNECT_TIMEOUT_MS,
    )?;
    validate_timeout(
        profile.rpc_timeout_ms,
        MIN_RPC_TIMEOUT_MS,
        MAX_RPC_TIMEOUT_MS,
    )?;
    let endpoint = grpc::normalize_endpoint(&profile.endpoint).map_err(ToOwned::to_owned)?;
    let server_name = grpc::validate_server_name(profile.tls.server_name.as_deref())
        .map_err(ToOwned::to_owned)?;
    validate_tls_shape(&endpoint, &profile.tls, server_name.as_deref())?;

    let mut prepared_source = match profile.source {
        GrpcSchemaSource::LocalProto {
            proto_selection_id,
            import_root_selection_id,
        } => prepare_local_source(selections, proto_selection_id, import_root_selection_id).await?,
        GrpcSchemaSource::Reflection => PreparedSource {
            pool: DescriptorPool::new(),
            kind: "reflection-pending".into(),
            label: None,
            consumed: Vec::new(),
        },
    };

    let prepared_credential = match profile.tls.credential_id.as_deref() {
        Some(id) => Some(credentials.resolve_for_connection(app, id).await?),
        None => None,
    };
    validate_resolved_tls(&endpoint, &profile.tls, prepared_credential.as_ref())?;
    let connect_timeout = Duration::from_millis(profile.connect_timeout_ms);
    let channel = build_channel(
        &endpoint,
        profile.tls.root_mode,
        server_name.as_deref(),
        prepared_credential,
        connect_timeout,
    )
    .await?;

    if prepared_source.kind == "reflection-pending" {
        let (pool, kind) = fetch_reflection_pool(channel.clone()).await?;
        prepared_source.pool = pool;
        prepared_source.kind = kind.into();
    }
    let projection =
        grpc::validate_descriptor_pool(&prepared_source.pool).map_err(ToOwned::to_owned)?;
    let methods = grpc::method_map(&prepared_source.pool).map_err(ToOwned::to_owned)?;
    let source = GrpcSourceProjection {
        kind: prepared_source.kind,
        label: prepared_source.label,
        descriptor_file_count: projection.descriptor_file_count,
        service_count: projection.service_count,
    };
    let tls = GrpcTlsProjection {
        mode: if endpoint.tls {
            profile.tls.root_mode.as_str().into()
        } else {
            "plaintext".into()
        },
        encrypted: endpoint.tls,
        credential_used: profile.tls.credential_id.is_some(),
        server_name_overridden: server_name.is_some(),
    };
    let snapshot = ConnectionSnapshot {
        channel,
        _pool: Arc::new(prepared_source.pool),
        methods: Arc::new(methods),
        rpc_timeout: Duration::from_millis(profile.rpc_timeout_ms),
    };
    let source_claim = if prepared_source.consumed.is_empty() {
        None
    } else {
        Some(
            selections
                .claim_many(&prepared_source.consumed)
                .map_err(ToOwned::to_owned)?,
        )
    };
    let connection_id = match state.insert_connection(snapshot) {
        Ok(connection_id) => connection_id,
        Err(code) => return Err(code.to_string()),
    };
    if let Some(claim) = source_claim {
        if let Err(code) = claim.finish(true) {
            let _ = state.disconnect(&connection_id);
            return Err(code.to_string());
        }
    }
    Ok(GrpcConnectResult {
        connection_id,
        authority: endpoint.authority,
        source,
        tls,
        methods: projection.methods,
        rpc_timeout_ms: profile.rpc_timeout_ms,
    })
}

async fn prepare_local_source(
    selections: &GrpcSelectionState,
    proto_selection_id: String,
    import_root_selection_id: Option<String>,
) -> Result<PreparedSource, String> {
    let proto = selections
        .review(&proto_selection_id, GrpcSelectionKind::Proto)
        .map_err(ToOwned::to_owned)?;
    let (import_root, import_root_id) = if let Some(selection_id) = import_root_selection_id {
        let root = selections
            .review(&selection_id, GrpcSelectionKind::ImportRoot)
            .map_err(ToOwned::to_owned)?;
        ((root.canonical, root.identity), Some(selection_id))
    } else {
        (
            proto
                .default_import_root
                .clone()
                .ok_or_else(|| grpc::SOURCE_SELECTION_INVALID.to_string())?,
            None,
        )
    };
    let file = proto.canonical.clone();
    let file_identity = proto.identity;
    let (root, root_identity) = import_root;
    let pool = tauri::async_runtime::spawn_blocking(move || {
        grpc::compile_local_proto(&file, file_identity, &root, root_identity)
    })
    .await
    .map_err(|_| grpc::SOURCE_INVALID.to_string())?
    .map_err(ToOwned::to_owned)?;
    let mut consumed = vec![(proto_selection_id, GrpcSelectionKind::Proto)];
    if let Some(selection_id) = import_root_id {
        consumed.push((selection_id, GrpcSelectionKind::ImportRoot));
    }
    Ok(PreparedSource {
        pool,
        kind: "local-proto".into(),
        label: Some(proto.label),
        consumed,
    })
}

fn validate_timeout(value: u64, minimum: u64, maximum: u64) -> Result<(), String> {
    if (minimum..=maximum).contains(&value) {
        Ok(())
    } else {
        Err(grpc::INVALID_PROFILE.into())
    }
}

fn validate_tls_shape(
    endpoint: &grpc::NormalizedGrpcEndpoint,
    tls: &GrpcTlsProfile,
    server_name: Option<&str>,
) -> Result<(), String> {
    if !endpoint.tls
        && (tls.root_mode != GrpcRootMode::Native
            || tls.credential_id.is_some()
            || server_name.is_some())
    {
        return Err(grpc::INVALID_PROFILE.into());
    }
    if tls
        .credential_id
        .as_ref()
        .is_some_and(|value| validate_opaque_id(value).is_err())
    {
        return Err(grpc::CREDENTIAL_INVALID.into());
    }
    Ok(())
}

fn validate_resolved_tls(
    endpoint: &grpc::NormalizedGrpcEndpoint,
    tls: &GrpcTlsProfile,
    credential: Option<&PreparedTlsCredential>,
) -> Result<(), String> {
    if !endpoint.tls {
        return if credential.is_none() {
            Ok(())
        } else {
            Err(grpc::INVALID_PROFILE.into())
        };
    }
    if tls.root_mode.uses_custom() && credential.and_then(|value| value.ca_pem.as_ref()).is_none() {
        return Err(grpc::CREDENTIAL_INVALID.into());
    }
    if tls.root_mode == GrpcRootMode::Native
        && credential.is_some_and(|value| {
            value.client_certificate_pem.is_none() || value.client_key_pem.is_none()
        })
    {
        return Err(grpc::CREDENTIAL_INVALID.into());
    }
    Ok(())
}

async fn build_channel(
    endpoint: &grpc::NormalizedGrpcEndpoint,
    root_mode: GrpcRootMode,
    server_name: Option<&str>,
    credential: Option<PreparedTlsCredential>,
    connect_timeout: Duration,
) -> Result<Channel, String> {
    let mut builder = Endpoint::from_shared(endpoint.uri.clone())
        .map_err(|_| grpc::INVALID_PROFILE.to_string())?
        .connect_timeout(connect_timeout);
    if endpoint.tls {
        let mut tls = ClientTlsConfig::new().timeout(connect_timeout);
        if root_mode.uses_native() {
            tls = tls.with_native_roots();
        }
        if let Some(server_name) = server_name {
            tls = tls.domain_name(server_name.to_string());
        }
        if let Some(credential) = credential {
            if root_mode.uses_custom() {
                let ca = credential
                    .ca_pem
                    .as_ref()
                    .ok_or_else(|| grpc::CREDENTIAL_INVALID.to_string())?;
                tls = tls.ca_certificate(Certificate::from_pem(ca.as_bytes()));
            }
            match (
                credential.client_certificate_pem.as_ref(),
                credential.client_key_pem.as_ref(),
            ) {
                (Some(certificate), Some(key)) => {
                    tls = tls.identity(Identity::from_pem(certificate.as_bytes(), key.as_bytes()));
                }
                (None, None) => {}
                _ => return Err(grpc::CREDENTIAL_INVALID.into()),
            }
        }
        builder = builder
            .tls_config(tls)
            .map_err(|_| grpc::TLS_FAILED.to_string())?;
    }
    match tokio::time::timeout(connect_timeout, builder.connect()).await {
        Err(_) => Err(grpc::CONNECT_TIMEOUT.into()),
        Ok(Err(_)) if endpoint.tls => Err(grpc::TLS_FAILED.into()),
        Ok(Err(_)) => Err(grpc::PROTOCOL_FAILED.into()),
        Ok(Ok(channel)) => Ok(channel),
    }
}

#[tauri::command]
pub fn cancel_grpc(
    state: tauri::State<'_, Arc<GrpcState>>,
    connection_id: String,
    request_id: String,
) -> Result<bool, String> {
    state
        .cancel_request(&connection_id, &request_id)
        .map_err(ToOwned::to_owned)
}

#[tauri::command]
pub fn disconnect_grpc(
    state: tauri::State<'_, Arc<GrpcState>>,
    connection_id: String,
) -> Result<(), String> {
    state.disconnect(&connection_id).map_err(ToOwned::to_owned)
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ReflectionFailure {
    Unimplemented,
    Failed,
}

#[derive(Default)]
struct DescriptorAccumulator {
    raw_by_name: BTreeMap<String, Vec<u8>>,
    proto_by_name: BTreeMap<String, FileDescriptorProto>,
    received_bytes: usize,
}

impl DescriptorAccumulator {
    fn add_batch(&mut self, batch: Vec<Vec<u8>>) -> Result<(), ReflectionFailure> {
        for raw in batch {
            if raw.is_empty() || raw.len() > grpc::MAX_DESCRIPTOR_FILE_BYTES {
                return Err(ReflectionFailure::Failed);
            }
            self.received_bytes = self
                .received_bytes
                .checked_add(raw.len())
                .ok_or(ReflectionFailure::Failed)?;
            if self.received_bytes > grpc::MAX_DESCRIPTOR_TOTAL_BYTES {
                return Err(ReflectionFailure::Failed);
            }
            let mut proto = FileDescriptorProto::decode(raw.as_slice())
                .map_err(|_| ReflectionFailure::Failed)?;
            let name = proto.name.clone().ok_or(ReflectionFailure::Failed)?;
            if !valid_reflected_name(&name) {
                return Err(ReflectionFailure::Failed);
            }
            if let Some(existing) = self.raw_by_name.get(&name) {
                if existing != &raw {
                    return Err(ReflectionFailure::Failed);
                }
                continue;
            }
            if self.raw_by_name.len() >= grpc::MAX_DESCRIPTOR_FILES {
                return Err(ReflectionFailure::Failed);
            }
            proto.source_code_info = None;
            self.raw_by_name.insert(name.clone(), raw);
            self.proto_by_name.insert(name, proto);
        }
        Ok(())
    }

    fn into_pool(self) -> Result<DescriptorPool, ReflectionFailure> {
        if self.proto_by_name.is_empty() {
            return Err(ReflectionFailure::Failed);
        }
        let descriptor_set = FileDescriptorSet {
            file: self.proto_by_name.into_values().collect(),
        };
        let pool = DescriptorPool::from_file_descriptor_set(descriptor_set)
            .map_err(|_| ReflectionFailure::Failed)?;
        grpc::validate_descriptor_pool(&pool).map_err(|_| ReflectionFailure::Failed)?;
        Ok(pool)
    }
}

async fn fetch_reflection_pool(channel: Channel) -> Result<(DescriptorPool, &'static str), String> {
    match fetch_reflection_v1(channel.clone()).await {
        Ok(pool) => Ok((pool, "reflection-v1")),
        Err(ReflectionFailure::Unimplemented) => fetch_reflection_v1alpha(channel)
            .await
            .map(|pool| (pool, "reflection-v1alpha"))
            .map_err(|_| grpc::REFLECTION_UNAVAILABLE.to_string()),
        Err(ReflectionFailure::Failed) => Err(grpc::REFLECTION_UNAVAILABLE.into()),
    }
}

async fn fetch_reflection_v1(channel: Channel) -> Result<DescriptorPool, ReflectionFailure> {
    use tonic_reflection::pb::v1::server_reflection_client::ServerReflectionClient;
    use tonic_reflection::pb::v1::server_reflection_request::MessageRequest;
    use tonic_reflection::pb::v1::server_reflection_response::MessageResponse;
    use tonic_reflection::pb::v1::ServerReflectionRequest;

    let list_request = ServerReflectionRequest {
        host: String::new(),
        message_request: Some(MessageRequest::ListServices(String::new())),
    };
    let (sender, receiver) = mpsc::channel(1);
    sender
        .send(list_request.clone())
        .await
        .map_err(|_| ReflectionFailure::Failed)?;
    let mut request = Request::new(ReceiverStream::new(receiver));
    request.set_timeout(COMBINED_CONNECT_CEILING);
    let mut client = ServerReflectionClient::new(channel)
        .max_encoding_message_size(grpc::MAX_MESSAGE_BYTES)
        .max_decoding_message_size(grpc::MAX_DESCRIPTOR_TOTAL_BYTES);
    let response = client
        .server_reflection_info(request)
        .await
        .map_err(classify_initial_reflection_status)?;
    let mut stream = response.into_inner();
    let response = stream
        .message()
        .await
        .map_err(classify_initial_reflection_status)?
        .ok_or(ReflectionFailure::Failed)?;
    if response.original_request.as_ref() != Some(&list_request) {
        return Err(ReflectionFailure::Failed);
    }
    let services = match response.message_response {
        Some(MessageResponse::ListServicesResponse(response)) => response.service,
        Some(MessageResponse::ErrorResponse(error)) if error.error_code == 12 => {
            return Err(ReflectionFailure::Unimplemented);
        }
        _ => return Err(ReflectionFailure::Failed),
    };
    let services = validate_reflected_services(services.into_iter().map(|service| service.name))?;
    let mut descriptors = DescriptorAccumulator::default();
    for service in services {
        let descriptor_request = ServerReflectionRequest {
            host: String::new(),
            message_request: Some(MessageRequest::FileContainingSymbol(service)),
        };
        sender
            .send(descriptor_request.clone())
            .await
            .map_err(|_| ReflectionFailure::Failed)?;
        let response = stream
            .message()
            .await
            .map_err(|_| ReflectionFailure::Failed)?
            .ok_or(ReflectionFailure::Failed)?;
        if response.original_request.as_ref() != Some(&descriptor_request) {
            return Err(ReflectionFailure::Failed);
        }
        match response.message_response {
            Some(MessageResponse::FileDescriptorResponse(response)) => {
                descriptors.add_batch(response.file_descriptor_proto)?;
            }
            _ => return Err(ReflectionFailure::Failed),
        }
    }
    drop(sender);
    descriptors.into_pool()
}

async fn fetch_reflection_v1alpha(channel: Channel) -> Result<DescriptorPool, ReflectionFailure> {
    use tonic_reflection::pb::v1alpha::server_reflection_client::ServerReflectionClient;
    use tonic_reflection::pb::v1alpha::server_reflection_request::MessageRequest;
    use tonic_reflection::pb::v1alpha::server_reflection_response::MessageResponse;
    use tonic_reflection::pb::v1alpha::ServerReflectionRequest;

    let list_request = ServerReflectionRequest {
        host: String::new(),
        message_request: Some(MessageRequest::ListServices(String::new())),
    };
    let (sender, receiver) = mpsc::channel(1);
    sender
        .send(list_request.clone())
        .await
        .map_err(|_| ReflectionFailure::Failed)?;
    let mut request = Request::new(ReceiverStream::new(receiver));
    request.set_timeout(COMBINED_CONNECT_CEILING);
    let mut client = ServerReflectionClient::new(channel)
        .max_encoding_message_size(grpc::MAX_MESSAGE_BYTES)
        .max_decoding_message_size(grpc::MAX_DESCRIPTOR_TOTAL_BYTES);
    let response = client
        .server_reflection_info(request)
        .await
        .map_err(|_| ReflectionFailure::Failed)?;
    let mut stream = response.into_inner();
    let response = stream
        .message()
        .await
        .map_err(|_| ReflectionFailure::Failed)?
        .ok_or(ReflectionFailure::Failed)?;
    if response.original_request.as_ref() != Some(&list_request) {
        return Err(ReflectionFailure::Failed);
    }
    let services = match response.message_response {
        Some(MessageResponse::ListServicesResponse(response)) => response.service,
        _ => return Err(ReflectionFailure::Failed),
    };
    let services = validate_reflected_services(services.into_iter().map(|service| service.name))?;
    let mut descriptors = DescriptorAccumulator::default();
    for service in services {
        let descriptor_request = ServerReflectionRequest {
            host: String::new(),
            message_request: Some(MessageRequest::FileContainingSymbol(service)),
        };
        sender
            .send(descriptor_request.clone())
            .await
            .map_err(|_| ReflectionFailure::Failed)?;
        let response = stream
            .message()
            .await
            .map_err(|_| ReflectionFailure::Failed)?
            .ok_or(ReflectionFailure::Failed)?;
        if response.original_request.as_ref() != Some(&descriptor_request) {
            return Err(ReflectionFailure::Failed);
        }
        match response.message_response {
            Some(MessageResponse::FileDescriptorResponse(response)) => {
                descriptors.add_batch(response.file_descriptor_proto)?;
            }
            _ => return Err(ReflectionFailure::Failed),
        }
    }
    drop(sender);
    descriptors.into_pool()
}

fn classify_initial_reflection_status(status: Status) -> ReflectionFailure {
    if status.code() == Code::Unimplemented {
        ReflectionFailure::Unimplemented
    } else {
        ReflectionFailure::Failed
    }
}

fn validate_reflected_services(
    values: impl IntoIterator<Item = String>,
) -> Result<Vec<String>, ReflectionFailure> {
    let mut seen = BTreeSet::new();
    for value in values {
        if !valid_reflected_name(&value) || !seen.insert(value) || seen.len() > grpc::MAX_SERVICES {
            return Err(ReflectionFailure::Failed);
        }
    }
    let services = seen
        .into_iter()
        .filter(|value| !value.starts_with("grpc.reflection."))
        .collect::<Vec<_>>();
    if services.is_empty() {
        Err(ReflectionFailure::Failed)
    } else {
        Ok(services)
    }
}

fn valid_reflected_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= grpc::MAX_NAME_BYTES.saturating_mul(4)
        && value.is_ascii()
        && !value.chars().any(char::is_control)
        && !value.bytes().any(|byte| byte.is_ascii_whitespace())
}

struct RpcOutcome {
    status: &'static str,
    messages: Vec<DynamicMessage>,
}

#[tauri::command]
pub async fn invoke_grpc(
    state: tauri::State<'_, Arc<GrpcState>>,
    connection_id: String,
    request_id: String,
    method: String,
    messages: Vec<String>,
) -> Result<GrpcInvokeResult, String> {
    let started_at_ms = now_unix_ms(grpc::PROTOCOL_FAILED)?;
    let started = Instant::now();
    let (connection, mut cancellation) = state
        .begin_request(&connection_id, &request_id)
        .map_err(ToOwned::to_owned)?;
    let _request = ActiveRequestGuard {
        state: state.inner().as_ref(),
        connection_id,
        request_id,
    };
    let descriptor = connection
        .methods
        .get(if valid_summary_name(&method) {
            method.as_str()
        } else {
            ""
        })
        .cloned()
        .ok_or_else(|| grpc::METHOD_UNAVAILABLE.to_string())?;
    let request_message_count = messages.len();
    let parse_descriptor = descriptor.clone();
    let parsed = tauri::async_runtime::spawn_blocking(move || {
        grpc::parse_request_messages(&parse_descriptor, &messages)
    })
    .await
    .map_err(|_| grpc::REQUEST_INVALID.to_string())?
    .map_err(ToOwned::to_owned)?;
    if *cancellation.borrow() {
        return Err(grpc::REQUEST_CANCELLED.into());
    }

    let operation = invoke_rpc(&connection, &descriptor, parsed);
    tokio::pin!(operation);
    let outcome = tokio::select! {
        biased;
        changed = cancellation.changed() => {
            let _ = changed;
            return Err(grpc::REQUEST_CANCELLED.into());
        }
        _ = tokio::time::sleep(connection.rpc_timeout) => {
            return Err(grpc::REQUEST_TIMEOUT.into());
        }
        outcome = &mut operation => outcome?,
    };
    let responses = serialize_responses(outcome.messages)?;
    Ok(GrpcInvokeResult {
        ok: outcome.status == "OK",
        status: outcome.status.into(),
        response_message_count: responses.len(),
        responses,
        request_message_count,
        started_at_ms,
        elapsed_ms: elapsed_ms(started),
    })
}

async fn invoke_rpc(
    connection: &ConnectionSnapshot,
    method: &MethodDescriptor,
    messages: Vec<DynamicMessage>,
) -> Result<RpcOutcome, String> {
    let path = grpc::method_path(method)
        .map_err(ToOwned::to_owned)?
        .parse::<PathAndQuery>()
        .map_err(|_| grpc::DESCRIPTOR_INVALID.to_string())?;
    let codec = grpc::DynamicGrpcCodec::new(method);
    let mut client = tonic::client::Grpc::new(connection.channel.clone())
        .max_encoding_message_size(grpc::MAX_MESSAGE_BYTES)
        .max_decoding_message_size(grpc::MAX_MESSAGE_BYTES);
    client
        .ready()
        .await
        .map_err(|_| grpc::PROTOCOL_FAILED.to_string())?;
    match GrpcRpcKind::from_method(method) {
        GrpcRpcKind::Unary => {
            let message = messages
                .into_iter()
                .next()
                .ok_or_else(|| grpc::REQUEST_INVALID.to_string())?;
            let mut request = Request::new(message);
            request.set_timeout(connection.rpc_timeout);
            match client.unary(request, path, codec).await {
                Ok(response) => Ok(RpcOutcome {
                    status: "OK",
                    messages: vec![response.into_inner()],
                }),
                Err(status) => Ok(status_outcome(status)),
            }
        }
        GrpcRpcKind::ServerStreaming => {
            let message = messages
                .into_iter()
                .next()
                .ok_or_else(|| grpc::REQUEST_INVALID.to_string())?;
            let mut request = Request::new(message);
            request.set_timeout(connection.rpc_timeout);
            match client.server_streaming(request, path, codec).await {
                Ok(response) => collect_response_stream(response.into_inner()).await,
                Err(status) => Ok(status_outcome(status)),
            }
        }
        GrpcRpcKind::ClientStreaming => {
            let mut request = Request::new(tokio_stream::iter(messages));
            request.set_timeout(connection.rpc_timeout);
            match client.client_streaming(request, path, codec).await {
                Ok(response) => Ok(RpcOutcome {
                    status: "OK",
                    messages: vec![response.into_inner()],
                }),
                Err(status) => Ok(status_outcome(status)),
            }
        }
        GrpcRpcKind::BidirectionalStreaming => {
            let mut request = Request::new(tokio_stream::iter(messages));
            request.set_timeout(connection.rpc_timeout);
            match client.streaming(request, path, codec).await {
                Ok(response) => collect_response_stream(response.into_inner()).await,
                Err(status) => Ok(status_outcome(status)),
            }
        }
    }
}

async fn collect_response_stream(
    mut stream: tonic::Streaming<DynamicMessage>,
) -> Result<RpcOutcome, String> {
    let mut messages = Vec::new();
    let mut encoded_total = 0usize;
    loop {
        let message = match stream.message().await {
            Ok(Some(message)) => message,
            Ok(None) => break,
            Err(status) => return Ok(status_outcome(status)),
        };
        if messages.len() >= grpc::MAX_STREAM_MESSAGES {
            return Err(grpc::RESPONSE_TOO_LARGE.into());
        }
        let encoded = message.encoded_len();
        if encoded > grpc::MAX_MESSAGE_BYTES {
            return Err(grpc::RESPONSE_TOO_LARGE.into());
        }
        encoded_total = encoded_total
            .checked_add(encoded)
            .ok_or_else(|| grpc::RESPONSE_TOO_LARGE.to_string())?;
        if encoded_total > grpc::MAX_RESPONSE_TOTAL_BYTES {
            return Err(grpc::RESPONSE_TOO_LARGE.into());
        }
        messages.push(message);
    }
    Ok(RpcOutcome {
        status: "OK",
        messages,
    })
}

fn status_outcome(status: Status) -> RpcOutcome {
    RpcOutcome {
        status: grpc::stable_status_name(status.code()),
        messages: Vec::new(),
    }
}

fn serialize_responses(messages: Vec<DynamicMessage>) -> Result<Vec<Value>, String> {
    let mut output = Vec::with_capacity(messages.len());
    let mut total = 0usize;
    for message in messages {
        let value = grpc::serialize_response_message(&message).map_err(ToOwned::to_owned)?;
        let length = serde_json::to_vec(&value)
            .map_err(|_| grpc::PROTOCOL_FAILED.to_string())?
            .len();
        total = total
            .checked_add(length)
            .ok_or_else(|| grpc::RESPONSE_TOO_LARGE.to_string())?;
        if total > grpc::MAX_RESPONSE_TOTAL_BYTES {
            return Err(grpc::RESPONSE_TOO_LARGE.into());
        }
        output.push(value);
    }
    Ok(output)
}

#[tauri::command]
pub async fn export_grpc_summary(
    app: tauri::AppHandle,
    summary: GrpcExchangeSummary,
) -> Result<bool, String> {
    validate_exchange_summary(&summary)?;
    let document = GrpcExchangeExport {
        schema: "devbox.api-playground.grpc-exchange/v1",
        version: 1,
        exchange: &summary,
    };
    let bytes =
        serde_json::to_vec_pretty(&document).map_err(|_| grpc::EXPORT_FAILED.to_string())?;
    if bytes.len() > MAX_EXPORT_BYTES {
        return Err(grpc::EXPORT_FAILED.into());
    }
    let selected = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_file_name("grpc-exchange.json")
            .add_filter("JSON", &["json"])
            .blocking_save_file()
    })
    .await
    .map_err(|_| grpc::EXPORT_FAILED.to_string())?;
    let Some(selected) = selected else {
        return Ok(false);
    };
    let path = selected
        .into_path()
        .map_err(|_| grpc::EXPORT_FAILED.to_string())?;
    tauri::async_runtime::spawn_blocking(move || save_export_path(&path, &bytes))
        .await
        .map_err(|_| grpc::EXPORT_FAILED.to_string())??;
    Ok(true)
}

fn validate_exchange_summary(summary: &GrpcExchangeSummary) -> Result<(), String> {
    if !matches!(
        summary.source_kind.as_str(),
        "local-proto" | "reflection-v1" | "reflection-v1alpha"
    ) || !valid_summary_name(&summary.service)
        || !valid_summary_name(&summary.method)
        || summary.request_message_count == 0
        || summary.request_message_count > grpc::MAX_STREAM_MESSAGES
        || summary.response_message_count > grpc::MAX_STREAM_MESSAGES
        || summary.started_at_ms == 0
        || summary.started_at_ms > MAX_ECMASCRIPT_DATE_MS
        || summary.elapsed_ms > MAX_JS_SAFE_INTEGER
        || !valid_status_name(&summary.status)
        || !matches!(
            summary.tls_mode.as_str(),
            "plaintext" | "native" | "custom" | "native+custom"
        )
        || (!summary.rpc_kind.accepts_multiple_requests() && summary.request_message_count != 1)
        || (!matches!(
            summary.rpc_kind,
            GrpcRpcKind::ServerStreaming | GrpcRpcKind::BidirectionalStreaming
        ) && summary.response_message_count > 1)
        || (summary.status == "OK"
            && matches!(
                summary.rpc_kind,
                GrpcRpcKind::Unary | GrpcRpcKind::ClientStreaming
            )
            && summary.response_message_count != 1)
        || (summary.tls_mode == "plaintext" && summary.credential_used)
    {
        return Err(grpc::EXPORT_FAILED.into());
    }
    Ok(())
}

fn valid_summary_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= grpc::MAX_NAME_BYTES
        && value.is_ascii()
        && !value.chars().any(char::is_control)
        && !value.bytes().any(|byte| byte.is_ascii_whitespace())
}

fn valid_status_name(value: &str) -> bool {
    matches!(
        value,
        "OK" | "CANCELLED"
            | "UNKNOWN"
            | "INVALID_ARGUMENT"
            | "DEADLINE_EXCEEDED"
            | "NOT_FOUND"
            | "ALREADY_EXISTS"
            | "PERMISSION_DENIED"
            | "RESOURCE_EXHAUSTED"
            | "FAILED_PRECONDITION"
            | "ABORTED"
            | "OUT_OF_RANGE"
            | "UNIMPLEMENTED"
            | "INTERNAL"
            | "UNAVAILABLE"
            | "DATA_LOSS"
            | "UNAUTHENTICATED"
    )
}

fn save_export_path(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    if !path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("json"))
    {
        return Err(grpc::EXPORT_FAILED.into());
    }
    let parent = path
        .parent()
        .ok_or_else(|| grpc::EXPORT_FAILED.to_string())?;
    devbox_filesystem::ensure_no_links(parent).map_err(|_| grpc::EXPORT_FAILED.to_string())?;
    match std::fs::symlink_metadata(path) {
        Ok(_) => {
            devbox_filesystem::ensure_no_links(path)
                .map_err(|_| grpc::EXPORT_FAILED.to_string())?;
            devbox_filesystem::filesystem_identity(path, false)
                .map_err(|_| grpc::EXPORT_FAILED.to_string())?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(grpc::EXPORT_FAILED.into()),
    }
    let parent_identity = devbox_filesystem::filesystem_identity(parent, true)
        .map_err(|_| grpc::EXPORT_FAILED.to_string())?;
    devbox_filesystem::atomic_write(path, bytes).map_err(|_| grpc::EXPORT_FAILED.to_string())?;
    if devbox_filesystem::filesystem_identity(parent, true)
        .map_err(|_| grpc::EXPORT_FAILED.to_string())?
        != parent_identity
        || devbox_filesystem::filesystem_identity(path, false).is_err()
    {
        return Err(grpc::EXPORT_FAILED.into());
    }
    Ok(())
}

fn validate_connection_id(value: &str) -> Result<(), &'static str> {
    validate_opaque_id(value).map_err(|_| grpc::CONNECTION_STALE)
}

fn validate_request_id(value: &str) -> Result<(), &'static str> {
    if value.is_empty()
        || value.len() > MAX_REQUEST_ID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        Err(grpc::REQUEST_INVALID)
    } else {
        Ok(())
    }
}

fn now_unix_ms(error: &'static str) -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|value| u64::try_from(value.as_millis()).ok())
        .filter(|value| *value > 0 && *value <= MAX_JS_SAFE_INTEGER)
        .ok_or_else(|| error.to_string())
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis())
        .unwrap_or(MAX_JS_SAFE_INTEGER)
        .min(MAX_JS_SAFE_INTEGER)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tempfile::TempDir;
    use tokio::sync::oneshot;
    use tokio_stream::wrappers::TcpListenerStream;
    use tokio_stream::Stream;
    use tonic::codegen::{http, Body, BoxFuture, Service, StdError};
    use tonic::transport::Server;
    use tonic::{Response, Streaming};

    #[derive(Clone, PartialEq, Message)]
    struct FixtureMessage {
        #[prost(string, tag = "1")]
        name: String,
        #[prost(int64, tag = "2")]
        count: i64,
    }

    type FixtureStream =
        Pin<Box<dyn Stream<Item = Result<FixtureMessage, Status>> + Send + 'static>>;

    #[derive(Clone, Default)]
    struct EchoFixtureServer;

    struct UnaryFixture;

    impl tonic::server::UnaryService<FixtureMessage> for UnaryFixture {
        type Response = FixtureMessage;
        type Future = BoxFuture<Response<Self::Response>, Status>;

        fn call(&mut self, request: Request<FixtureMessage>) -> Self::Future {
            Box::pin(async move { Ok(Response::new(request.into_inner())) })
        }
    }

    struct ServerStreamingFixture;

    impl tonic::server::ServerStreamingService<FixtureMessage> for ServerStreamingFixture {
        type Response = FixtureMessage;
        type ResponseStream = FixtureStream;
        type Future = BoxFuture<Response<Self::ResponseStream>, Status>;

        fn call(&mut self, request: Request<FixtureMessage>) -> Self::Future {
            Box::pin(async move {
                let first = request.into_inner();
                let second = FixtureMessage {
                    name: format!("{}-next", first.name),
                    count: first.count.saturating_add(1),
                };
                let stream: FixtureStream = Box::pin(tokio_stream::iter([Ok(first), Ok(second)]));
                Ok(Response::new(stream))
            })
        }
    }

    struct ClientStreamingFixture;

    impl tonic::server::ClientStreamingService<FixtureMessage> for ClientStreamingFixture {
        type Response = FixtureMessage;
        type Future = BoxFuture<Response<Self::Response>, Status>;

        fn call(&mut self, request: Request<Streaming<FixtureMessage>>) -> Self::Future {
            Box::pin(async move {
                let mut stream = request.into_inner();
                let mut names = Vec::new();
                let mut count = 0_i64;
                while let Some(message) = stream.message().await? {
                    names.push(message.name);
                    count = count.saturating_add(message.count);
                }
                Ok(Response::new(FixtureMessage {
                    name: names.join(","),
                    count,
                }))
            })
        }
    }

    struct BidirectionalStreamingFixture;

    impl tonic::server::StreamingService<FixtureMessage> for BidirectionalStreamingFixture {
        type Response = FixtureMessage;
        type ResponseStream = FixtureStream;
        type Future = BoxFuture<Response<Self::ResponseStream>, Status>;

        fn call(&mut self, request: Request<Streaming<FixtureMessage>>) -> Self::Future {
            Box::pin(async move {
                let mut request_stream = request.into_inner();
                let mut responses = Vec::new();
                while let Some(message) = request_stream.message().await? {
                    responses.push(Ok(message));
                }
                let stream: FixtureStream = Box::pin(tokio_stream::iter(responses));
                Ok(Response::new(stream))
            })
        }
    }

    impl<B> Service<http::Request<B>> for EchoFixtureServer
    where
        B: Body + Send + 'static,
        B::Error: Into<StdError> + Send + 'static,
    {
        type Response = http::Response<tonic::body::Body>;
        type Error = Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;

        fn poll_ready(&mut self, _context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, request: http::Request<B>) -> Self::Future {
            match request.uri().path() {
                "/demo.Echo/Unary" => Box::pin(async move {
                    let codec =
                        tonic_prost::ProstCodec::<FixtureMessage, FixtureMessage>::default();
                    let response = tonic::server::Grpc::new(codec)
                        .unary(UnaryFixture, request)
                        .await;
                    Ok(response)
                }),
                "/demo.Echo/Server" => Box::pin(async move {
                    let codec =
                        tonic_prost::ProstCodec::<FixtureMessage, FixtureMessage>::default();
                    let response = tonic::server::Grpc::new(codec)
                        .server_streaming(ServerStreamingFixture, request)
                        .await;
                    Ok(response)
                }),
                "/demo.Echo/Client" => Box::pin(async move {
                    let codec =
                        tonic_prost::ProstCodec::<FixtureMessage, FixtureMessage>::default();
                    let response = tonic::server::Grpc::new(codec)
                        .client_streaming(ClientStreamingFixture, request)
                        .await;
                    Ok(response)
                }),
                "/demo.Echo/Bidi" => Box::pin(async move {
                    let codec =
                        tonic_prost::ProstCodec::<FixtureMessage, FixtureMessage>::default();
                    let response = tonic::server::Grpc::new(codec)
                        .streaming(BidirectionalStreamingFixture, request)
                        .await;
                    Ok(response)
                }),
                _ => Box::pin(async move {
                    let mut response = http::Response::new(tonic::body::Body::default());
                    let headers = response.headers_mut();
                    headers.insert(Status::GRPC_STATUS, (Code::Unimplemented as i32).into());
                    headers.insert(
                        http::header::CONTENT_TYPE,
                        tonic::metadata::GRPC_CONTENT_TYPE,
                    );
                    Ok(response)
                }),
            }
        }
    }

    impl tonic::server::NamedService for EchoFixtureServer {
        const NAME: &'static str = "demo.Echo";
    }

    #[derive(Clone)]
    struct FixtureRouter<R> {
        echo: EchoFixtureServer,
        reflection: R,
    }

    impl<R, B> Service<http::Request<B>> for FixtureRouter<R>
    where
        B: Body + Send + 'static,
        B::Error: Into<StdError> + Send + 'static,
        R: Service<
                http::Request<B>,
                Response = http::Response<tonic::body::Body>,
                Error = Infallible,
            > + Clone
            + Send
            + 'static,
        R::Future: Send + 'static,
    {
        type Response = http::Response<tonic::body::Body>;
        type Error = Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;

        fn poll_ready(&mut self, _context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, request: http::Request<B>) -> Self::Future {
            if request.uri().path().starts_with("/demo.Echo/") {
                self.echo.call(request)
            } else {
                Box::pin(self.reflection.call(request))
            }
        }
    }

    fn fixture_pool() -> (TempDir, DescriptorPool) {
        let temp = TempDir::new().unwrap();
        let proto = temp.path().join("echo.proto");
        std::fs::write(
            &proto,
            r#"syntax = "proto3";
package demo;
message Payload {
  string name = 1;
  int64 count = 2;
}
service Echo {
  rpc Unary(Payload) returns (Payload);
  rpc Server(Payload) returns (stream Payload);
  rpc Client(stream Payload) returns (Payload);
  rpc Bidi(stream Payload) returns (stream Payload);
}
"#,
        )
        .unwrap();
        let file_identity = devbox_filesystem::filesystem_identity(&proto, false).unwrap();
        let root_identity = devbox_filesystem::filesystem_identity(temp.path(), true).unwrap();
        let pool =
            grpc::compile_local_proto(&proto, file_identity, temp.path(), root_identity).unwrap();
        (temp, pool)
    }

    async fn plaintext_channel(address: std::net::SocketAddr) -> Channel {
        let endpoint = grpc::normalize_endpoint(&format!("http://{address}")).unwrap();
        build_channel(
            &endpoint,
            GrpcRootMode::Native,
            None,
            None,
            Duration::from_secs(2),
        )
        .await
        .unwrap()
    }

    async fn invoke_fixture(
        snapshot: &ConnectionSnapshot,
        full_name: &str,
        messages: &[&str],
    ) -> Vec<Value> {
        let method = snapshot.methods.get(full_name).unwrap();
        let raw = messages
            .iter()
            .map(|value| (*value).to_string())
            .collect::<Vec<_>>();
        let parsed = grpc::parse_request_messages(method, &raw).unwrap();
        let outcome = invoke_rpc(snapshot, method, parsed).await.unwrap();
        assert_eq!(outcome.status, "OK");
        serialize_responses(outcome.messages).unwrap()
    }

    fn snapshot() -> ConnectionSnapshot {
        ConnectionSnapshot {
            channel: Endpoint::from_static("http://127.0.0.1:9").connect_lazy(),
            _pool: Arc::new(DescriptorPool::new()),
            methods: Arc::new(HashMap::new()),
            rpc_timeout: Duration::from_secs(1),
        }
    }

    #[tokio::test]
    async fn reflection_v1_and_all_rpc_shapes_dispatch_over_real_transport() {
        let (_temp, pool) = fixture_pool();
        let descriptors = pool.encode_to_vec();
        let reflection = tonic_reflection::server::Builder::configure()
            .register_encoded_file_descriptor_set(&descriptors)
            .build_v1()
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (shutdown, shutdown_receiver) = oneshot::channel();
        let server = tokio::spawn(async move {
            Server::builder()
                .serve_with_incoming_shutdown(
                    FixtureRouter {
                        echo: EchoFixtureServer,
                        reflection,
                    },
                    TcpListenerStream::new(listener),
                    async move {
                        let _ = shutdown_receiver.await;
                    },
                )
                .await
                .unwrap();
        });

        let channel = plaintext_channel(address).await;
        let (reflected_pool, source_kind) = fetch_reflection_pool(channel.clone()).await.unwrap();
        assert_eq!(source_kind, "reflection-v1");
        let methods = grpc::method_map(&reflected_pool).unwrap();
        assert_eq!(methods.len(), 4);
        let snapshot = ConnectionSnapshot {
            channel,
            _pool: Arc::new(reflected_pool),
            methods: Arc::new(methods),
            rpc_timeout: Duration::from_secs(2),
        };

        assert_eq!(
            invoke_fixture(
                &snapshot,
                "demo.Echo.Unary",
                &[r#"{"name":"unary","count":"1"}"#],
            )
            .await,
            vec![serde_json::json!({"name": "unary", "count": "1"})]
        );
        assert_eq!(
            invoke_fixture(
                &snapshot,
                "demo.Echo.Server",
                &[r#"{"name":"server","count":"2"}"#],
            )
            .await,
            vec![
                serde_json::json!({"name": "server", "count": "2"}),
                serde_json::json!({"name": "server-next", "count": "3"}),
            ]
        );
        assert_eq!(
            invoke_fixture(
                &snapshot,
                "demo.Echo.Client",
                &[
                    r#"{"name":"one","count":"3"}"#,
                    r#"{"name":"two","count":"4"}"#,
                ],
            )
            .await,
            vec![serde_json::json!({"name": "one,two", "count": "7"})]
        );
        assert_eq!(
            invoke_fixture(
                &snapshot,
                "demo.Echo.Bidi",
                &[
                    r#"{"name":"left","count":"5"}"#,
                    r#"{"name":"right","count":"6"}"#,
                ],
            )
            .await,
            vec![
                serde_json::json!({"name": "left", "count": "5"}),
                serde_json::json!({"name": "right", "count": "6"}),
            ]
        );

        let _ = shutdown.send(());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn reflection_falls_back_to_v1alpha_only_for_unimplemented_v1() {
        let (_temp, pool) = fixture_pool();
        let descriptors = pool.encode_to_vec();
        let reflection = tonic_reflection::server::Builder::configure()
            .register_encoded_file_descriptor_set(&descriptors)
            .build_v1alpha()
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (shutdown, shutdown_receiver) = oneshot::channel();
        let server = tokio::spawn(async move {
            Server::builder()
                .serve_with_incoming_shutdown(
                    reflection,
                    TcpListenerStream::new(listener),
                    async move {
                        let _ = shutdown_receiver.await;
                    },
                )
                .await
                .unwrap();
        });

        let (reflected_pool, source_kind) = fetch_reflection_pool(plaintext_channel(address).await)
            .await
            .unwrap();
        assert_eq!(source_kind, "reflection-v1alpha");
        assert_eq!(grpc::method_map(&reflected_pool).unwrap().len(), 4);

        let _ = shutdown.send(());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn request_reservations_are_owned_bounded_and_released() {
        let state = GrpcState::default();
        let connection_id = state.insert_connection(snapshot()).unwrap();
        let mut guards = Vec::new();
        for index in 0..MAX_ACTIVE_REQUESTS {
            let request_id = format!("request-{index}");
            assert!(state.begin_request(&connection_id, &request_id).is_ok());
            guards.push(request_id);
        }
        assert_eq!(
            state
                .begin_request(&connection_id, "request-overflow")
                .err()
                .unwrap(),
            grpc::REQUEST_LIMIT
        );
        state.finish_request(&connection_id, &guards[0]);
        assert!(state
            .begin_request(&connection_id, "request-replacement")
            .is_ok());
        state.finish_request(&connection_id, "request-replacement");
        assert!(state.disconnect(&connection_id).is_ok());
    }

    #[tokio::test]
    async fn disconnect_signals_owned_requests() {
        let state = GrpcState::default();
        let connection_id = state.insert_connection(snapshot()).unwrap();
        let (_, cancellation) = state.begin_request(&connection_id, "request-1").unwrap();
        state.disconnect(&connection_id).unwrap();
        assert!(*cancellation.borrow());
    }

    #[test]
    fn descriptor_accumulator_rejects_conflicting_duplicate_name() {
        let one = FileDescriptorProto {
            name: Some("demo.proto".into()),
            package: Some("one".into()),
            ..Default::default()
        }
        .encode_to_vec();
        let two = FileDescriptorProto {
            name: Some("demo.proto".into()),
            package: Some("two".into()),
            ..Default::default()
        }
        .encode_to_vec();
        let mut accumulator = DescriptorAccumulator::default();
        accumulator.add_batch(vec![one]).unwrap();
        assert_eq!(
            accumulator.add_batch(vec![two]).unwrap_err(),
            ReflectionFailure::Failed
        );
    }

    #[test]
    fn reflection_boundaries_reject_invalid_services_and_descriptor_payloads() {
        assert_eq!(
            validate_reflected_services(["demo.Greeter".into(), "demo.Greeter".into()])
                .unwrap_err(),
            ReflectionFailure::Failed
        );
        assert_eq!(
            validate_reflected_services(["grpc.reflection.v1.ServerReflection".into()])
                .unwrap_err(),
            ReflectionFailure::Failed
        );
        assert_eq!(
            validate_reflected_services(["bad service".into()]).unwrap_err(),
            ReflectionFailure::Failed
        );

        let mut accumulator = DescriptorAccumulator::default();
        assert_eq!(
            accumulator.add_batch(vec![vec![0xff]]).unwrap_err(),
            ReflectionFailure::Failed
        );
        assert_eq!(
            accumulator
                .add_batch(vec![vec![0_u8; grpc::MAX_DESCRIPTOR_FILE_BYTES + 1]])
                .unwrap_err(),
            ReflectionFailure::Failed
        );
        assert_eq!(
            DescriptorAccumulator::default().into_pool().unwrap_err(),
            ReflectionFailure::Failed
        );
    }

    #[test]
    fn tls_profiles_never_attach_material_to_plaintext() {
        let plaintext = grpc::normalize_endpoint("http://127.0.0.1:50051").unwrap();
        let https = grpc::normalize_endpoint("https://example.test").unwrap();
        let native = GrpcTlsProfile {
            root_mode: GrpcRootMode::Native,
            server_name: None,
            credential_id: None,
        };
        assert!(validate_tls_shape(&plaintext, &native, None).is_ok());
        assert!(validate_resolved_tls(&plaintext, &native, None).is_ok());

        let custom = GrpcTlsProfile {
            root_mode: GrpcRootMode::Custom,
            server_name: None,
            credential_id: Some("a".repeat(32)),
        };
        assert_eq!(
            validate_tls_shape(&plaintext, &custom, None).unwrap_err(),
            grpc::INVALID_PROFILE
        );
        assert_eq!(
            validate_resolved_tls(&https, &custom, None).unwrap_err(),
            grpc::CREDENTIAL_INVALID
        );
    }

    #[test]
    fn reflection_fallback_is_only_unimplemented() {
        assert_eq!(
            classify_initial_reflection_status(Status::unimplemented("hidden")),
            ReflectionFailure::Unimplemented
        );
        assert_eq!(
            classify_initial_reflection_status(Status::permission_denied("hidden")),
            ReflectionFailure::Failed
        );
    }

    #[test]
    fn summary_export_is_allowlisted_and_atomic() {
        let summary = GrpcExchangeSummary {
            source_kind: "local-proto".into(),
            service: "demo.Echo".into(),
            method: "Unary".into(),
            rpc_kind: GrpcRpcKind::Unary,
            request_message_count: 1,
            response_message_count: 1,
            started_at_ms: 1,
            elapsed_ms: 25,
            status: "OK".into(),
            tls_mode: "native".into(),
            credential_used: true,
        };
        validate_exchange_summary(&summary).unwrap();
        let document = serde_json::to_vec(&GrpcExchangeExport {
            schema: "devbox.api-playground.grpc-exchange/v1",
            version: 1,
            exchange: &summary,
        })
        .unwrap();
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("grpc.json");
        save_export_path(&path, &document).unwrap();
        let persisted: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(
            persisted["schema"],
            "devbox.api-playground.grpc-exchange/v1"
        );
        assert!(persisted.to_string().find("credentialId").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn summary_export_rejects_dangling_destination_link() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new().unwrap();
        let path = temp.path().join("grpc.json");
        symlink(temp.path().join("missing.json"), &path).unwrap();
        assert_eq!(
            save_export_path(&path, b"{}").unwrap_err(),
            grpc::EXPORT_FAILED
        );
    }
}
