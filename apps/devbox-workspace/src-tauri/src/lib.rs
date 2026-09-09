mod component;
pub mod core;
mod definitions;
pub mod file_owner;
mod files_host;
pub mod host;
mod lsp_host;
pub mod platform;
mod private_metadata;
pub mod project_owner;
mod source_host;

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
