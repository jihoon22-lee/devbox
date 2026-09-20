include!("../../../crates/product-shell-tauri/build_support.rs");
fn main() {
    let _bundle_staging = lock_bundle_staging();
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .plugin(
                "suite",
                tauri_build::InlinedPlugin::new().commands(&["connection"]),
            )
            .plugin(
                "product-shell",
                tauri_build::InlinedPlugin::new().commands(&["describe", "route_status"]),
            )
            .plugin(
                "api-studio",
                tauri_build::InlinedPlugin::new().commands(&["execute", "legacy_export_message"]),
            ),
    )
    .expect("failed to generate API Studio capabilities");
}
