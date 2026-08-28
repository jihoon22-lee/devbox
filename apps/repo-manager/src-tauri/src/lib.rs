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
            commands::repo_cleanup_preview,
            commands::repo_cleanup,
            commands::repo_cleanup_cancel,
            commands::repo_preflight,
            commands::repo_history,
            commands::repo_commit_detail,
            commands::repo_diff,
            commands::repo_changes,
            commands::repo_stage,
            commands::repo_unstage,
            commands::repo_commit,
            commands::repo_local_cancel,
            commands::repo_remote_status,
            commands::repo_fetch,
            commands::repo_pull,
            commands::repo_push,
            commands::repo_remote_cancel,
            commands::open_targets,
            commands::open_in,
            commands::repository_copy_path,
            commands::open_repository_folder,
        ])
        .setup(|app| {
            devbox_window_state_tauri::restore_main_window(app.handle());
            app.manage(applink::PendingOpen::new());
            match devbox_applink::parse_argv(&std::env::args().collect::<Vec<_>>()) {
                Ok(Some(request)) => app.state::<applink::PendingOpen>().set(request),
                Ok(None) => {}
                Err(_) => eprintln!("applink: invalid request"),
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            devbox_window_state_tauri::handle_window_event(window, event);
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
