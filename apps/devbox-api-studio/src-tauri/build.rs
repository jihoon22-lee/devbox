fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
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
