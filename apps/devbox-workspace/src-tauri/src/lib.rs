mod component;
pub mod core;
pub mod file_owner;
mod files_host;
pub mod host;
pub mod platform;
pub mod project_owner;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    product_shell_tauri::run_with("workspace", tauri::generate_context!(), |builder| {
        builder
            .plugin(tauri_plugin_dialog::init())
            .plugin(tauri_plugin_clipboard_manager::init())
            .plugin(tauri_plugin_opener::init())
            .plugin(component::plugin())
    })
    .expect("error while running Devbox Workspace");
}
