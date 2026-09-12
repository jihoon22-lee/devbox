fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .plugin(
                "product-shell",
                tauri_build::InlinedPlugin::new().commands(&["describe", "route_status"]),
            )
            .plugin(
                "commands",
                tauri_build::InlinedPlugin::new().commands(&["command_search", "command_preview"]),
            ),
    )
    .expect("failed to generate product capabilities");
}
