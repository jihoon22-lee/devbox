//! Windows-only, DPAPI-sealed gRPC TLS credential storage.
//!
//! File paths, PEM bodies, DPAPI envelopes, and storage paths stay native.
//! Renderer-facing commands expose only opaque identifiers and safe metadata.

use crate::commands::grpc_selection::{
    pick_grpc_selection, random_hex_128, validate_opaque_id, GrpcNativeSelection,
    GrpcSelectionKind, GrpcSelectionState, ReviewedGrpcSelection,
};
use crate::core::{grpc, oauth};
use crate::platform::platform_grpc_sealer;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use devbox_filesystem::{ensure_no_links, filesystem_identity, open_filesystem_object};
use rustls_pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(target_os = "windows")]
use tauri::Manager;
use tokio::sync::Mutex as AsyncMutex;
use zeroize::{Zeroize, Zeroizing};

const STORE_SCHEMA: &str = "devbox.api-playground.grpc-tls-credentials";
const STORE_VERSION: u32 = 1;
const MAX_CREDENTIALS: usize = 16;
const MAX_STORE_BYTES: usize = 4 * 1024 * 1024;
const MAX_CA_BYTES: usize = 256 * 1024;
const MAX_CLIENT_CERTIFICATE_BYTES: usize = 256 * 1024;
const MAX_CLIENT_KEY_BYTES: usize = 128 * 1024;
const MAX_SEALED_FIELD_BYTES: usize = 1024 * 1024;
const MAX_LABEL_BYTES: usize = 256;
const MAX_CERTIFICATES: usize = 64;
const MAX_ECMASCRIPT_DATE_MS: u64 = 8_640_000_000_000_000;

#[derive(Default)]
pub struct GrpcCredentialState {
    store: AsyncMutex<Option<CredentialStore>>,
    mutation: AsyncMutex<()>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CredentialStore {
    schema: String,
    version: u32,
    credentials: Vec<PersistedCredential>,
}

impl Default for CredentialStore {
    fn default() -> Self {
        Self {
            schema: STORE_SCHEMA.into(),
            version: STORE_VERSION,
            credentials: Vec::new(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PersistedCredential {
    credential_id: String,
    label: String,
    ca_pem: Option<String>,
    client_certificate_pem: Option<String>,
    client_key_pem: Option<String>,
    created_at_ms: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrpcCredentialProjection {
    credential_id: String,
    label: String,
    has_custom_ca: bool,
    has_client_identity: bool,
    created_at_ms: u64,
}

/// Plaintext exists only while constructing one native TLS channel.
/// Deliberately does not implement `Clone`, `Debug`, or serialization.
pub(crate) struct PreparedTlsCredential {
    pub(crate) ca_pem: Option<Zeroizing<String>>,
    pub(crate) client_certificate_pem: Option<Zeroizing<String>>,
    pub(crate) client_key_pem: Option<Zeroizing<String>>,
}

impl GrpcCredentialState {
    async fn store_snapshot(&self, app: &tauri::AppHandle) -> Result<CredentialStore, String> {
        let mut store = self.store.lock().await;
        if store.is_none() {
            let path = credential_store_path(app)?;
            let loaded = tauri::async_runtime::spawn_blocking(move || load_store_path(&path))
                .await
                .map_err(|_| grpc::CREDENTIAL_STORAGE_FAILED.to_string())??;
            *store = Some(loaded);
        }
        store
            .clone()
            .ok_or_else(|| grpc::CREDENTIAL_STORAGE_FAILED.to_string())
    }

    async fn replace_store(
        &self,
        app: &tauri::AppHandle,
        next: CredentialStore,
    ) -> Result<(), String> {
        validate_store(&next)?;
        let path = credential_store_path(app)?;
        let saved = next.clone();
        tauri::async_runtime::spawn_blocking(move || save_store_path(&path, &saved))
            .await
            .map_err(|_| grpc::CREDENTIAL_STORAGE_FAILED.to_string())??;
        *self.store.lock().await = Some(next);
        Ok(())
    }

    pub(crate) async fn resolve_for_connection(
        &self,
        app: &tauri::AppHandle,
        credential_id: &str,
    ) -> Result<PreparedTlsCredential, String> {
        validate_credential_id(credential_id)?;
        let _mutation = self.mutation.lock().await;
        let store = self.store_snapshot(app).await?;
        let credential = store
            .credentials
            .iter()
            .find(|value| value.credential_id == credential_id)
            .ok_or_else(|| grpc::CREDENTIAL_INVALID.to_string())?;
        let ca_pem = credential
            .ca_pem
            .as_deref()
            .map(|value| unseal_pem(value, MAX_CA_BYTES))
            .transpose()?;
        let client_certificate_pem = credential
            .client_certificate_pem
            .as_deref()
            .map(|value| unseal_pem(value, MAX_CLIENT_CERTIFICATE_BYTES))
            .transpose()?;
        let client_key_pem = credential
            .client_key_pem
            .as_deref()
            .map(|value| unseal_pem(value, MAX_CLIENT_KEY_BYTES))
            .transpose()?;
        if let Some(value) = ca_pem.as_deref() {
            validate_certificate_pem(value)?;
        }
        match (client_certificate_pem.as_deref(), client_key_pem.as_deref()) {
            (Some(certificate), Some(key)) => {
                validate_certificate_pem(certificate)?;
                validate_private_key_pem(key)?;
            }
            (None, None) => {}
            _ => return Err(grpc::CREDENTIAL_INVALID.into()),
        }
        Ok(PreparedTlsCredential {
            ca_pem,
            client_certificate_pem,
            client_key_pem,
        })
    }
}

#[tauri::command]
pub async fn pick_grpc_ca(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<GrpcSelectionState>>,
) -> Result<Option<GrpcNativeSelection>, String> {
    let _ = credential_store_path(&app)?;
    pick_grpc_selection(app, state.inner().as_ref(), GrpcSelectionKind::Ca).await
}

#[tauri::command]
pub async fn pick_grpc_client_certificate(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<GrpcSelectionState>>,
) -> Result<Option<GrpcNativeSelection>, String> {
    let _ = credential_store_path(&app)?;
    pick_grpc_selection(
        app,
        state.inner().as_ref(),
        GrpcSelectionKind::ClientCertificate,
    )
    .await
}

#[tauri::command]
pub async fn pick_grpc_client_key(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<GrpcSelectionState>>,
) -> Result<Option<GrpcNativeSelection>, String> {
    let _ = credential_store_path(&app)?;
    pick_grpc_selection(app, state.inner().as_ref(), GrpcSelectionKind::ClientKey).await
}

#[tauri::command]
pub async fn import_grpc_tls_credential(
    app: tauri::AppHandle,
    selections: tauri::State<'_, Arc<GrpcSelectionState>>,
    state: tauri::State<'_, Arc<GrpcCredentialState>>,
    label: String,
    ca_selection_id: Option<String>,
    client_certificate_selection_id: Option<String>,
    client_key_selection_id: Option<String>,
) -> Result<GrpcCredentialProjection, String> {
    let _ = credential_store_path(&app)?;
    validate_label(&label)?;
    let has_ca = ca_selection_id.is_some();
    let has_certificate = client_certificate_selection_id.is_some();
    let has_key = client_key_selection_id.is_some();
    if (!has_ca && !has_certificate && !has_key) || has_certificate != has_key {
        return Err(grpc::CREDENTIAL_INVALID.into());
    }

    let ca = review_optional(
        selections.inner().as_ref(),
        ca_selection_id.as_deref(),
        GrpcSelectionKind::Ca,
    )?;
    let certificate = review_optional(
        selections.inner().as_ref(),
        client_certificate_selection_id.as_deref(),
        GrpcSelectionKind::ClientCertificate,
    )?;
    let key = review_optional(
        selections.inner().as_ref(),
        client_key_selection_id.as_deref(),
        GrpcSelectionKind::ClientKey,
    )?;
    let material = tauri::async_runtime::spawn_blocking(move || {
        let ca = ca
            .as_ref()
            .map(|value| read_selected_pem(value, MAX_CA_BYTES, validate_certificate_pem))
            .transpose()?;
        let certificate = certificate
            .as_ref()
            .map(|value| {
                read_selected_pem(
                    value,
                    MAX_CLIENT_CERTIFICATE_BYTES,
                    validate_certificate_pem,
                )
            })
            .transpose()?;
        let key = key
            .as_ref()
            .map(|value| read_selected_pem(value, MAX_CLIENT_KEY_BYTES, validate_private_key_pem))
            .transpose()?;
        Ok::<_, String>((ca, certificate, key))
    })
    .await
    .map_err(|_| grpc::CREDENTIAL_INVALID.to_string())??;

    let consumed = [
        ca_selection_id.map(|value| (value, GrpcSelectionKind::Ca)),
        client_certificate_selection_id.map(|value| (value, GrpcSelectionKind::ClientCertificate)),
        client_key_selection_id.map(|value| (value, GrpcSelectionKind::ClientKey)),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    let claim = selections
        .claim_many(&consumed)
        .map_err(ToOwned::to_owned)?;
    let outcome: Result<PersistedCredential, String> = async {
        let _mutation = state.mutation.lock().await;
        let mut store = state.store_snapshot(&app).await?;
        if store.credentials.len() >= MAX_CREDENTIALS {
            return Err(grpc::CREDENTIAL_STORAGE_FAILED.into());
        }
        if store
            .credentials
            .iter()
            .any(|credential| credential.label == label)
        {
            return Err(grpc::CREDENTIAL_INVALID.into());
        }
        let credential_id = random_unique_credential_id(&store)?;
        let created_at_ms = now_unix_ms()?;
        let persisted = PersistedCredential {
            credential_id,
            label,
            ca_pem: material
                .0
                .as_deref()
                .map(|value| seal_pem(value.as_str()))
                .transpose()?,
            client_certificate_pem: material
                .1
                .as_deref()
                .map(|value| seal_pem(value.as_str()))
                .transpose()?,
            client_key_pem: material
                .2
                .as_deref()
                .map(|value| seal_pem(value.as_str()))
                .transpose()?,
            created_at_ms,
        };
        store.credentials.push(persisted.clone());
        state.replace_store(&app, store).await?;
        Ok(persisted)
    }
    .await;
    match outcome {
        Ok(persisted) => {
            claim.finish(true).map_err(ToOwned::to_owned)?;
            Ok(project_credential(&persisted))
        }
        Err(code) => {
            let _ = claim.finish(false);
            Err(code)
        }
    }
}

#[tauri::command]
pub async fn list_grpc_tls_credentials(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<GrpcCredentialState>>,
) -> Result<Vec<GrpcCredentialProjection>, String> {
    let _ = credential_store_path(&app)?;
    let _mutation = state.mutation.lock().await;
    let mut values = state
        .store_snapshot(&app)
        .await?
        .credentials
        .iter()
        .map(project_credential)
        .collect::<Vec<_>>();
    values.sort_by(|left, right| {
        right
            .created_at_ms
            .cmp(&left.created_at_ms)
            .then_with(|| left.credential_id.cmp(&right.credential_id))
    });
    Ok(values)
}

#[tauri::command]
pub async fn delete_grpc_tls_credential(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<GrpcCredentialState>>,
    credential_id: String,
) -> Result<bool, String> {
    let _ = credential_store_path(&app)?;
    validate_credential_id(&credential_id)?;
    let _mutation = state.mutation.lock().await;
    let mut store = state.store_snapshot(&app).await?;
    let original = store.credentials.len();
    store
        .credentials
        .retain(|value| value.credential_id != credential_id);
    if store.credentials.len() == original {
        return Ok(false);
    }
    state.replace_store(&app, store).await?;
    Ok(true)
}

fn review_optional(
    selections: &GrpcSelectionState,
    selection_id: Option<&str>,
    kind: GrpcSelectionKind,
) -> Result<Option<ReviewedGrpcSelection>, String> {
    selection_id
        .map(|id| selections.review(id, kind).map_err(ToOwned::to_owned))
        .transpose()
}

fn read_selected_pem(
    selected: &ReviewedGrpcSelection,
    limit: usize,
    validate: fn(&str) -> Result<(), String>,
) -> Result<Zeroizing<String>, String> {
    ensure_no_links(&selected.canonical).map_err(|_| grpc::CREDENTIAL_INVALID.to_string())?;
    let (mut file, identity) = open_filesystem_object(&selected.canonical, false)
        .map_err(|_| grpc::CREDENTIAL_INVALID.to_string())?;
    if identity != selected.identity {
        return Err(grpc::CREDENTIAL_INVALID.into());
    }
    let length = usize::try_from(
        file.metadata()
            .map_err(|_| grpc::CREDENTIAL_INVALID.to_string())?
            .len(),
    )
    .map_err(|_| grpc::CREDENTIAL_INVALID.to_string())?;
    if length == 0 || length > limit {
        return Err(grpc::CREDENTIAL_INVALID.into());
    }
    let mut bytes = Zeroizing::new(Vec::with_capacity(length));
    file.by_ref()
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| grpc::CREDENTIAL_INVALID.to_string())?;
    if bytes.len() != length
        || bytes.len() > limit
        || filesystem_identity(&selected.canonical, false)
            .map_err(|_| grpc::CREDENTIAL_INVALID.to_string())?
            != selected.identity
    {
        return Err(grpc::CREDENTIAL_INVALID.into());
    }
    let owned = std::mem::take(&mut *bytes);
    let text = match String::from_utf8(owned) {
        Ok(value) => Zeroizing::new(value),
        Err(error) => {
            let mut invalid = error.into_bytes();
            invalid.zeroize();
            return Err(grpc::CREDENTIAL_INVALID.into());
        }
    };
    validate(&text)?;
    Ok(text)
}

fn validate_certificate_pem(value: &str) -> Result<(), String> {
    validate_exact_pem_blocks(value, &["CERTIFICATE"])?;
    let mut count = 0usize;
    for certificate in CertificateDer::pem_slice_iter(value.as_bytes()) {
        let _ = certificate.map_err(|_| grpc::CREDENTIAL_INVALID.to_string())?;
        count = count.saturating_add(1);
        if count > MAX_CERTIFICATES {
            return Err(grpc::CREDENTIAL_INVALID.into());
        }
    }
    if count == 0 {
        Err(grpc::CREDENTIAL_INVALID.into())
    } else {
        Ok(())
    }
}

fn validate_private_key_pem(value: &str) -> Result<(), String> {
    validate_exact_pem_blocks(value, &["PRIVATE KEY", "RSA PRIVATE KEY", "EC PRIVATE KEY"])?;
    let mut count = 0usize;
    for key in PrivateKeyDer::pem_slice_iter(value.as_bytes()) {
        let _ = key.map_err(|_| grpc::CREDENTIAL_INVALID.to_string())?;
        count = count.saturating_add(1);
    }
    if count == 1 {
        Ok(())
    } else {
        Err(grpc::CREDENTIAL_INVALID.into())
    }
}

fn validate_exact_pem_blocks(value: &str, allowed: &[&str]) -> Result<(), String> {
    if value.is_empty() || value.contains('\0') || value.chars().any(|ch| ch == '\u{feff}') {
        return Err(grpc::CREDENTIAL_INVALID.into());
    }
    let mut active: Option<&str> = None;
    let mut blocks = 0usize;
    for line in value.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(label) = active {
            let end = format!("-----END {label}-----");
            if line == end {
                active = None;
                blocks = blocks.saturating_add(1);
            } else if line.starts_with("-----BEGIN ") || line.starts_with("-----END ") {
                return Err(grpc::CREDENTIAL_INVALID.into());
            }
        } else if line.trim().is_empty() {
            continue;
        } else {
            let label = allowed
                .iter()
                .copied()
                .find(|label| line == format!("-----BEGIN {label}-----"))
                .ok_or_else(|| grpc::CREDENTIAL_INVALID.to_string())?;
            active = Some(label);
        }
    }
    if active.is_some() || blocks == 0 {
        Err(grpc::CREDENTIAL_INVALID.into())
    } else {
        Ok(())
    }
}

fn project_credential(value: &PersistedCredential) -> GrpcCredentialProjection {
    GrpcCredentialProjection {
        credential_id: value.credential_id.clone(),
        label: value.label.clone(),
        has_custom_ca: value.ca_pem.is_some(),
        has_client_identity: value.client_certificate_pem.is_some()
            && value.client_key_pem.is_some(),
        created_at_ms: value.created_at_ms,
    }
}

fn seal_pem(value: &str) -> Result<String, String> {
    let sealer = platform_grpc_sealer();
    seal_pem_with(sealer.as_ref(), value)
}

fn seal_pem_with(sealer: &dyn devbox_secrets::Sealer, value: &str) -> Result<String, String> {
    devbox_secrets::seal_v1(sealer, value)
        .map(|bytes| B64.encode(bytes))
        .map_err(|_| grpc::CREDENTIAL_STORAGE_FAILED.to_string())
}

fn unseal_pem(encoded: &str, plaintext_limit: usize) -> Result<Zeroizing<String>, String> {
    let sealer = platform_grpc_sealer();
    unseal_pem_with(sealer.as_ref(), encoded, plaintext_limit)
}

fn unseal_pem_with(
    sealer: &dyn devbox_secrets::Sealer,
    encoded: &str,
    plaintext_limit: usize,
) -> Result<Zeroizing<String>, String> {
    if encoded.is_empty() || encoded.len() > MAX_SEALED_FIELD_BYTES {
        return Err(grpc::CREDENTIAL_STORAGE_FAILED.into());
    }
    let mut bytes = Zeroizing::new(
        B64.decode(encoded)
            .map_err(|_| grpc::CREDENTIAL_STORAGE_FAILED.to_string())?,
    );
    let value = devbox_secrets::unseal_v1(sealer, &bytes)
        .map_err(|_| grpc::CREDENTIAL_STORAGE_FAILED.to_string())?;
    bytes.zeroize();
    if value.is_empty() || value.len() > plaintext_limit {
        return Err(grpc::CREDENTIAL_STORAGE_FAILED.into());
    }
    Ok(value)
}

fn credential_store_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
        Err(grpc::CREDENTIAL_STORAGE_UNAVAILABLE.into())
    }
    #[cfg(target_os = "windows")]
    {
        let root = app
            .path()
            .app_local_data_dir()
            .map_err(|_| grpc::CREDENTIAL_STORAGE_FAILED.to_string())?;
        Ok(root.join("grpc").join("tls-credentials.json"))
    }
}

fn load_store_path(path: &Path) -> Result<CredentialStore, String> {
    prepare_store_parent(path)?;
    match std::fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CredentialStore::default());
        }
        Err(_) => return Err(grpc::CREDENTIAL_STORAGE_FAILED.into()),
    }
    ensure_no_links(path).map_err(|_| grpc::CREDENTIAL_STORAGE_FAILED.to_string())?;
    let (mut file, identity) = open_filesystem_object(path, false)
        .map_err(|_| grpc::CREDENTIAL_STORAGE_FAILED.to_string())?;
    let length = usize::try_from(
        file.metadata()
            .map_err(|_| grpc::CREDENTIAL_STORAGE_FAILED.to_string())?
            .len(),
    )
    .map_err(|_| grpc::CREDENTIAL_STORAGE_FAILED.to_string())?;
    if length == 0 || length > MAX_STORE_BYTES {
        return Err(grpc::CREDENTIAL_STORAGE_FAILED.into());
    }
    let mut bytes = Zeroizing::new(Vec::with_capacity(length));
    file.by_ref()
        .take((MAX_STORE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| grpc::CREDENTIAL_STORAGE_FAILED.to_string())?;
    if bytes.len() != length
        || bytes.len() > MAX_STORE_BYTES
        || filesystem_identity(path, false)
            .map_err(|_| grpc::CREDENTIAL_STORAGE_FAILED.to_string())?
            != identity
    {
        return Err(grpc::CREDENTIAL_STORAGE_FAILED.into());
    }
    decode_store(&bytes)
}

fn save_store_path(path: &Path, store: &CredentialStore) -> Result<(), String> {
    prepare_store_parent(path)?;
    match std::fs::symlink_metadata(path) {
        Ok(_) => ensure_no_links(path).map_err(|_| grpc::CREDENTIAL_STORAGE_FAILED.to_string())?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(grpc::CREDENTIAL_STORAGE_FAILED.into()),
    }
    let parent = path
        .parent()
        .ok_or_else(|| grpc::CREDENTIAL_STORAGE_FAILED.to_string())?;
    let parent_identity = filesystem_identity(parent, true)
        .map_err(|_| grpc::CREDENTIAL_STORAGE_FAILED.to_string())?;
    let bytes = Zeroizing::new(encode_store(store)?);
    devbox_filesystem::atomic_write(path, bytes.as_slice())
        .map_err(|_| grpc::CREDENTIAL_STORAGE_FAILED.to_string())?;
    if filesystem_identity(parent, true).map_err(|_| grpc::CREDENTIAL_STORAGE_FAILED.to_string())?
        != parent_identity
        || filesystem_identity(path, false).is_err()
    {
        return Err(grpc::CREDENTIAL_STORAGE_FAILED.into());
    }
    Ok(())
}

fn prepare_store_parent(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| grpc::CREDENTIAL_STORAGE_FAILED.to_string())?;
    create_directory_no_links(parent)
}

fn create_directory_no_links(path: &Path) -> Result<(), String> {
    if path.exists() {
        ensure_no_links(path).map_err(|_| grpc::CREDENTIAL_STORAGE_FAILED.to_string())?;
        filesystem_identity(path, true).map_err(|_| grpc::CREDENTIAL_STORAGE_FAILED.to_string())?;
        return Ok(());
    }
    let parent = path
        .parent()
        .filter(|parent| *parent != path)
        .ok_or_else(|| grpc::CREDENTIAL_STORAGE_FAILED.to_string())?;
    create_directory_no_links(parent)?;
    match std::fs::create_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(grpc::CREDENTIAL_STORAGE_FAILED.into()),
    }
    ensure_no_links(path).map_err(|_| grpc::CREDENTIAL_STORAGE_FAILED.to_string())?;
    filesystem_identity(path, true)
        .map(|_| ())
        .map_err(|_| grpc::CREDENTIAL_STORAGE_FAILED.to_string())
}

fn encode_store(store: &CredentialStore) -> Result<Vec<u8>, String> {
    validate_store(store)?;
    let bytes =
        serde_json::to_vec(store).map_err(|_| grpc::CREDENTIAL_STORAGE_FAILED.to_string())?;
    if bytes.len() > MAX_STORE_BYTES {
        Err(grpc::CREDENTIAL_STORAGE_FAILED.into())
    } else {
        Ok(bytes)
    }
}

fn decode_store(bytes: &[u8]) -> Result<CredentialStore, String> {
    let value = oauth::parse_unique_json(bytes, MAX_STORE_BYTES, grpc::CREDENTIAL_STORAGE_FAILED)
        .map_err(ToOwned::to_owned)?;
    let store = serde_json::from_value::<CredentialStore>(value)
        .map_err(|_| grpc::CREDENTIAL_STORAGE_FAILED.to_string())?;
    validate_store(&store)?;
    Ok(store)
}

fn validate_store(store: &CredentialStore) -> Result<(), String> {
    if store.schema != STORE_SCHEMA
        || store.version != STORE_VERSION
        || store.credentials.len() > MAX_CREDENTIALS
    {
        return Err(grpc::CREDENTIAL_STORAGE_FAILED.into());
    }
    let mut ids = BTreeSet::new();
    let mut labels = BTreeSet::new();
    for credential in &store.credentials {
        validate_credential_id(&credential.credential_id)
            .map_err(|_| grpc::CREDENTIAL_STORAGE_FAILED.to_string())?;
        validate_label(&credential.label)
            .map_err(|_| grpc::CREDENTIAL_STORAGE_FAILED.to_string())?;
        if !ids.insert(credential.credential_id.clone())
            || !labels.insert(credential.label.clone())
            || credential.created_at_ms == 0
            || credential.created_at_ms > MAX_ECMASCRIPT_DATE_MS
            || credential
                .ca_pem
                .as_ref()
                .is_some_and(|value| !valid_sealed_field(value))
            || credential.client_certificate_pem.is_some() != credential.client_key_pem.is_some()
            || credential
                .client_certificate_pem
                .as_ref()
                .is_some_and(|value| !valid_sealed_field(value))
            || credential
                .client_key_pem
                .as_ref()
                .is_some_and(|value| !valid_sealed_field(value))
            || (credential.ca_pem.is_none() && credential.client_certificate_pem.is_none())
        {
            return Err(grpc::CREDENTIAL_STORAGE_FAILED.into());
        }
    }
    Ok(())
}

fn validate_label(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > MAX_LABEL_BYTES
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        Err(grpc::CREDENTIAL_INVALID.into())
    } else {
        Ok(())
    }
}

fn validate_credential_id(value: &str) -> Result<(), String> {
    validate_opaque_id(value).map_err(|_| grpc::CREDENTIAL_INVALID.to_string())
}

fn random_credential_id() -> Result<String, String> {
    random_hex_128().map_err(|_| grpc::CREDENTIAL_STORAGE_FAILED.to_string())
}

fn random_unique_credential_id(store: &CredentialStore) -> Result<String, String> {
    for _ in 0..4 {
        let value = random_credential_id()?;
        if !store
            .credentials
            .iter()
            .any(|credential| credential.credential_id == value)
        {
            return Ok(value);
        }
    }
    Err(grpc::CREDENTIAL_STORAGE_FAILED.into())
}

fn valid_sealed_field(value: &str) -> bool {
    if value.is_empty() || value.len() > MAX_SEALED_FIELD_BYTES {
        return false;
    }
    B64.decode(value)
        .ok()
        .is_some_and(|bytes| bytes.len() > 1 && bytes[0] == devbox_secrets::BLOB_VERSION)
}

fn now_unix_ms() -> Result<u64, String> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|value| u64::try_from(value.as_millis()).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| grpc::CREDENTIAL_STORAGE_FAILED.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use devbox_secrets::SealError;
    use tempfile::TempDir;

    const CERTIFICATE: &str = "-----BEGIN CERTIFICATE-----\nMAA=\n-----END CERTIFICATE-----\n";
    const PRIVATE_KEY: &str = "-----BEGIN PRIVATE KEY-----\nMAA=\n-----END PRIVATE KEY-----\n";

    fn store() -> CredentialStore {
        CredentialStore {
            schema: STORE_SCHEMA.into(),
            version: STORE_VERSION,
            credentials: vec![PersistedCredential {
                credential_id: "a".repeat(32),
                label: "Local test".into(),
                ca_pem: Some(B64.encode([devbox_secrets::BLOB_VERSION, 7_u8])),
                client_certificate_pem: None,
                client_key_pem: None,
                created_at_ms: 1,
            }],
        }
    }

    #[test]
    fn pem_shape_rejects_prose_unknown_blocks_and_multiple_keys() {
        assert!(validate_certificate_pem(CERTIFICATE).is_ok());
        assert!(validate_private_key_pem(PRIVATE_KEY).is_ok());
        assert!(validate_certificate_pem(&format!("note\n{CERTIFICATE}")).is_err());
        assert!(validate_certificate_pem(PRIVATE_KEY).is_err());
        assert!(validate_private_key_pem(&format!("{PRIVATE_KEY}{PRIVATE_KEY}")).is_err());
    }

    #[test]
    fn store_roundtrips_and_rejects_duplicate_json_keys() {
        let encoded = encode_store(&store()).unwrap();
        let decoded = decode_store(&encoded).unwrap();
        assert_eq!(decoded.credentials.len(), 1);
        let duplicate = br#"{"schema":"devbox.api-playground.grpc-tls-credentials","schema":"devbox.api-playground.grpc-tls-credentials","version":1,"credentials":[]}"#;
        assert_eq!(
            decode_store(duplicate).err().unwrap(),
            grpc::CREDENTIAL_STORAGE_FAILED
        );
    }

    #[test]
    fn store_io_rejects_link_and_preserves_projection_only() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("grpc").join("tls-credentials.json");
        save_store_path(&path, &store()).unwrap();
        let loaded = load_store_path(&path).unwrap();
        let projection = project_credential(&loaded.credentials[0]);
        assert_eq!(projection.label, "Local test");
        assert!(projection.has_custom_ca);
        assert!(!projection.has_client_identity);

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let link = temp.path().join("linked.json");
            symlink(&path, &link).unwrap();
            assert_eq!(
                load_store_path(&link).err().unwrap(),
                grpc::CREDENTIAL_STORAGE_FAILED
            );
            let dangling = temp.path().join("dangling.json");
            symlink(temp.path().join("missing.json"), &dangling).unwrap();
            assert_eq!(
                load_store_path(&dangling).err().unwrap(),
                grpc::CREDENTIAL_STORAGE_FAILED
            );
        }
    }

    #[test]
    fn invalid_store_shape_fails_closed() {
        let mut value = store();
        value.credentials[0].created_at_ms = MAX_ECMASCRIPT_DATE_MS + 1;
        assert_eq!(
            validate_store(&value).unwrap_err(),
            grpc::CREDENTIAL_STORAGE_FAILED
        );
        value.credentials[0].created_at_ms = 1;
        value.credentials[0].client_certificate_pem =
            Some(B64.encode([devbox_secrets::BLOB_VERSION, 8_u8]));
        assert_eq!(
            validate_store(&value).unwrap_err(),
            grpc::CREDENTIAL_STORAGE_FAILED
        );
        value.credentials[0].client_key_pem =
            Some(B64.encode([devbox_secrets::BLOB_VERSION, 9_u8]));
        value.credentials.push(value.credentials[0].clone());
        assert_eq!(
            validate_store(&value).unwrap_err(),
            grpc::CREDENTIAL_STORAGE_FAILED
        );
    }

    struct MockSealer;

    impl devbox_secrets::Sealer for MockSealer {
        fn seal(&self, plaintext: &str) -> Result<Vec<u8>, SealError> {
            Ok(plaintext.bytes().rev().chain([0]).collect())
        }

        fn unseal(&self, ciphertext: &[u8]) -> Result<Zeroizing<String>, SealError> {
            let value = ciphertext.strip_suffix(&[0]).unwrap_or(ciphertext);
            String::from_utf8(value.iter().rev().copied().collect())
                .map(Zeroizing::new)
                .map_err(|_| SealError::InvalidInput)
        }
    }

    #[test]
    fn sealed_pem_roundtrips_without_plaintext_projection() {
        let sealed = seal_pem_with(&MockSealer, PRIVATE_KEY).unwrap();
        assert!(!sealed.contains("PRIVATE KEY"));
        let opened = unseal_pem_with(&MockSealer, &sealed, MAX_CLIENT_KEY_BYTES).unwrap();
        assert_eq!(opened.as_str(), PRIVATE_KEY);
        assert_eq!(
            unseal_pem_with(&MockSealer, &sealed, PRIVATE_KEY.len() - 1).unwrap_err(),
            grpc::CREDENTIAL_STORAGE_FAILED
        );
    }
}
