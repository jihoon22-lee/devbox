pub mod file;
pub mod folder;
#[cfg(feature = "desktop")]
pub mod installer;
#[cfg(feature = "desktop")]
pub mod lsp;
#[cfg(feature = "preview")]
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

    pub async fn watch_file(
        path: String,
        manager: State<'_, Arc<WatcherManager>>,
    ) -> Result<(), String> {
        let manager = Arc::clone(manager.inner());
        tauri::async_runtime::spawn_blocking(move || manager.register(Path::new(&path)))
            .await
            .map_err(|error| format!("파일 감시 등록 작업이 중단되었습니다: {error}"))?
    }

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
