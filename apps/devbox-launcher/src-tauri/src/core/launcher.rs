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
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::Path;

use super::preferences::{validate_result_id, Preferences};

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
    pub snapshot_version: u32,
    /// Optional named sidecar in the producer/version directory. When absent,
    /// Launcher reads the conventional `summary.json`.
    pub snapshot_name: Option<&'static str>,
    /// Run Manager keeps an exact flat summary for old consumers; use it only
    /// when the named primary sidecar is missing.
    pub legacy_flat_summary: bool,
}

pub const SOURCES: &[SourceSpec] = &[
    SourceSpec {
        producer: "workbench",
        view: "profiles",
        aliases: &[],
        target_app: "workbench",
        target_kind: "profile",
        snapshot_version: SNAPSHOT_SCHEMA_VERSION,
        snapshot_name: Some("profiles"),
        legacy_flat_summary: false,
    },
    SourceSpec {
        producer: "repo-manager",
        view: "repositories",
        aliases: &[],
        target_app: "repo-manager",
        target_kind: "path",
        snapshot_version: SNAPSHOT_SCHEMA_VERSION,
        snapshot_name: Some("repositories"),
        legacy_flat_summary: false,
    },
    SourceSpec {
        producer: "run-manager",
        view: "jobs-services",
        aliases: &[],
        target_app: "run-manager",
        target_kind: "task",
        snapshot_version: SNAPSHOT_SCHEMA_VERSION,
        snapshot_name: Some("jobs-services"),
        legacy_flat_summary: true,
    },
    SourceSpec {
        producer: "everything-plus",
        view: "saved-queries",
        aliases: &[],
        target_app: "everything-plus",
        target_kind: "query",
        snapshot_version: SNAPSHOT_SCHEMA_VERSION,
        snapshot_name: None,
        legacy_flat_summary: false,
    },
    SourceSpec {
        producer: "wsl-desktop",
        view: "profiles",
        aliases: &[],
        target_app: "wsl-desktop",
        target_kind: "profile",
        snapshot_version: SNAPSHOT_SCHEMA_VERSION,
        snapshot_name: Some("profiles"),
        legacy_flat_summary: false,
    },
];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub id: String,
    /// Digest of the exact catalog or snapshot entry shown to the renderer.
    /// Commands require it again so an old selection cannot resolve to a
    /// renamed or otherwise changed producer payload.
    pub revision: String,
    pub label: String,
    pub detail: Option<String>,
    pub source: String,
    pub target_app: String,
    pub target_kind: String,
    pub stale: bool,
    pub explicit_preview: bool,
    pub favorite: bool,
    pub recent: bool,
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

        let catalog_revision = revision(&[b"catalog/v1", catalog_json.as_bytes()]);
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
                    revision: catalog_revision.clone(),
                    label: app.display_name.clone(),
                    detail: Some("Devbox 앱".into()),
                    source: "catalog".into(),
                    target_app: app.id.clone(),
                    target_kind: "app".into(),
                    stale: false,
                    explicit_preview: false,
                    favorite: false,
                    recent: false,
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
                    entries.push(catalog_action(app, action, &catalog_revision));
                }
            }
        }

        entries.push(IndexedEntry {
            result: SearchResult {
                id: CLIPBOARD_PREVIEW_ID.into(),
                revision: revision(&[b"builtin/v1", CLIPBOARD_PREVIEW_ID.as_bytes()]),
                label: "클립보드 미리보기".into(),
                detail: Some("현재 선택 영역, 없으면 클립보드 · 전달하지 않음".into()),
                source: "launcher".into(),
                target_app: "devbox-launcher".into(),
                target_kind: "clipboard-preview".into(),
                stale: false,
                explicit_preview: true,
                favorite: false,
                recent: false,
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
                        revision: entry.revision,
                        label: entry.label,
                        detail: entry.detail,
                        source: spec.producer.into(),
                        target_app: spec.target_app.into(),
                        target_kind: spec.target_kind.into(),
                        stale,
                        explicit_preview: false,
                        favorite: false,
                        recent: false,
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

    #[cfg(test)]
    pub fn search(&self, query: &str) -> Result<SearchResponse, String> {
        self.search_with_preferences(query, &Preferences::default())
    }

    pub fn search_with_preferences(
        &self,
        query: &str,
        preferences: &Preferences,
    ) -> Result<SearchResponse, String> {
        validate_query(query)?;
        preferences.validate()?;
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
                    preference_rank(preferences, &left.result.id)
                        .cmp(&preference_rank(preferences, &right.result.id))
                })
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
            .map(|(_, entry)| {
                let mut result = entry.result.clone();
                result.favorite = preferences.is_favorite(&result.id);
                result.recent = preferences.recent_rank(&result.id).is_some();
                result
            })
            .collect();
        Ok(SearchResponse {
            results,
            sources: self.diagnostics.clone(),
        })
    }

    pub fn resolve_checked(
        &self,
        result_id: &str,
        expected_revision: &str,
        allow_stale: bool,
    ) -> Result<ResolvedAction, String> {
        let entry = self.checked_entry(result_id, expected_revision, allow_stale)?;
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

    pub fn validate_selection(
        &self,
        result_id: &str,
        expected_revision: &str,
    ) -> Result<(), String> {
        self.checked_entry(result_id, expected_revision, true)
            .map(|_| ())
    }

    fn checked_entry(
        &self,
        result_id: &str,
        expected_revision: &str,
        allow_stale: bool,
    ) -> Result<&IndexedEntry, String> {
        validate_result_id(result_id).map_err(|_| ERR_ACTION.to_string())?;
        if !valid_revision(expected_revision) {
            return Err(ERR_ACTION.into());
        }
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.result.id == result_id)
            .ok_or(ERR_ACTION)?;
        if entry.result.revision != expected_revision || (entry.result.stale && !allow_stale) {
            return Err(ERR_ACTION.into());
        }
        Ok(entry)
    }

    #[cfg(test)]
    pub fn resolve(&self, result_id: &str) -> Result<ResolvedAction, String> {
        let revision = self
            .entries
            .iter()
            .find(|entry| entry.result.id == result_id)
            .map(|entry| entry.result.revision.clone())
            .ok_or(ERR_ACTION)?;
        self.resolve_checked(result_id, &revision, true)
    }

    /// Resolve a catalog-owned explicit text action. The action's payload kind
    /// is the only handoff kind accepted; renderer-supplied target apps are
    /// never trusted.
    pub fn resolve_text_action(
        &self,
        result_id: &str,
        expected_revision: &str,
    ) -> Result<(String, String), String> {
        let entry = self.checked_entry(result_id, expected_revision, false)?;
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

fn preference_rank(preferences: &Preferences, id: &str) -> (u8, usize) {
    if let Some(rank) = preferences.favorite_rank(id) {
        (0, rank)
    } else if let Some(rank) = preferences.recent_rank(id) {
        (1, rank)
    } else {
        (2, usize::MAX)
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
        // Result IDs are bounded, validated non-secret identifiers. Including
        // them preserves stable technical aliases such as `clipboard-preview`
        // when the visible label is localized.
        .or_else(|| field_match_score(&result.id, needle, 9))
        .or_else(|| field_match_score(&result.source, needle, 12))
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

fn catalog_action(app: &CatalogApp, action: &CatalogAction, revision: &str) -> IndexedEntry {
    IndexedEntry {
        result: SearchResult {
            id: format!("catalog/action/{}/{}", app.id, action.action_id),
            revision: revision.into(),
            label: action.label.clone(),
            detail: Some(format!("{} · 명시적 텍스트 미리보기", app.display_name)),
            source: "catalog".into(),
            target_app: action.target.clone(),
            target_kind: "handoff".into(),
            stale: false,
            explicit_preview: true,
            favorite: false,
            recent: false,
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
    let primary = read_source_at(spec, root, spec.snapshot_version, spec.snapshot_name, false);
    if primary.0 != "missing" {
        return primary;
    }
    if spec.legacy_flat_summary {
        read_source_at(spec, root, spec.snapshot_version, None, true)
    } else {
        primary
    }
}

fn read_source_at(
    spec: &SourceSpec,
    root: &Path,
    snapshot_version: u32,
    snapshot_name: Option<&str>,
    allow_legacy_flat: bool,
) -> (String, Vec<ParsedEntry>, bool) {
    let path = match snapshot_name {
        Some(name) => match devbox_integration::named_view_snapshot_path_in(
            root,
            spec.producer,
            snapshot_version,
            name,
        ) {
            Ok(path) => path,
            Err(_) => return ("corrupt".into(), Vec::new(), false),
        },
        None => devbox_integration::snapshot_path_in(root, spec.producer, snapshot_version),
    };
    match devbox_filesystem::ensure_no_links(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return ("missing".into(), Vec::new(), false)
        }
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            return ("permission".into(), Vec::new(), false)
        }
        Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => {
            return ("linked".into(), Vec::new(), false)
        }
        Err(_) => return ("corrupt".into(), Vec::new(), false),
    }
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
    let (source_file, source_identity) =
        match devbox_filesystem::open_filesystem_object(&path, false) {
            Ok(opened) => opened,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return ("missing".into(), Vec::new(), false)
            }
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                return ("permission".into(), Vec::new(), false)
            }
            Err(_) => return ("corrupt".into(), Vec::new(), false),
        };
    let envelope_result = match snapshot_name {
        Some(name) => devbox_integration::read_named_view_snapshot_in(
            root,
            spec.producer,
            snapshot_version,
            name,
        ),
        None => devbox_integration::read_snapshot_in(root, spec.producer, snapshot_version),
    };
    let envelope = match envelope_result {
        Ok(Some(value)) => value,
        Ok(None) => return ("missing".into(), Vec::new(), false),
        Err(_) => return ("corrupt".into(), Vec::new(), false),
    };
    if devbox_filesystem::ensure_no_links(&path).is_err()
        || devbox_filesystem::filesystem_identity(&path, false).ok() != Some(source_identity)
    {
        return ("corrupt".into(), Vec::new(), false);
    }
    let age = source_file
        .metadata()
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
    if selected_view.is_none() && allow_legacy_flat {
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
    revision: String,
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
                revision: revision(&[b"snapshot/run-manager/legacy/v1", service.id.as_bytes()]),
                label: format!("Run Manager · {}", service.id),
                detail: Some("service · 실행 중".into()),
                target: Target::Task { id: service.id },
            })
        })
        .collect()
}

fn parse_entry(spec: &SourceSpec, value: Value) -> Result<ParsedEntry, &'static str> {
    let encoded = serde_json::to_vec(&value).map_err(|_| ERR_INDEX)?;
    let entry_revision = revision(&[
        b"snapshot/v1",
        spec.producer.as_bytes(),
        spec.view.as_bytes(),
        &encoded,
    ]);
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
        revision: entry_revision,
        label: raw.label,
        detail: raw.detail,
        target,
    })
}

fn revision(parts: &[&[u8]]) -> String {
    let mut digest = Sha256::new();
    for part in parts {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part);
    }
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = digest.finalize();
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn valid_revision(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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
    {
        return Err(ERR_PRIVACY.into());
    }
    let (payload, _) =
        devbox_applink::ToolboxTextPayload::from_selected_text("devbox-launcher", text)
            .map_err(|_| ERR_PRIVACY.to_string())?;
    Ok(TextAction {
        kind: handoff_kind.into(),
        text: payload.text,
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
        let named = SOURCES
            .iter()
            .find(|source| source.producer == producer && source.view == view)
            .and_then(|source| source.snapshot_name);
        if let Some(name) = named {
            devbox_integration::write_named_view_snapshot_atomic(&envelope, root, name).unwrap();
        } else {
            write_envelope(root, producer, envelope);
        }
    }

    fn write_envelope(root: &std::path::Path, producer: &str, envelope: Envelope) {
        let version = envelope.schema_version;
        devbox_integration::write_atomic(
            &envelope,
            &devbox_integration::snapshot_dir_in(root, producer, version),
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
            revision: "a".repeat(64),
            label: "Workbench".into(),
            detail: None,
            source: "catalog".into(),
            target_app: "workbench".into(),
            target_kind: "app".into(),
            stale: false,
            explicit_preview: false,
            favorite: false,
            recent: false,
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
    fn favorites_and_recents_rank_without_persisting_result_metadata() {
        let index = Index::build(catalog(), &root("preference-ranking")).unwrap();
        let mut preferences = Preferences::default();
        preferences
            .record_recent("catalog/app/everything-plus")
            .unwrap();
        preferences
            .set_favorite("catalog/app/workbench", true)
            .unwrap();

        let response = index.search_with_preferences("", &preferences).unwrap();
        assert_eq!(response.results[0].id, "catalog/app/workbench");
        assert!(response.results[0].favorite);
        assert_eq!(response.results[1].id, "catalog/app/everything-plus");
        assert!(response.results[1].recent);
        assert!(response
            .results
            .iter()
            .skip(2)
            .all(|result| { !result.favorite || result.id == "catalog/app/workbench" }));
    }

    #[test]
    fn renamed_or_removed_snapshot_selection_fails_revision_revalidation() {
        let root = root("renamed-selection");
        write(&root, vec![entry(serde_json::json!({"id": "p-1"}))], 0);
        let first = Index::build(catalog(), &root)
            .unwrap()
            .search("Devbox")
            .unwrap()
            .results
            .into_iter()
            .find(|result| result.source == "workbench")
            .unwrap();

        write(
            &root,
            vec![serde_json::json!({
                "id": "profile-1",
                "label": "Renamed",
                "targetApp": "workbench",
                "targetKind": "profile",
                "payloadVersion": 1,
                "payload": {"id": "p-1"}
            })],
            0,
        );
        let renamed = Index::build(catalog(), &root).unwrap();
        assert!(renamed
            .resolve_checked(&first.id, &first.revision, false)
            .is_err());

        write(&root, Vec::new(), 0);
        let removed = Index::build(catalog(), &root).unwrap();
        assert!(removed
            .resolve_checked(&first.id, &first.revision, false)
            .is_err());
    }

    #[test]
    fn stale_selection_requires_confirmation_after_current_revalidation() {
        let root = root("stale-selection");
        write(
            &root,
            vec![entry(serde_json::json!({"id": "p-1"}))],
            STALE_AFTER_MS,
        );
        let index = Index::build(catalog(), &root).unwrap();
        let result = index
            .search("Devbox")
            .unwrap()
            .results
            .into_iter()
            .find(|result| result.source == "workbench")
            .unwrap();
        assert!(result.stale);
        assert!(index
            .resolve_checked(&result.id, &result.revision, false)
            .is_err());
        assert_eq!(
            index
                .resolve_checked(&result.id, &result.revision, true)
                .unwrap()
                .app_id,
            "workbench"
        );
    }

    #[test]
    fn prefixed_max_length_entry_id_remains_actionable() {
        let root = root("prefixed-id-bound");
        let entry_id = "a".repeat(MAX_ENTRY_ID_BYTES);
        write(
            &root,
            vec![serde_json::json!({
                "id": entry_id,
                "label": "Long ID",
                "targetApp": "workbench",
                "targetKind": "profile",
                "payloadVersion": 1,
                "payload": {"id": "profile-1"}
            })],
            0,
        );
        let index = Index::build(catalog(), &root).unwrap();
        let result = index
            .search("Long ID")
            .unwrap()
            .results
            .into_iter()
            .find(|result| result.source == "workbench")
            .unwrap();
        assert!(result.id.len() > MAX_ENTRY_ID_BYTES);
        assert_eq!(result.revision.len(), 64);
        assert!(result
            .revision
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
        assert!(index
            .resolve_checked(&result.id, &result.revision, false)
            .is_ok());
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
        assert_eq!(
            validate_text_handoff("handoff:toolbox-text/v1", "token=sk-live")
                .unwrap()
                .text,
            "[REDACTED]"
        );
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
    fn run_manager_jobs_services_sidecar_is_primary() {
        let root = root("run-sidecar");
        let views = SnapshotViews::from([(
            "jobs-services".into(),
            SnapshotView {
                schema_version: SNAPSHOT_VIEW_SCHEMA_VERSION,
                freshness_ms: 0,
                entries: vec![serde_json::json!({
                    "id": "job-build",
                    "label": "Build task",
                    "detail": "Run Manager · job",
                    "targetApp": "run-manager",
                    "targetKind": "task",
                    "payloadVersion": 1,
                    "payload": {"id": "job-build"},
                })],
            },
        )]);
        let envelope = Envelope::with_views("run-manager", "0.5.0", views);
        devbox_integration::write_named_view_snapshot_atomic(&envelope, &root, "jobs-services")
            .unwrap();

        let index = Index::build(catalog(), &root).unwrap();
        let result = index
            .search("Build task")
            .unwrap()
            .results
            .into_iter()
            .find(|result| result.source == "run-manager")
            .unwrap();
        assert_eq!(result.target_kind, "task");
        assert_eq!(
            index.resolve(&result.id).unwrap().target,
            Target::Task {
                id: "job-build".into()
            }
        );
        assert_eq!(
            index
                .search("")
                .unwrap()
                .sources
                .into_iter()
                .find(|source| source.producer == "run-manager")
                .unwrap()
                .status,
            "fresh"
        );
    }

    #[test]
    fn run_manager_sidecar_missing_falls_back_to_flat_status() {
        let root = root("run-sidecar-fallback");
        write_envelope(
            &root,
            "run-manager",
            Envelope::new(
                "run-manager",
                "0.4.0",
                serde_json::json!({
                    "activeServices": [{"id": "service-legacy", "uptimeMs": 12}],
                    "runs": {"success": 2, "failed": 0},
                    "lastRunAtMs": null,
                }),
            ),
        );

        let index = Index::build(catalog(), &root).unwrap();
        let result = index
            .search("service-legacy")
            .unwrap()
            .results
            .into_iter()
            .find(|result| result.source == "run-manager")
            .unwrap();
        assert_eq!(
            index.resolve(&result.id).unwrap().target,
            Target::Task {
                id: "service-legacy".into()
            }
        );
    }

    #[test]
    fn run_manager_sidecar_corrupt_does_not_fall_back_to_flat_status() {
        let root = root("run-sidecar-corrupt");
        write_envelope(
            &root,
            "run-manager",
            Envelope::new(
                "run-manager",
                "0.4.0",
                serde_json::json!({
                    "activeServices": [{"id": "service-legacy", "uptimeMs": 12}],
                    "runs": {"success": 2, "failed": 0},
                    "lastRunAtMs": null,
                }),
            ),
        );
        let sidecar = devbox_integration::named_view_snapshot_path_in(
            &root,
            "run-manager",
            1,
            "jobs-services",
        )
        .unwrap();
        std::fs::create_dir_all(sidecar.parent().unwrap()).unwrap();
        std::fs::write(sidecar, b"not-json").unwrap();

        let index = Index::build(catalog(), &root).unwrap();
        let response = index.search("service-legacy").unwrap();
        assert!(!response
            .results
            .iter()
            .any(|result| result.source == "run-manager"));
        assert_eq!(
            response
                .sources
                .into_iter()
                .find(|source| source.producer == "run-manager")
                .unwrap()
                .status,
            "corrupt"
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_manager_sidecar_symlink_does_not_fall_back_to_flat_status() {
        use std::os::unix::fs::symlink;

        let root = root("run-sidecar-symlink");
        write_envelope(
            &root,
            "run-manager",
            Envelope::new(
                "run-manager",
                "0.4.0",
                serde_json::json!({
                    "activeServices": [{"id": "service-legacy", "uptimeMs": 12}],
                    "runs": {"success": 2, "failed": 0},
                    "lastRunAtMs": null,
                }),
            ),
        );
        let sidecar = devbox_integration::named_view_snapshot_path_in(
            &root,
            "run-manager",
            SNAPSHOT_SCHEMA_VERSION,
            "jobs-services",
        )
        .unwrap();
        std::fs::create_dir_all(sidecar.parent().unwrap()).unwrap();
        let outside = root.join("outside.json");
        std::fs::write(&outside, b"not-json").unwrap();
        symlink(outside, sidecar).unwrap();

        let index = Index::build(catalog(), &root).unwrap();
        let response = index.search("service-legacy").unwrap();
        assert!(!response
            .results
            .iter()
            .any(|result| result.source == "run-manager"));
        assert_eq!(
            response
                .sources
                .into_iter()
                .find(|source| source.producer == "run-manager")
                .unwrap()
                .status,
            "linked"
        );
    }

    #[test]
    fn explicit_text_action_cannot_use_generic_launch_path() {
        let index = Index::build(catalog(), &root("explicit-action")).unwrap();
        let result = index.search("transform text").unwrap().results[0].clone();
        assert!(result.explicit_preview);
        assert!(index.resolve(&result.id).is_err());
        assert_eq!(
            index
                .resolve_text_action(&result.id, &result.revision)
                .unwrap(),
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
        assert!(index
            .search("clipboard")
            .unwrap()
            .results
            .iter()
            .any(|result| result.id == CLIPBOARD_PREVIEW_ID));
        let result = index
            .search("클립보드")
            .unwrap()
            .results
            .into_iter()
            .find(|result| result.id == CLIPBOARD_PREVIEW_ID)
            .unwrap();
        assert!(result.explicit_preview);
        assert_eq!(result.target_kind, "clipboard-preview");
        assert!(index.resolve(&result.id).is_err());
        assert_eq!(
            index
                .resolve_text_action(&result.id, &result.revision)
                .unwrap(),
            ("devbox-launcher".into(), "clipboard-preview/v1".into())
        );
    }
}
