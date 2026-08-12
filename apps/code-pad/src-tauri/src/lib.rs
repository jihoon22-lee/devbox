pub mod commands;
pub mod core;
pub mod lsp;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::file::open_file,
            commands::file::save_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
