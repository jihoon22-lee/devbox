mod applink;
mod commands;
mod core;
mod platform;

use commands::workspace::{profile_store_state, run_registry, ProfileStoreState};
use std::sync::Arc;
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
                Err(_) => eprintln!("applink: 요청을 해석할 수 없습니다"),
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
                Err(_) => eprintln!("applink: 요청을 해석할 수 없습니다"),
            }
            app.manage(run_registry());
            app.manage(profile_store_state());
            // Life Log projects/v1 snapshot의 프로젝트 경로를 흡수 (read-only, §10.2)
            let store_state = app.state::<Arc<ProfileStoreState>>();
            match store_state.lock.lock() {
                Ok(_store_lock) => {
                    match commands::workspace::load_store_document(app.handle()) {
                        Ok(mut document) => match commands::workspace::absorb_life_log_projects(&mut document.store) {
                            Ok(report) => {
                                if report.unsupported_paths > 0 {
                                    eprintln!(
                                        "life-log snapshot: distro 정보가 없는 POSIX 프로젝트 {}개를 건너뛰었습니다",
                                        report.unsupported_paths
                                    );
                                }
                                if report.added > 0
                                    && commands::workspace::save_store_document(
                                        app.handle(),
                                        &document,
                                        &document.store,
                                    )
                                    .is_err()
                                {
                                    eprintln!("life-log snapshot: 프로필 저장소를 저장할 수 없습니다");
                                }
                            }
                            Err(_) => eprintln!("life-log snapshot: snapshot을 안전하게 읽을 수 없습니다"),
                        },
                        Err(_) => eprintln!("life-log snapshot: 프로필 저장소를 읽을 수 없습니다"),
                    }
                }
                Err(_) => {
                    eprintln!("life-log snapshot: 프로필 저장소를 사용할 수 없습니다");
                }
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
            commands::workspace::cancel_project_health,
            commands::environment::preview_project_environment,
            commands::environment::cancel_project_environment,
            commands::preflight::workspace_preflight,
            commands::workspace::cancel_start_workspace,
            commands::workspace::start_workspace,
            commands::workspace::stop_workspace,
            commands::workspace::current_workspace_run,
            core::runtime_suggestions::wsl_runtime_suggestions,
            commands::profile_actions::profile_open_targets,
            commands::profile_actions::profile_copy_path,
            commands::profile_actions::open_profile_in,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
