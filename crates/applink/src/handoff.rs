//! Versioned, one-time handoff storage.
//!
//! Payload bytes never travel through argv. A producer publishes one bounded
//! envelope below `pending/`, while consumers atomically publish an exclusive
//! claim record below `claimed/`. Only the holder of the random claim token can
//! acknowledge, restore, or renew that claim.

use crate::PROTOCOL_VERSION;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

pub const DEFAULT_HANDOFF_TTL_MS: u64 = 10 * 60 * 1_000;
pub const DEFAULT_CLAIM_LEASE_MS: u64 = 60 * 1_000;
pub const MAX_HANDOFF_BYTES: u64 = 10 * 1024 * 1024;

const HANDOFF_STORE_VERSION: u32 = 1;
const MAX_CLAIM_RECORD_BYTES: u64 = MAX_HANDOFF_BYTES + 4 * 1024;
const MAX_PAYLOAD_DEPTH: usize = 32;
const MAX_PAYLOAD_NODES: usize = 100_000;
const MAX_PAYLOAD_STRING_BYTES: usize = 1024 * 1024;
const MAX_KIND_BYTES: usize = 128;
const MAX_APP_ID_BYTES: usize = 64;
const MAX_CREATE_ATTEMPTS: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandoffError {
    InvalidRequest,
    InvalidPayload,
    TooLarge,
    Missing,
    AlreadyClaimed,
    WrongTarget,
    WrongKind,
    Expired,
    LeaseExpired,
    TokenMismatch,
    Corrupt,
    UnsafeStorage,
    Storage,
    RandomUnavailable,
}

impl fmt::Display for HandoffError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidRequest => "handoff request is invalid",
            Self::InvalidPayload => "handoff payload is not privacy-safe",
            Self::TooLarge => "handoff payload exceeds its size limit",
            Self::Missing => "handoff is unavailable",
            Self::AlreadyClaimed => "handoff is already being consumed",
            Self::WrongTarget => "handoff is not addressed to this consumer",
            Self::WrongKind => "handoff kind does not match",
            Self::Expired => "handoff has expired",
            Self::LeaseExpired => "handoff claim lease has expired",
            Self::TokenMismatch => "handoff claim token does not match",
            Self::Corrupt => "handoff state is corrupt",
            Self::UnsafeStorage => "handoff storage is unsafe",
            Self::Storage => "handoff storage operation failed",
            Self::RandomUnavailable => "secure random generation is unavailable",
        })
    }
}

impl std::error::Error for HandoffError {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HandoffEnvelope {
    pub protocol_version: u32,
    pub id: String,
    pub kind: String,
    pub source_app: String,
    pub target_app: Option<String>,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffDescriptor {
    pub id: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HandoffClaim {
    pub envelope: HandoffEnvelope,
    pub claim_token: String,
    pub lease_until_ms: u64,
}

#[derive(Debug, Clone)]
pub struct CreateHandoff {
    pub kind: String,
    pub source_app: String,
    pub target_app: Option<String>,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct HandoffStore {
    root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ClaimRecord {
    schema_version: u32,
    consumer_app: String,
    claim_token: String,
    claimed_at_ms: u64,
    lease_until_ms: u64,
    envelope: HandoffEnvelope,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LeaseRecord {
    schema_version: u32,
    id: String,
    consumer_app: String,
    claim_token: String,
    lease_until_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PublishError {
    Exists,
    Storage,
}

/// Resolve the versioned store below the shared devbox data root.
pub fn handoff_root_in(common_root: &Path) -> PathBuf {
    common_root.join("handoff/v1")
}

impl HandoffStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn create(
        &self,
        request: CreateHandoff,
        now_ms: u64,
    ) -> Result<HandoffDescriptor, HandoffError> {
        self.create_with_ttl(request, now_ms, DEFAULT_HANDOFF_TTL_MS)
    }

    pub fn create_with_ttl(
        &self,
        request: CreateHandoff,
        now_ms: u64,
        ttl_ms: u64,
    ) -> Result<HandoffDescriptor, HandoffError> {
        if now_ms == 0 || ttl_ms == 0 || ttl_ms > DEFAULT_HANDOFF_TTL_MS {
            return Err(HandoffError::InvalidRequest);
        }
        validate_kind(&request.kind)?;
        validate_app_id(&request.source_app)?;
        if let Some(target) = &request.target_app {
            validate_app_id(target)?;
        }
        validate_payload(&request.payload)?;
        let expires_at_ms = now_ms
            .checked_add(ttl_ms)
            .ok_or(HandoffError::InvalidRequest)?;
        let root = self.prepare_layout()?;

        for _ in 0..MAX_CREATE_ATTEMPTS {
            let id = random_hex_128()?;
            if managed_file_exists(&pending_path(&root, &id))?
                || managed_file_exists(&claimed_path(&root, &id))?
                || managed_file_exists(&lease_path(&root, &id))?
            {
                continue;
            }
            let envelope = HandoffEnvelope {
                protocol_version: PROTOCOL_VERSION,
                id: id.clone(),
                kind: request.kind.clone(),
                source_app: request.source_app.clone(),
                target_app: request.target_app.clone(),
                created_at_ms: now_ms,
                expires_at_ms,
                payload: request.payload.clone(),
            };
            validate_envelope(&envelope)?;
            let bytes = encode_bounded(&envelope, MAX_HANDOFF_BYTES)?;
            match publish_new(&pending_path(&root, &id), &bytes) {
                Ok(()) => {
                    return Ok(HandoffDescriptor {
                        id,
                        kind: request.kind,
                    })
                }
                Err(PublishError::Exists) => continue,
                Err(PublishError::Storage) => return Err(HandoffError::Storage),
            }
        }
        Err(HandoffError::RandomUnavailable)
    }

    pub fn claim(
        &self,
        id: &str,
        expected_kind: &str,
        consumer_app: &str,
        now_ms: u64,
    ) -> Result<HandoffClaim, HandoffError> {
        validate_id(id)?;
        validate_kind(expected_kind)?;
        validate_app_id(consumer_app)?;
        if now_ms == 0 {
            return Err(HandoffError::InvalidRequest);
        }
        let root = self.prepare_layout()?;
        self.reconcile_existing_claim(&root, id, now_ms)?;

        let pending = pending_path(&root, id);
        let envelope = match read_json::<HandoffEnvelope>(&pending, MAX_HANDOFF_BYTES) {
            Ok(envelope) => envelope,
            Err(HandoffError::Corrupt | HandoffError::TooLarge) => {
                remove_managed_file(&pending)?;
                return Err(HandoffError::Corrupt);
            }
            Err(error) => return Err(error),
        };
        if let Err(error) = validate_envelope(&envelope) {
            if matches!(error, HandoffError::Corrupt | HandoffError::InvalidPayload) {
                remove_managed_file(&pending)?;
                return Err(HandoffError::Corrupt);
            }
            return Err(error);
        }
        validate_consumer(&envelope, id, expected_kind, consumer_app)?;
        if now_ms >= envelope.expires_at_ms {
            remove_managed_file(&pending)?;
            return Err(HandoffError::Expired);
        }

        let lease_until_ms = now_ms
            .saturating_add(DEFAULT_CLAIM_LEASE_MS)
            .min(envelope.expires_at_ms);
        if lease_until_ms <= now_ms {
            remove_managed_file(&pending)?;
            return Err(HandoffError::Expired);
        }
        let claim_token = random_hex_128()?;
        let record = ClaimRecord {
            schema_version: HANDOFF_STORE_VERSION,
            consumer_app: consumer_app.to_string(),
            claim_token: claim_token.clone(),
            claimed_at_ms: now_ms,
            lease_until_ms,
            envelope: envelope.clone(),
        };
        validate_claim_record(&record)?;
        let bytes = encode_bounded(&record, MAX_CLAIM_RECORD_BYTES)?;
        let claimed = claimed_path(&root, id);
        match publish_new(&claimed, &bytes) {
            Ok(()) => {}
            Err(PublishError::Exists) => return Err(HandoffError::AlreadyClaimed),
            Err(PublishError::Storage) => return Err(HandoffError::Storage),
        }
        if remove_managed_file(&pending).is_err() {
            let _ = remove_claim_if_token(&claimed, &claim_token);
            return Err(HandoffError::Storage);
        }
        remove_lease_if_token(&lease_path(&root, id), &claim_token)?;

        Ok(HandoffClaim {
            envelope,
            claim_token,
            lease_until_ms,
        })
    }

    pub fn ack(
        &self,
        claim: &HandoffClaim,
        consumer_app: &str,
        now_ms: u64,
    ) -> Result<(), HandoffError> {
        let root = self.prepare_layout()?;
        let claimed = claimed_path(&root, &claim.envelope.id);
        let record = self.read_and_validate_claim(&root, claim, consumer_app, now_ms)?;
        remove_claim_if_token(&claimed, &record.claim_token)?;
        let _ = remove_lease_if_token(&lease_path(&root, &record.envelope.id), &record.claim_token);
        Ok(())
    }

    pub fn restore(
        &self,
        claim: &HandoffClaim,
        consumer_app: &str,
        now_ms: u64,
    ) -> Result<(), HandoffError> {
        let root = self.prepare_layout()?;
        let record = self.read_and_validate_claim(&root, claim, consumer_app, now_ms)?;
        let pending = pending_path(&root, &record.envelope.id);
        let bytes = encode_bounded(&record.envelope, MAX_HANDOFF_BYTES)?;
        let created_pending = match publish_new(&pending, &bytes) {
            Ok(()) => true,
            Err(PublishError::Exists) => {
                let existing = read_json::<HandoffEnvelope>(&pending, MAX_HANDOFF_BYTES)?;
                if existing != record.envelope {
                    return Err(HandoffError::Storage);
                }
                false
            }
            Err(PublishError::Storage) => return Err(HandoffError::Storage),
        };
        let claimed = claimed_path(&root, &record.envelope.id);
        if let Err(error) = remove_claim_if_token(&claimed, &record.claim_token) {
            if created_pending {
                let _ = remove_envelope_if_equal(&pending, &record.envelope);
            }
            return Err(error);
        }
        let _ = remove_lease_if_token(&lease_path(&root, &record.envelope.id), &record.claim_token);
        Ok(())
    }

    pub fn renew(
        &self,
        claim: &HandoffClaim,
        consumer_app: &str,
        now_ms: u64,
        lease_ms: u64,
    ) -> Result<HandoffClaim, HandoffError> {
        if lease_ms == 0 || lease_ms > DEFAULT_CLAIM_LEASE_MS {
            return Err(HandoffError::InvalidRequest);
        }
        let root = self.prepare_layout()?;
        let record = self.read_and_validate_claim(&root, claim, consumer_app, now_ms)?;
        let current_lease_until_ms = effective_lease_until(&root, &record)?;
        let lease_until_ms = current_lease_until_ms.max(
            now_ms
                .saturating_add(lease_ms)
                .min(record.envelope.expires_at_ms),
        );
        if lease_until_ms <= now_ms {
            return Err(HandoffError::Expired);
        }
        let lease = LeaseRecord {
            schema_version: HANDOFF_STORE_VERSION,
            id: record.envelope.id.clone(),
            consumer_app: record.consumer_app.clone(),
            claim_token: record.claim_token.clone(),
            lease_until_ms,
        };
        let bytes = encode_bounded(&lease, 4 * 1024)?;
        let path = lease_path(&root, &record.envelope.id);
        reject_link_slot(&path)?;
        devbox_filesystem::atomic_write(&path, &bytes).map_err(|_| HandoffError::Storage)?;

        // A concurrent ack may have committed consumption after the first
        // validation. A renewal sidecar can never resurrect the payload; if
        // the primary claim is gone or changed, remove only this token's lease.
        match read_json::<ClaimRecord>(
            &claimed_path(&root, &record.envelope.id),
            MAX_CLAIM_RECORD_BYTES,
        ) {
            Ok(current) if current.claim_token == record.claim_token => {}
            _ => {
                let _ = remove_lease_if_token(&path, &record.claim_token);
                return Err(HandoffError::Missing);
            }
        }
        Ok(HandoffClaim {
            envelope: record.envelope,
            claim_token: record.claim_token,
            lease_until_ms,
        })
    }

    fn read_and_validate_claim(
        &self,
        root: &Path,
        claim: &HandoffClaim,
        consumer_app: &str,
        now_ms: u64,
    ) -> Result<ClaimRecord, HandoffError> {
        validate_id(&claim.envelope.id)?;
        validate_kind(&claim.envelope.kind)?;
        validate_app_id(consumer_app)?;
        validate_id(&claim.claim_token)?;
        if now_ms == 0 {
            return Err(HandoffError::InvalidRequest);
        }
        let path = claimed_path(root, &claim.envelope.id);
        let record = read_json::<ClaimRecord>(&path, MAX_CLAIM_RECORD_BYTES)?;
        validate_claim_record(&record).map_err(|_| HandoffError::Corrupt)?;
        if record.envelope.id != claim.envelope.id
            || record.envelope.kind != claim.envelope.kind
            || record.claim_token != claim.claim_token
        {
            return Err(HandoffError::TokenMismatch);
        }
        // The claim carries the exact envelope snapshot that the consumer
        // previewed.  A managed claim file changed in place must not be
        // acknowledged as if it were the same immutable payload.
        if record.envelope != claim.envelope {
            return Err(HandoffError::Corrupt);
        }
        validate_consumer(
            &record.envelope,
            &claim.envelope.id,
            &claim.envelope.kind,
            consumer_app,
        )?;
        if record.consumer_app != consumer_app {
            return Err(HandoffError::TokenMismatch);
        }
        if now_ms >= record.envelope.expires_at_ms {
            remove_claim_if_token(&path, &record.claim_token)?;
            remove_lease_if_token(&lease_path(root, &record.envelope.id), &record.claim_token)?;
            return Err(HandoffError::Expired);
        }
        let effective_lease = effective_lease_until(root, &record)?;
        if now_ms >= effective_lease {
            return Err(HandoffError::LeaseExpired);
        }
        Ok(record)
    }

    fn reconcile_existing_claim(
        &self,
        root: &Path,
        id: &str,
        now_ms: u64,
    ) -> Result<(), HandoffError> {
        let path = claimed_path(root, id);
        let record = match read_optional_json::<ClaimRecord>(&path, MAX_CLAIM_RECORD_BYTES) {
            Ok(Some(record)) => record,
            Ok(None) => {
                remove_orphan_lease(&lease_path(root, id))?;
                return Ok(());
            }
            Err(HandoffError::Corrupt | HandoffError::TooLarge) => {
                remove_managed_file(&path)?;
                remove_orphan_lease(&lease_path(root, id))?;
                return Err(HandoffError::Corrupt);
            }
            Err(error) => return Err(error),
        };
        if validate_claim_record(&record).is_err() || record.envelope.id != id {
            remove_managed_file(&path)?;
            remove_orphan_lease(&lease_path(root, id))?;
            return Err(HandoffError::Corrupt);
        }
        if now_ms >= record.envelope.expires_at_ms {
            remove_claim_if_token(&path, &record.claim_token)?;
            remove_lease_if_token(&lease_path(root, id), &record.claim_token)?;
            let pending = pending_path(root, id);
            match read_optional_json::<HandoffEnvelope>(&pending, MAX_HANDOFF_BYTES) {
                Ok(Some(envelope)) if envelope.id == id && now_ms >= envelope.expires_at_ms => {
                    remove_managed_file(&pending)?;
                }
                Err(HandoffError::Corrupt | HandoffError::TooLarge) => {
                    remove_managed_file(&pending)?;
                }
                Ok(_) | Err(HandoffError::Missing) => {}
                Err(error) => return Err(error),
            }
            return Err(HandoffError::Expired);
        }
        if now_ms < effective_lease_until(root, &record)? {
            return Err(HandoffError::AlreadyClaimed);
        }

        let pending = pending_path(root, id);
        let bytes = encode_bounded(&record.envelope, MAX_HANDOFF_BYTES)?;
        match publish_new(&pending, &bytes) {
            Ok(()) => {}
            Err(PublishError::Exists) => {
                let existing = read_json::<HandoffEnvelope>(&pending, MAX_HANDOFF_BYTES)?;
                if existing != record.envelope {
                    return Err(HandoffError::Storage);
                }
            }
            Err(PublishError::Storage) => return Err(HandoffError::Storage),
        }
        remove_claim_if_token(&path, &record.claim_token)?;
        remove_lease_if_token(&lease_path(root, id), &record.claim_token)
    }

    fn prepare_layout(&self) -> Result<PathBuf, HandoffError> {
        if !self.root.is_absolute() {
            return Err(HandoffError::UnsafeStorage);
        }
        ensure_directory_tree(&self.root)?;
        let root = canonicalize_path(&self.root).map_err(|_| HandoffError::UnsafeStorage)?;
        if dangerous_root(&root) {
            return Err(HandoffError::UnsafeStorage);
        }
        ensure_directory_component(&root, "pending")?;
        ensure_directory_component(&root, "claimed")?;
        Ok(root)
    }
}

fn validate_envelope(envelope: &HandoffEnvelope) -> Result<(), HandoffError> {
    if envelope.protocol_version != PROTOCOL_VERSION
        || envelope.created_at_ms == 0
        || envelope.expires_at_ms <= envelope.created_at_ms
        || envelope.expires_at_ms - envelope.created_at_ms > DEFAULT_HANDOFF_TTL_MS
    {
        return Err(HandoffError::Corrupt);
    }
    validate_id(&envelope.id).map_err(|_| HandoffError::Corrupt)?;
    validate_kind(&envelope.kind).map_err(|_| HandoffError::Corrupt)?;
    validate_app_id(&envelope.source_app).map_err(|_| HandoffError::Corrupt)?;
    if let Some(target) = &envelope.target_app {
        validate_app_id(target).map_err(|_| HandoffError::Corrupt)?;
    }
    validate_payload(&envelope.payload)?;
    let _ = encode_bounded(envelope, MAX_HANDOFF_BYTES)?;
    Ok(())
}

fn validate_claim_record(record: &ClaimRecord) -> Result<(), HandoffError> {
    if record.schema_version != HANDOFF_STORE_VERSION
        || record.claimed_at_ms == 0
        || record.lease_until_ms <= record.claimed_at_ms
        || record.lease_until_ms > record.envelope.expires_at_ms
    {
        return Err(HandoffError::Corrupt);
    }
    validate_app_id(&record.consumer_app).map_err(|_| HandoffError::Corrupt)?;
    validate_id(&record.claim_token).map_err(|_| HandoffError::Corrupt)?;
    validate_envelope(&record.envelope).map_err(|_| HandoffError::Corrupt)
}

fn validate_consumer(
    envelope: &HandoffEnvelope,
    id: &str,
    expected_kind: &str,
    consumer_app: &str,
) -> Result<(), HandoffError> {
    if envelope.id != id {
        return Err(HandoffError::Corrupt);
    }
    if envelope.kind != expected_kind {
        return Err(HandoffError::WrongKind);
    }
    if envelope
        .target_app
        .as_ref()
        .is_some_and(|target| target != consumer_app)
    {
        return Err(HandoffError::WrongTarget);
    }
    Ok(())
}

fn validate_id(value: &str) -> Result<(), HandoffError> {
    if value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(HandoffError::InvalidRequest)
    }
}

fn validate_kind(value: &str) -> Result<(), HandoffError> {
    if value.is_empty() || value.len() > MAX_KIND_BYTES {
        return Err(HandoffError::InvalidRequest);
    }
    let Some((name, version)) = value.rsplit_once("/v") else {
        return Err(HandoffError::InvalidRequest);
    };
    if !valid_slug(name, MAX_KIND_BYTES)
        || version.is_empty()
        || version.starts_with('0')
        || !version.bytes().all(|byte| byte.is_ascii_digit())
        || version.parse::<u32>().is_err()
    {
        return Err(HandoffError::InvalidRequest);
    }
    Ok(())
}

fn validate_app_id(value: &str) -> Result<(), HandoffError> {
    if valid_slug(value, MAX_APP_ID_BYTES) {
        Ok(())
    } else {
        Err(HandoffError::InvalidRequest)
    }
}

fn valid_slug(value: &str, max_bytes: usize) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= max_bytes
        && bytes.first().is_some_and(u8::is_ascii_lowercase)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && bytes
            .iter()
            .copied()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn validate_payload(payload: &Value) -> Result<(), HandoffError> {
    let mut nodes = 0_usize;
    validate_payload_value(payload, None, 0, &mut nodes)
}

fn validate_payload_value(
    value: &Value,
    field: Option<&str>,
    depth: usize,
    nodes: &mut usize,
) -> Result<(), HandoffError> {
    if depth > MAX_PAYLOAD_DEPTH || *nodes >= MAX_PAYLOAD_NODES {
        return Err(HandoffError::InvalidPayload);
    }
    *nodes += 1;
    match value {
        Value::Object(object) => {
            validate_named_sensitive_value(object)?;
            for (key, child) in object {
                if key.is_empty() || key.len() > 256 {
                    return Err(HandoffError::InvalidPayload);
                }
                if sensitive_field(key) && !is_safe_sensitive_value(child) {
                    return Err(HandoffError::InvalidPayload);
                }
                if path_field(key) {
                    let Some(path) = child.as_str() else {
                        return Err(HandoffError::InvalidPayload);
                    };
                    validate_safe_payload_path(path)?;
                }
                validate_payload_value(child, Some(key), depth + 1, nodes)?;
            }
        }
        Value::Array(values) => {
            for child in values {
                validate_payload_value(child, field, depth + 1, nodes)?;
            }
        }
        Value::String(text) => {
            if text.len() > MAX_PAYLOAD_STRING_BYTES
                || looks_like_raw_credential(text)
                || (field.is_some_and(sensitive_field) && !is_exact_secret_reference(text))
            {
                return Err(HandoffError::InvalidPayload);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
    Ok(())
}

fn validate_named_sensitive_value(
    object: &serde_json::Map<String, Value>,
) -> Result<(), HandoffError> {
    let label = object.iter().find_map(|(key, value)| {
        matches!(
            normalize_field_name(key).as_str(),
            "name" | "key" | "headername"
        )
        .then(|| value.as_str())
        .flatten()
    });
    if !label.is_some_and(sensitive_field) {
        return Ok(());
    }
    let candidate = object.iter().find_map(|(key, value)| {
        matches!(normalize_field_name(key).as_str(), "value" | "content").then_some(value)
    });
    if candidate.is_none_or(is_safe_sensitive_value) {
        Ok(())
    } else {
        Err(HandoffError::InvalidPayload)
    }
}

fn is_safe_sensitive_value(value: &Value) -> bool {
    value.is_null()
        || value
            .as_str()
            .is_some_and(|text| text.is_empty() || is_exact_secret_reference(text))
}

fn sensitive_field(value: &str) -> bool {
    let compact = normalize_field_name(value);
    matches!(
        compact.as_str(),
        "authorization"
            | "proxyauthorization"
            | "cookie"
            | "setcookie"
            | "password"
            | "passwd"
            | "secret"
            | "secrets"
            | "secretkey"
            | "token"
            | "accesstoken"
            | "refreshtoken"
            | "sessiontoken"
            | "idtoken"
            | "jwt"
            | "credential"
            | "credentials"
            | "apikey"
            | "xapikey"
            | "xauth"
            | "accesskey"
            | "clientsecret"
            | "clientid"
            | "privatekey"
            | "signingkey"
    ) || [
        "authorization",
        "cookie",
        "password",
        "passwd",
        "secret",
        "token",
        "credential",
        "apikey",
        "privatekey",
    ]
    .iter()
    .any(|needle| compact.contains(needle))
}

fn path_field(value: &str) -> bool {
    matches!(
        normalize_field_name(value).as_str(),
        "path" | "filepath" | "sourcepath" | "binarypath" | "temppath"
    )
}

fn normalize_field_name(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn validate_safe_payload_path(value: &str) -> Result<(), HandoffError> {
    if value.is_empty() || value.len() > 32 * 1024 || value.contains('\0') {
        return Err(HandoffError::InvalidPayload);
    }
    let path = Path::new(value);
    if !path.is_absolute() || is_filesystem_root(path) {
        return Err(HandoffError::InvalidPayload);
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(Path::new(std::path::MAIN_SEPARATOR_STR)),
            Component::Normal(value) => current.push(value),
            Component::CurDir | Component::ParentDir => return Err(HandoffError::InvalidPayload),
        }
        if !current.is_absolute() {
            continue;
        }
        let metadata = fs::symlink_metadata(&current).map_err(|_| HandoffError::InvalidPayload)?;
        if is_link_or_reparse(&metadata) {
            return Err(HandoffError::InvalidPayload);
        }
    }
    let metadata = fs::symlink_metadata(path).map_err(|_| HandoffError::InvalidPayload)?;
    if !metadata.is_file() && !metadata.is_dir() {
        return Err(HandoffError::InvalidPayload);
    }
    canonicalize_path(path)
        .map(|_| ())
        .map_err(|_| HandoffError::InvalidPayload)
}

fn is_exact_secret_reference(value: &str) -> bool {
    let Some(name) = value
        .strip_prefix("${")
        .and_then(|candidate| candidate.strip_suffix('}'))
    else {
        return false;
    };
    !name.is_empty()
        && name.len() <= 128
        && name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '-')
        })
}

fn looks_like_raw_credential(value: &str) -> bool {
    let trimmed = value.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("bearer ")
        || lower.starts_with("basic ")
        || lower.starts_with("sk-")
        || lower.contains("-----begin private key-----")
        || lower.contains("-----begin rsa private key-----")
        || lower.contains("-----begin openssh private key-----")
    {
        return true;
    }
    if has_unsafe_sensitive_assignment(value) {
        return true;
    }
    if trimmed.split_once("://").is_some_and(|(_, rest)| {
        rest.split(['/', '?', '#'])
            .next()
            .is_some_and(|authority| authority.contains('@'))
    }) {
        return true;
    }
    let mut jwt_parts = trimmed.split('.');
    jwt_parts.next().is_some_and(|part| part.len() >= 8)
        && jwt_parts.next().is_some_and(|part| part.len() >= 8)
        && jwt_parts.next().is_some_and(|part| part.len() >= 8)
        && jwt_parts.next().is_none()
}

fn has_unsafe_sensitive_assignment(value: &str) -> bool {
    value
        .split(['&', ',', ';', '?', '\n', '\r', '\t'])
        .any(|segment| {
            segment.char_indices().any(|(operator, character)| {
                if !matches!(character, '=' | ':') {
                    return false;
                }
                let before = &segment[..operator];
                let key_end = before.trim_end().len();
                let key_start = before[..key_end]
                    .char_indices()
                    .rev()
                    .find(|(_, character)| {
                        !character.is_ascii_alphanumeric() && !matches!(character, '_' | '-' | '.')
                    })
                    .map_or(0, |(index, character)| index + character.len_utf8());
                let key = &before[key_start..key_end];
                let raw_value = segment[operator + character.len_utf8()..].trim_start();
                sensitive_field(key)
                    && !raw_value.is_empty()
                    && !is_safe_assignment_reference(raw_value)
            })
        })
}

/// Accept an exact reference inside the small amount of JSON punctuation that
/// the conservative assignment scanner sees (`"${TOKEN}"}`).  Braces are not
/// trimmed from the reference itself: `${TOKEN}` must remain an exact value,
/// while arbitrary text around it remains rejected.
fn is_safe_assignment_reference(raw_value: &str) -> bool {
    let value = raw_value.trim();
    let Some(quote) = value
        .chars()
        .next()
        .filter(|character| *character == '"' || *character == '\'')
    else {
        return is_exact_secret_reference(value.trim_end_matches(','));
    };
    let quote_width = quote.len_utf8();
    let rest = &value[quote_width..];
    let Some(end) = rest.find(quote) else {
        return false;
    };
    let candidate = &rest[..end];
    let trailing = rest[end + quote_width..].trim();
    trailing
        .chars()
        .all(|character| matches!(character, '}' | ']' | ','))
        && is_exact_secret_reference(candidate)
}

fn encode_bounded<T: Serialize>(value: &T, max: u64) -> Result<Vec<u8>, HandoffError> {
    let bytes = serde_json::to_vec(value).map_err(|_| HandoffError::Corrupt)?;
    if bytes.len() as u64 > max {
        Err(HandoffError::TooLarge)
    } else {
        Ok(bytes)
    }
}

fn random_hex_128() -> Result<String, HandoffError> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| HandoffError::RandomUnavailable)?;
    let mut output = String::with_capacity(32);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").map_err(|_| HandoffError::RandomUnavailable)?;
    }
    Ok(output)
}

fn pending_path(root: &Path, id: &str) -> PathBuf {
    root.join("pending").join(format!("{id}.json"))
}

fn claimed_path(root: &Path, id: &str) -> PathBuf {
    root.join("claimed").join(format!("{id}.json"))
}

fn lease_path(root: &Path, id: &str) -> PathBuf {
    root.join("claimed").join(format!("{id}.lease.json"))
}

fn publish_new(path: &Path, contents: &[u8]) -> Result<(), PublishError> {
    let parent = path.parent().ok_or(PublishError::Storage)?;
    let name = path
        .file_name()
        .ok_or(PublishError::Storage)?
        .to_string_lossy();
    for _ in 0..MAX_CREATE_ATTEMPTS {
        let nonce = random_hex_128().map_err(|_| PublishError::Storage)?;
        let temporary = parent.join(format!(".{name}.{nonce}.tmp"));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = match options.open(&temporary) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(PublishError::Storage),
        };
        let written = file
            .write_all(contents)
            .and_then(|()| file.flush())
            .and_then(|()| file.sync_all());
        drop(file);
        if written.is_err() {
            let _ = fs::remove_file(&temporary);
            return Err(PublishError::Storage);
        }
        let linked = fs::hard_link(&temporary, path);
        return match linked {
            Ok(()) => {
                // A published descriptor is not considered durable until the
                // containing directory has also been flushed.  Returning
                // success after a failed directory sync would let a producer
                // report a handoff that can disappear on crash/restart.
                let temporary_removed = fs::remove_file(&temporary).is_ok();
                if !temporary_removed {
                    let _ = fs::remove_file(path);
                    let _ = sync_parent(path);
                    return Err(PublishError::Storage);
                }
                if sync_parent(path).is_err() {
                    // The target was created by this invocation and the
                    // random managed name cannot alias a pre-existing
                    // descriptor.  Remove it before reporting failure so a
                    // retry cannot observe a false-success orphan.
                    let _ = fs::remove_file(path);
                    let _ = sync_parent(path);
                    return Err(PublishError::Storage);
                }
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let _ = fs::remove_file(&temporary);
                Err(PublishError::Exists)
            }
            Err(_) => {
                let _ = fs::remove_file(&temporary);
                Err(PublishError::Storage)
            }
        };
    }
    Err(PublishError::Storage)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path, max: u64) -> Result<T, HandoffError> {
    let bytes = read_bounded(path, max)?;
    serde_json::from_slice(&bytes).map_err(|_| HandoffError::Corrupt)
}

fn read_optional_json<T: for<'de> Deserialize<'de>>(
    path: &Path,
    max: u64,
) -> Result<Option<T>, HandoffError> {
    match fs::symlink_metadata(path) {
        Ok(_) => read_json(path, max).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(HandoffError::Storage),
    }
}

fn read_bounded(path: &Path, max: u64) -> Result<Vec<u8>, HandoffError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            HandoffError::Missing
        } else {
            HandoffError::Storage
        }
    })?;
    if is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Err(HandoffError::UnsafeStorage);
    }
    if metadata.len() > max {
        return Err(HandoffError::TooLarge);
    }
    let file = File::open(path).map_err(|_| HandoffError::Storage)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| HandoffError::Storage)?;
    if bytes.len() as u64 > max {
        return Err(HandoffError::TooLarge);
    }
    Ok(bytes)
}

fn effective_lease_until(root: &Path, record: &ClaimRecord) -> Result<u64, HandoffError> {
    let path = lease_path(root, &record.envelope.id);
    let lease = match read_optional_json::<LeaseRecord>(&path, 4 * 1024) {
        Ok(Some(lease)) => lease,
        Ok(None) => return Ok(record.lease_until_ms),
        Err(HandoffError::Corrupt | HandoffError::TooLarge) => {
            remove_orphan_lease(&path)?;
            return Ok(record.lease_until_ms);
        }
        Err(error) => return Err(error),
    };
    if lease.schema_version != HANDOFF_STORE_VERSION
        || lease.id != record.envelope.id
        || lease.consumer_app != record.consumer_app
        || lease.claim_token != record.claim_token
        || lease.lease_until_ms <= record.claimed_at_ms
        || lease.lease_until_ms > record.envelope.expires_at_ms
    {
        remove_orphan_lease(&path)?;
        return Ok(record.lease_until_ms);
    }
    Ok(record.lease_until_ms.max(lease.lease_until_ms))
}

fn remove_claim_if_token(path: &Path, token: &str) -> Result<(), HandoffError> {
    let record = read_json::<ClaimRecord>(path, MAX_CLAIM_RECORD_BYTES)?;
    if record.claim_token != token {
        return Err(HandoffError::TokenMismatch);
    }
    remove_managed_file(path)
}

fn remove_envelope_if_equal(path: &Path, expected: &HandoffEnvelope) -> Result<(), HandoffError> {
    let envelope = read_json::<HandoffEnvelope>(path, MAX_HANDOFF_BYTES)?;
    if &envelope != expected {
        return Err(HandoffError::Storage);
    }
    remove_managed_file(path)
}

fn remove_lease_if_token(path: &Path, token: &str) -> Result<(), HandoffError> {
    match read_optional_json::<LeaseRecord>(path, 4 * 1024)? {
        Some(lease) if lease.claim_token == token => remove_managed_file(path),
        Some(_) | None => Ok(()),
    }
}

fn remove_orphan_lease(path: &Path) -> Result<(), HandoffError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if is_link_or_reparse(&metadata) || !metadata.is_file() => {
            Err(HandoffError::UnsafeStorage)
        }
        Ok(_) => fs::remove_file(path).map_err(|_| HandoffError::Storage),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(HandoffError::Storage),
    }
}

fn remove_managed_file(path: &Path) -> Result<(), HandoffError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            HandoffError::Missing
        } else {
            HandoffError::Storage
        }
    })?;
    if is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Err(HandoffError::UnsafeStorage);
    }
    fs::remove_file(path).map_err(|_| HandoffError::Storage)
}

fn managed_file_exists(path: &Path) -> Result<bool, HandoffError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if is_link_or_reparse(&metadata) || !metadata.is_file() => {
            Err(HandoffError::UnsafeStorage)
        }
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(HandoffError::Storage),
    }
}

fn reject_link_slot(path: &Path) -> Result<(), HandoffError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if is_link_or_reparse(&metadata) || !metadata.is_file() => {
            Err(HandoffError::UnsafeStorage)
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(HandoffError::Storage),
    }
}

fn ensure_directory_tree(path: &Path) -> Result<(), HandoffError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(Path::new(std::path::MAIN_SEPARATOR_STR)),
            Component::Normal(value) => current.push(value),
            Component::CurDir | Component::ParentDir => return Err(HandoffError::UnsafeStorage),
        }
        if !current.is_absolute() {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if is_link_or_reparse(&metadata) || !metadata.is_dir() => {
                return Err(HandoffError::UnsafeStorage)
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|_| HandoffError::Storage)?;
                let metadata = fs::symlink_metadata(&current).map_err(|_| HandoffError::Storage)?;
                if is_link_or_reparse(&metadata) || !metadata.is_dir() {
                    return Err(HandoffError::UnsafeStorage);
                }
            }
            Err(_) => return Err(HandoffError::Storage),
        }
    }
    Ok(())
}

fn ensure_directory_component(parent: &Path, name: &str) -> Result<PathBuf, HandoffError> {
    let path = parent.join(name);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if is_link_or_reparse(&metadata) || !metadata.is_dir() => {
            Err(HandoffError::UnsafeStorage)
        }
        Ok(_) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(&path).map_err(|_| HandoffError::Storage)?;
            let metadata = fs::symlink_metadata(&path).map_err(|_| HandoffError::Storage)?;
            if is_link_or_reparse(&metadata) || !metadata.is_dir() {
                Err(HandoffError::UnsafeStorage)
            } else {
                Ok(path)
            }
        }
        Err(_) => Err(HandoffError::Storage),
    }
}

fn canonicalize_path(path: &Path) -> Result<PathBuf, std::io::Error> {
    path.canonicalize().map(normalize_canonical_path)
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> Result<(), std::io::Error> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("handoff path has no parent"))?;
    File::open(parent).and_then(|directory| directory.sync_all())
}

#[cfg(not(unix))]
fn sync_parent(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

#[cfg(windows)]
fn normalize_canonical_path(path: PathBuf) -> PathBuf {
    let value = path.to_string_lossy();
    if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = value.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path
    }
}

#[cfg(not(windows))]
fn normalize_canonical_path(path: PathBuf) -> PathBuf {
    path
}

fn dangerous_root(path: &Path) -> bool {
    if is_filesystem_root(path) {
        return true;
    }
    if std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .and_then(|home| canonicalize_path(&PathBuf::from(home)).ok())
        .is_some_and(|home| same_path_identity(path, &home))
    {
        return true;
    }
    std::env::current_dir()
        .and_then(|cwd| canonicalize_path(&cwd))
        .is_ok_and(|cwd| same_path_identity(path, &cwd))
}

fn is_filesystem_root(path: &Path) -> bool {
    let mut saw_root = false;
    for component in path.components() {
        match component {
            Component::Prefix(_) => {}
            Component::RootDir => saw_root = true,
            Component::CurDir | Component::ParentDir | Component::Normal(_) => return false,
        }
    }
    saw_root
}

fn same_path_identity(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        normalize_windows_identity(left) == normalize_windows_identity(right)
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

#[cfg(windows)]
fn normalize_windows_identity(path: &Path) -> String {
    let mut value = path.to_string_lossy().replace('/', "\\");
    if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        value = format!(r"\\{rest}");
    } else if let Some(rest) = value.strip_prefix(r"\\?\") {
        value = rest.to_string();
    }
    while value.len() > 3 && value.ends_with('\\') {
        value.pop();
    }
    value.to_ascii_lowercase()
}

#[cfg(windows)]
fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};

    static NEXT_ROOT: AtomicUsize = AtomicUsize::new(0);

    struct TestRoot {
        path: PathBuf,
    }

    impl TestRoot {
        fn new(label: &str) -> Self {
            let sequence = NEXT_ROOT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "devbox-applink-handoff-{label}-{}-{sequence}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            Self { path }
        }

        fn store(&self) -> HandoffStore {
            HandoffStore::new(&self.path)
        }

        fn pending(&self, id: &str) -> PathBuf {
            self.path.join("pending").join(format!("{id}.json"))
        }

        fn claimed(&self, id: &str) -> PathBuf {
            self.path.join("claimed").join(format!("{id}.json"))
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn request(target: Option<&str>) -> CreateHandoff {
        CreateHandoff {
            kind: "api-request/v1".into(),
            source_app: "webhook-lab".into(),
            target_app: target.map(str::to_string),
            payload: json!({
                "method": "POST",
                "url": "https://example.test/hooks",
                "headers": [{"name": "Authorization", "value": "${API_TOKEN}"}],
                "body": "{\"ok\":true}"
            }),
        }
    }

    #[test]
    fn create_claim_and_ack_is_one_time_and_path_free() {
        let root = TestRoot::new("roundtrip");
        let store = root.store();
        let descriptor = store
            .create(request(Some("api-playground")), 1_000)
            .unwrap();

        assert_eq!(descriptor.kind, "api-request/v1");
        assert_eq!(descriptor.id.len(), 32);
        assert_eq!(
            crate::OpenTarget::from(descriptor.clone()),
            crate::OpenTarget::Handoff {
                kind: descriptor.kind.clone(),
                id: descriptor.id.clone(),
            }
        );
        assert!(root.pending(&descriptor.id).is_file());
        let claim = store
            .claim(&descriptor.id, "api-request/v1", "api-playground", 2_000)
            .unwrap();
        assert_eq!(claim.envelope.payload["method"], "POST");
        assert!(!root.pending(&descriptor.id).exists());
        assert!(root.claimed(&descriptor.id).is_file());

        store.ack(&claim, "api-playground", 3_000).unwrap();
        assert!(!root.claimed(&descriptor.id).exists());
        assert_eq!(
            store.ack(&claim, "api-playground", 4_000),
            Err(HandoffError::Missing)
        );
        assert_eq!(
            store.claim(&descriptor.id, "api-request/v1", "api-playground", 4_000,),
            Err(HandoffError::Missing)
        );
    }

    #[test]
    fn ack_rejects_a_claim_record_with_changed_payload_metadata() {
        let root = TestRoot::new("immutable");
        let store = root.store();
        let descriptor = store.create(request(None), 1_000).unwrap();
        let claim = store
            .claim(&descriptor.id, "api-request/v1", "api-playground", 2_000)
            .unwrap();
        let mut record: Value =
            serde_json::from_slice(&fs::read(root.claimed(&descriptor.id)).unwrap()).unwrap();
        record["envelope"]["payload"]["body"] = json!("changed after preview");
        fs::write(
            root.claimed(&descriptor.id),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();

        assert_eq!(
            store.ack(&claim, "api-playground", 3_000),
            Err(HandoffError::Corrupt)
        );
    }

    #[test]
    fn wrong_target_and_kind_leave_pending_bytes_for_the_right_consumer() {
        let root = TestRoot::new("routing");
        let store = root.store();
        let descriptor = store
            .create(request(Some("api-playground")), 1_000)
            .unwrap();
        let before = fs::read(root.pending(&descriptor.id)).unwrap();

        assert_eq!(
            store.claim(
                &descriptor.id,
                "knowledge-draft/v1",
                "api-playground",
                2_000,
            ),
            Err(HandoffError::WrongKind)
        );
        assert_eq!(
            store.claim(&descriptor.id, "api-request/v1", "knowledge-base", 2_000,),
            Err(HandoffError::WrongTarget)
        );
        assert_eq!(fs::read(root.pending(&descriptor.id)).unwrap(), before);
        assert!(store
            .claim(&descriptor.id, "api-request/v1", "api-playground", 2_000,)
            .is_ok());
    }

    #[test]
    fn restore_requires_the_exact_token_and_requeues_until_ttl() {
        let root = TestRoot::new("restore");
        let store = root.store();
        let descriptor = store.create(request(None), 1_000).unwrap();
        let claim = store
            .claim(&descriptor.id, "api-request/v1", "api-playground", 2_000)
            .unwrap();
        let mut forged = claim.clone();
        forged.claim_token = "0".repeat(32);
        assert_eq!(
            store.restore(&forged, "api-playground", 3_000),
            Err(HandoffError::TokenMismatch)
        );
        assert!(!root.pending(&descriptor.id).exists());

        store.restore(&claim, "api-playground", 3_000).unwrap();
        assert!(root.pending(&descriptor.id).is_file());
        let second = store
            .claim(&descriptor.id, "api-request/v1", "knowledge-base", 4_000)
            .unwrap();
        assert_ne!(second.claim_token, claim.claim_token);
    }

    #[test]
    fn lease_blocks_duplicate_claim_then_recovers_after_consumer_crash() {
        let root = TestRoot::new("lease");
        let store = root.store();
        let descriptor = store.create(request(None), 1_000).unwrap();
        let first = store
            .claim(&descriptor.id, "api-request/v1", "api-playground", 2_000)
            .unwrap();
        assert_eq!(first.lease_until_ms, 62_000);
        assert_eq!(
            store.claim(&descriptor.id, "api-request/v1", "knowledge-base", 61_999,),
            Err(HandoffError::AlreadyClaimed)
        );
        assert_eq!(
            store.ack(&first, "api-playground", 62_000),
            Err(HandoffError::LeaseExpired)
        );

        let recovered = store
            .claim(&descriptor.id, "api-request/v1", "knowledge-base", 62_000)
            .unwrap();
        assert_ne!(recovered.claim_token, first.claim_token);
        assert_eq!(
            store.ack(&first, "api-playground", 63_000),
            Err(HandoffError::TokenMismatch)
        );
        store.ack(&recovered, "knowledge-base", 63_000).unwrap();
    }

    #[test]
    fn renewal_is_monotonic_and_capped_by_payload_expiry() {
        let root = TestRoot::new("renew");
        let store = root.store();
        let descriptor = store
            .create_with_ttl(request(None), 1_000, 120_000)
            .unwrap();
        let claim = store
            .claim(&descriptor.id, "api-request/v1", "api-playground", 2_000)
            .unwrap();
        let renewed = store
            .renew(&claim, "api-playground", 61_000, DEFAULT_CLAIM_LEASE_MS)
            .unwrap();
        assert_eq!(renewed.lease_until_ms, 121_000);
        let not_shortened = store
            .renew(&renewed, "api-playground", 61_500, 1_000)
            .unwrap();
        assert_eq!(not_shortened.lease_until_ms, 121_000);
        assert_eq!(
            store.claim(&descriptor.id, "api-request/v1", "knowledge-base", 100_000,),
            Err(HandoffError::AlreadyClaimed)
        );
        store
            .ack(&not_shortened, "api-playground", 120_999)
            .unwrap();
    }

    #[test]
    fn expired_or_corrupt_pending_payload_is_deleted_without_echoing_bytes() {
        let root = TestRoot::new("cleanup");
        let store = root.store();
        let expired = store.create_with_ttl(request(None), 1_000, 100).unwrap();
        assert_eq!(
            store.claim(&expired.id, "api-request/v1", "api-playground", 1_100,),
            Err(HandoffError::Expired)
        );
        assert!(!root.pending(&expired.id).exists());

        let corrupt = store.create(request(None), 2_000).unwrap();
        let raw_secret = "Bearer must-not-be-reflected";
        fs::write(root.pending(&corrupt.id), format!("{{{raw_secret}")).unwrap();
        let error = store
            .claim(&corrupt.id, "api-request/v1", "api-playground", 3_000)
            .unwrap_err();
        assert_eq!(error, HandoffError::Corrupt);
        assert!(!error.to_string().contains(raw_secret));
        assert!(!root.pending(&corrupt.id).exists());
    }

    #[test]
    fn privacy_and_serialized_size_are_rejected_before_publication() {
        let root = TestRoot::new("privacy");
        let store = root.store();
        let raw_secret = "must-not-be-reflected";
        let mut unsafe_request = request(None);
        unsafe_request.payload = json!({"password": raw_secret});
        let error = store.create(unsafe_request, 1_000).unwrap_err();
        assert_eq!(error, HandoffError::InvalidPayload);
        assert!(!error.to_string().contains(raw_secret));

        let mut oversized = request(None);
        oversized.payload = Value::Array(
            (0..11)
                .map(|_| Value::String("x".repeat(MAX_PAYLOAD_STRING_BYTES)))
                .collect(),
        );
        assert_eq!(store.create(oversized, 1_000), Err(HandoffError::TooLarge));
        assert_eq!(fs::read_dir(root.path.join("pending")).unwrap().count(), 0);
    }

    #[test]
    fn named_secret_values_and_unsafe_payload_paths_are_rejected() {
        let root = TestRoot::new("payload-boundaries");
        fs::create_dir_all(&root.path).unwrap();
        let selected = root.path.join("selected.bin");
        fs::write(&selected, b"selected bytes").unwrap();
        let store = root.store();

        let mut raw_header = request(None);
        raw_header.payload = json!({
            "headers": [{"name": "X-Api-Key", "value": "opaque-credential"}]
        });
        assert_eq!(
            store.create(raw_header, 1_000),
            Err(HandoffError::InvalidPayload)
        );

        let mut raw_named_token = request(None);
        raw_named_token.payload = json!({
            "headers": [{"name": "X-Client-Token", "value": "opaque-credential"}]
        });
        assert_eq!(
            store.create(raw_named_token, 1_000),
            Err(HandoffError::InvalidPayload)
        );

        let mut raw_x_auth = request(None);
        raw_x_auth.payload = json!({
            "headers": [{"name": "X-Auth", "value": "opaque-credential"}]
        });
        assert_eq!(
            store.create(raw_x_auth, 1_000),
            Err(HandoffError::InvalidPayload)
        );

        let mut spaced_raw_assignment = request(None);
        spaced_raw_assignment.payload = json!({
            "body": "mode=test token = opaque-credential"
        });
        assert_eq!(
            store.create(spaced_raw_assignment, 1_000),
            Err(HandoffError::InvalidPayload)
        );

        let mut spaced_reference = request(None);
        spaced_reference.payload = json!({
            "body": "mode=test token = ${WEBHOOK_SECRET}"
        });
        assert!(store.create(spaced_reference, 1_000).is_ok());

        let mut relative = request(None);
        relative.payload = json!({"path": "../outside.bin"});
        assert_eq!(
            store.create(relative, 1_000),
            Err(HandoffError::InvalidPayload)
        );

        let mut safe = request(None);
        safe.payload = json!({"filePath": selected});
        assert!(store.create(safe, 1_000).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_payload_path_is_rejected() {
        use std::os::unix::fs::symlink;

        let root = TestRoot::new("payload-symlink");
        fs::create_dir_all(&root.path).unwrap();
        let selected = root.path.join("selected.bin");
        let linked = root.path.join("linked.bin");
        fs::write(&selected, b"selected bytes").unwrap();
        symlink(&selected, &linked).unwrap();

        let mut request = request(None);
        request.payload = json!({"path": linked});
        assert_eq!(
            root.store().create(request, 1_000),
            Err(HandoffError::InvalidPayload)
        );
    }

    #[test]
    fn concurrent_consumers_publish_exactly_one_claim() {
        let root = TestRoot::new("concurrent");
        let store = Arc::new(root.store());
        let descriptor = store.create(request(None), 1_000).unwrap();
        let barrier = Arc::new(Barrier::new(3));
        let mut handles = Vec::new();
        for consumer in ["api-playground", "knowledge-base"] {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            let id = descriptor.id.clone();
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                store.claim(&id, "api-request/v1", consumer, 2_000)
            }));
        }
        barrier.wait();
        let results = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| **result == Err(HandoffError::AlreadyClaimed))
                .count(),
            1
        );
    }

    #[test]
    fn relative_and_dangerous_roots_are_rejected() {
        let relative = HandoffStore::new("relative-handoff-root");
        assert_eq!(
            relative.create(request(None), 1_000),
            Err(HandoffError::UnsafeStorage)
        );

        let current = HandoffStore::new(std::env::current_dir().unwrap());
        assert_eq!(
            current.create(request(None), 1_000),
            Err(HandoffError::UnsafeStorage)
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_store_component_is_rejected_without_touching_target() {
        use std::os::unix::fs::symlink;

        let outer = TestRoot::new("symlink");
        fs::create_dir_all(&outer.path).unwrap();
        let target = outer.path.join("target");
        fs::create_dir(&target).unwrap();
        let link = outer.path.join("store-link");
        symlink(&target, &link).unwrap();
        let store = HandoffStore::new(&link);

        assert_eq!(
            store.create(request(None), 1_000),
            Err(HandoffError::UnsafeStorage)
        );
        assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
    }
}
