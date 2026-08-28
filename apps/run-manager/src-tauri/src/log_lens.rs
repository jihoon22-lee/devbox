//! Explicit Run Manager -> Log Lens producer handoff.
//!
//! Publishing is only reachable from an explicit UI action.  The payload is
//! written to the shared one-time store and the child process receives only
//! the opaque handoff kind/id through AppLink argv.

use crate::core::log_handoff;
use crate::logs::LogStream;
use crate::storage::{DatabaseState, StorageError};
use devbox_applink::{
    CreateHandoff, HandoffDescriptor, HandoffError, HandoffPublication, HandoffStore, OpenRequest,
};
use serde::Serialize;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, TryLockError};
use tauri::{AppHandle, Manager, State};

const HANDOFF_CAPABILITY: &str = "handoff:log-source/v1";

// Publishing and launching are one user-visible operation.  Serialize them
// in-process so two rapid UI/context-menu requests cannot leave two valid
// envelopes pointing at the same Log Lens window or make retry ownership
// ambiguous. The shared store still provides cross-process one-time claims.
static DISPATCH_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLensDispatch {
    pub handoff_id: String,
}

fn handoff_store() -> HandoffStore {
    HandoffStore::new(devbox_applink::handoff_root_in(
        &devbox_integration::common_root(),
    ))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn log_lens_is_installed() -> bool {
    devbox_launch::installed_targets(HANDOFF_CAPABILITY)
        .iter()
        .any(|target| target.id == log_handoff::TARGET_APP)
}

fn open_request(descriptor: &HandoffDescriptor) -> OpenRequest {
    OpenRequest {
        target: descriptor.clone().into(),
        from: Some(log_handoff::SOURCE_APP.to_string()),
    }
}

fn map_storage_error(error: StorageError) -> String {
    match error {
        StorageError::NotFound(_) => "run-not-found".to_string(),
        _ => "run-storage-failed".to_string(),
    }
}

fn dispatch_lock() -> Result<MutexGuard<'static, ()>, String> {
    match DISPATCH_LOCK.get_or_init(|| Mutex::new(())).try_lock() {
        Ok(guard) => Ok(guard),
        Err(TryLockError::Poisoned(poisoned)) => Ok(poisoned.into_inner()),
        Err(TryLockError::WouldBlock) => Err("handoff-busy".to_string()),
    }
}

fn cleanup_after_launch_failure(
    store: &HandoffStore,
    publication: &HandoffPublication,
) -> Result<(), String> {
    match store.remove_pending(publication) {
        Ok(()) | Err(HandoffError::Missing) => Ok(()),
        Err(_) => Err("handoff-cleanup-failed".to_string()),
    }
}

fn launch_or_cleanup<F>(
    store: &HandoffStore,
    publication: &HandoffPublication,
    launch: F,
) -> Result<u32, String>
where
    F: FnOnce() -> Result<u32, String>,
{
    match launch() {
        Ok(pid) => Ok(pid),
        Err(_) => {
            cleanup_after_launch_failure(store, publication)
                .map_err(|_| "handoff-cleanup-failed".to_string())?;
            Err("log-lens-launch-failed".to_string())
        }
    }
}

fn validate_run_log_dir_for_handoff(
    data_root: &Path,
    run_id: &str,
    log_dir: Option<&str>,
) -> Result<(), String> {
    let log_dir = log_dir.ok_or_else(|| "logs-unavailable".to_string())?;
    crate::logs::resolve_run_directory(data_root, log_dir, run_id)
        .map(|_| ())
        .map_err(|_| "logs-unavailable".to_string())
}

/// Publish one selected run stream and launch the installed Log Lens target.
/// The run database is consulted only to prove that the selected app-owned
/// log exists; its relative path is never copied into the payload or argv.
#[tauri::command]
pub fn open_run_log_in_log_lens(
    run_id: String,
    stream: LogStream,
    app: AppHandle,
    state: State<'_, Arc<DatabaseState>>,
) -> Result<LogLensDispatch, String> {
    let _dispatch_guard = dispatch_lock()?;
    let run = state
        .get_run(&run_id)
        .map_err(map_storage_error)?
        .ok_or_else(|| "run-not-found".to_string())?;
    let data_root = app
        .path()
        .app_local_data_dir()
        .map_err(|_| "logs-unavailable".to_string())?;
    validate_run_log_dir_for_handoff(&data_root, &run_id, run.log_dir.as_deref())?;
    if !log_lens_is_installed() {
        return Err("log-lens-unavailable".to_string());
    }
    let payload = log_handoff::payload_for_run(&run_id, stream)
        .map_err(|_| "log-source-invalid".to_string())?;
    let now = now_ms();
    if now == 0 {
        return Err("handoff-unavailable".to_string());
    }
    let store = handoff_store();
    let publication = store
        .create_with_publication(
            CreateHandoff {
                kind: log_handoff::HANDOFF_KIND.to_string(),
                source_app: log_handoff::SOURCE_APP.to_string(),
                target_app: Some(log_handoff::TARGET_APP.to_string()),
                payload,
            },
            now,
        )
        .map_err(|_| "handoff-unavailable".to_string())?;
    let request = open_request(&publication.descriptor);
    // Do not leave a descriptor that no caller can reliably associate with
    // this failed launch.  The cleanup re-reads the exact immutable envelope
    // before removing it and never touches claimed state.
    launch_or_cleanup(&store, &publication, || {
        devbox_launch::launch_open(log_handoff::TARGET_APP, &request)
    })?;
    Ok(LogLensDispatch {
        handoff_id: publication.descriptor.id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_is_the_catalog_handoff_kind() {
        assert_eq!(HANDOFF_CAPABILITY, "handoff:log-source/v1");
        assert_eq!(log_handoff::HANDOFF_KIND, "log-source/v1");
    }

    #[test]
    fn dispatch_response_never_contains_a_path_or_log_bytes() {
        let dispatch = LogLensDispatch {
            handoff_id: "a".repeat(32),
        };
        let json = serde_json::to_string(&dispatch).expect("dispatch json");
        assert!(!json.contains("path"));
        assert!(!json.contains("log"));
    }

    #[test]
    fn launch_failure_cleanup_removes_the_new_pending_envelope() {
        let root = tempfile::tempdir().expect("handoff root");
        let store = HandoffStore::new(root.path().join("handoff/v1"));
        let publication = store
            .create_with_publication(
                CreateHandoff {
                    kind: log_handoff::HANDOFF_KIND.into(),
                    source_app: log_handoff::SOURCE_APP.into(),
                    target_app: Some(log_handoff::TARGET_APP.into()),
                    payload: log_handoff::payload_for_run("run-1", LogStream::Stdout)
                        .expect("payload"),
                },
                1_000,
            )
            .expect("publication");

        let error = launch_or_cleanup(&store, &publication, || Err("spawn failed".to_string()))
            .expect_err("launch failure");
        assert_eq!(error, "log-lens-launch-failed");
        assert_eq!(
            store.claim(
                &publication.descriptor.id,
                log_handoff::HANDOFF_KIND,
                log_handoff::TARGET_APP,
                2_000,
            ),
            Err(devbox_applink::HandoffError::Missing)
        );
    }

    #[test]
    fn handoff_requires_the_canonical_app_owned_run_directory() {
        let root = tempfile::tempdir().expect("app data root");
        let run_directory = root.path().join("logs/runs/run-1");
        std::fs::create_dir_all(&run_directory).expect("run logs");

        assert!(
            validate_run_log_dir_for_handoff(root.path(), "run-1", Some("logs/runs/run-1"),)
                .is_ok()
        );
        assert_eq!(
            validate_run_log_dir_for_handoff(root.path(), "run-1", None),
            Err("logs-unavailable".to_string())
        );
        assert_eq!(
            validate_run_log_dir_for_handoff(root.path(), "run-1", Some("logs/runs/other")),
            Err("logs-unavailable".to_string())
        );
    }

    #[test]
    fn applink_argv_contains_only_the_opaque_handoff_reference() {
        let request = open_request(&HandoffDescriptor {
            id: "a".repeat(32),
            kind: log_handoff::HANDOFF_KIND.into(),
        });
        assert_eq!(
            devbox_launch::open_argv(&request),
            vec![
                "--handoff-kind",
                "log-source/v1",
                "--handoff-id",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "--from",
                "run-manager",
            ]
        );
    }
}
