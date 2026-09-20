use code_pad_lib::lsp::{
    CapabilitySet, ClientStatus, IncomingMessage, InitializeConfig, JsonRpcMessage, LspClient,
    LspProcess, PositionEncoding, ProcessSpec, SyncKind, WorkspaceRoot,
};
use std::path::PathBuf;
use std::time::Duration;
use tempfile::tempdir;

fn fixture(mode: &str) -> ProcessSpec {
    let test_binary = std::env::current_exe()
        .expect("test executable path")
        .parent()
        .and_then(|deps| deps.parent())
        .expect("Cargo test executable should live below target/debug/deps")
        .join(if cfg!(windows) {
            "fake-lsp-server.exe"
        } else {
            "fake-lsp-server"
        });
    ProcessSpec::new(test_binary, PathBuf::from(env!("CARGO_MANIFEST_DIR")))
        .env("FAKE_LSP_MODE", mode)
}

async fn initialize(mode: &str, process_id: u32) -> (LspProcess, LspClient, CapabilitySet) {
    let directory = tempdir().unwrap();
    let workspace = WorkspaceRoot::new(directory.path()).unwrap();
    let process = LspProcess::spawn(fixture(mode)).await.unwrap();
    let client = LspClient::new(process.clone());
    let capabilities = client
        .initialize(
            &workspace,
            InitializeConfig::new("0.3.0").with_process_id(Some(process_id)),
        )
        .await
        .unwrap();
    (process, client, capabilities)
}

async fn receive_notification(
    receiver: &mut tokio::sync::broadcast::Receiver<IncomingMessage>,
    expected: &str,
) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if matches!(
                receiver.recv().await.unwrap(),
                IncomingMessage::Message(JsonRpcMessage::Notification { method, .. })
                    if method == expected
            ) {
                return;
            }
        }
    })
    .await
    .expect("timed out waiting for fixture notification");
}

#[tokio::test]
async fn initialize_contract_and_static_capabilities_are_normalized() {
    let (_process, client, capabilities) = initialize("validate_initialize", 4242).await;
    assert_eq!(client.status(), ClientStatus::Ready);
    assert_eq!(capabilities.position_encoding, PositionEncoding::Utf8);
    assert_eq!(capabilities.sync_kind, Some(SyncKind::Incremental));
    assert!(capabilities.open_close && capabilities.save);
    assert!(capabilities.supports("textDocument/completion"));
    assert!(!capabilities.supports("textDocument/hover"));
    assert_eq!(client.server_info().await.unwrap().name, "fake-lsp");
    client.stop().await.unwrap();
}

#[tokio::test]
async fn dynamic_registration_and_unregistration_update_the_allowlist() {
    let directory = tempdir().unwrap();
    let workspace = WorkspaceRoot::new(directory.path()).unwrap();
    let process = LspProcess::spawn(fixture("dynamic_capabilities"))
        .await
        .unwrap();
    let mut events = process.subscribe();
    let client = LspClient::new(process.clone());
    let initial = client
        .initialize(&workspace, InitializeConfig::new("0.3.0"))
        .await
        .unwrap();
    assert!(!initial.supports("textDocument/hover"));

    receive_notification(&mut events, "fixture/registered").await;
    assert!(client.capabilities().await.supports("textDocument/hover"));
    receive_notification(&mut events, "fixture/unregistered").await;
    assert!(!client.capabilities().await.supports("textDocument/hover"));
    client.stop().await.unwrap();
}

#[tokio::test]
async fn unsupported_position_encoding_degrades_without_becoming_ready() {
    let directory = tempdir().unwrap();
    let workspace = WorkspaceRoot::new(directory.path()).unwrap();
    let process = LspProcess::spawn(fixture("invalid_position"))
        .await
        .unwrap();
    let client = LspClient::new(process);
    assert!(client
        .initialize(&workspace, InitializeConfig::new("0.3.0"))
        .await
        .is_err());
    assert_eq!(client.status(), ClientStatus::Degraded);
    client.stop().await.unwrap();
}
