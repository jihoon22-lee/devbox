//! Deterministic stdio server used by the transport/process integration tests.
//! It intentionally writes protocol data only to stdout and diagnostics only to
//! stderr, matching the boundary required of real language servers.

use code_pad_lib::lsp::{JsonRpcMessage, JsonRpcReader, JsonRpcWriter, RpcError, RpcId};
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::Duration;
use tokio::io::{AsyncWriteExt, Stdout};
use tokio::sync::Mutex;

type Writer = Arc<Mutex<JsonRpcWriter<Stdout>>>;
type Cancellations = Arc<Mutex<HashSet<u64>>>;
type MutationChanges = Arc<Mutex<u32>>;

static FAKE_LOG_LOCK: OnceLock<StdMutex<()>> = OnceLock::new();

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let argument_mode =
        env::args().find_map(|argument| argument.strip_prefix("--fake-mode=").map(str::to_owned));
    let mode = argument_mode
        .or_else(|| env::var("FAKE_LSP_MODE").ok())
        .unwrap_or_default();
    let fake_marker =
        env::args().find_map(|argument| argument.strip_prefix("--fake-marker=").map(PathBuf::from));
    let fake_log =
        env::args().find_map(|argument| argument.strip_prefix("--fake-log=").map(PathBuf::from));
    if mode == "stderr" {
        let mut stderr = tokio::io::stderr();
        let bytes = vec![b'x'; 100 * 1024];
        let _ = stderr.write_all(&bytes).await;
        let _ = stderr.flush().await;
    }

    let writer: Writer = Arc::new(Mutex::new(JsonRpcWriter::new(tokio::io::stdout())));
    let cancellations: Cancellations = Arc::new(Mutex::new(HashSet::new()));
    let mutation_changes: MutationChanges = Arc::new(Mutex::new(0));
    let mut reader = JsonRpcReader::new(tokio::io::stdin());
    loop {
        let message = match reader.read_message().await {
            Ok(Some(message)) => message,
            Ok(None) | Err(_) if mode != "hang_shutdown" => break,
            Ok(None) | Err(_) => {
                tokio::time::sleep(Duration::from_secs(60)).await;
                continue;
            }
        };
        match message {
            JsonRpcMessage::Request { id, method, params } => {
                let writer = Arc::clone(&writer);
                let cancellations = Arc::clone(&cancellations);
                let mode = mode.clone();
                let fake_marker = fake_marker.clone();
                let fake_log = fake_log.clone();
                let mutation_changes = Arc::clone(&mutation_changes);
                tokio::spawn(async move {
                    handle_request(
                        writer,
                        cancellations,
                        mode,
                        fake_marker,
                        fake_log,
                        mutation_changes,
                        id,
                        method,
                        params,
                    )
                    .await;
                });
            }
            JsonRpcMessage::Notification { method, params } => {
                if method == "$/cancelRequest" {
                    if let Some(id) = params
                        .as_ref()
                        .and_then(|params| params.get("id"))
                        .and_then(Value::as_u64)
                    {
                        cancellations.lock().await.insert(id);
                    }
                } else if method == "textDocument/didOpen" || method == "textDocument/didChange" {
                    let writer = Arc::clone(&writer);
                    let cancellations = Arc::clone(&cancellations);
                    let mode = mode.clone();
                    let fake_marker = fake_marker.clone();
                    let fake_log = fake_log.clone();
                    let mutation_changes = Arc::clone(&mutation_changes);
                    tokio::spawn(async move {
                        handle_request(
                            writer,
                            cancellations,
                            mode,
                            fake_marker,
                            fake_log,
                            mutation_changes,
                            RpcId::Null,
                            method,
                            params,
                        )
                        .await;
                    });
                } else if method == "initialized" && mode == "dynamic_capabilities" {
                    send(
                        &writer,
                        JsonRpcMessage::request(
                            700_u64,
                            "client/registerCapability",
                            Some(json!({
                                "registrations": [{
                                    "id": "hover-registration",
                                    "method": "textDocument/hover"
                                }]
                            })),
                        ),
                    )
                    .await;
                } else if method == "exit" && mode != "hang_shutdown" {
                    break;
                }
            }
            JsonRpcMessage::Response { id, .. }
                if mode == "dynamic_capabilities" && id.as_u64() == Some(700) =>
            {
                send(
                    &writer,
                    JsonRpcMessage::notification("fixture/registered", Some(json!({}))),
                )
                .await;
                tokio::time::sleep(Duration::from_millis(100)).await;
                send(
                    &writer,
                    JsonRpcMessage::request(
                        701_u64,
                        "client/unregisterCapability",
                        Some(json!({
                            "unregisterations": [{
                                "id": "hover-registration",
                                "method": "textDocument/hover"
                            }]
                        })),
                    ),
                )
                .await;
            }
            JsonRpcMessage::Response { id, .. }
                if mode == "dynamic_capabilities" && id.as_u64() == Some(701) =>
            {
                send(
                    &writer,
                    JsonRpcMessage::notification("fixture/unregistered", Some(json!({}))),
                )
                .await;
            }
            JsonRpcMessage::Response { .. } | JsonRpcMessage::Error { .. } => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_request(
    writer: Writer,
    cancellations: Cancellations,
    mode: String,
    fake_marker: Option<PathBuf>,
    fake_log: Option<PathBuf>,
    mutation_changes: MutationChanges,
    id: RpcId,
    method: String,
    params: Option<Value>,
) {
    match method.as_str() {
        "initialize" => {
            let argv: Vec<String> = env::args().skip(1).collect();
            if mode == "slow_initialize" {
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
            if mode == "validate_initialize" && !valid_initialize_params(params.as_ref()) {
                send(
                    &writer,
                    JsonRpcMessage::error(id, RpcError::new(-32602, "invalid initialize contract")),
                )
                .await;
                return;
            }
            let capabilities = match mode.as_str() {
                "invalid_position" => json!({ "positionEncoding": "utf-32" }),
                "dynamic_capabilities" => json!({ "hoverProvider": false }),
                "no_hover" => json!({
                    "positionEncoding": "utf-8",
                    "textDocumentSync": { "openClose": true, "change": 2, "save": true },
                    "completionProvider": true,
                    "hoverProvider": false,
                    "definitionProvider": true,
                    "referencesProvider": {},
                    "diagnosticProvider": {}
                }),
                "no_sync" => json!({
                    "positionEncoding": "utf-8",
                    "renameProvider": { "prepareProvider": true },
                    "documentFormattingProvider": true
                }),
                "stale_features"
                | "supersede_features"
                | "mutation_features"
                | "stale_mutations"
                | "mutation_partial_failure" => json!({
                    "positionEncoding": "utf-8",
                    "textDocumentSync": { "openClose": true, "change": 2, "save": true },
                    "completionProvider": true,
                    "hoverProvider": true,
                    "definitionProvider": true,
                    "referencesProvider": {},
                    "renameProvider": { "prepareProvider": true },
                    "documentFormattingProvider": true,
                    "diagnosticProvider": {}
                }),
                _ => json!({
                    "positionEncoding": "utf-8",
                    "textDocumentSync": { "openClose": true, "change": 2, "save": true },
                    "completionProvider": true,
                    "hoverProvider": false,
                    "definitionProvider": true,
                    "referencesProvider": {},
                    "renameProvider": { "prepareProvider": true },
                    "documentFormattingProvider": true,
                    "diagnosticProvider": {}
                }),
            };
            send(
                &writer,
                JsonRpcMessage::response(
                    id,
                    json!({
                        "capabilities": capabilities,
                        "serverInfo": { "name": "fake-lsp", "version": "1.0.0" },
                        "argv": argv
                    }),
                ),
            )
            .await;
        }
        "shutdown" => {
            if mode != "hang_shutdown" {
                send(&writer, JsonRpcMessage::response(id, Value::Null)).await;
            }
        }
        "first" => {
            tokio::time::sleep(Duration::from_millis(60)).await;
            send(&writer, JsonRpcMessage::response(id, json!("first"))).await;
        }
        "second" => {
            send(&writer, JsonRpcMessage::response(id, json!("second"))).await;
        }
        "fail" => {
            send(
                &writer,
                JsonRpcMessage::error(id, RpcError::new(-32001, "fixture failure")),
            )
            .await;
        }
        "slow" => {
            tokio::time::sleep(Duration::from_millis(400)).await;
            let cancelled = cancellations.lock().await.remove(&id.as_u64().unwrap_or(0));
            if cancelled {
                send(
                    &writer,
                    JsonRpcMessage::error(id, RpcError::new(-32800, "request cancelled")),
                )
                .await;
            } else {
                send(&writer, JsonRpcMessage::response(id, json!("slow"))).await;
            }
        }
        "textDocument/diagnostic"
        | "textDocument/completion"
        | "textDocument/hover"
        | "textDocument/definition"
        | "textDocument/references"
        | "textDocument/rename"
        | "textDocument/formatting" => {
            if mode == "mutation_partial_failure" && method == "textDocument/didChange" {
                let mut changes = mutation_changes.lock().await;
                *changes += 1;
                if *changes >= 2 {
                    std::process::exit(17);
                }
            } else if mode == "stale_features" && method == "textDocument/completion" {
                tokio::time::sleep(Duration::from_millis(120)).await;
            } else if mode == "supersede_features"
                && matches!(
                    method.as_str(),
                    "textDocument/completion" | "textDocument/hover"
                )
                && params
                    .as_ref()
                    .and_then(|params| params.pointer("/position/character"))
                    .and_then(Value::as_u64)
                    == Some(0)
            {
                tokio::time::sleep(Duration::from_millis(400)).await;
            } else if mode == "slow_features"
                && matches!(
                    method.as_str(),
                    "textDocument/completion" | "textDocument/hover"
                )
            {
                tokio::time::sleep(Duration::from_secs(3)).await;
            } else if mode == "stale_mutations"
                && matches!(
                    method.as_str(),
                    "textDocument/rename" | "textDocument/formatting"
                )
            {
                tokio::time::sleep(Duration::from_millis(120)).await;
            }
            if cancellations.lock().await.remove(&id.as_u64().unwrap_or(0)) {
                send(
                    &writer,
                    JsonRpcMessage::error(id, RpcError::new(-32800, "request cancelled")),
                )
                .await;
                return;
            }
            let uri = params
                .as_ref()
                .and_then(|params| params.get("textDocument"))
                .and_then(|document| document.get("uri"))
                .and_then(Value::as_str)
                .unwrap_or("file:///fixture.rs");
            let range = json!({
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 2 }
            });
            let result = match method.as_str() {
                "textDocument/diagnostic" => json!({
                    "kind": "full",
                    "resultId": "fixture-diagnostics",
                    "items": [{ "range": range.clone(), "severity": 2, "message": "fixture diagnostic" }]
                }),
                "textDocument/completion" => json!({
                    "isIncomplete": false,
                    "items": [{ "label": "fixture" }]
                }),
                "textDocument/hover" => json!({
                    "contents": { "kind": "markdown", "value": "**fixture**" }
                }),
                "textDocument/definition" => json!([
                    { "uri": uri, "range": range.clone() },
                    { "uri": "file:///outside-workspace.rs", "range": range }
                ]),
                "textDocument/references" => json!([
                    { "uri": uri, "range": range.clone() },
                    { "uri": "file:///outside-workspace.rs", "range": range }
                ]),
                "textDocument/rename" => {
                    let sibling = uri
                        .rsplit_once('/')
                        .map(|(directory, _)| format!("{directory}/lib.rs"))
                        .unwrap_or_else(|| uri.to_owned());
                    let mut changes = Map::new();
                    changes.insert(
                        uri.to_owned(),
                        json!([{ "range": range.clone(), "newText": "renamed" }]),
                    );
                    changes.insert(
                        sibling,
                        json!([{ "range": range.clone(), "newText": "renamed" }]),
                    );
                    json!({ "changes": changes })
                }
                "textDocument/formatting" => json!([{
                    "range": range,
                    "newText": "formatted"
                }]),
                _ => Value::Null,
            };
            send(&writer, JsonRpcMessage::response(id, result)).await;
        }
        "crash" => {
            std::process::exit(17);
        }
        "emitUnknown" => {
            send(
                &writer,
                JsonRpcMessage::response(999_999_u64, json!("unknown")),
            )
            .await;
            send(&writer, JsonRpcMessage::response(id, json!("done"))).await;
        }
        _ => {
            if method == "textDocument/didOpen" || method == "textDocument/didChange" {
                if method == "textDocument/didOpen" {
                    if let Some(log) = fake_log.as_ref() {
                        append_fake_log(log, params.as_ref());
                    }
                    if mode == "crash_on_open"
                        || (mode == "crash_once"
                            && match fake_marker.as_ref() {
                                None => true,
                                Some(marker) => !marker.exists(),
                            })
                    {
                        if let Some(marker) = fake_marker.as_ref() {
                            let _ = fs::write(marker, b"crashed");
                        }
                        std::process::exit(17);
                    }
                } else if mode == "mutation_partial_failure" {
                    let mut changes = mutation_changes.lock().await;
                    *changes += 1;
                    if *changes >= 1 {
                        std::process::exit(17);
                    }
                }
                let uri = params
                    .as_ref()
                    .and_then(|params| params.get("textDocument"))
                    .and_then(|document| document.get("uri"))
                    .and_then(Value::as_str)
                    .unwrap_or("file:///fixture");
                let version = params
                    .as_ref()
                    .and_then(|params| params.get("textDocument"))
                    .and_then(|document| document.get("version"))
                    .cloned()
                    .unwrap_or(Value::Null);
                send(
                    &writer,
                    JsonRpcMessage::notification(
                        "textDocument/publishDiagnostics",
                        Some(json!({
                            "uri": uri,
                            "version": version,
                            "diagnostics": [{
                                "range": {
                                    "start": { "line": 0, "character": 0 },
                                    "end": { "line": 0, "character": 2 }
                                },
                                "message": "fixture diagnostic",
                                "severity": 2
                            }]
                        })),
                    ),
                )
                .await;
            }
        }
    }
}

fn append_fake_log(path: &PathBuf, params: Option<&Value>) {
    let Some(document) = params.and_then(|value| value.get("textDocument")) else {
        return;
    };
    let lock = FAKE_LOG_LOCK.get_or_init(|| StdMutex::new(()));
    let Ok(_guard) = lock.lock() else {
        return;
    };
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let line = format!("{}\n", document);
    let _ = file.write_all(line.as_bytes());
    let _ = file.flush();
    let _ = file.sync_data();
}

fn valid_initialize_params(params: Option<&Value>) -> bool {
    let Some(params) = params else {
        return false;
    };
    params.get("processId").and_then(Value::as_u64) == Some(4242)
        && params.pointer("/clientInfo/name").and_then(Value::as_str) == Some("code-pad")
        && params
            .pointer("/clientInfo/version")
            .and_then(Value::as_str)
            == Some("0.3.0")
        && params.get("rootUri").and_then(Value::as_str).is_some()
        && params
            .pointer("/workspaceFolders/0/uri")
            .and_then(Value::as_str)
            == params.get("rootUri").and_then(Value::as_str)
        && params
            .pointer("/capabilities/general/positionEncodings")
            .and_then(Value::as_array)
            == Some(&vec![json!("utf-16"), json!("utf-8")])
        && params
            .pointer("/capabilities/workspace/applyEdit")
            .and_then(Value::as_bool)
            == Some(true)
}

async fn send(writer: &Writer, message: JsonRpcMessage) {
    let mut writer = writer.lock().await;
    let _ = writer.write_message(&message).await;
}
