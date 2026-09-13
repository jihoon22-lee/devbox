mod component;
mod core;
mod federation;
mod lifecycle;
mod migration;
mod project_provider;
mod search;
mod session_receive;
#[path = "../../../devbox-control-center/src-tauri/src/suite.rs"]
mod suite;
pub use search::{disconnect_project_provider, install_project_snapshot};
mod startup;
mod storage_space;
mod vault_owner;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    product_shell_tauri::run_with("knowledge", tauri::generate_context!(), |builder| {
        builder
            .plugin(suite::plugin(
                "knowledge",
                Some(federation::handle),
                &[
                    product_contract::transport::Source::Notes,
                    product_contract::transport::Source::Files,
                    product_contract::transport::Source::SavedQueries,
                ],
            ))
            .plugin(tauri_plugin_clipboard_manager::init())
            .plugin(tauri_plugin_opener::init())
            .plugin(component::plugin())
            .plugin(project_provider::plugin())
            .plugin(session_receive::plugin())
    })
    .expect("error while running Devbox Knowledge");
}

mod vault_binding;

mod file_send;
