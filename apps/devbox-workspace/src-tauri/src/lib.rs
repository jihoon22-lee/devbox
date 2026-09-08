pub mod core;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    product_shell_tauri::run("workspace", tauri::generate_context!())
        .expect("error while running Devbox Workspace");
}
