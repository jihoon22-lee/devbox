mod commands;
mod core;

use commands::tracking::{spawn_poller, AppState};
use core::db::init;
use core::sessionizer::Sessionizer;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tauri::Manager;

// TODO(0.5.0): v0.4.x 이전 사용자를 위한 1회성 마이그레이션. 두 릴리스 뒤 제거한다.
const LEGACY_IDENTIFIER: &str = "com.workbench.lifelog";

/// 구 identifier 디렉터리가 있고 새 디렉터리가 없으면 통째로 옮긴다.
/// 실패해도 앱을 막지 않는다 (로그만 남기고 다음 실행에서 재시도).
fn migrate_local_data(app: &tauri::AppHandle, legacy_id: &str) -> std::io::Result<()> {
    let new_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    if new_dir.exists() {
        return Ok(());
    }
    let legacy_dir = new_dir
        .parent()
        .expect("local data dir has a parent")
        .join(legacy_id);
    if !legacy_dir.exists() {
        return Ok(());
    }
    std::fs::rename(&legacy_dir, &new_dir)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
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
            commands::queries::timeline,
            commands::queries::app_stats,
        ])
        .setup(|app| {
            if let Err(error) = migrate_local_data(app.handle(), LEGACY_IDENTIFIER) {
                eprintln!("devbox: local data migration will retry next launch: {error}");
            }
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
            });
            app.manage(state);
            spawn_poller(app.handle());
            setup_tray(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // 닫기 버튼 = 트레이로 숨기기 (백그라운드 추적 유지)
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
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
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}
