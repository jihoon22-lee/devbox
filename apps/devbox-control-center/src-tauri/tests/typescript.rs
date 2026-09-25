use std::path::PathBuf;
#[test]
fn export_typescript_bindings() {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../packages/control-center-features/src/generated");
    if out.exists() {
        std::fs::remove_dir_all(&out).unwrap();
    }
    std::fs::create_dir_all(&out).unwrap();
    let cfg = ts_rs::Config::new()
        .with_large_int("number")
        .with_out_dir(&out);
    let mut export = product_ipc::TypeExporter::new(&cfg);
    use devbox_control_center_lib::ipc;
    for (file, name, results) in [
        (
            "tools-results.ts",
            "ToolsResults",
            ipc::tools::result_types(&mut export).unwrap(),
        ),
        (
            "delivery-results.ts",
            "DeliveryResults",
            ipc::delivery::result_types(&mut export).unwrap(),
        ),
    ] {
        std::fs::write(out.join(file), export.results(name, &results)).unwrap();
    }
}
