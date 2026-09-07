fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().plugin(
        "product-shell",
        tauri_build::InlinedPlugin::new().commands(&["describe", "route_status"]),
    ))
    .expect("failed to generate product shell capabilities");
}
