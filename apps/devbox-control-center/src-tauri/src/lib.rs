mod federation;
mod shortcuts;
use suite_runtime as suite;
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(windows)]
    match bootstrap::interactive::resume_before_shell() {
        Ok(true) => return,
        Ok(false) => {}
        Err(issue) => {
            eprintln!("{issue}");
            return;
        }
    }
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

#[cfg(windows)]
mod legacy_cleanup;
mod tools_host;
#[cfg(windows)]
mod updates;

pub mod bootstrap;

#[cfg(windows)]
#[path = "platform/legacy_installer.rs"]
mod legacy_installer;

#[cfg(windows)]
mod migration_evidence;
