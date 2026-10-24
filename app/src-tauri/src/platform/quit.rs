use crate::core::{
    connection::IpcError,
    quit::{Quit, Status, note_window},
};
use std::{sync::Mutex, time::Duration};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use tokio::sync::Notify;

#[derive(Default)]
pub struct QuitState {
    state: Mutex<Quit>,
    changed: Notify,
}
fn labels(app: &AppHandle) -> Vec<String> {
    app.webview_windows()
        .into_keys()
        .filter(|label| note_window(label))
        .collect()
}
pub fn approved(app: &AppHandle) -> bool {
    app.state::<QuitState>()
        .state
        .lock()
        .expect("quit state")
        .approved
}
pub fn destroyed(app: &AppHandle, label: &str) {
    let state = app.state::<QuitState>();
    state.state.lock().expect("quit state").invalidate(label);
    state.changed.notify_one();
}
#[tauri::command]
pub fn quit_window_ready(
    window: WebviewWindow,
    state: State<'_, QuitState>,
) -> Result<u64, IpcError> {
    if !note_window(window.label()) {
        return Err(IpcError::new("문서 창이 아닙니다"));
    }
    let lease = state
        .state
        .lock()
        .expect("quit state")
        .register(window.label());
    state.changed.notify_one();
    Ok(lease)
}
#[tauri::command]
pub fn quit_window_not_ready(window: WebviewWindow, state: State<'_, QuitState>, lease: u64) {
    state
        .state
        .lock()
        .expect("quit state")
        .unregister(window.label(), lease);
    state.changed.notify_one();
}
#[tauri::command]
pub fn quit_prepared(
    window: WebviewWindow,
    state: State<'_, QuitState>,
    lease: u64,
    round: u64,
    safe: bool,
) {
    state
        .state
        .lock()
        .expect("quit state")
        .ack(window.label(), lease, round, safe);
    state.changed.notify_one();
}
#[tauri::command]
pub async fn quit_app(app: AppHandle, window: WebviewWindow) -> Result<(), IpcError> {
    super::preferences::main_only(&window)?;
    let state = app.state::<QuitState>();
    let round = state
        .state
        .lock()
        .expect("quit state")
        .start(labels(&app))
        .ok_or_else(|| IpcError::new("문서 창의 저장 준비를 기다린 뒤 다시 시도하세요"))?;
    let result = async {
        for label in labels(&app) {
            app.emit_to(&label, "devbox://prepare-quit", round)
                .map_err(|_| IpcError::new("문서 창에 저장을 요청하지 못했습니다"))?;
        }
        loop {
            // Notify retains a permit when the last ACK arrives between status and await.
            let status = state
                .state
                .lock()
                .expect("quit state")
                .status(round, &labels(&app));
            match status {
                Status::Ready => return Ok(()),
                Status::Failed => {
                    return Err(IpcError::new("문서를 저장하지 못해 종료를 취소했습니다"));
                }
                Status::Waiting => state.changed.notified().await,
            }
        }
    };
    let safe = matches!(
        tokio::time::timeout(Duration::from_secs(30), result).await,
        Ok(Ok(()))
    );
    // Creation/destruction of native windows also runs on the UI thread. Recheck and
    // authorize exit in one UI turn, so a newly created WebView cannot slip past the ACK set.
    let (tx, rx) = tokio::sync::oneshot::channel();
    let commit_app = app.clone();
    let scheduled = app.run_on_main_thread(move || {
        let state = commit_app.state::<QuitState>();
        let approved = {
            let mut quit = state.state.lock().expect("quit state");
            let safe = safe && quit.status(round, &labels(&commit_app)) == Status::Ready;
            quit.finish(round, safe);
            safe
        };
        if approved {
            commit_app.exit(0);
        } else {
            let _ = commit_app.emit("devbox://cancel-quit", round);
        }
        let _ = tx.send(approved);
    });
    if scheduled.is_err() {
        state.state.lock().expect("quit state").finish(round, false);
        let _ = app.emit("devbox://cancel-quit", round);
        return Err(IpcError::new("종료를 준비하지 못했습니다"));
    }
    if rx.await.unwrap_or(false) {
        Ok(())
    } else {
        Err(IpcError::new(
            "문서를 안전하게 저장하지 못해 종료를 취소했습니다",
        ))
    }
}
