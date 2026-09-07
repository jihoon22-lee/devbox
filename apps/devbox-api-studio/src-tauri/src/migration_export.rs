//! A disposable API storage exporter. The parent owns its process tree and fixes
//! its WebView2 environment to a consistent owned copy, never the legacy profile.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::{Manager, WebviewWindow};
const MAX_BYTES: usize = 20 * 1024 * 1024;
const LABEL: &str = "legacy-api-export";
const EMPTY_ENVIRONMENT: &str = r#"{"version":1,"environments":[]}"#;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegacyApiExport {
    pub schema_version: u32,
    pub collections: Option<Value>,
    pub history: Option<Value>,
    pub environments: Option<Value>,
    pub grpc_history: Option<Value>,
    pub notices: Vec<ExportNotice>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportNotice {
    pub store: String,
    pub code: String,
    pub count: u64,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkerTicket {
    schema_version: u32,
    nonce: String,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkerResult {
    schema_version: u32,
    nonce: String,
    data: LegacyApiExport,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportRequest {
    nonce: String,
    action: ExportAction,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum ExportAction {
    Open {},
    Environment { raw: Option<String> },
    Sanitize { serialized: String },
    Complete { data: LegacyApiExport },
    Failed {},
}
struct Session {
    verified_profile: bool,
    completed: bool,
    environment: Option<String>,
    environment_present: bool,
    sanitized_bytes: usize,
}
struct ExportState {
    stage: PathBuf,
    nonce: String,
    started: Instant,
    session: Mutex<Session>,
}
fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    devbox_filesystem::ensure_no_links(path).map_err(|_| "legacy_export_storage_invalid")?;
    let (mut file, identity) = devbox_filesystem::open_filesystem_object(path, false)
        .map_err(|_| "legacy_export_storage_invalid")?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "legacy_export_storage_invalid")?;
    if bytes.len() > MAX_BYTES
        || devbox_filesystem::filesystem_identity(path, false)
            .map_err(|_| "legacy_export_storage_invalid")?
            != identity
    {
        return Err("legacy_export_storage_invalid".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "legacy_export_storage_invalid".into())
}
pub fn write_ticket(stage: &Path, nonce: &str) -> Result<(), String> {
    if !nonce_valid(nonce) {
        return Err("legacy_export_ticket_invalid".into());
    }
    devbox_filesystem::ensure_no_links(stage).map_err(|_| "legacy_export_storage_invalid")?;
    let path = stage.join("worker-ticket.json");
    if path.exists() {
        return Err("legacy_export_ticket_exists".into());
    }
    let bytes = serde_json::to_vec(&WorkerTicket {
        schema_version: 1,
        nonce: nonce.into(),
    })
    .map_err(|_| "legacy_export_ticket_invalid")?;
    devbox_filesystem::atomic_write(path, &bytes)
        .map_err(|_| "legacy_export_storage_invalid".into())
}
#[cfg_attr(not(windows), allow(dead_code))]
pub fn read_result(stage: &Path, nonce: &str) -> Result<LegacyApiExport, String> {
    let result: WorkerResult = read(&stage.join("worker-result.json"))?;
    if result.schema_version != 1 || result.nonce != nonce {
        return Err("legacy_export_result_invalid".into());
    }
    validate_shape(&result.data)?;
    Ok(result.data)
}
fn nonce_valid(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn validate_shape(data: &LegacyApiExport) -> Result<(), String> {
    if data.schema_version != 1
        || data.notices.len() > 16
        || serde_json::to_vec(data).map_or(true, |bytes| bytes.len() > MAX_BYTES)
    {
        return Err("legacy_export_result_invalid".into());
    }
    for (value, field, version) in [
        (&data.collections, "collections", 2),
        (&data.history, "history", 2),
        (&data.environments, "environments", 1),
    ] {
        if let Some(value) = value {
            if value.get("version").and_then(Value::as_u64) != Some(version)
                || value
                    .get(field)
                    .and_then(Value::as_array)
                    .is_none_or(|records| records.len() > 10_000)
            {
                return Err("legacy_export_result_invalid".into());
            }
        }
    }
    if let Some(grpc) = &data.grpc_history {
        if grpc.get("schema").and_then(Value::as_str)
            != Some("devbox.api-playground.grpc-history/v1")
            || grpc
                .get("entries")
                .and_then(Value::as_array)
                .is_none_or(|entries| entries.len() > 50)
        {
            return Err("legacy_export_result_invalid".into());
        }
    }
    if data.notices.iter().any(|notice| {
        !["collections", "history", "environments", "grpc-history"].contains(&notice.store.as_str())
            || ![
                "legacy-history-excluded",
                "records-excluded",
                "values-redacted",
                "secret-reconnect-required",
            ]
            .contains(&notice.code.as_str())
            || notice.count > 10_000
    }) {
        return Err("legacy_export_result_invalid".into());
    }
    Ok(())
}
fn local_export_url(url: &tauri::Url) -> bool {
    url.path() == "/migration-export.html"
        && ((url.scheme() == "http" && url.host_str() == Some("tauri.localhost"))
            || (url.scheme() == "tauri" && url.host_str() == Some("localhost")))
}
#[tauri::command]
pub async fn legacy_export_message(
    window: WebviewWindow,
    request: ExportRequest,
) -> Result<Value, String> {
    if window.label() != LABEL || !window.url().is_ok_and(|url| local_export_url(&url)) {
        return Err("legacy_export_caller_rejected".into());
    }
    tauri::async_runtime::spawn_blocking(move || handle(&window, request))
        .await
        .map_err(|_| "legacy_export_failed".to_string())?
}
fn handle(window: &WebviewWindow, request: ExportRequest) -> Result<Value, String> {
    let app = window.app_handle();
    let state = app
        .try_state::<ExportState>()
        .ok_or("legacy_export_caller_rejected")?;
    if request.nonce != state.nonce || state.started.elapsed() > Duration::from_secs(60) {
        return Err("legacy_export_caller_rejected".into());
    }
    let mut session = state.session.lock().map_err(|_| "legacy_export_failed")?;
    if session.completed {
        return Err("legacy_export_replayed".into());
    }
    if matches!(request.action, ExportAction::Open {}) {
        return Ok(json!({ "ready": session.verified_profile }));
    }
    if !session.verified_profile {
        return Err("legacy_export_profile_unverified".into());
    }
    match request.action {
        ExportAction::Environment { raw } if session.environment.is_none() => {
            let present = raw.is_some();
            let (safe, missing) = api_playground_lib::component::prepare_legacy_environment(
                raw.as_deref().unwrap_or(EMPTY_ENVIRONMENT),
            )?;
            session.environment = Some(safe.clone());
            session.environment_present = present;
            Ok(
                json!({ "serialized": if present { Some(safe) } else { None }, "missingSecrets": missing }),
            )
        }
        ExportAction::Sanitize { serialized } => {
            session.sanitized_bytes = session
                .sanitized_bytes
                .checked_add(serialized.len())
                .filter(|value| *value <= MAX_BYTES * 4)
                .ok_or("legacy_export_too_large")?;
            let environment = session
                .environment
                .as_ref()
                .ok_or("legacy_export_phase_invalid")?;
            api_playground_lib::component::sanitize_legacy_json(serialized, environment)
                .map(Value::String)
        }
        ExportAction::Complete { data } => {
            validate_shape(&data)?;
            let environment = session
                .environment
                .as_ref()
                .ok_or("legacy_export_phase_invalid")?;
            let expected_environment: Value =
                serde_json::from_str(environment).map_err(|_| "legacy_export_phase_invalid")?;
            if data.environments.as_ref()
                != session.environment_present.then_some(&expected_environment)
            {
                return Err("legacy_export_environment_mismatch".into());
            }
            for value in [&data.collections, &data.history].into_iter().flatten() {
                let serialized =
                    serde_json::to_string(value).map_err(|_| "legacy_export_result_invalid")?;
                let checked =
                    api_playground_lib::component::sanitize_legacy_json(serialized, environment)?;
                if serde_json::from_str::<Value>(&checked)
                    .map_err(|_| "legacy_export_result_invalid")?
                    != *value
                {
                    return Err("legacy_export_redaction_mismatch".into());
                }
            }
            let path = state.stage.join("worker-result.json");
            if path.exists() {
                return Err("legacy_export_replayed".into());
            }
            let bytes = serde_json::to_vec(&WorkerResult {
                schema_version: 1,
                nonce: state.nonce.clone(),
                data,
            })
            .map_err(|_| "legacy_export_result_invalid")?;
            devbox_filesystem::atomic_write(path, &bytes)
                .map_err(|_| "legacy_export_storage_invalid")?;
            session.completed = true;
            drop(session);
            app.exit(0);
            Ok(Value::Null)
        }
        ExportAction::Failed {} => {
            session.completed = true;
            drop(session);
            app.exit(2);
            Ok(Value::Null)
        }
        _ => Err("legacy_export_phase_invalid".into()),
    }
}

/// Strict owned-worker mode; no ordinary application startup or single-instance
/// routing occurs here. The ticket and profile live only in the parent's stage.
pub fn worker_argument(args: &[String]) -> Result<Option<String>, String> {
    if !args
        .iter()
        .any(|arg| arg.starts_with("--migration-export-worker"))
    {
        return Ok(None);
    }
    if args.len() != 2
        || args[0] != "--migration-export-worker"
        || uuid::Uuid::parse_str(&args[1]).is_err()
        || args[1].len() != 36
    {
        return Err("legacy_worker_arguments_invalid".into());
    }
    Ok(Some(args[1].clone()))
}
pub fn run_worker(stage_id: String, mut context: tauri::Context<tauri::Wry>) -> tauri::Result<()> {
    product_shell_tauri::isolate_installation(&mut context)?;
    context.config_mut().app.windows.clear();
    tauri::Builder::default()
        .plugin(tauri::plugin::Builder::<tauri::Wry, ()>::new("api-studio").invoke_handler(tauri::generate_handler![legacy_export_message]).build())
        .setup(move |app| {
            let stage = app.path().app_local_data_dir()?.join("imports/staging").join(&stage_id);
            devbox_filesystem::ensure_no_links(&stage).map_err(|_| std::io::Error::other("legacy_export_storage_invalid"))?;
            let ticket: WorkerTicket = read(&stage.join("worker-ticket.json")).map_err(std::io::Error::other)?;
            if ticket.schema_version != 1 || !nonce_valid(&ticket.nonce) { return Err(std::io::Error::other("legacy_export_ticket_invalid").into()); }
            fs::OpenOptions::new().write(true).create_new(true).open(stage.join("worker-started"))?;
            let copy = stage.join("webview-copy");
            devbox_filesystem::ensure_no_links(&copy).map_err(|_| std::io::Error::other("legacy_export_storage_invalid"))?;
            let nonce = ticket.nonce;
            let script = format!("Object.defineProperty(window, '__DEVBOX_API_EXPORT__', {{ value: {}, writable: false }});", serde_json::to_string(&nonce)?);
            app.manage(ExportState { stage, nonce, started: Instant::now(), session: Mutex::new(Session { verified_profile: false, completed: false, environment: None, environment_present: false, sanitized_bytes: 0 }) });
            let window = tauri::WebviewWindowBuilder::new(app, LABEL, tauri::WebviewUrl::App("migration-export.html".into()))
                .data_directory(copy.clone()).visible(false).skip_taskbar(true).focused(false)
                .initialization_script(script).on_navigation(local_export_url).build()?;
            let handle = app.handle().clone();
            crate::platform::legacy_profile::verify_profile(&window, copy, move |result| {
                if result.is_err() { handle.exit(2); return; }
                if let Some(state) = handle.try_state::<ExportState>() {
                    if let Ok(mut session) = state.session.lock() { session.verified_profile = true; }
                }
            })?;
            Ok(())
        }).run(context)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn worker_args_cannot_select_paths_and_current_raw_history_is_not_a_valid_export() {
        assert_eq!(worker_argument(&["--route=requests".into()]).unwrap(), None);
        assert!(
            worker_argument(&["--migration-export-worker".into(), "../source".into()]).is_err()
        );
        assert!(worker_argument(&[
            "--migration-export-worker".into(),
            uuid::Uuid::new_v4().to_string()
        ])
        .unwrap()
        .is_some());
        let data = LegacyApiExport {
            schema_version: 1,
            collections: None,
            history: Some(json!({ "version": 1, "history": [] })),
            environments: None,
            grpc_history: None,
            notices: vec![],
        };
        assert!(validate_shape(&data).is_err());
    }
}
