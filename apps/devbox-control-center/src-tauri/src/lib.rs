mod federation;
mod shortcuts;
use suite_runtime as suite;
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(windows)]
    if std::env::args().skip(1).eq(["--stop-agent-for-update"]) {
        std::process::exit(
            if suite::stop_agent_for_update(env!("CARGO_PKG_VERSION")).is_ok() {
                0
            } else {
                2
            },
        );
    }
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
                env!("CARGO_PKG_VERSION"),
                Some(federation::handle),
                &[],
            ))
            .plugin(commands::plugin())
            .plugin(tauri_plugin_dialog::init())
            .plugin(tauri_plugin_opener::init())
            .plugin(ipc::plugin())
    })
    .expect("error while running Devbox Control Center");
}
pub mod core;

mod command_receipts;
mod commands;

pub mod ipc;
#[cfg(windows)]
mod updates;

pub mod bootstrap;

#[cfg(windows)]
mod owner_evidence;
