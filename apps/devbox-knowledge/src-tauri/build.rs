include!("../../../crates/product-shell-tauri/wsl_build_support.rs");
include!("../../../crates/product-shell-tauri/build_support.rs");
fn main() {
    let _bundle_staging = lock_bundle_staging();
    helper_digest();
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
                "knowledge",
                tauri_build::InlinedPlugin::new().commands(&["execute", "activity"]),
            ),
    )
    .expect("failed to generate Knowledge capabilities");
}
