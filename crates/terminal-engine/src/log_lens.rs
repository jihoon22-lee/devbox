//! Explicit WSL Desktop -> Log Lens producer handoff.
//!
//! This module never executes a WSL command.  It validates the selected fixed
//! adapter configuration, writes a short-lived one-time envelope, and sends
//! only the opaque handoff id/kind through AppLink argv.

use crate::core::log_handoff;
use devbox_applink::{
    CreateHandoff, HandoffDescriptor, HandoffError, HandoffPublication, HandoffStore, OpenRequest,
};
use serde::Serialize;
use std::sync::{Mutex, MutexGuard, OnceLock, TryLockError};

const HANDOFF_CAPABILITY: &str = "handoff:log-source/v1";

// Keep publication and launch single-flight inside this producer.  Without
// this guard a double-click can create two pending envelopes before either
// Log Lens instance has claimed the first one.
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

fn publish_and_launch(payload: serde_json::Value) -> Result<LogLensDispatch, String> {
    let _dispatch_guard = dispatch_lock()?;
    if !log_lens_is_installed() {
        return Err("log-lens-unavailable".to_string());
    }
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
    // A failed spawn must not strand a descriptor that has no usable caller.
    // Remove only the exact immutable envelope just published.
    launch_or_cleanup(&store, &publication, || {
        devbox_launch::launch_open(log_handoff::TARGET_APP, &request)
    })?;
    Ok(LogLensDispatch {
        handoff_id: publication.descriptor.id,
    })
}

/// Publish a fixed WSL file adapter configuration after an explicit user
/// action.  The file is read later by Log Lens, not by this command.
#[tauri::command]
pub fn open_wsl_file_in_log_lens(
    distro: String,
    wsl_path: String,
) -> Result<LogLensDispatch, String> {
    let payload = log_handoff::file_payload(&distro, &wsl_path)
        .map_err(|_| "log-source-invalid".to_string())?;
    publish_and_launch(payload)
}

/// Publish a fixed WSL journal adapter configuration after an explicit user
/// action.  `journalctl` execution remains exclusively in Log Lens's fixed
/// read-only adapter.
#[tauri::command]
pub fn open_wsl_journal_in_log_lens(
    distro: String,
    unit: Option<String>,
) -> Result<LogLensDispatch, String> {
    let payload = log_handoff::journal_payload(&distro, unit.as_deref())
        .map_err(|_| "log-source-invalid".to_string())?;
    publish_and_launch(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn producer_payloads_are_bounded_before_store_or_launch() {
        let file = log_handoff::file_payload("Ubuntu", "/var/log/app.log").unwrap();
        let journal = log_handoff::journal_payload("Ubuntu", None).unwrap();
        assert!(serde_json::to_vec(&file).unwrap().len() <= log_handoff::MAX_PAYLOAD_BYTES);
        assert!(serde_json::to_vec(&journal).unwrap().len() <= log_handoff::MAX_PAYLOAD_BYTES);
    }

    #[test]
    fn dispatch_contains_only_the_opaque_handoff_id() {
        let dispatch = LogLensDispatch {
            handoff_id: "a".repeat(32),
        };
        let json = serde_json::to_string(&dispatch).unwrap();
        assert!(json.contains("handoffId"));
        assert!(!json.contains("Ubuntu"));
        assert!(!json.contains("var/log"));
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
                    payload: log_handoff::file_payload("Ubuntu", "/var/log/app.log")
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
    fn applink_argv_does_not_contain_the_wsl_adapter_configuration() {
        let request = open_request(&HandoffDescriptor {
            id: "b".repeat(32),
            kind: log_handoff::HANDOFF_KIND.into(),
        });
        let argv = devbox_launch::open_argv(&request).expect("valid handoff AppLink request");
        assert_eq!(argv[0], "--handoff-kind");
        assert_eq!(argv[1], "log-source/v1");
        assert_eq!(argv[2], "--handoff-id");
        assert_eq!(argv[3], "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
        assert_eq!(argv[4], "--from");
        assert_eq!(argv[5], "wsl-desktop");
        assert!(!argv
            .iter()
            .any(|value| value.contains("Ubuntu") || value.contains("var/log")));
    }
}
