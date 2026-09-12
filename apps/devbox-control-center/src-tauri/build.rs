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
                "commands",
                tauri_build::InlinedPlugin::new().commands(&[
                    "command_search",
                    "command_preview",
                    "command_source",
                    "command_open",
                    "command_status",
                ]),
            ),
    )
    .expect("failed to generate product capabilities");
}
