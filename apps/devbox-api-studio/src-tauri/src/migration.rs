//! Native startup coordinator for the three API Studio legacy sources.
use crate::core::{
    import_model::{BrowserState, Documents, StoreKind},
    import_repository::{read_file, Repository},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::Manager;
const MAX_FILE: usize = 20 * 1024 * 1024;
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum LegacyApp {
    ApiPlayground,
    WebhookLab,
    DeveloperToolbox,
}
impl LegacyApp {
    fn identifier(self) -> &'static str {
        match self {
            Self::ApiPlayground => "com.devbox.apiplayground",
            Self::WebhookLab => "com.devbox.webhooklab",
            Self::DeveloperToolbox => "com.devbox.developertoolbox",
        }
    }
}
const LEGACY: [LegacyApp; 3] = [
    LegacyApp::ApiPlayground,
    LegacyApp::WebhookLab,
    LegacyApp::DeveloperToolbox,
];
#[derive(Default)]
struct Work {
    stage: &'static str,
    current: Option<(String, Arc<AtomicBool>)>,
    cancelled: VecDeque<(String, Instant)>,
}
pub struct MigrationState {
    root: PathBuf,
    legacy_root: PathBuf,
    installation: String,
    active: AtomicBool,
    force_import: bool,
    work: Arc<Mutex<Work>>,
}
struct WorkGuard {
    work: Arc<Mutex<Work>>,
    id: String,
    cancelled: Arc<AtomicBool>,
}
impl WorkGuard {
    fn stage(&self, stage: &'static str) {
        if let Ok(mut work) = self.work.lock() {
            if work.current.as_ref().is_some_and(|(id, _)| id == &self.id) {
                work.stage = stage;
            }
        }
    }
}
impl Drop for WorkGuard {
    fn drop(&mut self) {
        if let Ok(mut work) = self.work.lock() {
            if work.current.as_ref().is_some_and(|(id, _)| id == &self.id) {
                work.current.take();
            }
        }
    }
}
#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StartupFlags {
    schema_version: u32,
    setup_done: bool,
    request_import: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceStatus {
    app: LegacyApp,
    present: bool,
    readable: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Issue {
    store: String,
    code: String,
    count: usize,
}
fn io_error() -> String {
    "migration_storage_unavailable".into()
}
fn flags(root: &Path) -> Result<StartupFlags, String> {
    let value = match read_file(&root.join("imports/startup.json"), 4096)? {
        None => StartupFlags {
            schema_version: 1,
            ..StartupFlags::default()
        },
        Some(raw) => serde_json::from_str(&raw).map_err(|_| "migration_journal_invalid")?,
    };
    if value.schema_version != 1 {
        return Err("migration_journal_future_schema".into());
    }
    Ok(value)
}
fn save_flags(root: &Path, value: &StartupFlags) -> Result<(), String> {
    let directory = root.join("imports");
    fs::create_dir_all(&directory).map_err(|_| io_error())?;
    devbox_filesystem::ensure_no_links(&directory).map_err(|_| "migration_path_invalid")?;
    devbox_filesystem::atomic_write(
        directory.join("startup.json"),
        &serde_json::to_vec(value).map_err(|_| io_error())?,
    )
    .map_err(|_| io_error())
}
fn fixture_root(default: PathBuf) -> Result<PathBuf, String> {
    #[cfg(debug_assertions)]
    if let Some(value) = std::env::var_os("DEVBOX_API_MIGRATION_FIXTURE_ROOT") {
        if std::env::var("GITHUB_ACTIONS").ok().as_deref() != Some("true")
            || std::env::var("RUNNER_ENVIRONMENT").ok().as_deref() != Some("github-hosted")
        {
            return Err("migration_fixture_rejected".into());
        }
        let root = PathBuf::from(value);
        devbox_filesystem::ensure_no_links(&root).map_err(|_| "migration_fixture_rejected")?;
        let root = root
            .canonicalize()
            .map_err(|_| "migration_fixture_rejected")?;
        let temporary = std::env::temp_dir()
            .canonicalize()
            .map_err(|_| "migration_fixture_rejected")?;
        if !root.starts_with(temporary)
            || !root
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("devbox-api-migration-fixture-"))
        {
            return Err("migration_fixture_rejected".into());
        }
        return Ok(root);
    }
    Ok(default)
}
pub fn initialize(app: &tauri::AppHandle) -> Result<(), String> {
    let root = app.path().app_local_data_dir().map_err(|_| io_error())?;
    fs::create_dir_all(&root).map_err(|_| io_error())?;
    devbox_filesystem::ensure_no_links(&root).map_err(|_| "migration_path_invalid")?;
    let legacy_root = fixture_root(app.path().local_data_dir().map_err(|_| io_error())?)?;
    let identifier = app.config().identifier.clone();
    let installation = identifier
        .rsplit(".i")
        .next()
        .filter(|part| part.len() == 64 && part.bytes().all(|c| c.is_ascii_hexdigit()))
        .ok_or("migration_installation_invalid")?
        .to_string();
    let force_import = std::env::args().any(|arg| arg == "--import-legacy");
    if !app.manage(MigrationState {
        root,
        legacy_root,
        installation,
        active: AtomicBool::new(false),
        force_import,
        work: Arc::new(Mutex::new(Work::default())),
    }) {
        return Err("migration_state_conflict".into());
    }
    Ok(())
}
pub fn require_active(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<MigrationState>();
    if state.active.load(Ordering::Acquire) {
        Ok(())
    } else {
        Err("migration_startup_required".into())
    }
}
impl MigrationState {
    fn repository(&self) -> Result<Repository, String> {
        Repository::open(&self.root, &self.installation)
    }
    fn begin(&self, id: String) -> Result<WorkGuard, String> {
        if uuid::Uuid::parse_str(&id).is_err() || id.len() != 36 {
            return Err("migration_operation_invalid".into());
        }
        let mut work = self.work.lock().map_err(|_| io_error())?;
        if self.active.load(Ordering::Acquire) {
            return Err("migration_startup_required".into());
        }
        work.cancelled
            .retain(|(_, created)| created.elapsed() < Duration::from_secs(10));
        if work.cancelled.iter().any(|(cancelled, _)| cancelled == &id) {
            return Err("legacy_snapshot_cancelled".into());
        }
        if work.current.is_some() {
            return Err("migration_busy".into());
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        work.current = Some((id.clone(), Arc::clone(&cancelled)));
        work.stage = "starting";
        Ok(WorkGuard {
            work: Arc::clone(&self.work),
            id,
            cancelled,
        })
    }
    fn cancel(&self, id: String) -> Result<(), String> {
        if uuid::Uuid::parse_str(&id).is_err() || id.len() != 36 {
            return Err("migration_operation_invalid".into());
        }
        let mut work = self.work.lock().map_err(|_| io_error())?;
        if let Some((current, token)) = &work.current {
            if current == &id {
                token.store(true, Ordering::Release);
                return Ok(());
            }
        }
        if work.cancelled.len() >= 64 {
            work.cancelled.pop_front();
        }
        work.cancelled.push_back((id, Instant::now()));
        Ok(())
    }
}
fn source_status(root: &Path) -> Vec<SourceStatus> {
    LEGACY
        .iter()
        .map(|app| {
            let path = root.join(app.identifier());
            let present = path.exists();
            SourceStatus {
                app: *app,
                present,
                readable: !present
                    || (path.is_dir() && devbox_filesystem::ensure_no_links(&path).is_ok()),
            }
        })
        .collect()
}
fn profile_choices(root: &Path) -> Result<Vec<Value>, String> {
    let directory = root
        .join(LegacyApp::WebhookLab.identifier())
        .join("service-profiles");
    if !directory.exists() {
        return Ok(vec![]);
    }
    devbox_filesystem::ensure_no_links(&directory).map_err(|_| "migration_path_invalid")?;
    let mut choices = Vec::new();
    let mut count = 0;
    for entry in fs::read_dir(&directory).map_err(|_| io_error())? {
        count += 1;
        if count > 256 {
            return Err("migration_store_too_large".into());
        }
        let entry = entry.map_err(|_| io_error())?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "migration_path_invalid")?;
        let Some(id) = name.strip_suffix(".json") else {
            continue;
        };
        if !uuid::Uuid::parse_str(id).is_ok_and(|value| value.to_string() == id) {
            return Err("migration_schema_invalid".into());
        }
        devbox_filesystem::ensure_no_links(entry.path()).map_err(|_| "migration_path_invalid")?;
        choices.push(json!({ "id": id, "bytes": entry.metadata().map_err(|_| io_error())?.len() }));
    }
    choices.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
    Ok(choices)
}
fn api_owner(kind: StoreKind, bytes: &[u8]) -> Result<(), String> {
    api_playground_lib::component::validate_migration_native_store(
        match kind {
            StoreKind::OAuth => "oauth",
            StoreKind::GrpcTls => "grpc-tls",
            _ => return Err("migration_schema_invalid".into()),
        },
        bytes,
    )
}
fn insert_json(
    docs: &mut Documents,
    kind: StoreKind,
    root: &Path,
    relative: &str,
) -> Result<(), String> {
    if let Some(raw) = read_file(&root.join(relative), MAX_FILE)? {
        let value: Value = serde_json::from_str(&raw).map_err(|_| "migration_schema_invalid")?;
        crate::core::import_model::validate_document(kind, &value, &api_owner)?;
        docs.insert(kind, value);
    }
    Ok(())
}
fn native_sources(
    root: &Path,
    selected: &[LegacyApp],
    profile_ids: &Option<Vec<String>>,
) -> Result<(Documents, Vec<Issue>), String> {
    let mut docs = Documents::new();
    let mut issues = Vec::new();
    if selected.contains(&LegacyApp::WebhookLab) {
        let source = root.join(LegacyApp::WebhookLab.identifier());
        if source.exists() {
            devbox_filesystem::ensure_no_links(&source).map_err(|_| "migration_path_invalid")?;
        }
        insert_json(&mut docs, StoreKind::Fixtures, &source, "fixtures.json")?;
        let directory = source.join("service-profiles");
        if directory.exists() {
            devbox_filesystem::ensure_no_links(&directory).map_err(|_| "migration_path_invalid")?;
            let mut entries = fs::read_dir(&directory)
                .map_err(|_| io_error())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| io_error())?;
            if entries.len() > 256 {
                return Err("migration_store_too_large".into());
            }
            entries.sort_by_key(|entry| entry.file_name());
            let mut profiles = Vec::new();
            let mut bytes = 0usize;
            for entry in entries {
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| "migration_path_invalid")?;
                let Some(id) = name.strip_suffix(".json") else {
                    continue;
                };
                if !uuid::Uuid::parse_str(id).is_ok_and(|value| value.to_string() == id) {
                    return Err("migration_schema_invalid".into());
                }
                if profile_ids
                    .as_ref()
                    .is_some_and(|ids| !ids.iter().any(|selected| selected == id))
                {
                    continue;
                }
                let raw = read_file(&entry.path(), 8 * 1024 * 1024)?
                    .ok_or("migration_source_changed_or_large")?;
                bytes += raw.len();
                if bytes > MAX_FILE {
                    return Err("migration_store_too_large".into());
                }
                let value: Value =
                    serde_json::from_str(&raw).map_err(|_| "migration_schema_invalid")?;
                if value.get("id").and_then(Value::as_str) != Some(id) {
                    return Err("migration_schema_invalid".into());
                }
                profiles.push(value);
            }
            let value = json!({ "schemaVersion": 1, "profiles": profiles });
            crate::core::import_model::validate_document(StoreKind::Profiles, &value, &api_owner)?;
            docs.insert(StoreKind::Profiles, value);
        }
    }
    if selected.contains(&LegacyApp::DeveloperToolbox) {
        let source = root.join(LegacyApp::DeveloperToolbox.identifier());
        if source.exists() {
            devbox_filesystem::ensure_no_links(&source).map_err(|_| "migration_path_invalid")?;
        }
        insert_json(
            &mut docs,
            StoreKind::Workflows,
            &source,
            "smart-workflows.json",
        )?;
    }
    if selected.contains(&LegacyApp::ApiPlayground) {
        let source = root.join(LegacyApp::ApiPlayground.identifier());
        if source.exists() {
            devbox_filesystem::ensure_no_links(&source).map_err(|_| "migration_path_invalid")?;
        }
        for (relative, kind, owner_kind) in [
            ("oauth/mcp-grants.json", StoreKind::OAuth, "oauth"),
            ("grpc/tls-credentials.json", StoreKind::GrpcTls, "grpc-tls"),
        ] {
            if let Some(raw) = read_file(&source.join(relative), MAX_FILE)? {
                let (value, missing) = api_playground_lib::component::prepare_legacy_native_store(
                    owner_kind,
                    raw.as_bytes(),
                )?;
                if !missing.is_empty() {
                    issues.push(Issue {
                        store: kind.key().into(),
                        code: "secret-reconnect-required".into(),
                        count: missing.len(),
                    });
                }
                docs.insert(kind, value);
            }
        }
    }
    Ok((docs, issues))
}
pub const COMMANDS: &[&str] = &[
    "migration_status",
    "prepare_migration",
    "apply_migration",
    "rollback_migration",
    "acknowledge_migration",
    "cancel_migration",
    "finish_startup",
    "request_import_on_restart",
];
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PrepareInput {
    operation_id: String,
    sources: Vec<LegacyApp>,
    browser: BrowserState,
    profile_ids: Option<Vec<String>>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ApplyInput {
    operation_id: String,
    id: String,
    browser: BrowserState,
}
fn decode<T: serde::de::DeserializeOwned>(args: Value) -> Result<T, String> {
    serde_json::from_value(args).map_err(|_| "migration_request_invalid".into())
}
fn encode<T: Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|_| "migration_response_invalid".into())
}
fn no_args(args: &Value) -> Result<(), String> {
    if args.as_object().is_some_and(|object| object.is_empty()) {
        Ok(())
    } else {
        Err("migration_request_invalid".into())
    }
}
pub async fn dispatch(app: &tauri::AppHandle, method: &str, args: Value) -> Result<Value, String> {
    let state = app.state::<MigrationState>();
    match method {
        "migration_status" => {
            no_args(&args)?;
            let work = state.work.lock().map_err(|_| io_error())?;
            if work.current.is_some() {
                return Ok(
                    json!({ "busy": true, "stage": work.stage, "operationId": work.current.as_ref().map(|(id, _)| id) }),
                );
            }
            let config = flags(&state.root)?;
            let repo = state.repository()?;
            let pending = repo.pending()?;
            let sources = source_status(&state.legacy_root);
            let review_needed = state.force_import
                || config.request_import
                || (!config.setup_done && sources.iter().any(|source| source.present));
            Ok(
                json!({ "busy": false, "stage": work.stage, "active": state.active.load(Ordering::Acquire), "reviewNeeded": review_needed, "pending": pending, "sources": sources, "profiles": profile_choices(&state.legacy_root)? }),
            )
        }
        "prepare_migration" => {
            let input: PrepareInput = decode(args)?;
            if input.sources.is_empty()
                || input.sources.len() > 3
                || LEGACY.iter().any(|source| {
                    input
                        .sources
                        .iter()
                        .filter(|value| *value == source)
                        .count()
                        > 1
                })
                || input.profile_ids.as_ref().is_some_and(|ids| {
                    ids.len() > 64
                        || ids.iter().any(|id| {
                            !uuid::Uuid::parse_str(id).is_ok_and(|value| value.to_string() == *id)
                        })
                })
            {
                return Err("migration_request_invalid".into());
            }
            let guard = state.begin(input.operation_id)?;
            let mut repo = state.repository()?;
            if repo.pending()?.is_some() {
                return Err("migration_recovery_required".into());
            }
            let (id, stage) = repo.new_stage()?;
            let result = async {
                let repo = &mut repo;
                guard.stage("native-stores");
                let source_root = state.legacy_root.clone();
                let selected = input.sources.clone();
                let profiles = input.profile_ids.clone();
                let (mut documents, mut issues) = tauri::async_runtime::spawn_blocking(move || {
                    native_sources(&source_root, &selected, &profiles)
                })
                .await
                .map_err(|_| "migration_source_unavailable".to_string())??;
                if input.sources.contains(&LegacyApp::ApiPlayground) {
                    let source = state
                        .legacy_root
                        .join(LegacyApp::ApiPlayground.identifier());
                    if [
                        "EBWebView/Default/Local Storage/leveldb",
                        "Default/Local Storage/leveldb",
                    ]
                    .iter()
                    .any(|relative| source.join(relative).exists())
                    {
                        guard.stage("api-snapshot");
                        let copy_stage = stage.clone();
                        let cancelled = Arc::clone(&guard.cancelled);
                        let (_, receipt) = tauri::async_runtime::spawn_blocking(move || {
                            crate::platform::legacy_profile::snapshot(
                                &source,
                                &copy_stage,
                                &cancelled,
                            )
                        })
                        .await
                        .map_err(|_| "migration_source_unavailable".to_string())??;
                        devbox_filesystem::atomic_write(
                            stage.join("closed-source.json"),
                            &serde_json::to_vec(&receipt)
                                .map_err(|_| "migration_response_invalid")?,
                        )
                        .map_err(|_| io_error())?;
                        let nonce = uuid::Uuid::new_v4().simple().to_string();
                        crate::migration_export::write_ticket(&stage, &nonce)?;
                        guard.stage("api-export");
                        let exported = crate::platform::legacy_profile::run_export_worker(
                            &stage,
                            &id,
                            &nonce,
                            &guard.cancelled,
                            |stage| guard.stage(stage),
                        )
                        .await;
                        // The worker has closed its owned Job before returning.
                        // Purge the raw copy even when export or cancellation failed.
                        guard.stage(match exported.as_ref().err().map(String::as_str) {
                            None => "api-export-copy-cleanup",
                            Some("legacy_worker_cleanup_failed") => "api-export-job-cleanup-failed",
                            Some("legacy_worker_failed") => "api-export-process-failed",
                            Some("legacy_worker_timeout") => "api-export-timeout",
                            Some(
                                "legacy_worker_unavailable" | "owned_worker_assignment_failed",
                            ) => "api-export-launch-failed",
                            Some(_) => "api-export-result-failed",
                        });
                        let cleanup = repo.clear_export_copy(&id);
                        let exported = exported?;
                        if let Err(error) = &cleanup {
                            guard.stage(match error.as_str() {
                                "migration_cleanup_readonly" => "api-export-copy-readonly",
                                "migration_cleanup_access_denied" => {
                                    "api-export-copy-access-denied"
                                }
                                "migration_cleanup_changed" => "api-export-copy-changed",
                                "migration_path_invalid" => "api-export-copy-path-rejected",
                                "migration_store_too_large" => "api-export-copy-entry-limit",
                                _ => "api-export-copy-cleanup-failed",
                            });
                        }
                        cleanup?;
                        for (kind, value) in [
                            (StoreKind::Collections, exported.collections),
                            (StoreKind::History, exported.history),
                            (StoreKind::Environments, exported.environments),
                            (StoreKind::GrpcHistory, exported.grpc_history),
                        ] {
                            if let Some(value) = value {
                                documents.insert(kind, value);
                            }
                        }
                        issues.extend(exported.notices.into_iter().map(|notice| Issue {
                            store: notice.store,
                            code: notice.code,
                            count: notice.count as usize,
                        }));
                    }
                }
                if documents.is_empty() && issues.is_empty() {
                    return Err("migration_no_sources".into());
                }
                guard.stage("merge-plan");
                let review =
                    repo.prepare(&id, &documents, input.browser, &guard.cancelled, api_owner)?;
                Ok(json!({ "review": review, "issues": issues }))
            }
            .await;
            // Remove partial acquisition data as well as completed exports.
            // An accepted/reviewed plan only needs its normalized snapshot.
            let cleanup = repo.clear_export_copy(&id);
            match result {
                Ok(value) => {
                    cleanup?;
                    Ok(value)
                }
                Err(error) => Err(error),
            }
        }
        "apply_migration" => {
            let input: ApplyInput = decode(args)?;
            let guard = state.begin(input.operation_id)?;
            let repo = state.repository()?;
            encode(repo.activate(&input.id, &input.browser, &guard.cancelled)?)
        }
        "rollback_migration" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Input {
                operation_id: String,
                id: String,
            }
            let input: Input = decode(args)?;
            let _guard = state.begin(input.operation_id)?;
            encode(state.repository()?.rollback(&input.id)?)
        }
        "acknowledge_migration" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Input {
                operation_id: String,
                id: String,
                browser: BrowserState,
                rollback: bool,
            }
            let input: Input = decode(args)?;
            let _guard = state.begin(input.operation_id)?;
            state
                .repository()?
                .acknowledge(&input.id, &input.browser, input.rollback)?;
            Ok(Value::Null)
        }
        "cancel_migration" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Input {
                operation_id: String,
            }
            let input: Input = decode(args)?;
            state.cancel(input.operation_id)?;
            Ok(Value::Null)
        }
        "finish_startup" => {
            no_args(&args)?;
            let work = state.work.lock().map_err(|_| io_error())?;
            if work.current.is_some() {
                return Err("migration_busy".into());
            }
            if state.repository()?.pending()?.is_some() {
                return Err("migration_recovery_required".into());
            }
            save_flags(
                &state.root,
                &StartupFlags {
                    schema_version: 1,
                    setup_done: true,
                    request_import: false,
                },
            )?;
            state.active.store(true, Ordering::Release);
            Ok(Value::Null)
        }
        "request_import_on_restart" => {
            no_args(&args)?;
            save_flags(
                &state.root,
                &StartupFlags {
                    schema_version: 1,
                    setup_done: true,
                    request_import: true,
                },
            )?;
            app.restart()
        }
        _ => Err("migration_request_invalid".into()),
    }
}
/// Only closed fixed codes cross the product IPC boundary; source paths and
/// native error text never become renderer-visible diagnostics.
pub fn issue(error: &str) -> &'static str {
    match error {
        "legacy_app_must_be_closed" => "source-open",
        "legacy_snapshot_cancelled" | "snapshot cancelled" | "migration cancelled" => "cancelled",
        "migration_busy" => "busy",
        "migration_recovery_required" => "recovery-required",
        "migration_destination_changed" | "destination changed; review a new plan" => {
            "destination-changed"
        }
        "migration_store_too_large"
        | "legacy_store_too_large"
        | "migration_source_changed_or_large" => "source-large-or-changed",
        "migration_schema_invalid"
        | "legacy_api_storage_invalid"
        | "migration_journal_future_schema"
        | "unsupported source schema" => "schema-invalid",
        "legacy_api_secret_reconnect_required" => "secret-reconnect-required",
        "migration_no_sources" => "no-sources",
        "legacy_browser_export_requires_windows" => "windows-required",
        "migration_startup_required" => "restart-required",
        _ => "unavailable",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> MigrationState {
        MigrationState {
            root: PathBuf::new(),
            legacy_root: PathBuf::new(),
            installation: "fixture".into(),
            active: AtomicBool::new(false),
            force_import: false,
            work: Arc::new(Mutex::new(Work::default())),
        }
    }
    #[test]
    fn cancellation_before_admission_busy_ownership_and_started_phase_are_preserved() {
        let state = state();
        let cancelled = uuid::Uuid::new_v4().to_string();
        state.cancel(cancelled.clone()).unwrap();
        assert!(state.begin(cancelled).is_err());
        let id = uuid::Uuid::new_v4().to_string();
        let guard = state.begin(id.clone()).unwrap();
        assert!(state.begin(uuid::Uuid::new_v4().to_string()).is_err());
        state.cancel(id).unwrap();
        assert!(guard.cancelled.load(Ordering::Acquire));
        drop(guard);
        let next = state.begin(uuid::Uuid::new_v4().to_string()).unwrap();
        drop(next);
        state.active.store(true, Ordering::Release);
        assert!(state.begin(uuid::Uuid::new_v4().to_string()).is_err());
    }
    #[test]
    fn windows_native_source_fixture_obeys_the_real_domain_codecs() {
        let root = tempfile::tempdir().unwrap();
        let fixtures: Value =
            serde_json::from_str(include_str!("../fixtures/legacy-native.json")).unwrap();
        let webhook = root.path().join(LegacyApp::WebhookLab.identifier());
        let toolbox = root.path().join(LegacyApp::DeveloperToolbox.identifier());
        fs::create_dir_all(webhook.join("service-profiles")).unwrap();
        fs::create_dir_all(&toolbox).unwrap();
        fs::write(
            webhook.join("fixtures.json"),
            serde_json::to_vec(&fixtures["fixtures"]).unwrap(),
        )
        .unwrap();
        let profile = &fixtures["profile"];
        fs::write(
            webhook
                .join("service-profiles")
                .join(format!("{}.json", profile["id"].as_str().unwrap())),
            serde_json::to_vec(profile).unwrap(),
        )
        .unwrap();
        let workflow_path = toolbox.join("smart-workflows.json");
        let original = serde_json::to_vec(&fixtures["workflows"]).unwrap();
        fs::write(&workflow_path, &original).unwrap();
        let (documents, issues) = native_sources(
            root.path(),
            &[LegacyApp::WebhookLab, LegacyApp::DeveloperToolbox],
            &None,
        )
        .unwrap();
        assert_eq!(documents.len(), 3);
        assert!(issues.is_empty());
        assert_eq!(fs::read(&workflow_path).unwrap(), original);
        let mut invalid = fixtures["workflows"].clone();
        invalid["pipelines"][0]["inputType"] = json!("text");
        fs::write(&workflow_path, serde_json::to_vec(&invalid).unwrap()).unwrap();
        assert!(native_sources(root.path(), &[LegacyApp::DeveloperToolbox], &None).is_err());
    }
}
