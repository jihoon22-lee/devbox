//! Pure parser and selector for the devbox application catalog.
//!
//! This crate deliberately does not inspect install roots, resolve executables,
//! launch processes, or write the runtime copy. Those platform and mutation
//! boundaries belong to `crates/launch` and Devbox Manager.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;

pub const SCHEMA_V1: u32 = 1;
pub const SCHEMA_V2: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogError {
    InvalidJson,
    UnsupportedSchema {
        schema_version: u32,
    },
    MissingCatalogRevision,
    InvalidCatalogRevision,
    EmptyCatalog,
    InvalidApp {
        index: usize,
        field: &'static str,
    },
    DuplicateAppId {
        index: usize,
    },
    DuplicateAppIdentity {
        index: usize,
        field: &'static str,
    },
    InvalidCapability {
        app_index: usize,
        field: &'static str,
    },
    DuplicateCapability {
        app_index: usize,
        field: &'static str,
    },
    InvalidAction {
        app_index: usize,
        action_index: usize,
        field: &'static str,
    },
    DuplicateActionId {
        app_index: usize,
        action_index: usize,
    },
}

impl fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson => write!(formatter, "catalog JSON is invalid"),
            Self::UnsupportedSchema { schema_version } => {
                write!(
                    formatter,
                    "unsupported catalog schemaVersion: {schema_version}"
                )
            }
            Self::MissingCatalogRevision => {
                write!(formatter, "catalog v2 requires catalogRevision")
            }
            Self::InvalidCatalogRevision => {
                write!(formatter, "catalogRevision must be greater than zero")
            }
            Self::EmptyCatalog => write!(formatter, "catalog must contain at least one app"),
            Self::InvalidApp { index, field } => {
                write!(formatter, "catalog app {index} has invalid {field}")
            }
            Self::DuplicateAppId { index } => {
                write!(formatter, "catalog app {index} duplicates an earlier id")
            }
            Self::DuplicateAppIdentity { index, field } => {
                write!(
                    formatter,
                    "catalog app {index} duplicates an earlier {field}"
                )
            }
            Self::InvalidCapability { app_index, field } => {
                write!(formatter, "catalog app {app_index} has invalid {field}")
            }
            Self::DuplicateCapability { app_index, field } => {
                write!(formatter, "catalog app {app_index} has duplicate {field}")
            }
            Self::InvalidAction {
                app_index,
                action_index,
                field,
            } => write!(
                formatter,
                "catalog app {app_index} action {action_index} has invalid {field}"
            ),
            Self::DuplicateActionId {
                app_index,
                action_index,
            } => write!(
                formatter,
                "catalog app {app_index} action {action_index} duplicates an earlier actionId"
            ),
        }
    }
}

impl std::error::Error for CatalogError {}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogAction {
    pub action_id: String,
    pub action_version: u32,
    pub label: String,
    pub target: String,
    pub payload_kind: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogApp {
    pub id: String,
    pub display_name: String,
    pub product_name: String,
    pub identifier: String,
    pub cargo_package: String,
    pub app_dir: String,
    pub release: bool,
    pub manager_visible: bool,
    pub self_managed: bool,
    pub accepts: Vec<String>,
    pub produces: Vec<String>,
    pub actions: Vec<CatalogAction>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub schema_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub catalog_revision: Option<u64>,
    pub apps: Vec<CatalogApp>,
}

impl Catalog {
    pub fn revision_floor(&self) -> u64 {
        self.catalog_revision.unwrap_or(0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppRef {
    pub id: String,
    pub display_name: String,
    pub accepts: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogSource {
    BuildTime,
    Runtime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeFallbackReason {
    Missing,
    Invalid,
    MissingRevision,
    Stale {
        runtime_revision: u64,
        build_revision: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogSelection {
    pub catalog: Catalog,
    pub source: CatalogSource,
    pub fallback_reason: Option<RuntimeFallbackReason>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawCatalog {
    schema_version: u32,
    #[serde(default)]
    catalog_revision: Option<u64>,
    apps: Vec<RawCatalogApp>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawCatalogApp {
    id: String,
    display_name: String,
    product_name: String,
    identifier: String,
    cargo_package: String,
    app_dir: String,
    release: bool,
    manager_visible: bool,
    self_managed: bool,
    #[serde(default)]
    accepts: Vec<String>,
    #[serde(default)]
    produces: Vec<String>,
    #[serde(default)]
    actions: Vec<RawCatalogAction>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawCatalogAction {
    action_id: String,
    action_version: u32,
    label: String,
    target: String,
    payload_kind: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CapabilityShape {
    Basic,
    Handoff,
    Snapshot,
}

pub fn parse_catalog(input: &str) -> Result<Catalog, CatalogError> {
    let raw: RawCatalog = serde_json::from_str(input).map_err(|_| CatalogError::InvalidJson)?;
    if raw.apps.is_empty() {
        return Err(CatalogError::EmptyCatalog);
    }

    let catalog_revision = match raw.schema_version {
        SCHEMA_V1 => None,
        SCHEMA_V2 => match raw.catalog_revision {
            None => return Err(CatalogError::MissingCatalogRevision),
            Some(0) => return Err(CatalogError::InvalidCatalogRevision),
            Some(revision) => Some(revision),
        },
        schema_version => return Err(CatalogError::UnsupportedSchema { schema_version }),
    };

    let mut app_ids = HashSet::new();
    let mut identifiers = HashSet::new();
    let mut cargo_packages = HashSet::new();
    let mut app_dirs = HashSet::new();
    let mut apps = Vec::with_capacity(raw.apps.len());
    for (index, raw_app) in raw.apps.into_iter().enumerate() {
        validate_app_identity(&raw_app, index)?;
        if !app_ids.insert(raw_app.id.clone()) {
            return Err(CatalogError::DuplicateAppId { index });
        }
        if !identifiers.insert(raw_app.identifier.clone()) {
            return Err(CatalogError::DuplicateAppIdentity {
                index,
                field: "identifier",
            });
        }
        if !cargo_packages.insert(raw_app.cargo_package.clone()) {
            return Err(CatalogError::DuplicateAppIdentity {
                index,
                field: "cargoPackage",
            });
        }
        if !app_dirs.insert(raw_app.app_dir.clone()) {
            return Err(CatalogError::DuplicateAppIdentity {
                index,
                field: "appDir",
            });
        }

        let (accepts, produces, actions) = if raw.schema_version == SCHEMA_V1 {
            // A v1 document cannot opt into v2 routing by adding unversioned
            // fields. It always normalizes to the legacy empty capability set.
            (Vec::new(), Vec::new(), Vec::new())
        } else {
            validate_capabilities(&raw_app.accepts, &raw_app.id, index, "accepts", true)?;
            validate_capabilities(&raw_app.produces, &raw_app.id, index, "produces", false)?;
            validate_actions(&raw_app, index)?;
            (raw_app.accepts, raw_app.produces, raw_app.actions)
        };

        apps.push(CatalogApp {
            id: raw_app.id,
            display_name: raw_app.display_name,
            product_name: raw_app.product_name,
            identifier: raw_app.identifier,
            cargo_package: raw_app.cargo_package,
            app_dir: raw_app.app_dir,
            release: raw_app.release,
            manager_visible: raw_app.manager_visible,
            self_managed: raw_app.self_managed,
            accepts,
            produces,
            actions: actions
                .into_iter()
                .map(|action| CatalogAction {
                    action_id: action.action_id,
                    action_version: action.action_version,
                    label: action.label,
                    target: action.target,
                    payload_kind: action.payload_kind,
                })
                .collect(),
        });
    }

    validate_action_links(&apps)?;
    Ok(Catalog {
        schema_version: raw.schema_version,
        catalog_revision,
        apps,
    })
}

/// Select the runtime copy only when it is valid v2 data and its monotonic
/// revision is at least the build-time floor. Every runtime read failure is a
/// non-fatal fallback; invalid build-time data remains an explicit error.
pub fn select_catalog(
    build_time: &str,
    runtime: Option<&str>,
) -> Result<CatalogSelection, CatalogError> {
    let build_catalog = parse_catalog(build_time)?;
    let Some(runtime_input) = runtime else {
        return Ok(CatalogSelection {
            catalog: build_catalog,
            source: CatalogSource::BuildTime,
            fallback_reason: Some(RuntimeFallbackReason::Missing),
        });
    };
    let Ok(runtime_catalog) = parse_catalog(runtime_input) else {
        return Ok(CatalogSelection {
            catalog: build_catalog,
            source: CatalogSource::BuildTime,
            fallback_reason: Some(RuntimeFallbackReason::Invalid),
        });
    };
    let Some(runtime_revision) = runtime_catalog.catalog_revision else {
        return Ok(CatalogSelection {
            catalog: build_catalog,
            source: CatalogSource::BuildTime,
            fallback_reason: Some(RuntimeFallbackReason::MissingRevision),
        });
    };
    if runtime_revision < build_catalog.revision_floor() {
        let build_revision = build_catalog.revision_floor();
        return Ok(CatalogSelection {
            catalog: build_catalog,
            source: CatalogSource::BuildTime,
            fallback_reason: Some(RuntimeFallbackReason::Stale {
                runtime_revision,
                build_revision,
            }),
        });
    }
    Ok(CatalogSelection {
        catalog: runtime_catalog,
        source: CatalogSource::Runtime,
        fallback_reason: None,
    })
}

/// Return catalog entries that explicitly accept an exact capability. Install
/// state is intentionally not part of this pure filter.
pub fn capable_targets(catalog: &Catalog, capability: &str) -> Vec<AppRef> {
    catalog
        .apps
        .iter()
        .filter(|app| app.accepts.iter().any(|accepted| accepted == capability))
        .map(|app| AppRef {
            id: app.id.clone(),
            display_name: app.display_name.clone(),
            accepts: app.accepts.clone(),
        })
        .collect()
}

pub fn capable_producers(catalog: &Catalog, capability: &str) -> Vec<AppRef> {
    catalog
        .apps
        .iter()
        .filter(|app| app.produces.iter().any(|produced| produced == capability))
        .map(|app| AppRef {
            id: app.id.clone(),
            display_name: app.display_name.clone(),
            accepts: app.accepts.clone(),
        })
        .collect()
}

fn validate_app_identity(app: &RawCatalogApp, index: usize) -> Result<(), CatalogError> {
    if !valid_slug(&app.id, 64) {
        return invalid_app(index, "id");
    }
    if !valid_text(&app.display_name, 128) {
        return invalid_app(index, "displayName");
    }
    if !valid_text(&app.product_name, 128) {
        return invalid_app(index, "productName");
    }
    if !valid_identifier(&app.identifier) {
        return invalid_app(index, "identifier");
    }
    if !valid_slug(&app.cargo_package, 64) {
        return invalid_app(index, "cargoPackage");
    }
    if app.app_dir != format!("apps/{}", app.id) {
        return invalid_app(index, "appDir");
    }
    Ok(())
}

fn validate_capabilities(
    capabilities: &[String],
    app_id: &str,
    app_index: usize,
    field: &'static str,
    accepts: bool,
) -> Result<(), CatalogError> {
    let mut seen = HashSet::new();
    for capability in capabilities {
        let Some(shape) = capability_shape(capability) else {
            return Err(CatalogError::InvalidCapability { app_index, field });
        };
        let allowed = if accepts {
            matches!(shape, CapabilityShape::Basic | CapabilityShape::Handoff)
        } else {
            matches!(shape, CapabilityShape::Handoff | CapabilityShape::Snapshot)
        };
        if !allowed {
            return Err(CatalogError::InvalidCapability { app_index, field });
        }
        if !accepts
            && shape == CapabilityShape::Snapshot
            && !capability.starts_with(&format!("snapshot:{app_id}/"))
        {
            return Err(CatalogError::InvalidCapability { app_index, field });
        }
        if !seen.insert(capability) {
            return Err(CatalogError::DuplicateCapability { app_index, field });
        }
    }
    Ok(())
}

fn validate_actions(app: &RawCatalogApp, app_index: usize) -> Result<(), CatalogError> {
    let mut action_ids = HashSet::new();
    for (action_index, action) in app.actions.iter().enumerate() {
        if !valid_slug(&action.action_id, 96) {
            return invalid_action(app_index, action_index, "actionId");
        }
        if !action_ids.insert(&action.action_id) {
            return Err(CatalogError::DuplicateActionId {
                app_index,
                action_index,
            });
        }
        if action.action_version == 0 {
            return invalid_action(app_index, action_index, "actionVersion");
        }
        if !valid_text(&action.label, 128) {
            return invalid_action(app_index, action_index, "label");
        }
        if !valid_slug(&action.target, 64) {
            return invalid_action(app_index, action_index, "target");
        }
        if capability_shape(&action.payload_kind) != Some(CapabilityShape::Handoff) {
            return invalid_action(app_index, action_index, "payloadKind");
        }
    }
    Ok(())
}

fn validate_action_links(apps: &[CatalogApp]) -> Result<(), CatalogError> {
    for (app_index, app) in apps.iter().enumerate() {
        for (action_index, action) in app.actions.iter().enumerate() {
            let Some(target) = apps.iter().find(|candidate| candidate.id == action.target) else {
                return invalid_action(app_index, action_index, "target");
            };
            if !target.accepts.contains(&action.payload_kind) {
                return invalid_action(app_index, action_index, "target");
            }
        }
    }
    Ok(())
}

fn capability_shape(value: &str) -> Option<CapabilityShape> {
    if matches!(value, "path" | "workspace" | "query" | "profile") {
        return Some(CapabilityShape::Basic);
    }
    if let Some(kind) = value.strip_prefix("handoff:") {
        return valid_versioned_kind(kind).then_some(CapabilityShape::Handoff);
    }
    if let Some(rest) = value.strip_prefix("snapshot:") {
        let mut parts = rest.split('/');
        let producer = parts.next()?;
        let kind = parts.next()?;
        let version = parts.next()?;
        if parts.next().is_none()
            && valid_slug(producer, 64)
            && valid_slug(kind, 64)
            && valid_version(version)
        {
            return Some(CapabilityShape::Snapshot);
        }
    }
    None
}

fn valid_versioned_kind(value: &str) -> bool {
    let Some((kind, version)) = value.rsplit_once('/') else {
        return false;
    };
    valid_slug(kind, 96) && valid_version(version)
}

fn valid_version(value: &str) -> bool {
    let Some(digits) = value.strip_prefix('v') else {
        return false;
    };
    !digits.is_empty()
        && !digits.starts_with('0')
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && digits.parse::<u32>().is_ok()
}

fn valid_slug(value: &str, max_len: usize) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= max_len
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.iter().copied().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'-' | b'+' | b'_' | b'.')
        })
}

fn valid_identifier(value: &str) -> bool {
    let Some(suffix) = value.strip_prefix("com.devbox.") else {
        return false;
    };
    value.len() <= 128
        && !suffix.is_empty()
        && suffix.split('.').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

fn valid_text(value: &str, max_len: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_len
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

fn invalid_app<T>(index: usize, field: &'static str) -> Result<T, CatalogError> {
    Err(CatalogError::InvalidApp { index, field })
}

fn invalid_action<T>(
    app_index: usize,
    action_index: usize,
    field: &'static str,
) -> Result<T, CatalogError> {
    Err(CatalogError::InvalidAction {
        app_index,
        action_index,
        field,
    })
}
