mod commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::manager::latest,
            commands::manager::installed,
            commands::manager::install,
            commands::manager::launch,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
