// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
mod applink;
mod commands;
mod core;

use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // 두 번째 process가 별도 Repo Manager 창을 만들기 전에 argv를 기존
        // instance로 전달하도록 첫 plugin으로 등록한다.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            match devbox_applink::parse_argv(&args) {
                Ok(Some(request)) => {
                    app.state::<applink::PendingOpen>().set(request.clone());
                    let _ = app.emit("devbox://open", request);
                }
                Ok(None) => {}
                Err(_) => eprintln!("applink: invalid request"),
            }
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            applink::take_pending_open,
            commands::scan_root,
            commands::prepare_inbound_repository,
            commands::repo_status,
            commands::worktrees,
            commands::create_worktree,
            commands::worktree_clean,
            commands::open_targets,
            commands::open_in,
        ])
        .setup(|app| {
            app.manage(applink::PendingOpen::new());
            match devbox_applink::parse_argv(&std::env::args().collect::<Vec<_>>()) {
                Ok(Some(request)) => app.state::<applink::PendingOpen>().set(request),
                Ok(None) => {}
                Err(_) => eprintln!("applink: invalid request"),
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
