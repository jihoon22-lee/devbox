mod component;
pub mod ipc;
pub mod core;
pub mod definitions;
mod dependencies_host;
mod development_host;
mod federation;
pub mod file_owner;
mod files_host;
pub mod host;
mod lsp_host;
pub mod platform;
mod private_metadata;
mod problems_host;
pub mod project_owner;
mod runtime_host;
mod session_preflight;
pub mod session_summary;
mod source_host;
use suite_runtime as suite;
pub mod terminal_commands;
mod terminal_host;
mod terminal_profiles;
mod wsl_controls;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    product_shell_tauri::run_with("workspace", tauri::generate_context!(), |builder| {
        builder
            .plugin(suite::plugin(
                "workspace",
                env!("CARGO_PKG_VERSION"),
                Some(federation::handle),
                &[
                    product_contract::transport::Source::Projects,
                    product_contract::transport::Source::Repositories,
                    product_contract::transport::Source::Tasks,
                    product_contract::transport::Source::Services,
                    product_contract::transport::Source::Runs,
                ],
            ))
            .plugin(tauri_plugin_dialog::init())
            .plugin(tauri_plugin_clipboard_manager::init())
            .plugin(tauri_plugin_opener::init())
            .plugin(tauri_plugin_notification::init())
            .plugin(component::plugin())
            .plugin(project_provider::plugin())
    })
    .expect("error while running Devbox Workspace");
}

mod project_provider;

mod file_receive;

mod selection_send;

mod selection_logs;

mod webhook_logs;
