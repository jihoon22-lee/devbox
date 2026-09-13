mod federation;
mod shortcuts;
mod suite;
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    product_shell_tauri::run_with("control-center", tauri::generate_context!(), |builder| {
        builder
            .plugin(suite::plugin(
                "control-center",
                Some(federation::handle),
                &[],
            ))
            .plugin(commands::plugin())
            .plugin(tauri_plugin_dialog::init())
            .plugin(tauri_plugin_opener::init())
            .plugin(tools_host::plugin())
    })
    .expect("error while running Devbox Control Center");
}
pub mod core;

mod command_receipts;
mod commands;

mod launcher_import;

mod tools_host;
