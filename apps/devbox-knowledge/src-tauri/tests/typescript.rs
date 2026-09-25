//! The native DTO graph is the only source of the checked-in frontend bindings.
use std::path::PathBuf;
#[test]
fn export_typescript_bindings() {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../packages/knowledge-features/src/generated");
    if out.exists() {
        std::fs::remove_dir_all(&out).unwrap();
    }
    std::fs::create_dir_all(&out).unwrap();
    let cfg = ts_rs::Config::new()
        .with_large_int("number")
        .with_out_dir(&out);
    let mut export = product_ipc::TypeExporter::new(&cfg);
    let results = devbox_knowledge_lib::ipc::activity::result_types(&mut export).unwrap();
    std::fs::write(
        out.join("activity-results.ts"),
        export.results("ActivityResults", &results),
    )
    .unwrap();
}
