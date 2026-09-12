fn main() {
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
                tauri_build::InlinedPlugin::new().commands(&["execute"]),
            ),
    )
    .expect("failed to generate Knowledge capabilities");
}
