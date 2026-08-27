mod commands;
pub mod core;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(commands::AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::summarize_source,
            commands::receive_log_source,
            commands::fixed_adapter,
            commands::read_source,
            commands::read_sources,
            commands::cancel_read,
            commands::filter_log_records,
            commands::export_log_records,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Log Lens");
}
