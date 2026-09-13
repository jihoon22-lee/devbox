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
    })
    .expect("error while running Devbox Control Center");
}
pub mod core;

mod command_receipts;
mod commands;
