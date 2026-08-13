use code_pad_lib::lsp::{
    initial_catalog, save_to_app_local_data_dir, InstalledServer, InstalledServerIndex, LspConfig,
    LspManager, LspManagerError, LspPosition, ServerRef, LSP_CONFIG_SCHEMA_VERSION,
    LSP_INSTALLED_SCHEMA_VERSION,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
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
    let reloaded = manager
        .reload_document("rust", &opened.uri, "reloaded from disk\n".into())
        .await
        .unwrap();
    assert_eq!(reloaded.version, 3);
    assert!(reloaded.content_changes[0].range.is_none());
    let reload_wire = serde_json::to_value(&reloaded).unwrap();
    assert!(reload_wire["contentChanges"][0].get("range").is_none());
    assert_eq!(manager.statuses().await[0].document_count, 1);
    manager.save_document("rust", &opened.uri).await.unwrap();
    manager.close_document("rust", &opened.uri).await.unwrap();
    assert_eq!(manager.statuses().await[0].document_count, 0);
    manager.stop("rust").await.unwrap();
    assert!(manager.statuses().await.is_empty());
}

fn feature_config(
    workspace: &std::path::Path,
    executable: &std::path::Path,
    mode: &str,
) -> LspConfig {
    LspConfig {
        version: LSP_CONFIG_SCHEMA_VERSION,
        enabled: true,
        workspace_root: workspace.to_string_lossy().into_owned(),
        server_by_language: BTreeMap::from([(
            "rust".into(),
            ServerRef::Local {
                installed_path: executable.to_string_lossy().into_owned(),
                executable: None,
                args: vec![format!("--fake-mode={mode}")],
            },
        )]),
        custom_servers: Vec::new(),
        update_policy: Default::default(),
    }
}

#[tokio::test]
async fn read_features_use_typed_adapters_stale_versions_and_workspace_filters() {
    let app_data = tempdir().unwrap();
    let workspace = tempdir().unwrap();
    let document = workspace.path().join("main.rs");
    fs::write(&document, "fixture\n").unwrap();
    let executable = fixture_binary().canonicalize().unwrap();
    let config = feature_config(workspace.path(), &executable, "stale_features");
    save_to_app_local_data_dir(app_data.path(), &config).unwrap();

    let manager = Arc::new(LspManager::new(app_data.path(), "0.3.0"));
    manager.start("rust").await.unwrap();
    let opened = manager
        .open_document("rust", &document, "fixture\n".into())
        .await
        .unwrap();

    let diagnostics = manager.pull_diagnostics("rust", &opened.uri).await.unwrap();
    assert!(!diagnostics.stale);
    assert_eq!(diagnostics.value.diagnostics.len(), 1);
    assert_eq!(diagnostics.metadata.version, 1);

    let completion = manager
        .completion("rust", &opened.uri, LspPosition::new(0, 0))
        .await
        .unwrap();
    assert_eq!(completion.value.items[0].label, "fixture");

    let hover = manager
        .hover("rust", &opened.uri, LspPosition::new(0, 0))
        .await
        .unwrap();
    assert_eq!(hover.value.unwrap().text, "**fixture**");

    let definition = manager
        .definition("rust", &opened.uri, LspPosition::new(0, 0))
        .await
        .unwrap();
    assert_eq!(definition.value.locations.len(), 1);
    assert_eq!(definition.value.rejected, 1);

    let references = manager
        .references("rust", &opened.uri, LspPosition::new(0, 0), true)
        .await
        .unwrap();
    assert_eq!(references.value.locations.len(), 1);
    assert_eq!(references.value.rejected, 1);

    let request_manager = Arc::clone(&manager);
    let request_uri = opened.uri.clone();
    let request = tokio::spawn(async move {
        request_manager
            .completion("rust", &request_uri, LspPosition::new(0, 0))
            .await
    });
    tokio::time::sleep(Duration::from_millis(40)).await;
    manager
        .change_document("rust", &opened.uri, "changed\n".into(), true)
        .await
        .unwrap();
    let stale = request.await.unwrap().unwrap();
    assert!(stale.stale);
    assert_eq!(stale.metadata.version, 1);

    manager.stop("rust").await.unwrap();
}

#[tokio::test]
async fn read_features_fail_closed_on_capability_and_cancel_on_timeout() {
    let app_data = tempdir().unwrap();
    let workspace = tempdir().unwrap();
    let document = workspace.path().join("main.rs");
    fs::write(&document, "fixture\n").unwrap();
    let executable = fixture_binary().canonicalize().unwrap();
    save_to_app_local_data_dir(
        app_data.path(),
        &feature_config(workspace.path(), &executable, "no_hover"),
    )
    .unwrap();
    let manager = LspManager::new(app_data.path(), "0.3.0");
    manager.start("rust").await.unwrap();
    let opened = manager
        .open_document("rust", &document, "fixture\n".into())
        .await
        .unwrap();
    assert!(matches!(
        manager
            .hover("rust", &opened.uri, LspPosition::new(0, 0))
            .await,
        Err(LspManagerError::UnsupportedFeature { method, .. }) if method == "textDocument/hover"
    ));
    manager.stop("rust").await.unwrap();

    let slow_app_data = tempdir().unwrap();
    let slow_workspace = tempdir().unwrap();
    let slow_document = slow_workspace.path().join("main.rs");
    fs::write(&slow_document, "fixture\n").unwrap();
    save_to_app_local_data_dir(
        slow_app_data.path(),
        &feature_config(slow_workspace.path(), &executable, "slow_features"),
    )
    .unwrap();
    let slow_manager = LspManager::new(slow_app_data.path(), "0.3.0");
    slow_manager.start("rust").await.unwrap();
    let slow_opened = slow_manager
        .open_document("rust", &slow_document, "fixture\n".into())
        .await
        .unwrap();
    let error = slow_manager
        .completion("rust", &slow_opened.uri, LspPosition::new(0, 0))
        .await
        .unwrap_err();
    assert!(error.to_string().to_ascii_lowercase().contains("timed out"));
    slow_manager.stop("rust").await.unwrap();
}

#[tokio::test]
async fn newer_completion_and_hover_cancel_the_previous_request() {
    let app_data = tempdir().unwrap();
    let workspace = tempdir().unwrap();
    let document = workspace.path().join("main.rs");
    fs::write(&document, "fixture\n").unwrap();
    let executable = fixture_binary().canonicalize().unwrap();
    save_to_app_local_data_dir(
        app_data.path(),
        &feature_config(workspace.path(), &executable, "supersede_features"),
    )
    .unwrap();

    let manager = Arc::new(LspManager::new(app_data.path(), "0.3.0"));
    manager.start("rust").await.unwrap();
    let opened = manager
        .open_document("rust", &document, "fixture\n".into())
        .await
        .unwrap();

    let first_manager = Arc::clone(&manager);
    let first_uri = opened.uri.clone();
    let first_completion = tokio::spawn(async move {
        first_manager
            .completion("rust", &first_uri, LspPosition::new(0, 0))
            .await
    });
    tokio::time::sleep(Duration::from_millis(40)).await;
    let current_completion = manager
        .completion("rust", &opened.uri, LspPosition::new(0, 1))
        .await
        .unwrap();
    assert_eq!(current_completion.value.items[0].label, "fixture");
    assert!(first_completion
        .await
        .unwrap()
        .unwrap_err()
        .to_string()
        .to_ascii_lowercase()
        .contains("cancelled"));

    let first_manager = Arc::clone(&manager);
    let first_uri = opened.uri.clone();
    let first_hover = tokio::spawn(async move {
        first_manager
            .hover("rust", &first_uri, LspPosition::new(0, 0))
            .await
    });
    tokio::time::sleep(Duration::from_millis(40)).await;
    let current_hover = manager
        .hover("rust", &opened.uri, LspPosition::new(0, 1))
        .await
        .unwrap();
    assert_eq!(current_hover.value.unwrap().text, "**fixture**");
    assert!(first_hover
        .await
        .unwrap()
        .unwrap_err()
        .to_string()
        .to_ascii_lowercase()
        .contains("cancelled"));

    manager.stop("rust").await.unwrap();
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

#[tokio::test]
async fn managed_start_revalidates_index_and_starts_only_the_reviewed_native_command() {
    let app_data = tempdir().unwrap();
    let workspace = tempdir().unwrap();
    let workspace_root = workspace.path().canonicalize().unwrap();
    let executable = fixture_binary().canonicalize().unwrap();
    let manager = LspManager::new(app_data.path(), "0.3.0");
    let manifest = initial_catalog()
        .into_iter()
        .find(|manifest| manifest.id == "rust-analyzer")
        .unwrap();
    let destination = app_data
        .path()
        .join("lsp/servers")
        .join(&manifest.id)
        .join(&manifest.version)
        .join(&manifest.platform);
    fs::create_dir_all(&destination).unwrap();
    let command_path = destination.join(&manifest.command.executable);
    fs::copy(&executable, &command_path).unwrap();
    let installed_path = destination.canonicalize().unwrap();
    let installed = InstalledServer {
        manifest_id: manifest.id.clone(),
        version: manifest.version.clone(),
        platform: manifest.platform.clone(),
        sha256: manifest.artifact.sha256.clone(),
        source_url: manifest.source_url.clone(),
        license: manifest.license.clone(),
        artifact_url: manifest.artifact.url.clone(),
        installed_path: installed_path.to_string_lossy().into_owned(),
        entrypoint: manifest.files.entrypoint.clone(),
        runtime: manifest.runtime.clone(),
        installed_at: "2026-08-13T00:00:00Z".into(),
        package_lock_sha256: manifest.files.package_lock_sha256.clone(),
    };
    let index_path = app_data.path().join("lsp/installed.json");
    let write_index = |server: InstalledServer| {
        let index = InstalledServerIndex {
            version: LSP_INSTALLED_SCHEMA_VERSION,
            servers: vec![server],
        };
        fs::write(&index_path, index.to_json().unwrap()).unwrap();
    };
    write_index(installed.clone());
    save_to_app_local_data_dir(
        app_data.path(),
        &LspConfig {
            version: LSP_CONFIG_SCHEMA_VERSION,
            enabled: true,
            workspace_root: workspace_root.to_string_lossy().into_owned(),
            server_by_language: BTreeMap::from([(
                "rust".into(),
                ServerRef::managed(&manifest.id, &manifest.version),
            )]),
            custom_servers: Vec::new(),
            update_policy: Default::default(),
        },
    )
    .unwrap();

    manager.start("rust").await.unwrap();
    assert_eq!(manager.statuses().await[0].language_id, "rust");
    manager.stop("rust").await.unwrap();

    let mut tampered_path = installed.clone();
    tampered_path.installed_path = workspace_root.to_string_lossy().into_owned();
    write_index(tampered_path);
    assert!(manager.start("rust").await.is_err());

    let mut tampered_entrypoint = installed;
    tampered_entrypoint.entrypoint = "missing.exe".into();
    write_index(tampered_entrypoint);
    assert!(manager.start("rust").await.is_err());
}
