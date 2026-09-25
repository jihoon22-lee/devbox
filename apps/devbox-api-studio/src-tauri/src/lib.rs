mod api_workspace;
mod component;
mod component_errors;
mod core;
mod federation;
mod handoff;
mod knowledge;
mod lifecycle;
mod mock_draft;
mod platform;
mod service_worker;
use suite_runtime as suite;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if let Some(profile) = service_worker::argument(&std::env::args().collect::<Vec<_>>())
        .expect("invalid service worker arguments")
    {
        service_worker::run(profile, tauri::generate_context!()).expect("service worker failed");
        return;
    }
    product_shell_tauri::run_with("api-studio", tauri::generate_context!(), |builder| {
        builder
            .plugin(suite::plugin(
                "api-studio",
                env!("CARGO_PKG_VERSION"),
                Some(federation::handle),
                &[],
            ))
            .plugin(tauri_plugin_clipboard_manager::init())
            .plugin(tauri_plugin_dialog::init())
            .plugin(tauri_plugin_opener::init())
            .plugin(component::plugin())
            .plugin(lifecycle::plugin())
    })
    .expect("error while running Devbox API Studio");
}

mod selection_receive;

mod webhook_logs;
