//! Devbox Launcher의 bounded, read-only index와 action 재검증.
//!
//! 이 모듈은 producer의 데이터베이스를 열지 않는다. 정해진 integration snapshot과
//! build-time catalog만 읽고, 각 결과를 다시 확인한 뒤에만 명시적 실행 계층으로
//! 넘긴다. snapshot 하나가 손상되어도 다른 producer의 결과는 유지한다.

use devbox_applink::{contains_sensitive_value, QueryFilter};
use devbox_catalog::{parse_catalog, Catalog, CatalogAction, CatalogApp};
use devbox_filesystem::parse_safe_project_path;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;

pub const CATALOG_SCHEMA_VERSION: u32 = 2;
pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;
pub const SNAPSHOT_VIEW_SCHEMA_VERSION: u32 = 1;
pub const MAX_QUERY_BYTES: usize = 512;
pub const MAX_ENTRY_ID_BYTES: usize = 128;
pub const MAX_LABEL_BYTES: usize = 256;
pub const MAX_DETAIL_BYTES: usize = 512;
pub const MAX_ENTRIES_PER_SOURCE: usize = 2_048;
pub const MAX_RESULTS: usize = 256;
pub const MAX_HANDOFF_TEXT_BYTES: usize = 64 * 1024;
pub const STALE_AFTER_MS: u64 = 15 * 60 * 1_000;
pub const CLIPBOARD_PREVIEW_ID: &str = "builtin/clipboard-preview";

const ERR_INDEX: &str = "Launcher 결과를 읽을 수 없습니다";
const ERR_ACTION: &str = "Launcher 동작을 확인할 수 없습니다";
const ERR_PRIVACY: &str = "Launcher에 안전하지 않은 값이 있습니다";
const ERR_BOUNDS: &str = "Launcher 입력 범위를 초과했습니다";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceSpec {
    pub producer: &'static str,
    pub view: &'static str,
    pub aliases: &'static [&'static str],
    pub target_app: &'static str,
    pub target_kind: &'static str,
}

pub const SOURCES: &[SourceSpec] = &[
    SourceSpec {
        producer: "workbench",
        view: "profiles",
        aliases: &[],
        target_app: "workbench",
        target_kind: "profile",
    },
    SourceSpec {
        producer: "repo-manager",
        view: "repositories",
        aliases: &[],
        target_app: "repo-manager",
        target_kind: "path",
    },
    SourceSpec {
        producer: "run-manager",
        view: "jobs-services",
        aliases: &["status"],
        target_app: "run-manager",
        target_kind: "task",
    },
    SourceSpec {
        producer: "everything-plus",
        view: "saved-queries",
        aliases: &[],
        target_app: "everything-plus",
        target_kind: "query",
    },
    SourceSpec {
        producer: "wsl-desktop",
        view: "profiles",
        aliases: &[],
        target_app: "wsl-desktop",
        target_kind: "profile",
    },
];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub id: String,
    pub label: String,
    pub detail: Option<String>,
    pub source: String,
    pub target_app: String,
    pub target_kind: String,
    pub stale: bool,
    pub explicit_preview: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SourceDiagnostic {
    pub producer: String,
    pub view: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    pub sources: Vec<SourceDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Path {
        path: String,
    },
    Profile {
        id: String,
    },
    /// Reserved for a producer that exposes a workspace payload. The current
    /// bootstrap sources use path/profile/query/task, but the AppLink target
    /// remains typed so a future source cannot smuggle an arbitrary argv.
    #[allow(dead_code)]
    Workspace {
        path: String,
    },
    Query {
        text: String,
        filter: Option<QueryFilter>,
    },
    Task {
        id: String,
    },
    /// Explicit local fallback: show the current selection/clipboard without
    /// creating a handoff or launching another application.
    ClipboardPreview,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAction {
    pub app_id: String,
    pub target: Target,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextAction {
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawEntry {
    id: String,
    label: String,
    #[serde(default)]
    detail: Option<String>,
    target_app: String,
    target_kind: String,
    payload_version: u32,
    payload: Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct IdPayload {
    id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PathPayload {
    path: String,
}

#[derive(Debug, Clone, Deserialize)]
// Payload version 1 is strict: a future semantic field requires a version
// bump. Older installed Launchers keep their own text-only behavior, while the
// current consumer must not silently ignore a field from a corrupt snapshot.
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QueryPayload {
    text: String,
    #[serde(default)]
    filter: Option<QueryFilter>,
}

#[derive(Debug, Clone)]
struct IndexedEntry {
    result: SearchResult,
    target: Target,
}

#[derive(Debug, Clone)]
pub struct Index {
    catalog: Catalog,
    entries: Vec<IndexedEntry>,
    diagnostics: Vec<SourceDiagnostic>,
}

impl Index {
    pub fn build(catalog_json: &str, integration_root: &Path) -> Result<Self, String> {
        let catalog = parse_catalog(catalog_json).map_err(|_| ERR_INDEX.to_string())?;
        if catalog.schema_version != CATALOG_SCHEMA_VERSION {
            return Err(ERR_INDEX.into());
        }

        let mut entries = Vec::new();
        for app in &catalog.apps {
            // The Launcher mirrors the user-facing catalog, not Manager's
            // hidden/self-management entries. Exclude itself as well: opening
            // the already focused transient window is neither useful nor a
            // distinct catalog action.
            if !app.manager_visible || app.id == "devbox-launcher" {
                continue;
            }
            entries.push(IndexedEntry {
                result: SearchResult {
                    id: format!("catalog/app/{}", app.id),
                    label: app.display_name.clone(),
                    detail: Some("Devbox 앱".into()),
                    source: "catalog".into(),
                    target_app: app.id.clone(),
                    target_kind: "app".into(),
                    stale: false,
                    explicit_preview: false,
                },
                target: Target::Task { id: app.id.clone() },
            });
            for action in &app.actions {
                // A static action is discoverable only when the catalog also
                // declares a receiver capability for its payload kind. This
                // keeps an unimplemented producer from presenting a dead
                // handoff in the Launcher UI.
                if catalog.apps.iter().any(|target| {
                    target.id == action.target
                        && target
                            .accepts
                            .iter()
                            .any(|capability| capability == &action.payload_kind)
                }) && supports_plain_text_action(&action.payload_kind)
                {
                    entries.push(catalog_action(app, action));
                }
            }
        }

        entries.push(IndexedEntry {
            result: SearchResult {
                id: CLIPBOARD_PREVIEW_ID.into(),
                label: "Clipboard 미리보기".into(),
                detail: Some("현재 선택 영역, 없으면 clipboard · 전달하지 않음".into()),
                source: "launcher".into(),
                target_app: "devbox-launcher".into(),
                target_kind: "clipboard-preview".into(),
                stale: false,
                explicit_preview: true,
            },
            target: Target::ClipboardPreview,
        });

        let mut diagnostics = Vec::with_capacity(SOURCES.len());
        for spec in SOURCES {
            let (status, source_entries, stale) = read_source(spec, integration_root);
            diagnostics.push(SourceDiagnostic {
                producer: spec.producer.into(),
                view: spec.view.into(),
                status,
            });
            for entry in source_entries {
                let id = format!("snapshot/{}/{}", spec.producer, entry.id);
                entries.push(IndexedEntry {
                    result: SearchResult {
                        id,
                        label: entry.label,
                        detail: entry.detail,
                        source: spec.producer.into(),
                        target_app: spec.target_app.into(),
                        target_kind: spec.target_kind.into(),
                        stale,
                        explicit_preview: false,
                    },
                    target: entry.target,
                });
            }
        }

        entries.sort_by(|left, right| {
            left.result
                .label
                .to_lowercase()
                .cmp(&right.result.label.to_lowercase())
                .then_with(|| left.result.source.cmp(&right.result.source))
                .then_with(|| left.result.id.cmp(&right.result.id))
        });
        Ok(Self {
            catalog,
            entries,
            diagnostics,
        })
    }

    pub fn search(&self, query: &str) -> Result<SearchResponse, String> {
        validate_query(query)?;
        let needle = query.trim().to_lowercase();
        // Rank matches before applying the result cap. A plain alphabetic
        // sort can hide an exact match behind the first 256 catalog entries,
        // while retaining every source's bounded input limit.
        let mut matches: Vec<(u8, &IndexedEntry)> = self
            .entries
            .iter()
            .filter_map(|entry| match_score(&entry.result, &needle).map(|score| (score, entry)))
            .collect();
        matches.sort_by(|(left_score, left), (right_score, right)| {
            left_score
                .cmp(right_score)
                .then_with(|| {
                    left.result
                        .label
                        .to_lowercase()
                        .cmp(&right.result.label.to_lowercase())
                })
                .then_with(|| left.result.source.cmp(&right.result.source))
                .then_with(|| left.result.id.cmp(&right.result.id))
        });
        let results = matches
            .into_iter()
            .take(MAX_RESULTS)
            .map(|(_, entry)| entry.result.clone())
            .collect();
        Ok(SearchResponse {
            results,
            sources: self.diagnostics.clone(),
        })
    }

    pub fn resolve(&self, result_id: &str) -> Result<ResolvedAction, String> {
        if result_id.len() > MAX_ENTRY_ID_BYTES || result_id.chars().any(char::is_control) {
            return Err(ERR_ACTION.into());
        }
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.result.id == result_id)
            .ok_or(ERR_ACTION)?;
        // Static text actions have a separate preview/confirm boundary. They
        // must never be launchable through the generic result command, even
        // if a forged renderer request supplies their opaque id.
        if entry.result.explicit_preview {
            return Err(ERR_ACTION.into());
        }
        if entry.result.source == "catalog" && entry.result.target_kind == "app" {
            return Ok(ResolvedAction {
                app_id: entry.result.target_app.clone(),
                target: entry.target.clone(),
            });
        }
        let target_app = self
            .catalog
            .apps
            .iter()
            .find(|app| app.id == entry.result.target_app)
            .ok_or(ERR_ACTION)?;
        if !target_accepts(target_app, &entry.result.target_kind) {
            return Err(ERR_ACTION.into());
        }
        Ok(ResolvedAction {
            app_id: entry.result.target_app.clone(),
            target: entry.target.clone(),
        })
    }

    /// Resolve a catalog-owned explicit text action. The action's payload kind
    /// is the only handoff kind accepted; renderer-supplied target apps are
    /// never trusted.
    pub fn resolve_text_action(&self, result_id: &str) -> Result<(String, String), String> {
        if result_id.len() > MAX_ENTRY_ID_BYTES || result_id.chars().any(char::is_control) {
            return Err(ERR_ACTION.into());
        }
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.result.id == result_id)
            .ok_or(ERR_ACTION)?;
        if result_id == CLIPBOARD_PREVIEW_ID
            && entry.result.target_kind == "clipboard-preview"
            && matches!(&entry.target, Target::ClipboardPreview)
        {
            return Ok(("devbox-launcher".into(), "clipboard-preview/v1".into()));
        }
        if !entry.result.explicit_preview
            || entry.result.source != "catalog"
            || !result_id.starts_with("catalog/action/")
        {
            return Err(ERR_ACTION.into());
        }
        let action = self
            .catalog
            .apps
            .iter()
            .flat_map(|app| app.actions.iter().map(move |action| (app, action)))
            .find(|(app, action)| {
                format!("catalog/action/{}/{}", app.id, action.action_id) == result_id
            })
            .ok_or(ERR_ACTION)?;
        let target = self
            .catalog
            .apps
            .iter()
            .find(|app| app.id == action.1.target)
            .ok_or(ERR_ACTION)?;
        if !target
            .accepts
            .iter()
            .any(|capability| capability == &action.1.payload_kind)
        {
            return Err(ERR_ACTION.into());
        }
        Ok((target.id.clone(), action.1.payload_kind.clone()))
    }
}

fn match_score(result: &SearchResult, needle: &str) -> Option<u8> {
    if needle.is_empty() {
        return Some(20);
    }
    field_match_score(&result.label, needle, 0)
        .or_else(|| {
            result
                .detail
                .as_deref()
                .and_then(|detail| field_match_score(detail, needle, 3))
        })
        .or_else(|| field_match_score(&result.target_app, needle, 6))
        .or_else(|| field_match_score(&result.source, needle, 9))
}

fn field_match_score(value: &str, needle: &str, base: u8) -> Option<u8> {
    let value = value.to_lowercase();
    if value == needle {
        Some(base)
    } else if value.starts_with(needle) {
        Some(base + 1)
    } else if value.contains(needle) {
        Some(base + 2)
    } else {
        None
    }
}

fn catalog_action(app: &CatalogApp, action: &CatalogAction) -> IndexedEntry {
    IndexedEntry {
        result: SearchResult {
            id: format!("catalog/action/{}/{}", app.id, action.action_id),
            label: action.label.clone(),
            detail: Some(format!("{} · 명시적 텍스트 미리보기", app.display_name)),
            source: "catalog".into(),
            target_app: action.target.clone(),
            target_kind: "handoff".into(),
            stale: false,
            explicit_preview: true,
        },
        target: Target::Task {
            id: action.payload_kind.clone(),
        },
    }
}

fn target_accepts(app: &CatalogApp, kind: &str) -> bool {
    match kind {
        "path" => app.accepts.iter().any(|v| v == "path"),
        "profile" => app.accepts.iter().any(|v| v == "profile"),
        "workspace" => app.accepts.iter().any(|v| v == "workspace"),
        "query" => app.accepts.iter().any(|v| v == "query"),
        "task" => app.accepts.iter().any(|v| v == "task"),
        "clipboard-preview" => false,
        "handoff" => app.accepts.iter().any(|v| v.starts_with("handoff:")),
        _ => false,
    }
}

fn read_source(spec: &SourceSpec, root: &Path) -> (String, Vec<ParsedEntry>, bool) {
    let path = devbox_integration::snapshot_path_in(root, spec.producer, SNAPSHOT_SCHEMA_VERSION);
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.file_type().is_file() => {
            return ("corrupt".into(), Vec::new(), false)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return ("missing".into(), Vec::new(), false)
        }
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            return ("permission".into(), Vec::new(), false)
        }
        Err(_) => return ("corrupt".into(), Vec::new(), false),
        Ok(_) => {}
    }
    // `symlink_metadata` can succeed while the ACL denies opening the file.
    // Preserve that distinction in the source diagnostics without exposing the
    // OS error or path to the renderer.
    match std::fs::File::open(&path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return ("missing".into(), Vec::new(), false)
        }
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            return ("permission".into(), Vec::new(), false)
        }
        Err(_) => return ("corrupt".into(), Vec::new(), false),
    }
    let envelope =
        match devbox_integration::read_snapshot_in(root, spec.producer, SNAPSHOT_SCHEMA_VERSION) {
            Ok(Some(value)) => value,
            Ok(None) => return ("missing".into(), Vec::new(), false),
            Err(_) => return ("corrupt".into(), Vec::new(), false),
        };
    let age = std::fs::symlink_metadata(&path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .map(|modified| modified.elapsed().unwrap_or_default().as_millis() as u64)
        .unwrap_or(u64::MAX);
    let file_stale = age >= STALE_AFTER_MS;
    let mut views = match envelope.views() {
        Ok(views) => views,
        Err(_) => return ("corrupt".into(), Vec::new(), false),
    };
    let selected_view = if views.contains_key(spec.view) {
        views.remove(spec.view)
    } else {
        spec.aliases.iter().find_map(|alias| views.remove(*alias))
    };
    // Run Manager shipped its first status producer as a flat envelope before
    // the multi-view contract was adopted. Keep this narrow compatibility
    // reader so an installed producer remains useful without opening its DB.
    if selected_view.is_none()
        && spec.producer == "run-manager"
        && spec.aliases.contains(&"status")
        && envelope.data.get("views").is_none()
    {
        let parsed = parse_legacy_run_snapshot(&envelope.data);
        return match parsed {
            Ok(entries) => (
                if file_stale { "stale" } else { "fresh" }.into(),
                entries,
                file_stale,
            ),
            Err(_) => ("corrupt".into(), Vec::new(), file_stale),
        };
    }
    let Some(view) = selected_view else {
        return ("corrupt".into(), Vec::new(), false);
    };
    if view.schema_version != SNAPSHOT_VIEW_SCHEMA_VERSION
        || view.entries.len() > MAX_ENTRIES_PER_SOURCE
    {
        return ("corrupt".into(), Vec::new(), false);
    }
    let stale = age.saturating_add(view.freshness_ms) >= STALE_AFTER_MS;
    let mut parsed = Vec::with_capacity(view.entries.len());
    let mut ids = BTreeSet::new();
    for value in view.entries {
        match parse_entry(spec, value) {
            Ok(entry) if ids.insert(entry.id.clone()) => parsed.push(entry),
            Ok(_) => return ("corrupt".into(), Vec::new(), stale),
            Err(_) => return ("corrupt".into(), Vec::new(), stale),
        }
    }
    (if stale { "stale" } else { "fresh" }.into(), parsed, stale)
}

#[derive(Debug, Clone)]
struct ParsedEntry {
    id: String,
    label: String,
    detail: Option<String>,
    target: Target,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyRunSnapshot {
    active_services: Vec<LegacyRunService>,
    runs: LegacyRunCounts,
    last_run_at_ms: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyRunService {
    id: String,
    uptime_ms: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyRunCounts {
    success: i64,
    failed: i64,
}

fn parse_legacy_run_snapshot(value: &Value) -> Result<Vec<ParsedEntry>, &'static str> {
    let snapshot: LegacyRunSnapshot =
        serde_json::from_value(value.clone()).map_err(|_| ERR_INDEX)?;
    if snapshot.active_services.len() > MAX_ENTRIES_PER_SOURCE
        || snapshot.runs.success < 0
        || snapshot.runs.failed < 0
        || snapshot
            .last_run_at_ms
            .is_some_and(|timestamp| timestamp < 0)
    {
        return Err(ERR_INDEX);
    }
    let mut ids = BTreeSet::new();
    snapshot
        .active_services
        .into_iter()
        .map(|service| {
            if service.uptime_ms < 0
                || !valid_id(&service.id, MAX_ENTRY_ID_BYTES)
                || contains_sensitive_value(&service.id)
                || !ids.insert(service.id.clone())
            {
                return Err(ERR_INDEX);
            }
            Ok(ParsedEntry {
                id: service.id.clone(),
                label: format!("Run Manager · {}", service.id),
                detail: Some("service · 실행 중".into()),
                target: Target::Task { id: service.id },
            })
        })
        .collect()
}

fn parse_entry(spec: &SourceSpec, value: Value) -> Result<ParsedEntry, &'static str> {
    let raw: RawEntry = serde_json::from_value(value).map_err(|_| ERR_INDEX)?;
    if raw.payload_version != 1
        || raw.target_app != spec.target_app
        || raw.target_kind != spec.target_kind
        || !valid_id(&raw.id, MAX_ENTRY_ID_BYTES)
        || contains_sensitive_value(&raw.id)
        || !valid_text(&raw.label, MAX_LABEL_BYTES)
        || contains_sensitive_value(&raw.label)
        || raw
            .detail
            .as_deref()
            .is_some_and(|detail| !valid_text(detail, MAX_DETAIL_BYTES))
    {
        return Err(ERR_INDEX);
    }
    if raw.detail.as_deref().is_some_and(contains_sensitive_value) {
        return Err(ERR_PRIVACY);
    }
    let target = match raw.target_kind.as_str() {
        "profile" | "task" => {
            let payload: IdPayload = serde_json::from_value(raw.payload).map_err(|_| ERR_INDEX)?;
            if !valid_id(&payload.id, MAX_ENTRY_ID_BYTES) || contains_sensitive_value(&payload.id) {
                return Err(ERR_INDEX);
            }
            if raw.target_kind == "profile" {
                Target::Profile { id: payload.id }
            } else {
                Target::Task { id: payload.id }
            }
        }
        "path" => {
            let payload: PathPayload =
                serde_json::from_value(raw.payload).map_err(|_| ERR_INDEX)?;
            let path = parse_safe_project_path(&payload.path)
                .ok_or(ERR_INDEX)?
                .into_string();
            Target::Path { path }
        }
        "query" => {
            let payload: QueryPayload =
                serde_json::from_value(raw.payload).map_err(|_| ERR_INDEX)?;
            validate_query(&payload.text).map_err(|_| ERR_BOUNDS)?;
            if payload.text.trim().is_empty() {
                return Err(ERR_BOUNDS);
            }
            let filter = payload
                .filter
                .map(|filter| filter.normalized().map_err(|_| ERR_BOUNDS))
                .transpose()?;
            Target::Query {
                text: payload.text,
                filter,
            }
        }
        _ => return Err(ERR_INDEX),
    };
    Ok(ParsedEntry {
        id: raw.id,
        label: raw.label,
        detail: raw.detail,
        target,
    })
}

pub fn validate_query(value: &str) -> Result<(), String> {
    if value.len() > MAX_QUERY_BYTES
        || value.chars().any(char::is_control)
        || contains_sensitive_value(value)
    {
        return Err(ERR_BOUNDS.into());
    }
    Ok(())
}

pub fn validate_text_handoff(kind: &str, text: &str) -> Result<TextAction, String> {
    let Some(handoff_kind) = kind.strip_prefix("handoff:") else {
        return Err(ERR_PRIVACY.into());
    };
    if !supports_plain_text_action(kind)
        || text.is_empty()
        || text.len() > MAX_HANDOFF_TEXT_BYTES
        // Newlines and tabs are ordinary source text. Reject only other C0
        // controls so a multiline selection can still reach a transformer.
        || text
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
        || contains_sensitive_value(text)
    {
        return Err(ERR_PRIVACY.into());
    }
    Ok(TextAction {
        kind: handoff_kind.into(),
        text: text.into(),
    })
}

fn supports_plain_text_action(kind: &str) -> bool {
    // Catalog capabilities describe business payloads, not merely transport
    // shapes. Only kinds with an explicit plain-text adapter may read the
    // current selection. In particular, `knowledge-draft/v1` is a structured
    // Life Log digest and must never be forged from clipboard text.
    kind == "handoff:toolbox-text/v1"
}

fn valid_id(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn valid_text(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;
    use devbox_integration::{Envelope, SnapshotView, SnapshotViews};
    use std::path::PathBuf;

    fn root(label: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("launcher-core-{}-{label}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn catalog() -> &'static str {
        r#"{"schemaVersion":2,"catalogRevision":1,"apps":[
          {"id":"developer-toolbox","displayName":"Developer Toolbox","productName":"DeveloperToolbox","identifier":"com.devbox.developertoolbox","cargoPackage":"developer-toolbox","appDir":"apps/developer-toolbox","release":true,"managerVisible":true,"selfManaged":false,"accepts":["handoff:toolbox-text/v1"],"produces":[],"actions":[]},
          {"id":"everything-plus","displayName":"Everything+","productName":"EverythingPlus","identifier":"com.devbox.everythingplus","cargoPackage":"everything-plus","appDir":"apps/everything-plus","release":true,"managerVisible":true,"selfManaged":false,"accepts":["query"],"produces":["snapshot:everything-plus/saved-queries/v1"],"actions":[]},
          {"id":"workbench","displayName":"Workbench","productName":"Workbench","identifier":"com.devbox.workbench","cargoPackage":"workbench","appDir":"apps/workbench","release":true,"managerVisible":true,"selfManaged":false,"accepts":["profile"],"produces":[],"actions":[]},
          {"id":"run-manager","displayName":"Run Manager","productName":"Run Manager","identifier":"com.devbox.runmanager","cargoPackage":"run-manager","appDir":"apps/run-manager","release":true,"managerVisible":true,"selfManaged":false,"accepts":["task"],"produces":[],"actions":[]},
          {"id":"launcher","displayName":"Launcher","productName":"Launcher","identifier":"com.devbox.launcher","cargoPackage":"launcher","appDir":"apps/launcher","release":true,"managerVisible":true,"selfManaged":false,"accepts":[],"produces":[],"actions":[{"actionId":"transform-text","actionVersion":1,"label":"Transform text","target":"developer-toolbox","payloadKind":"handoff:toolbox-text/v1"}]}
        ]}"#
    }

    fn write(root: &std::path::Path, entries: Vec<Value>, freshness_ms: u64) {
        write_view(root, "workbench", "profiles", entries, freshness_ms);
    }

    fn write_view(
        root: &std::path::Path,
        producer: &str,
        view: &str,
        entries: Vec<Value>,
        freshness_ms: u64,
    ) {
        let mut views = SnapshotViews::new();
        views.insert(
            view.into(),
            SnapshotView {
                schema_version: 1,
                freshness_ms,
                entries,
            },
        );
        let envelope = Envelope::with_views(producer, "0.1.0", views);
        write_envelope(root, producer, envelope);
    }

    fn write_envelope(root: &std::path::Path, producer: &str, envelope: Envelope) {
        devbox_integration::write_atomic(
            &envelope,
            &devbox_integration::snapshot_dir_in(root, producer, 1),
        )
        .unwrap();
    }

    fn entry(payload: Value) -> Value {
        serde_json::json!({"id":"profile-1","label":"Devbox","targetApp":"workbench","targetKind":"profile","payloadVersion":1,"payload":payload})
    }

    #[test]
    fn valid_snapshot_is_searchable_and_stale_is_visible() {
        let root = root("valid");
        write(
            &root,
            vec![entry(serde_json::json!({"id":"p-1"}))],
            STALE_AFTER_MS + 1,
        );
        let index = Index::build(catalog(), &root).unwrap();
        let response = index.search("devbox").unwrap();
        let result = response
            .results
            .iter()
            .find(|result| result.source == "workbench")
            .unwrap();
        assert!(result.stale);
        assert_eq!(index.resolve(&result.id).unwrap().app_id, "workbench");
    }

    #[test]
    fn everything_saved_query_filter_is_read_and_preserved_for_applink() {
        let root = root("everything-filter");
        write_view(
            &root,
            "everything-plus",
            "saved-queries",
            vec![serde_json::json!({
                "id": "saved-query-1",
                "label": "Rust sources",
                "detail": "Everything+ · saved query",
                "targetApp": "everything-plus",
                "targetKind": "query",
                "payloadVersion": 1,
                "payload": {
                    "text": "cargo",
                    "filter": {"extensions": ["rs"], "sourceRootId": 7}
                }
            })],
            0,
        );
        let index = Index::build(catalog(), &root).unwrap();
        let result = index
            .search("Rust sources")
            .unwrap()
            .results
            .into_iter()
            .find(|result| result.source == "everything-plus")
            .unwrap();
        assert_eq!(result.target_kind, "query");
        assert_eq!(
            index.resolve(&result.id).unwrap().target,
            Target::Query {
                text: "cargo".into(),
                filter: Some(QueryFilter {
                    extensions: vec!["rs".into()],
                    source_root_id: Some(7),
                    ..QueryFilter::default()
                })
            }
        );
    }

    #[test]
    fn everything_saved_query_rejects_unknown_payload_fields() {
        let root = root("everything-unknown-field");
        write_view(
            &root,
            "everything-plus",
            "saved-queries",
            vec![serde_json::json!({
                "id": "saved-query-1",
                "label": "Rust sources",
                "targetApp": "everything-plus",
                "targetKind": "query",
                "payloadVersion": 1,
                "payload": {"text": "cargo", "futureField": true}
            })],
            0,
        );
        let response = Index::build(catalog(), &root).unwrap().search("").unwrap();
        assert!(response
            .sources
            .iter()
            .any(|source| source.producer == "everything-plus" && source.status == "corrupt"));
        assert!(!response
            .results
            .iter()
            .any(|result| result.source == "everything-plus"));
    }

    #[test]
    fn stale_boundary_is_inclusive() {
        let root = root("stale-boundary");
        write(
            &root,
            vec![entry(serde_json::json!({"id":"p-1"}))],
            STALE_AFTER_MS,
        );
        let response = Index::build(catalog(), &root).unwrap().search("").unwrap();
        assert_eq!(
            response
                .sources
                .iter()
                .find(|source| source.producer == "workbench")
                .unwrap()
                .status,
            "stale"
        );
    }

    #[test]
    fn exact_label_match_is_ranked_before_prefix_and_contains_matches() {
        let exact = SearchResult {
            id: "exact".into(),
            label: "Workbench".into(),
            detail: None,
            source: "catalog".into(),
            target_app: "workbench".into(),
            target_kind: "app".into(),
            stale: false,
            explicit_preview: false,
        };
        let prefix = SearchResult {
            label: "Workbench Helper".into(),
            ..exact.clone()
        };
        let contains = SearchResult {
            label: "Project Workbench".into(),
            ..exact.clone()
        };
        assert!(match_score(&exact, "workbench") < match_score(&prefix, "workbench"));
        assert!(match_score(&prefix, "workbench") < match_score(&contains, "workbench"));
    }

    #[test]
    fn search_applies_result_bound_after_ranking() {
        let root = root("bounded-results");
        let entries = (0..MAX_ENTRIES_PER_SOURCE)
            .map(|index| {
                serde_json::json!({
                    "id": format!("profile-{index}"),
                    "label": format!("Profile {index}"),
                    "targetApp": "workbench",
                    "targetKind": "profile",
                    "payloadVersion": 1,
                    "payload": {"id": format!("p-{index}")}
                })
            })
            .collect();
        write(&root, entries, 0);
        let response = Index::build(catalog(), &root).unwrap().search("").unwrap();
        assert_eq!(response.results.len(), MAX_RESULTS);
        assert!(response.results.iter().all(|result| !result.id.is_empty()));
    }

    #[test]
    fn malformed_entry_isolated_as_corrupt() {
        let root = root("corrupt");
        write(
            &root,
            vec![entry(serde_json::json!({"id":"../../secret"}))],
            0,
        );
        let index = Index::build(catalog(), &root).unwrap();
        assert!(index
            .search("")
            .unwrap()
            .sources
            .iter()
            .any(|source| source.producer == "workbench" && source.status == "corrupt"));
        assert!(!index
            .search("devbox")
            .unwrap()
            .results
            .iter()
            .any(|result| result.source == "workbench"));
    }

    #[test]
    fn corrupt_source_does_not_suppress_a_healthy_source() {
        let root = root("source-isolation");
        write(
            &root,
            vec![entry(serde_json::json!({"id":"../../secret"}))],
            0,
        );
        write_envelope(
            &root,
            "run-manager",
            Envelope::new(
                "run-manager",
                "0.1.0",
                serde_json::json!({
                    "activeServices": [{"id": "service-healthy", "uptimeMs": 42}],
                    "runs": {"success": 1, "failed": 0},
                    "lastRunAtMs": null
                }),
            ),
        );
        let index = Index::build(catalog(), &root).unwrap();
        let response = index.search("service-healthy").unwrap();
        assert!(response
            .results
            .iter()
            .any(|result| result.source == "run-manager"));
        assert_eq!(
            response
                .sources
                .iter()
                .find(|source| source.producer == "workbench")
                .unwrap()
                .status,
            "corrupt"
        );
    }

    #[test]
    fn duplicate_snapshot_ids_are_isolated_as_corrupt() {
        let root = root("duplicate");
        write(&root, vec![entry(serde_json::json!({"id": "p-1"})); 2], 0);
        let index = Index::build(catalog(), &root).unwrap();
        let source = index
            .search("")
            .unwrap()
            .sources
            .into_iter()
            .find(|source| source.producer == "workbench")
            .unwrap();
        assert_eq!(source.status, "corrupt");
    }

    #[test]
    fn query_and_handoff_bounds_reject_secrets() {
        assert!(validate_query("Bearer top-secret").is_err());
        assert!(validate_query("find sk-live-value in logs").is_err());
        assert!(validate_text_handoff("handoff:toolbox-text/v1", "sk-live").is_err());
        assert_eq!(
            validate_text_handoff("handoff:toolbox-text/v1", "safe text")
                .unwrap()
                .kind,
            "toolbox-text/v1"
        );
        assert!(validate_text_handoff("handoff:toolbox-text/v1", "line one\nline two\t✓").is_ok());
    }

    #[test]
    fn legacy_run_status_snapshot_is_narrowly_supported() {
        let root = root("legacy-run");
        write_envelope(
            &root,
            "run-manager",
            Envelope::new(
                "run-manager",
                "0.1.0",
                serde_json::json!({
                    "activeServices": [{"id": "service-1", "uptimeMs": 42}],
                    "runs": {"success": 1, "failed": 0},
                    "lastRunAtMs": null
                }),
            ),
        );
        let index = Index::build(catalog(), &root).unwrap();
        let response = index.search("service-1").unwrap();
        let result = response
            .results
            .iter()
            .find(|result| result.source == "run-manager")
            .unwrap();
        assert_eq!(result.target_kind, "task");
        assert_eq!(
            index.resolve(&result.id).unwrap().target,
            Target::Task {
                id: "service-1".into()
            }
        );
    }

    #[test]
    fn explicit_text_action_cannot_use_generic_launch_path() {
        let index = Index::build(catalog(), &root("explicit-action")).unwrap();
        let result = index.search("transform text").unwrap().results[0].clone();
        assert!(result.explicit_preview);
        assert!(index.resolve(&result.id).is_err());
        assert_eq!(
            index.resolve_text_action(&result.id).unwrap(),
            ("developer-toolbox".into(), "handoff:toolbox-text/v1".into())
        );
    }

    #[test]
    fn structured_catalog_actions_are_not_exposed_as_plain_text_handoffs() {
        let catalog = catalog().replace("handoff:toolbox-text/v1", "handoff:knowledge-draft/v1");
        let index = Index::build(&catalog, &root("structured-action")).unwrap();
        assert!(index.search("transform text").unwrap().results.is_empty());
        assert!(validate_text_handoff("handoff:knowledge-draft/v1", "plain text").is_err());
    }

    #[test]
    fn clipboard_preview_is_local_explicit_and_not_a_handoff() {
        let index =
            Index::build(crate::commands::CATALOG_JSON, &root("clipboard-preview")).unwrap();
        assert!(!index
            .search("")
            .unwrap()
            .results
            .iter()
            .any(|result| result.target_kind == "handoff"));
        let result = index
            .search("clipboard")
            .unwrap()
            .results
            .into_iter()
            .find(|result| result.id == CLIPBOARD_PREVIEW_ID)
            .unwrap();
        assert!(result.explicit_preview);
        assert!(index.resolve(&result.id).is_err());
        assert_eq!(
            index.resolve_text_action(&result.id).unwrap(),
            ("devbox-launcher".into(), "clipboard-preview/v1".into())
        );
    }
}
