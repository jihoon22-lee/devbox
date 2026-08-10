mod commands;
mod core;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::wsl::list_distros,
            commands::wsl::run_wsl_command,
            commands::wsl::docker_ps,
            commands::wsl::docker_action,
            commands::wsl::git_status,
            commands::wsl::open_terminal,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
