pub mod file;
#[cfg(feature = "desktop")]
pub mod folder;
#[cfg(feature = "desktop")]
pub mod installer;
#[cfg(feature = "desktop")]
pub mod lsp;
#[cfg(feature = "desktop")]
pub mod preview;
#[cfg(feature = "desktop")]
pub mod recovery;
pub mod session;
#[cfg(feature = "desktop")]
pub mod watch {
    use crate::watcher::WatcherManager;
    use std::path::Path;
    use std::sync::Arc;
    use tauri::State;

    /// Registering/unregistering is separate from the pure file open command
    /// so the latter remains directly testable without a Tauri AppHandle.
    #[tauri::command]
    pub async fn watch_file(
        path: String,
        manager: State<'_, Arc<WatcherManager>>,
    ) -> Result<(), String> {
        let manager = Arc::clone(manager.inner());
        tauri::async_runtime::spawn_blocking(move || manager.register(Path::new(&path)))
            .await
            .map_err(|error| format!("파일 감시 등록 작업이 중단되었습니다: {error}"))?
    }

    #[tauri::command]
    pub async fn unwatch_file(
        path: String,
        manager: State<'_, Arc<WatcherManager>>,
    ) -> Result<(), String> {
        let manager = Arc::clone(manager.inner());
        tauri::async_runtime::spawn_blocking(move || manager.unregister(Path::new(&path)))
            .await
            .map_err(|error| format!("파일 감시 해제 작업이 중단되었습니다: {error}"))?
    }
    /// Typed product adapter; the native host owns caller/session/owner admission.
    pub(crate) async fn __component_watch_file(
        _component_app: &tauri::AppHandle,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        use tauri::Manager;
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Input {
            path: String,
        }
        let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
        watch_file(
            input.path,
            _component_app
                .try_state()
                .ok_or("component_state_unavailable")?,
        )
        .await?;
        serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
    }

    /// Typed product adapter; the native host owns caller/session/owner admission.
    pub(crate) async fn __component_unwatch_file(
        _component_app: &tauri::AppHandle,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        use tauri::Manager;
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Input {
            path: String,
        }
        let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
        unwatch_file(
            input.path,
            _component_app
                .try_state()
                .ok_or("component_state_unavailable")?,
        )
        .await?;
        serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
    }
}
