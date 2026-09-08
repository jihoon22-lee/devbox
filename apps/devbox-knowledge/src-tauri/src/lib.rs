mod component;
mod core;
mod lifecycle;
mod migration;
mod search;
mod startup;
mod storage_space;
mod vault_owner;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    product_shell_tauri::run_with("knowledge", tauri::generate_context!(), |builder| {
        builder
            .plugin(tauri_plugin_clipboard_manager::init())
            .plugin(tauri_plugin_opener::init())
            .plugin(component::plugin())
    })
    .expect("error while running Devbox Knowledge");
}
