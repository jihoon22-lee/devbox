mod component;
pub mod core;
pub mod definitions;
mod dependencies_host;
pub mod file_owner;
mod files_host;
pub mod host;
mod legacy_imports;
mod lsp_host;
pub mod platform;
mod private_metadata;
pub mod project_owner;
mod runtime_host;
mod source_host;
mod window_import;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    product_shell_tauri::run_with("workspace", tauri::generate_context!(), |builder| {
        builder
            .plugin(tauri_plugin_dialog::init())
            .plugin(tauri_plugin_clipboard_manager::init())
            .plugin(tauri_plugin_opener::init())
            .plugin(tauri_plugin_notification::init())
            .plugin(component::plugin())
    })
    .expect("error while running Devbox Workspace");
}
