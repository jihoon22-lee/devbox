#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    product_shell_tauri::run("knowledge", tauri::generate_context!())
        .expect("error while running Devbox Knowledge");
}
