use product_contract::operation_log::{Entry, OperationLog, Outcome};
use std::path::Path;
pub(super) enum Stage {
    Host,
    Components,
    Engine,
    Processes,
    LogProvider,
    Logs,
    Query,
    Dispatch,
}
impl Stage {
    fn method(&self) -> &'static str {
        match self {
            Self::Host => "initialize_host",
            Self::Components => "initialize_components",
            Self::Engine => "initialize_engine",
            Self::Processes => "initialize_processes",
            Self::LogProvider => "initialize_log_provider",
            Self::Logs => "initialize_logs",
            Self::Query => "runtime_query",
            Self::Dispatch => "runtime_dispatch",
        }
    }
}
fn fixed_code(issue: &str) -> &'static str {
    match issue {
        "initialization_failed" => "initialization_failed",
        "component_state_conflict" => "component_state_conflict",
        "component_storage_conflict" => "component_storage_conflict",
        "component_storage_unavailable" => "component_storage_unavailable",
        "component_storage_changed" => "component_storage_changed",
        "component_recovery_unavailable" => "component_recovery_unavailable",
        "component_window_unavailable" => "component_window_unavailable",
        "component_shutdown_hook_unavailable" => "component_shutdown_hook_unavailable",
        "runtime_owner_unavailable" => "runtime_owner_unavailable",
        "runtime_operation_unavailable" => "runtime_operation_unavailable",
        "store_unavailable" => "store_unavailable",
        "store_generation_changed" => "store_generation_changed",
        "store_pointer_changed" => "store_pointer_changed",
        "store_owner_busy" => "store_owner_busy",
        "setup_required" => "setup_required",
        "registry_unavailable" => "registry_unavailable",
        "registry_invalid" => "registry_invalid",
        "registry_read_only" => "registry_read_only",
        "request_expired" => "request_expired",
        "request_cancelled" => "request_cancelled",
        "invalid_request" => "invalid_request",
        "agent_request_rejected" => "agent_request_rejected",
        "busy" => "busy",
        "stale_context" => "stale_context",
        "session_invalid" => "session_invalid",
        "route_forbidden" => "route_forbidden",
        "component_forbidden" => "component_forbidden",
        "worker_unavailable" => "worker_unavailable",
        _ => "initialization_failed",
    }
}
/// Best-effort metadata only. Logging cannot change initialization or its error.
pub(super) fn observe<T, E: AsRef<str>>(
    root: &Path,
    stage: Stage,
    result: Result<T, E>,
) -> Result<T, E> {
    if let Err(error) = &result {
        if let Ok(log) = OperationLog::open(root.join("logs")) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|time| time.as_millis() as u64)
                .unwrap_or(0);
            log.append(&Entry::new(
                now,
                env!("CARGO_PKG_VERSION"),
                "agent",
                "runtime",
                stage.method(),
                0,
                Outcome::Failed,
                Some(fixed_code(error.as_ref())),
            ));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn records_only_fixed_stage_and_code_and_keeps_the_original_error() {
        let root = tempfile::tempdir().unwrap();
        let result: Result<(), String> = observe(
            root.path(),
            Stage::Engine,
            Err("component_storage_unavailable".into()),
        );
        assert_eq!(result.unwrap_err(), "component_storage_unavailable");
        let private = "C:/private/token=secret";
        let result: Result<(), &str> = observe(root.path(), Stage::Logs, Err(private));
        assert_eq!(result.unwrap_err(), private);
        let files: Vec<_> = std::fs::read_dir(root.path().join("logs"))
            .unwrap()
            .collect();
        let raw = std::fs::read_to_string(files[0].as_ref().unwrap().path()).unwrap();
        assert!(raw.contains("initialize_engine"));
        assert!(raw.contains("component_storage_unavailable"));
        assert!(raw.contains("initialize_logs"));
        assert!(raw.contains("initialization_failed"));
        assert!(!raw.contains("private") && !raw.contains("secret"));
    }
    #[test]
    fn successful_initialization_and_logging_failure_do_not_change_results() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(observe::<_, &str>(root.path(), Stage::Host, Ok(42)), Ok(42));
        assert!(!root.path().join("logs").exists());
        std::fs::write(root.path().join("logs"), "blocked").unwrap();
        assert_eq!(
            observe::<(), _>(root.path(), Stage::Host, Err("store_unavailable")),
            Err("store_unavailable")
        );
    }
}
