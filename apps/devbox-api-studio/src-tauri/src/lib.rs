mod component;
mod component_errors;
mod core;
mod handoff;
mod lifecycle;
mod migration;
mod migration_export;
mod platform;
mod service_worker;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(stage) =
        migration_export::worker_argument(&args).expect("invalid import worker arguments")
    {
        migration_export::run_worker(stage, tauri::generate_context!())
            .expect("import worker failed");
        return;
    }
    if let Some(profile) = service_worker::argument(&std::env::args().collect::<Vec<_>>())
        .expect("invalid service worker arguments")
    {
        service_worker::run(profile, tauri::generate_context!()).expect("service worker failed");
        return;
    }
    product_shell_tauri::run_with("api-studio", tauri::generate_context!(), |builder| {
        builder
            .plugin(tauri_plugin_clipboard_manager::init())
            .plugin(tauri_plugin_dialog::init())
            .plugin(tauri_plugin_opener::init())
            .plugin(component::plugin())
            .plugin(lifecycle::plugin())
    })
    .expect("error while running Devbox API Studio");
}
