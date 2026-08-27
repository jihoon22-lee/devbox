use code_pad_lib::lsp::{
    initial_catalog, save_to_app_local_data_dir, InstallSource, InstalledServer,
    InstalledServerIndex, LspConfig, LspEvent, LspManager, LspManagerError, LspPosition, ServerRef,
    LSP_CONFIG_SCHEMA_VERSION, LSP_INSTALLED_SCHEMA_VERSION,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use url::Url;

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
    manager.shutdown_for_exit().await.unwrap();
    assert!(manager.start("rust").await.is_err());
}

#[tokio::test]
async fn push_diagnostics_are_cached_and_published_with_current_version() {
    let app_data = tempdir().unwrap();
    let workspace = tempdir().unwrap();
    let document = workspace.path().join("main.rs");
    fs::write(&document, "fn main() {}\n").unwrap();
    let executable = fixture_binary().canonicalize().unwrap();
    save_to_app_local_data_dir(
        app_data.path(),
        &feature_config(workspace.path(), &executable, "push_diagnostics"),
    )
    .unwrap();
    let manager = LspManager::new(app_data.path(), "0.3.0");
    let mut events = manager.subscribe_events();
    manager.start("rust").await.unwrap();
    let opened = manager
        .open_document("rust", &document, "fn main() {}\n".into())
        .await
        .unwrap();
    let event = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let LspEvent::Diagnostics(event) = events.recv().await.unwrap() {
                break event;
            }
        }
    })
    .await
    .expect("push diagnostics event");
    assert_eq!(event.language_id, "rust");
    assert_eq!(event.response.metadata.uri, opened.uri);
    assert_eq!(event.response.metadata.version, 1);
    assert_eq!(
        event.response.value.origin,
        code_pad_lib::lsp::DiagnosticOrigin::Push
    );
    assert!(!event.response.stale);
    manager.stop("rust").await.unwrap();
}

#[tokio::test]
async fn crash_backoff_replays_the_latest_open_change_close_snapshot_atomically() {
    let app_data = tempdir().unwrap();
    let workspace = tempdir().unwrap();
    let marker = app_data.path().join("crash-once.marker");
    let log = app_data.path().join("did-open.jsonl");
    let first = workspace.path().join("first.rs");
    let second = workspace.path().join("second.rs");
    fs::write(&first, "first\n").unwrap();
    fs::write(&second, "second\n").unwrap();
    // Canonicalize so the test URIs match the manager's canonical document
    // URIs. On Windows `tempdir` may return an 8.3 short path while the
    // manager resolves the long path via `fs::canonicalize`.
    let first = fs::canonicalize(&first).unwrap();
    let second = fs::canonicalize(&second).unwrap();
    let executable = fixture_binary().canonicalize().unwrap();
    let config = feature_config_with_args(
        workspace.path(),
        &executable,
        "crash_once",
        [
            format!("--fake-marker={}", marker.display()),
            format!("--fake-log={}", log.display()),
        ],
    );
    save_to_app_local_data_dir(app_data.path(), &config).unwrap();

    let manager = LspManager::new(app_data.path(), "0.3.0");
    let mut events = manager.subscribe_events();
    manager.start("rust").await.unwrap();
    let first_uri = Url::from_file_path(&first).unwrap().to_string();
    let second_uri = Url::from_file_path(&second).unwrap().to_string();
    let _ = manager
        .open_document("rust", &first, "first\n".into())
        .await;

    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let LspEvent::Status(event) = events.recv().await.unwrap() {
                if event.restarting
                    && event.status.status == code_pad_lib::lsp::ClientStatus::Crashed
                {
                    break;
                }
            }
        }
    })
    .await
    .expect("crash status");
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if fs::read_to_string(&log)
                .map(|contents| contents.lines().count() >= 1)
                .unwrap_or(false)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("initial didOpen log");

    // Every mutation commits its authoritative store before reporting that
    // the dead process could not receive the notification.
    let _ = manager
        .change_document("rust", &first_uri, "latest first\n".into(), true)
        .await;
    let _ = manager
        .open_document("rust", &second, "latest second\n".into())
        .await;
    let _ = manager
        .change_document("rust", &second_uri, "latest second\n".into(), true)
        .await;
    let _ = manager.close_document("rust", &first_uri).await;

    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if let LspEvent::Status(event) = events.recv().await.unwrap() {
                if !event.restarting
                    && event.status.status == code_pad_lib::lsp::ClientStatus::Ready
                {
                    break;
                }
            }
        }
    })
    .await
    .expect("replacement session");

    // The replacement must replay only the authoritative document store. The
    // didOpen log is the ground truth: the second document only, with its
    // latest text, and no re-open of the closed first document.
    let lines = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let lines = fs::read_to_string(&log)
                .unwrap_or_default()
                .lines()
                .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                .collect::<Vec<_>>();
            if lines.len() >= 2 {
                break lines;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("initial and replacement didOpen entries");
    assert!(lines.len() >= 2, "initial and replacement didOpen entries");
    let replay = lines.last().unwrap();
    assert_eq!(replay["uri"], second_uri);
    assert_eq!(replay["version"], 1);
    assert_eq!(replay["text"], "latest second\n");
    assert!(lines[1..].iter().all(|entry| entry["uri"] != first_uri));

    manager.close_document("rust", &second_uri).await.unwrap();
    manager.stop("rust").await.unwrap();
}

#[tokio::test]
async fn circuit_open_allows_explicit_restart_after_repeated_crashes() {
    let app_data = tempdir().unwrap();
    let workspace = tempdir().unwrap();
    let marker = app_data.path().join("always-crash.marker");
    let document = workspace.path().join("main.rs");
    fs::write(&document, "fixture\n").unwrap();
    let executable = fixture_binary().canonicalize().unwrap();
    save_to_app_local_data_dir(
        app_data.path(),
        &feature_config_with_args(
            workspace.path(),
            &executable,
            "crash_on_open",
            [format!("--fake-marker={}", marker.display())],
        ),
    )
    .unwrap();

    let manager = LspManager::new(app_data.path(), "0.3.0");
    manager.start("rust").await.unwrap();
    let _ = manager
        .open_document("rust", &document, "fixture\n".into())
        .await;
    tokio::time::timeout(Duration::from_secs(7), async {
        loop {
            let status = manager
                .statuses()
                .await
                .into_iter()
                .find(|status| status.language_id == "rust");
            if status.is_some_and(|status| status.auto_restart_disabled) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("automatic restart circuit open");

    manager.restart("rust").await.unwrap();
    let status = manager
        .statuses()
        .await
        .into_iter()
        .find(|status| status.language_id == "rust")
        .unwrap();
    assert_eq!(status.status, code_pad_lib::lsp::ClientStatus::Ready);
    assert!(!status.auto_restart_disabled);
    manager.stop("rust").await.unwrap();
}

#[tokio::test]
async fn canceled_start_releases_its_reservation_before_orderly_shutdown() {
    let app_data = tempdir().unwrap();
    let workspace = tempdir().unwrap();
    let executable = fixture_binary().canonicalize().unwrap();
    save_to_app_local_data_dir(
        app_data.path(),
        &feature_config_with_args(
            workspace.path(),
            &executable,
            "slow_initialize",
            std::iter::empty::<String>(),
        ),
    )
    .unwrap();
    let manager = LspManager::new(app_data.path(), "0.3.0");
    let pending = tokio::spawn({
        let manager = manager.clone();
        async move { manager.start("rust").await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    pending.abort();
    let _ = pending.await;

    tokio::time::timeout(Duration::from_secs(2), manager.shutdown_for_exit())
        .await
        .expect("canceled start must not keep shutdown waiting")
        .unwrap();
    assert!(manager.start("rust").await.is_err());
}

fn feature_config(
    workspace: &std::path::Path,
    executable: &std::path::Path,
    mode: &str,
) -> LspConfig {
    feature_config_with_args(workspace, executable, mode, std::iter::empty::<String>())
}

fn feature_config_with_args<I>(
    workspace: &std::path::Path,
    executable: &std::path::Path,
    mode: &str,
    additional_args: I,
) -> LspConfig
where
    I: IntoIterator<Item = String>,
{
    let mut args = vec![format!("--fake-mode={mode}")];
    args.extend(additional_args);
    LspConfig {
        version: LSP_CONFIG_SCHEMA_VERSION,
        enabled: true,
        workspace_root: workspace.to_string_lossy().into_owned(),
        server_by_language: BTreeMap::from([(
            "rust".into(),
            ServerRef::Local {
                installed_path: executable.to_string_lossy().into_owned(),
                executable: None,
                args,
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

#[tokio::test]
async fn rename_and_formatting_apply_complete_dirty_buffers_atomically() {
    let app_data = tempdir().unwrap();
    let workspace = tempdir().unwrap();
    let main = workspace.path().join("main.rs");
    let library = workspace.path().join("lib.rs");
    fs::write(&main, "fixture\n").unwrap();
    fs::write(&library, "fixture library\n").unwrap();
    let executable = fixture_binary().canonicalize().unwrap();
    save_to_app_local_data_dir(
        app_data.path(),
        &feature_config(workspace.path(), &executable, "mutation_features"),
    )
    .unwrap();

    let manager = LspManager::new(app_data.path(), "0.3.0");
    manager.start("rust").await.unwrap();
    let main_opened = manager
        .open_document("rust", &main, "fixture\n".into())
        .await
        .unwrap();
    manager
        .open_document("rust", &library, "fixture library\n".into())
        .await
        .unwrap();

    let renamed = manager
        .rename(
            "rust",
            &main_opened.uri,
            LspPosition::new(0, 0),
            "renamed".into(),
        )
        .await
        .unwrap();
    assert_eq!(renamed.documents.len(), 2);
    assert!(renamed
        .documents
        .iter()
        .all(|document| document.version == 2 && document.text.starts_with("renamed")));

    let formatted = manager
        .formatting("rust", &main_opened.uri, 4, true)
        .await
        .unwrap();
    assert_eq!(formatted.documents.len(), 1);
    assert_eq!(formatted.documents[0].version, 3);
    assert_eq!(formatted.documents[0].text, "formattednamedxture\n");
    assert!(manager
        .rename(
            "rust",
            &main_opened.uri,
            LspPosition::new(0, 0),
            "bad\nname".into(),
        )
        .await
        .is_err());
    assert!(manager
        .formatting("rust", &main_opened.uri, 0, true)
        .await
        .is_err());
    manager.stop("rust").await.unwrap();
}

#[tokio::test]
async fn mutation_version_race_rejects_every_document_without_partial_commit() {
    let app_data = tempdir().unwrap();
    let workspace = tempdir().unwrap();
    let main = workspace.path().join("main.rs");
    let library = workspace.path().join("lib.rs");
    fs::write(&main, "fixture\n").unwrap();
    fs::write(&library, "fixture library\n").unwrap();
    let executable = fixture_binary().canonicalize().unwrap();
    save_to_app_local_data_dir(
        app_data.path(),
        &feature_config(workspace.path(), &executable, "stale_mutations"),
    )
    .unwrap();

    let manager = Arc::new(LspManager::new(app_data.path(), "0.3.0"));
    manager.start("rust").await.unwrap();
    let main_opened = manager
        .open_document("rust", &main, "fixture\n".into())
        .await
        .unwrap();
    manager
        .open_document("rust", &library, "fixture library\n".into())
        .await
        .unwrap();

    let request_manager = Arc::clone(&manager);
    let request_uri = main_opened.uri.clone();
    let request = tokio::spawn(async move {
        request_manager
            .rename(
                "rust",
                &request_uri,
                LspPosition::new(0, 0),
                "renamed".into(),
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(40)).await;
    manager
        .change_document("rust", &main_opened.uri, "changed\n".into(), true)
        .await
        .unwrap();
    let error = request.await.unwrap().unwrap_err().to_string();
    assert!(error.contains("version conflict"));

    let retry = manager
        .rename(
            "rust",
            &main_opened.uri,
            LspPosition::new(0, 0),
            "renamed".into(),
        )
        .await
        .unwrap();
    let library = retry
        .documents
        .iter()
        .find(|document| document.uri.ends_with("/lib.rs"))
        .unwrap();
    assert_eq!(library.version, 2, "the rejected request changed lib.rs");
    assert_eq!(library.text, "renamedxture library\n");
    manager.stop("rust").await.unwrap();
}

#[tokio::test]
async fn failed_multi_document_mutation_restarts_and_replays_the_authoritative_snapshot() {
    let app_data = tempdir().unwrap();
    let workspace = tempdir().unwrap();
    let marker = app_data.path().join("mutation-count");
    let log = app_data.path().join("did-open.jsonl");
    let main = workspace.path().join("main.rs");
    let library = workspace.path().join("lib.rs");
    fs::write(&main, "fixture\n").unwrap();
    fs::write(&library, "fixture library\n").unwrap();
    let executable = fixture_binary().canonicalize().unwrap();
    save_to_app_local_data_dir(
        app_data.path(),
        &feature_config_with_args(
            workspace.path(),
            &executable,
            "mutation_partial_failure",
            [
                format!("--fake-marker={}", marker.display()),
                format!("--fake-log={}", log.display()),
            ],
        ),
    )
    .unwrap();

    let manager = LspManager::new(app_data.path(), "0.3.0");
    manager.start("rust").await.unwrap();
    let main_opened = manager
        .open_document("rust", &main, "fixture\n".into())
        .await
        .unwrap();
    manager
        .open_document("rust", &library, "fixture library\n".into())
        .await
        .unwrap();

    let error = manager
        .rename(
            "rust",
            &main_opened.uri,
            LspPosition::new(0, 0),
            "renamed".into(),
        )
        .await
        .expect_err("the second didChange must fail");
    assert!(
        error.to_string().contains("workspace edit notification"),
        "unexpected partial mutation error: {error}"
    );

    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let status = manager
                .statuses()
                .await
                .into_iter()
                .find(|status| status.language_id == "rust");
            if status.is_some_and(|status| {
                status.status == code_pad_lib::lsp::ClientStatus::Ready
                    && status.document_count == 2
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("replacement session after partial mutation");

    let lines = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let lines = fs::read_to_string(&log)
                .unwrap_or_default()
                .lines()
                .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                .collect::<Vec<_>>();
            if lines.len() >= 4 {
                break lines;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("replacement didOpen entries");
    let replay = &lines[lines.len() - 2..];
    let main_replay = replay
        .iter()
        .find(|entry| {
            entry["uri"]
                .as_str()
                .is_some_and(|uri| uri.ends_with("/main.rs"))
        })
        .unwrap();
    let library_replay = replay
        .iter()
        .find(|entry| {
            entry["uri"]
                .as_str()
                .is_some_and(|uri| uri.ends_with("/lib.rs"))
        })
        .unwrap();
    assert_eq!(main_replay["version"], 1);
    assert_eq!(main_replay["text"], "fixture\n");
    assert_eq!(library_replay["version"], 1);
    assert_eq!(library_replay["text"], "fixture library\n");
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
        install_source: InstallSource::Network,
        last_verified_at: Some("2026-08-13T00:00:00Z".into()),
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
