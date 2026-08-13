use code_pad_lib::lsp::{
    save_to_app_local_data_dir, LspConfig, LspManager, ServerRef, LSP_CONFIG_SCHEMA_VERSION,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

fn fixture_binary() -> PathBuf {
    std::env::current_exe()
        .expect("test executable path")
        .parent()
        .and_then(|deps| deps.parent())
        .expect("Cargo test executable should live below target/debug/deps")
        .join(if cfg!(windows) {
            "fake-lsp-server.exe"
        } else {
            "fake-lsp-server"
        })
}

#[tokio::test]
async fn session_lifecycle_and_document_notifications_are_coordinated() {
    let app_data = tempdir().unwrap();
    let workspace = tempdir().unwrap();
    let document = workspace.path().join("main.rs");
    fs::write(&document, "fn main() {}\n").unwrap();
    let executable = fixture_binary().canonicalize().unwrap();
    let workspace_root = workspace.path().canonicalize().unwrap();
    let config = LspConfig {
        version: LSP_CONFIG_SCHEMA_VERSION,
        enabled: true,
        workspace_root: workspace_root.to_string_lossy().into_owned(),
        server_by_language: BTreeMap::from([(
            "rust".into(),
            ServerRef::local(executable.to_string_lossy()),
        )]),
        custom_servers: Vec::new(),
        update_policy: Default::default(),
    };
    save_to_app_local_data_dir(app_data.path(), &config).unwrap();

    let manager = LspManager::new(app_data.path(), "0.3.0");
    manager.start("rust").await.unwrap();
    assert!(manager.start("rust").await.is_err());

    let opened = manager
        .open_document("rust", &document, "fn main() {}\n".into())
        .await
        .unwrap();
    let changed = manager
        .change_document(
            "rust",
            &opened.uri,
            "fn main() { println!(\"ok\"); }\n".into(),
            true,
        )
        .await
        .unwrap();
    assert_eq!(changed.version, 2);
    assert_eq!(manager.statuses().await[0].document_count, 1);
    manager.save_document("rust", &opened.uri).await.unwrap();
    manager.close_document("rust", &opened.uri).await.unwrap();
    assert_eq!(manager.statuses().await[0].document_count, 0);
    manager.stop("rust").await.unwrap();
    assert!(manager.statuses().await.is_empty());
}

#[test]
fn corrupt_config_requires_explicit_recovery_before_replacement() {
    let app_data = tempdir().unwrap();
    let config_dir = app_data.path().join("lsp");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(config_dir.join("config.json"), "{broken").unwrap();
    let manager = LspManager::new(app_data.path(), "0.3.0");
    assert!(manager.save_config(&LspConfig::empty(), false).is_err());
    manager.save_config(&LspConfig::empty(), true).unwrap();
    assert!(manager.load_config().unwrap().persist_allowed);
}
