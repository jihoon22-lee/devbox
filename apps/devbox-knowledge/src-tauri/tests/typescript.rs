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
    use devbox_knowledge_lib::ipc;
    for (file, name, results) in [
        (
            "activity-results.ts",
            "ActivityResults",
            ipc::activity::result_types(&mut export).unwrap(),
        ),
        (
            "notes-results.ts",
            "NotesResults",
            ipc::notes::result_types(&mut export).unwrap(),
        ),
        (
            "search-results.ts",
            "SearchResults",
            ipc::search::result_types(&mut export).unwrap(),
        ),
        (
            "search-settings-results.ts",
            "SearchSettingsResults",
            ipc::search::settings_result_types(&mut export).unwrap(),
        ),
        (
            "opener-results.ts",
            "OpenerResults",
            ipc::search::opener_result_types(&mut export).unwrap(),
        ),
        (
            "setup-results.ts",
            "SetupResults",
            ipc::setup::result_types(&mut export).unwrap(),
        ),
        (
            "quit-results.ts",
            "QuitResults",
            ipc::commands::result_types(&mut export).unwrap(),
        ),
    ] {
        std::fs::write(out.join(file), export.results(name, &results)).unwrap();
    }
}
