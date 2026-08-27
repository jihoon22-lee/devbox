mod commands;
mod core;
mod integration;

use commands::tracking::{spawn_poller, AppState, DigestOperationState};
use core::db::init;
use core::sessionizer::Sessionizer;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tauri::Manager;

// TODO(0.5.0): v0.4.x 이전 사용자를 위한 1회성 마이그레이션. 두 릴리스 뒤 제거한다.
const LEGACY_IDENTIFIER: &str = "com.workbench.lifelog";
const CURRENT_IDENTIFIER: &str = "com.devbox.lifelog";

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
        // single-instance는 반드시 첫 플러그인이어야 한다: 이후 플러그인·setup이
        // 두 번째 프로세스에서 중복 초기화되기 전에 중복 실행을 종료한다.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            focus_main_window(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::digest::get_digest,
            commands::digest::cancel_digest,
            commands::digest::save_digest,
            commands::handoff::send_digest_to_knowledge,
            commands::export::export_life_log,
            commands::export::save_life_log,
            commands::life::set_projects,
            commands::life::get_projects,
            commands::life::get_day,
            commands::life::get_range,
            commands::tracking::start_tracking,
            commands::tracking::stop_tracking,
            commands::tracking::is_tracking,
            commands::tracking::set_idle_threshold,
            commands::tracking::get_idle_threshold,
            commands::privacy::get_privacy_rules,
            commands::privacy::set_privacy_rules,
            commands::privacy::redact_existing,
            commands::autostart::autostart_status,
            commands::autostart::set_autostart,
            commands::life::integration_sources,
            commands::life::project_attribution,
            commands::queries::timeline,
            commands::queries::app_stats,
        ])
        .setup(|app| {
            devbox_window_state_tauri::restore_main_window(app.handle());
            let dir = app.path().app_local_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let conn = init(&dir.join("data.db"))?;
            // activity-timeline 병합에 따른 1회성 세션 DB 흡수 (원본은 보존)
            let legacy = core::db::default_legacy_activity_db();
            if let Err(error) =
                core::db::absorb_activity_timeline(&conn, std::path::Path::new(&legacy))
            {
                eprintln!("devbox: activity-timeline 흡수 실패, 다음 실행에서 재시도: {error}");
            }
            let state = Arc::new(AppState {
                db: Mutex::new(conn),
                sessionizer: Mutex::new(Sessionizer::new()),
                tracking: AtomicBool::new(true),
                snapshot_writer: Mutex::new(()),
                digest_operations: Arc::new(DigestOperationState::default()),
                digest_handles: core::digest::DigestHandleStore::default(),
            });
            app.manage(state.clone());
            integration::spawn_snapshot_writer(state);
            spawn_poller(app.handle());
            setup_tray(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            devbox_window_state_tauri::handle_window_event(window, event);
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // 닫기 버튼 = 트레이로 숨기기 (백그라운드 추적 유지)
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn focus_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::TrayIconBuilder;

    let show = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    TrayIconBuilder::with_id("main-tray")
        .icon(
            app.default_window_icon()
                .cloned()
                .expect("missing default icon"),
        )
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => focus_main_window(app),
            "quit" => {
                if let Some(window) = app.get_webview_window("main") {
                    devbox_window_state_tauri::save_main_webview_window(&window);
                }
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;
    Ok(())
}
