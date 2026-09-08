//! Closed native owner registry; selecting a route never widens a service role.
use product_contract::{Operation, OperationState, Problem, ProblemCode, Provenance, RouteRequest};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};
use tauri::{Manager, State, WebviewWindow};
const MAX_ARGUMENT_BYTES: usize = 20 * 1024 * 1024;
#[derive(Default)]
struct Active(Arc<Mutex<HashSet<String>>>);
struct Reservation {
    active: Arc<Mutex<HashSet<String>>>,
    id: String,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        if let Ok(mut active) = self.active.lock() {
            active.remove(&self.id);
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    header: RouteRequest,
    component: String,
    method: String,
    args: Value,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Response {
    operation: Operation,
    value: Value,
}
const SEARCH_SETTINGS: &[&str] = &[
    "add_root",
    "remove_root",
    "index_now",
    "cancel_index",
    "save_saved_query",
    "delete_saved_query",
];
const OPENERS: &[&str] = &["open_file", "reveal_file", "open_targets", "open_in"];
fn allowed(component: &str, route: &str, method: &str) -> bool {
    match component {
        "knowledge.migration" => {
            route == "notes"
                && (crate::startup::COMMANDS.contains(&method)
                    || crate::migration::METHODS.contains(&method)
                    || crate::vault_binding::METHODS.contains(&method))
        }
        "knowledge.notes" => {
            matches!(route, "notes" | "daily")
                && method != "daily_note"
                && (knowledge_base_lib::component::COMMANDS.contains(&method)
                    || knowledge_base_lib::component::DAILY_METHODS.contains(&method)
                    || matches!(method, "read_clipboard_text" | "open_external_url"))
        }
        "knowledge.activity" => {
            route == "activity"
                && (life_log_lib::component::COMMANDS.contains(&method)
                    || crate::lifecycle::METHODS.contains(&method))
        }
        "knowledge.search" => {
            route == "search"
                && (everything_plus_lib::component::COMMANDS.contains(&method)
                    || crate::search::METHODS.contains(&method))
                && !matches!(method, "search_files" | "search_content")
                && !SEARCH_SETTINGS.contains(&method)
                && !OPENERS.contains(&method)
        }
        "knowledge.search-settings" => route == "search" && SEARCH_SETTINGS.contains(&method),
        "knowledge.opener" => route == "search" && OPENERS.contains(&method),
        _ => false,
    }
}
pub(crate) fn issue(error: &str) -> &'static str {
    match error {
        "component_args_invalid" => "invalid_request",
        "setup_required" => "setup_required",
        "store_busy" | "digest_busy" | "search_busy" => "busy",
        "search_stale" => "search_stale",
        "vault_change_invalid"
        | "vault_change_save_failed"
        | "vault_change_stale"
        | "vault_change_unavailable"
        | "vault_change_conflict"
        | "vault_change_same"
        | "vault_change_timeout" => match error {
            "vault_change_invalid" => "vault_change_invalid",
            "vault_change_save_failed" => "vault_change_save_failed",
            "vault_change_stale" => "vault_change_stale",
            "vault_change_unavailable" => "vault_change_unavailable",
            "vault_change_conflict" => "vault_change_conflict",
            "vault_change_same" => "vault_change_same",
            _ => "vault_change_timeout",
        },
        "vault_change_future" => "future_schema",
        "vault_change_cancelled" => "cancelled",
        "store_future_schema" => "future_schema",
        "store_manifest_invalid" | "store_path_invalid" => "store_invalid",
        "activity_consent_save_failed" => "consent_save_failed",
        "autostart_owner_conflict" => "autostart_owner_conflict",
        "provider_unavailable" => "provider_unavailable",
        "vault_binding_unavailable" => "vault_binding_unavailable",
        "component_initialization_failed" | "component_state_conflict" => "restart_required",
        "digest_cancelled" => "cancelled",
        "daily_preview_stale" => "preview_stale",
        "daily_target_exists" => "target_exists",
        "daily_vault_unavailable" => "vault_unavailable",
        "tray_unavailable" => "tray_unavailable",
        "close_policy_save_failed" => "close_policy_save_failed",
        "import_cancelled" | "snapshot cancelled" => "cancelled",
        "import_schema_unsupported" | "unsupported source schema" => "import_schema_unsupported",
        "import_preview_stale" | "import_job_stale" => "preview_stale",
        "import_source_changed" => "import_source_changed",
        "import_restart_required" => "import_restart_required",
        "import_limit_exceeded"
        | "import_storage_limit"
        | "source exceeds limit"
        | "snapshot exceeds limit" => "import_limit_exceeded",
        "import_disk_full" => "import_disk_full",
        "import_plan_invalid" | "import_path_invalid" => "store_invalid",
        "import_row_invalid" | "import_database_invalid" => "import_invalid",
        "legacy_writer_active" => "legacy_writer_active",
        "vault_owner_busy" => "vault_owner_busy",
        "vault_binding_invalid" | "vault_owner_unavailable" => "vault_binding_unavailable",
        "import_timed_out" | "snapshot timed out; quiesce source and retry" => "import_timed_out",
        _ if error.contains("만료") => "preview_expired",
        _ if error.contains("미리보기") && error.contains("오래") => "preview_stale",
        _ => "operation_failed",
    }
}
/// A disconnected vault can block an OS filesystem call. Keep those calls off
/// the shared IPC executor so Activity and indexed Search remain responsive.
async fn notes_dispatch(
    app: &tauri::AppHandle,
    method: &str,
    args: Value,
) -> Result<Value, String> {
    let app = app.clone();
    let method = method.to_owned();
    tauri::async_runtime::spawn_blocking(move || {
        if knowledge_base_lib::component::DAILY_METHODS.contains(&method.as_str()) {
            knowledge_base_lib::component::daily_dispatch(&app, &method, args)
        } else {
            tauri::async_runtime::block_on(knowledge_base_lib::component::dispatch(
                &app, &method, args,
            ))
        }
    })
    .await
    .map_err(|_| "component_worker_unavailable")?
}
#[tauri::command]
async fn execute(
    window: WebviewWindow,
    active: State<'_, Active>,
    request: Request,
) -> Result<Response, Problem> {
    let rejected = |code| Problem {
        code,
        provenance: Provenance {
            product: "knowledge".into(),
            component: "knowledge.dispatch".into(),
            request_id: "rejected".into(),
            revision: 1,
        },
    };
    if !allowed(&request.component, &request.header.route, &request.method)
        || !request.args.is_object()
        || serde_json::to_vec(&request.args).map_or(true, |bytes| bytes.len() > MAX_ARGUMENT_BYTES)
    {
        return Err(rejected(ProblemCode::InvalidRequest));
    }
    let provenance = product_shell_tauri::authorize(&window, &request.header, &request.component)?;
    let problem = |code| Problem {
        code,
        provenance: provenance.clone(),
    };
    let _reservation = {
        let mut ids = active
            .0
            .lock()
            .map_err(|_| problem(ProblemCode::Unavailable))?;
        if ids.contains(&request.header.request_id) {
            return Err(problem(ProblemCode::Replayed));
        }
        if ids.len() >= 64 {
            return Err(problem(ProblemCode::Overloaded));
        }
        ids.insert(request.header.request_id.clone());
        Reservation {
            active: active.0.clone(),
            id: request.header.request_id.clone(),
        }
    };
    let app = window.app_handle();
    if request.component != "knowledge.migration" {
        crate::startup::require_active(app).map_err(|_| problem(ProblemCode::Unavailable))?;
    }
    let value = match request.component.as_str() {
        "knowledge.migration" => crate::startup::dispatch(app, &request.method, request.args),
        "knowledge.notes"
            if knowledge_base_lib::component::DAILY_METHODS.contains(&request.method.as_str()) =>
        {
            notes_dispatch(app, &request.method, request.args).await
        }
        "knowledge.notes" => match request.method.as_str() {
            "read_clipboard_text" => {
                use tauri_plugin_clipboard_manager::ClipboardExt;
                if request.args.as_object().is_some_and(|args| args.is_empty()) {
                    app.clipboard()
                        .read_text()
                        .map(Value::String)
                        .map_err(|_| "clipboard_unavailable".into())
                } else {
                    Err("component_args_invalid".into())
                }
            }
            "open_external_url" => {
                use tauri_plugin_opener::OpenerExt;
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Url {
                    url: String,
                }
                match serde_json::from_value::<Url>(request.args) {
                    Ok(value)
                        if value.url.len() <= 8192
                            && !value.url.chars().any(char::is_control)
                            && (value.url.starts_with("https://")
                                || value.url.starts_with("http://")) =>
                    {
                        app.opener()
                            .open_url(value.url, None::<&str>)
                            .map(|_| Value::Null)
                            .map_err(|_| "opener_unavailable".into())
                    }
                    _ => Err("component_args_invalid".into()),
                }
            }
            "set_root" => Err("vault_binding_unavailable".into()),
            "open_targets" => Ok(json!([])),
            "open_in" => Err("provider_unavailable".into()),
            _ => notes_dispatch(app, &request.method, request.args).await,
        },
        "knowledge.activity" if crate::lifecycle::METHODS.contains(&request.method.as_str()) => {
            crate::lifecycle::dispatch(app, &request.method, request.args)
        }
        "knowledge.activity" if request.method == "send_digest_to_knowledge" => {
            life_log_lib::component::send_product_draft(app, request.args, |draft| {
                knowledge_base_lib::component::offer_product_draft(app, draft)
            })
            .await
        }
        "knowledge.activity" => {
            life_log_lib::component::dispatch(app, &request.method, request.args)
                .await
                .map(|value| crate::search::associate_activity(app, &request.method, value))
        }
        "knowledge.opener" if request.method == "open_targets" => Ok(json!([])),
        "knowledge.opener" if request.method == "open_in" => Err("provider_unavailable".into()),
        "knowledge.opener" => crate::search::open(app, &request.method, request.args).await,
        "knowledge.search" if crate::search::METHODS.contains(&request.method.as_str()) => {
            crate::search::dispatch(app, &request.method, request.args)
        }
        "knowledge.search" | "knowledge.search-settings" => {
            everything_plus_lib::component::dispatch(app, &request.method, request.args).await
        }
        _ => Err("component_method_invalid".into()),
    };
    let (outcome, value) = match value {
        Ok(value) => (OperationState::Succeeded {}, value),
        Err(error) => {
            let code = issue(&error);
            (
                if code == "cancelled" {
                    OperationState::Cancelled {}
                } else {
                    OperationState::Failed {
                        code: ProblemCode::Unavailable,
                    }
                },
                json!({"issue": code}),
            )
        }
    };
    Ok(Response {
        operation: Operation {
            provenance,
            outcome,
        },
        value,
    })
}
pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("knowledge")
        .invoke_handler(tauri::generate_handler![execute])
        .setup(|app, _| {
            app.manage(Active::default());
            crate::lifecycle::initialize(app);
            crate::startup::initialize(app).map_err(Into::into)
        })
        .on_event(crate::lifecycle::on_event)
        .build()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn query_service_cannot_mutate_notes_index_settings_or_launch() {
        assert!(allowed("knowledge.search", "search", "source_query"));
        for method in [
            "search_files",
            "search_content",
            "schedule_vault_change",
            "apply_vault_change",
            "write_file",
            "set_root",
            "start_tracking",
            "add_root",
            "index_now",
            "save_saved_query",
            "open_file",
            "open_in",
        ] {
            assert!(!allowed("knowledge.search", "search", method), "{method}");
        }
        assert!(allowed("knowledge.search-settings", "search", "add_root"));
        assert!(allowed("knowledge.opener", "search", "open_file"));
        assert!(!allowed("knowledge.activity", "activity", "write_file"));
        assert!(!allowed("knowledge.notes", "activity", "write_file"));
        assert!(!allowed("api-studio.api", "notes", "write_file"));
        assert!(!allowed("knowledge.notes", "daily", "daily_note"));
        assert!(allowed("knowledge.notes", "daily", "preview_daily"));
    }
    #[test]
    fn error_responses_never_return_raw_paths_or_user_values() {
        assert_eq!(
            issue("could not read C:/users/private/secret-note.md"),
            "operation_failed"
        );
        assert_eq!(issue("activity_consent_save_failed"), "consent_save_failed");
    }
}
