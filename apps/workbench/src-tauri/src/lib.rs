mod applink;
mod commands;
mod core;

use commands::workspace::run_registry;
use tauri::{Emitter, Manager};

// TODO(0.5.0): 신규 앱 — identifier 변경 이전 데이터가 없어 마이그레이션 없음.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // single-instance는 반드시 첫 플러그인이어야 한다: 이후 플러그인·setup이
        // 두 번째 프로세스에서 중복 초기화되기 전에 중복 실행을 종료한다.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            match devbox_applink::parse_argv(&args) {
                Ok(Some(req)) => {
                    app.state::<applink::PendingOpen>().set(req.clone());
                    let _ = app.emit("devbox://open", req);
                }
                Ok(None) => {}
                Err(e) => eprintln!("applink: {e}"),
            }
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            app.manage(applink::PendingOpen::new());
            match devbox_applink::parse_argv(&std::env::args().collect::<Vec<_>>()) {
                Ok(Some(req)) => app.state::<applink::PendingOpen>().set(req),
                Ok(None) => {}
                Err(e) => eprintln!("applink: {e}"),
            }
            app.manage(run_registry());
            // Life Log projects/v1 snapshot의 프로젝트 경로를 흡수 (read-only, §10.2)
            let mut store = crate::core::profile::ProfileStore::load(
                &app.handle()
                    .path()
                    .app_local_data_dir()
                    .ok()
                    .and_then(|d| std::fs::read_to_string(d.join("project-profiles.json")).ok())
                    .unwrap_or_default(),
            );
            match commands::workspace::absorb_life_log_projects(&mut store) {
                Ok(report) => {
                    if report.unsupported_paths > 0 {
                        eprintln!(
                            "life-log snapshot: distro 정보가 없는 POSIX 프로젝트 {}개를 건너뛰었습니다",
                            report.unsupported_paths
                        );
                    }
                    if report.added > 0 {
                        if let Err(error) = commands::workspace::save_store(app.handle(), &store) {
                            eprintln!("life-log snapshot: {error}");
                        }
                    }
                }
                Err(error) => eprintln!("life-log snapshot: {error}"),
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            applink::take_pending_open,
            commands::workspace::list_profiles,
            commands::workspace::create_profile,
            commands::workspace::update_profile,
            commands::workspace::delete_profile,
            commands::workspace::git_status,
            commands::workspace::project_health,
            commands::workspace::start_workspace,
            commands::workspace::stop_workspace,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
