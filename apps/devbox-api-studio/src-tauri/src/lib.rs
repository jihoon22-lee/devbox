mod component;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    product_shell_tauri::run_with("api-studio", tauri::generate_context!(), |builder| {
        builder
            .plugin(tauri_plugin_clipboard_manager::init())
            .plugin(tauri_plugin_dialog::init())
            .plugin(tauri_plugin_opener::init())
            .plugin(component::plugin())
    })
    .expect("error while running Devbox API Studio");
}
