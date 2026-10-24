use crate::core::{
    connection::IpcError,
    intents::{AppIntent, PendingIntents},
};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

#[derive(Default)]
pub struct Intents(Mutex<PendingIntents>);
pub fn disconnect(app: &AppHandle) {
    app.state::<Intents>()
        .0
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .disconnect();
}
fn drain(app: &AppHandle, state: &mut PendingIntents) {
    while let Some(intent) = state.front() {
        let result = match intent {
            AppIntent::Palette => app.emit_to("main", "devbox://palette", ()),
            AppIntent::Quit => app.emit_to("main", "devbox://quit", ()),
            AppIntent::BottomTerminal { was_focused } => {
                #[derive(Clone, serde::Serialize)]
                #[serde(rename_all = "camelCase")]
                struct Bottom {
                    was_focused: bool,
                }
                app.emit_to(
                    "main",
                    "devbox://bottom-terminal",
                    Bottom {
                        was_focused: *was_focused,
                    },
                )
            }
        };
        if let Err(error) = result {
            eprintln!("Cannot deliver app intent: {error}");
            break;
        }
        state.delivered();
    }
}
pub fn dispatch(app: &AppHandle, intent: AppIntent) {
    let state = app.state::<Intents>();
    let mut queue = state.0.lock().unwrap_or_else(|error| error.into_inner());
    queue.enqueue(intent);
    drain(app, &mut queue);
}
#[tauri::command]
pub fn app_intents_ready(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, Intents>,
) -> Result<u64, IpcError> {
    super::preferences::main_only(&window)?;
    let mut queue = state.0.lock().unwrap_or_else(|error| error.into_inner());
    let lease = queue.ready();
    drain(&app, &mut queue);
    Ok(lease)
}
#[tauri::command]
pub fn app_intents_not_ready(
    window: WebviewWindow,
    state: State<'_, Intents>,
    lease_id: u64,
) -> Result<(), IpcError> {
    super::preferences::main_only(&window)?;
    state
        .0
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .not_ready(lease_id);
    Ok(())
}
#[tauri::command]
pub fn hide_main(app: AppHandle, window: WebviewWindow) -> Result<(), IpcError> {
    super::preferences::main_only(&window)?;
    let main = app
        .get_webview_window("main")
        .ok_or_else(|| IpcError::new("메인 창 없음"))?;
    main.hide()
        .map_err(|error| IpcError::new(error.to_string()))
}
