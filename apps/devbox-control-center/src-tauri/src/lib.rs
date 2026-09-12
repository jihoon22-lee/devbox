#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    product_shell_tauri::run_with("control-center", tauri::generate_context!(), |builder| {
        builder.plugin(commands::plugin())
    })
    .expect("error while running Devbox Control Center");
}
pub mod core;

mod commands;
