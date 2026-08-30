mod applink;
mod commands;
mod core;

use tauri::{Emitter, Manager};

// TODO(0.5.0): v0.4.x 이전 사용자를 위한 1회성 마이그레이션. 두 릴리스 뒤 제거한다.
const LEGACY_IDENTIFIER: &str = "com.workbench.developertoolbox";
const CURRENT_IDENTIFIER: &str = "com.devbox.developertoolbox";

fn migrate_local_data() {
    let Some(base_dir) = dirs::data_local_dir() else {
        eprintln!(
            "devbox: local data migration will retry next launch: local data directory unavailable"
        );
        return;
    };
    if let Err(error) = devbox_filesystem::migrate_legacy_identifier_dir(
        base_dir,
        LEGACY_IDENTIFIER,
        CURRENT_IDENTIFIER,
    ) {
        eprintln!("devbox: local data migration will retry next launch: {error}");
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    migrate_local_data();
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if let Ok(Some(request)) = devbox_applink::parse_argv(&args) {
                if applink::is_toolbox_text_request(&request) {
                    app.state::<applink::PendingOpen>().set(request.clone());
                    let _ = app.emit("devbox://open", request);
                }
            }
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .manage(applink::PendingOpen::new())
        .manage(commands::text_handoff::PendingToolboxText::new())
        .setup(|app| {
            devbox_window_state_tauri::restore_main_window(app.handle());
            if let Ok(Some(request)) =
                devbox_applink::parse_argv(&std::env::args().collect::<Vec<_>>())
            {
                if applink::is_toolbox_text_request(&request) {
                    app.state::<applink::PendingOpen>().set(request);
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            devbox_window_state_tauri::handle_window_event(window, event);
        })
        .invoke_handler(tauri::generate_handler![
            applink::take_pending_open,
            commands::tools::hash,
            commands::tools::hmac_generate,
            commands::tools::hmac_verify,
            commands::tools::generate_uuid,
            commands::tools::generate_ids,
            commands::tools::regex_test,
            commands::tools::diff,
            commands::tools::jwt_verify,
            commands::qr::generate_qr,
            commands::workflows::load_workflow_metadata,
            commands::workflows::save_workflow_metadata,
            commands::handoff::create_api_request_handoff,
            commands::handoff::create_knowledge_draft_handoff,
            commands::text_handoff::preview_toolbox_text,
            commands::text_handoff::accept_toolbox_text,
            commands::text_handoff::discard_toolbox_text,
            commands::text_handoff::renew_toolbox_text,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
