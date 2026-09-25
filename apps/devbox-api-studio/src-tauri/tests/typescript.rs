use std::path::PathBuf;
#[test]
fn export_typescript_bindings() {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../packages/api-studio-features/src/generated");
    if out.exists() {
        std::fs::remove_dir_all(&out).unwrap();
    }
    std::fs::create_dir_all(&out).unwrap();
    let cfg = ts_rs::Config::new()
        .with_large_int("number")
        .with_out_dir(&out);
    let mut export = product_ipc::TypeExporter::new(&cfg);
    use devbox_api_studio_lib::ipc;
    for (file, name, results) in [
        (
            "api-results.ts",
            "ApiResults",
            ipc::api::result_types(&mut export).unwrap(),
        ),
        (
            "webhook-results.ts",
            "WebhookResults",
            ipc::webhooks::result_types(&mut export).unwrap(),
        ),
        (
            "transform-results.ts",
            "TransformResults",
            ipc::transforms::result_types(&mut export).unwrap(),
        ),
    ] {
        std::fs::write(out.join(file), export.results(name, &results)).unwrap();
    }
}
