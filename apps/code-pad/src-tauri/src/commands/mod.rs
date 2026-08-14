pub mod file;
pub mod folder;
pub mod installer;
pub mod lsp;
pub mod preview;
pub mod recovery;
pub mod session;
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
}
