use crate::core::{connection::IpcError, popout::Kind};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
#[tauri::command]
pub async fn popout_create(
    app: AppHandle,
    kind: Kind,
    id: Option<String>,
) -> Result<String, IpcError> {
    let (label, path) = kind
        .target(id.as_deref())
        .ok_or_else(|| IpcError::new("창 대상을 확인하세요"))?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    let ui = app.clone();
    app.run_on_main_thread(move || {
        let result = super::quit::window_admission(&ui, || {
            let window = match ui.get_webview_window(&label) {
                Some(window) => window,
                None => WebviewWindowBuilder::new(&ui, &label, WebviewUrl::App(path.into()))
                    .title("devbox")
                    .inner_size(1000.0, 700.0)
                    .min_inner_size(480.0, 320.0)
                    .build()?,
            };
            super::window::disable_browser_accelerators_and_show(&window, true, true)?;
            Ok(label)
        })
        .map_err(|_| IpcError::new("앱 종료를 준비 중이거나 창을 열 수 없습니다"));
        let _ = tx.send(result);
    })
    .map_err(|_| IpcError::new("창을 열 수 없습니다"))?;
    rx.await.map_err(|_| IpcError::new("창을 열 수 없습니다"))?
}
