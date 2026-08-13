//! Deterministic stdio server used by the transport/process integration tests.
//! It intentionally writes protocol data only to stdout and diagnostics only to
//! stderr, matching the boundary required of real language servers.

use code_pad_lib::lsp::{JsonRpcMessage, JsonRpcReader, JsonRpcWriter, RpcError, RpcId};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::env;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncWriteExt, Stdout};
use tokio::sync::Mutex;

type Writer = Arc<Mutex<JsonRpcWriter<Stdout>>>;
type Cancellations = Arc<Mutex<HashSet<u64>>>;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mode = env::var("FAKE_LSP_MODE").unwrap_or_default();
    if mode == "stderr" {
        let mut stderr = tokio::io::stderr();
        let bytes = vec![b'x'; 100 * 1024];
        let _ = stderr.write_all(&bytes).await;
        let _ = stderr.flush().await;
    }

    let writer: Writer = Arc::new(Mutex::new(JsonRpcWriter::new(tokio::io::stdout())));
    let cancellations: Cancellations = Arc::new(Mutex::new(HashSet::new()));
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
                tokio::spawn(async move {
                    handle_request(writer, cancellations, mode, id, method, params).await;
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
                    tokio::spawn(async move {
                        handle_request(writer, cancellations, mode, RpcId::Null, method, params)
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

async fn handle_request(
    writer: Writer,
    cancellations: Cancellations,
    mode: String,
    id: RpcId,
    method: String,
    params: Option<Value>,
) {
    match method.as_str() {
        "initialize" => {
            let argv: Vec<String> = env::args().skip(1).collect();
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
                let uri = params
                    .as_ref()
                    .and_then(|params| params.get("textDocument"))
                    .and_then(|document| document.get("uri"))
                    .and_then(Value::as_str)
                    .unwrap_or("file:///fixture");
                send(
                    &writer,
                    JsonRpcMessage::notification(
                        "textDocument/publishDiagnostics",
                        Some(json!({
                            "uri": uri,
                            "diagnostics": [{ "message": "fixture diagnostic", "severity": 2 }]
                        })),
                    ),
                )
                .await;
            }
        }
    }
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
