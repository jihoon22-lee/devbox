#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    product_shell_tauri::run("control-center", tauri::generate_context!())
        .expect("error while running Devbox Control Center");
}
pub mod core;
