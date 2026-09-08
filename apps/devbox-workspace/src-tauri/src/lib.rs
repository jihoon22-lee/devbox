mod component;
pub mod core;
pub mod host;
pub mod platform;
pub mod project_owner;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    product_shell_tauri::run_with("workspace", tauri::generate_context!(), |builder| {
        builder.plugin(component::plugin())
    })
    .expect("error while running Devbox Workspace");
}
