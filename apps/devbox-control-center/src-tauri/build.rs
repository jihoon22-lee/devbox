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
                    "command_cancel",
                    "command_open",
                    "command_status",
                    "command_preferences",
                    "command_shortcut",
                    "command_trigger_shortcut",
                    "command_import_launcher",
                ]),
            ),
    )
    .expect("failed to generate product capabilities");
}
