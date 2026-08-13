//! LSP initialization, capability normalization, and server-request handling.

use super::documents::{SyncKind, WorkspaceRoot};
use super::positions::{PositionEncoding, PositionError};
use super::process::{IncomingMessage, LspProcess, ProcessError, RequestError};
use super::transport::{JsonRpcMessage, RequestCancellation, RpcError, RpcId};
use lsp_types::TextDocumentSyncKind;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::fmt;
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::sync::{watch, Mutex, RwLock};

pub const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct InitializeConfig {
    pub process_id: Option<u32>,
    pub app_version: String,
    pub initialization_options: Option<Value>,
}

impl InitializeConfig {
    pub fn new(app_version: impl Into<String>) -> Self {
        Self {
            process_id: Some(std::process::id()),
            app_version: app_version.into(),
            initialization_options: None,
        }
    }

    pub fn with_process_id(mut self, process_id: Option<u32>) -> Self {
        self.process_id = process_id;
        self
    }

    pub fn with_initialization_options(mut self, options: Value) -> Result<Self, ClientError> {
        if !options.is_object() {
            return Err(ClientError::InvalidInitializeResult(
                "initializationOptions must be a JSON object".into(),
            ));
        }
        self.initialization_options = Some(options);
        Ok(self)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ClientStatus {
    Starting,
    Ready,
    Degraded,
    Stopped,
    Crashed,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub name: String,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitySet {
    pub position_encoding: PositionEncoding,
    pub legacy_position_encoding: bool,
    pub sync_kind: Option<SyncKind>,
    pub open_close: bool,
    pub save: bool,
    pub completion: bool,
    pub hover: bool,
    pub definition: bool,
    pub references: bool,
    pub rename: bool,
    pub formatting: bool,
    pub diagnostics: bool,
    dynamic_methods: BTreeSet<String>,
}

impl Default for CapabilitySet {
    fn default() -> Self {
        Self {
            position_encoding: PositionEncoding::Utf16,
            legacy_position_encoding: true,
            sync_kind: None,
            open_close: false,
            save: false,
            completion: false,
            hover: false,
            definition: false,
            references: false,
            rename: false,
            formatting: false,
            diagnostics: false,
            dynamic_methods: BTreeSet::new(),
        }
    }
}

impl CapabilitySet {
    pub fn from_initialize_result(
        result: &Value,
    ) -> Result<(Self, Option<ServerInfo>), ClientError> {
        let root = result.as_object().ok_or_else(|| {
            ClientError::InvalidInitializeResult("initialize result must be an object".into())
        })?;
        let capabilities = root
            .get("capabilities")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                ClientError::InvalidInitializeResult(
                    "initialize result.capabilities must be an object".into(),
                )
            })?;
        let encoding_value = capabilities.get("positionEncoding").and_then(Value::as_str);
        let (position_encoding, legacy_position_encoding) =
            PositionEncoding::negotiate(encoding_value).map_err(ClientError::Position)?;
        let (sync_kind, open_close, save) = parse_sync(capabilities.get("textDocumentSync"))?;

        let server_info = root.get("serverInfo").map(parse_server_info).transpose()?;
        Ok((
            Self {
                position_encoding,
                legacy_position_encoding,
                sync_kind,
                open_close,
                save,
                completion: capability_enabled(capabilities.get("completionProvider")),
                hover: capability_enabled(capabilities.get("hoverProvider")),
                definition: capability_enabled(capabilities.get("definitionProvider")),
                references: capability_enabled(capabilities.get("referencesProvider")),
                rename: capability_enabled(capabilities.get("renameProvider")),
                formatting: capability_enabled(capabilities.get("documentFormattingProvider")),
                diagnostics: capability_enabled(capabilities.get("diagnosticProvider")),
                dynamic_methods: BTreeSet::new(),
            },
            server_info,
        ))
    }

    pub fn supports(&self, method: &str) -> bool {
        self.dynamic_methods.contains(method)
            || match method {
                "textDocument/didOpen" | "textDocument/didClose" => self.open_close,
                "textDocument/didChange" => self.sync_kind.is_some(),
                "textDocument/didSave" => self.save,
                "textDocument/completion" => self.completion,
                "textDocument/hover" => self.hover,
                "textDocument/definition" => self.definition,
                "textDocument/references" => self.references,
                "textDocument/rename" => self.rename,
                "textDocument/formatting" => self.formatting,
                "textDocument/diagnostic" => self.diagnostics,
                _ => false,
            }
    }

    pub fn dynamic_methods(&self) -> impl Iterator<Item = &str> {
        self.dynamic_methods.iter().map(String::as_str)
    }

    fn register(&mut self, method: String) {
        if is_supported_dynamic_method(&method) {
            self.dynamic_methods.insert(method);
        }
    }

    fn unregister(&mut self, method: &str) {
        self.dynamic_methods.remove(method);
    }
}

#[derive(Debug)]
pub enum ClientError {
    Process(ProcessError),
    Request(RequestError),
    Position(PositionError),
    InvalidInitializeResult(String),
    InvalidServerRequest(String),
    NotReady,
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Process(error) => error.fmt(f),
            Self::Request(error) => error.fmt(f),
            Self::Position(error) => error.fmt(f),
            Self::InvalidInitializeResult(message) => {
                write!(f, "invalid language-server initialize result: {message}")
            }
            Self::InvalidServerRequest(message) => {
                write!(f, "invalid language-server request: {message}")
            }
            Self::NotReady => f.write_str("language server is not initialized"),
        }
    }
}

impl std::error::Error for ClientError {}

impl From<ProcessError> for ClientError {
    fn from(error: ProcessError) -> Self {
        Self::Process(error)
    }
}

impl From<RequestError> for ClientError {
    fn from(error: RequestError) -> Self {
        Self::Request(error)
    }
}

struct ClientInner {
    process: LspProcess,
    status: watch::Sender<ClientStatus>,
    capabilities: RwLock<CapabilitySet>,
    server_info: RwLock<Option<ServerInfo>>,
    initialization: Mutex<()>,
}

#[derive(Clone)]
pub struct LspClient {
    inner: Arc<ClientInner>,
}

impl LspClient {
    pub fn new(process: LspProcess) -> Self {
        let (status, _) = watch::channel(ClientStatus::Stopped);
        let mut incoming = process.subscribe();
        let inner = Arc::new(ClientInner {
            process,
            status,
            capabilities: RwLock::new(CapabilitySet::default()),
            server_info: RwLock::new(None),
            initialization: Mutex::new(()),
        });
        let weak = Arc::downgrade(&inner);
        tokio::spawn(async move {
            while let Ok(message) = incoming.recv().await {
                let Some(inner) = Weak::upgrade(&weak) else {
                    break;
                };
                handle_incoming(&inner, message).await;
            }
        });
        Self { inner }
    }

    pub async fn initialize(
        &self,
        workspace: &WorkspaceRoot,
        config: InitializeConfig,
    ) -> Result<CapabilitySet, ClientError> {
        let _guard = self.inner.initialization.lock().await;
        self.inner.status.send_replace(ClientStatus::Starting);
        let params = initialize_params(workspace, config);
        let result = match self
            .inner
            .process
            .request("initialize", Some(params), INITIALIZE_TIMEOUT)
            .await
        {
            Ok(result) => result,
            Err(error) => {
                self.inner.status.send_replace(ClientStatus::Degraded);
                return Err(ClientError::Request(error));
            }
        };
        let (capabilities, server_info) = match CapabilitySet::from_initialize_result(&result) {
            Ok(parsed) => parsed,
            Err(error) => {
                self.inner.status.send_replace(ClientStatus::Degraded);
                return Err(error);
            }
        };
        *self.inner.capabilities.write().await = capabilities.clone();
        *self.inner.server_info.write().await = server_info;
        if let Err(error) = self
            .inner
            .process
            .notify("initialized", Some(json!({})))
            .await
        {
            self.inner.status.send_replace(ClientStatus::Degraded);
            return Err(ClientError::Process(error));
        }
        self.inner.status.send_replace(ClientStatus::Ready);
        Ok(capabilities)
    }

    pub fn subscribe_status(&self) -> watch::Receiver<ClientStatus> {
        self.inner.status.subscribe()
    }

    pub fn status(&self) -> ClientStatus {
        self.inner.status.borrow().clone()
    }

    pub async fn capabilities(&self) -> CapabilitySet {
        self.inner.capabilities.read().await.clone()
    }

    pub async fn server_info(&self) -> Option<ServerInfo> {
        self.inner.server_info.read().await.clone()
    }

    pub async fn request(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, ClientError> {
        if self.status() != ClientStatus::Ready
            || !self.inner.capabilities.read().await.supports(method)
        {
            return Err(ClientError::NotReady);
        }
        self.inner
            .process
            .request(method, Some(params), timeout)
            .await
            .map_err(ClientError::Request)
    }

    /// Send a feature request that can be superseded by newer editor input.
    /// Capability and ready-state checks intentionally match `request`; only
    /// the cooperative cancellation signal differs.
    pub async fn request_with_cancel(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
        cancellation: RequestCancellation,
    ) -> Result<Value, ClientError> {
        if self.status() != ClientStatus::Ready
            || !self.inner.capabilities.read().await.supports(method)
        {
            return Err(ClientError::NotReady);
        }
        self.inner
            .process
            .request_with_cancel(method, Some(params), timeout, cancellation)
            .await
            .map_err(ClientError::Request)
    }

    pub async fn stop(&self) -> Result<(), ClientError> {
        let result = self.inner.process.shutdown().await;
        self.inner.status.send_replace(ClientStatus::Stopped);
        result.map_err(ClientError::Process)
    }
}

async fn handle_incoming(inner: &Arc<ClientInner>, incoming: IncomingMessage) {
    match incoming {
        IncomingMessage::Message(JsonRpcMessage::Request { id, method, params }) => {
            handle_server_request(inner, id, &method, params).await;
        }
        IncomingMessage::ProtocolError(_) => {
            inner.status.send_replace(ClientStatus::Degraded);
        }
        IncomingMessage::Exited { code: Some(0) } | IncomingMessage::Eof => {
            if *inner.status.borrow() != ClientStatus::Stopped {
                inner.status.send_replace(ClientStatus::Stopped);
            }
        }
        IncomingMessage::Exited { .. } => {
            inner.status.send_replace(ClientStatus::Crashed);
        }
        IncomingMessage::Message(_) | IncomingMessage::UnknownResponse(_) => {}
    }
}

async fn handle_server_request(
    inner: &Arc<ClientInner>,
    id: RpcId,
    method: &str,
    params: Option<Value>,
) {
    let result = match method {
        "client/registerCapability" => register_capabilities(inner, params).await,
        "client/unregisterCapability" => unregister_capabilities(inner, params).await,
        _ => {
            let _ = inner
                .process
                .respond_error(
                    id,
                    RpcError::new(-32601, "method not supported by Code Pad"),
                )
                .await;
            return;
        }
    };
    match result {
        Ok(()) => {
            let _ = inner.process.respond(id, Value::Null).await;
        }
        Err(error) => {
            let _ = inner
                .process
                .respond_error(id, RpcError::new(-32602, error.to_string()))
                .await;
        }
    }
}

async fn register_capabilities(
    inner: &Arc<ClientInner>,
    params: Option<Value>,
) -> Result<(), ClientError> {
    let registrations = params
        .as_ref()
        .and_then(|params| params.get("registrations"))
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ClientError::InvalidServerRequest("registrations must be an array".into())
        })?;
    let mut capabilities = inner.capabilities.write().await;
    for registration in registrations {
        let method = registration
            .get("method")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ClientError::InvalidServerRequest("registration method must be a string".into())
            })?;
        capabilities.register(method.to_owned());
    }
    Ok(())
}

async fn unregister_capabilities(
    inner: &Arc<ClientInner>,
    params: Option<Value>,
) -> Result<(), ClientError> {
    let params = params.as_ref().ok_or_else(|| {
        ClientError::InvalidServerRequest("unregistration params are required".into())
    })?;
    let registrations = params
        .get("unregisterations")
        .or_else(|| params.get("unregistrations"))
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ClientError::InvalidServerRequest("unregisterations must be an array".into())
        })?;
    let mut capabilities = inner.capabilities.write().await;
    for registration in registrations {
        let method = registration
            .get("method")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ClientError::InvalidServerRequest("unregistration method must be a string".into())
            })?;
        capabilities.unregister(method);
    }
    Ok(())
}

fn initialize_params(workspace: &WorkspaceRoot, config: InitializeConfig) -> Value {
    let root_uri = workspace.uri().as_str();
    let name = workspace
        .path()
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workspace");
    let mut params = Map::new();
    params.insert(
        "processId".into(),
        config.process_id.map_or(Value::Null, Value::from),
    );
    params.insert(
        "clientInfo".into(),
        json!({ "name": "code-pad", "version": config.app_version }),
    );
    params.insert("rootUri".into(), Value::String(root_uri.to_owned()));
    params.insert(
        "workspaceFolders".into(),
        json!([{ "uri": root_uri, "name": name }]),
    );
    if let Some(options) = config.initialization_options {
        params.insert("initializationOptions".into(), options);
    }
    params.insert("capabilities".into(), client_capabilities());
    Value::Object(params)
}

fn client_capabilities() -> Value {
    json!({
        "workspace": {
            "workspaceFolders": true,
            "applyEdit": true,
            "didChangeConfiguration": { "dynamicRegistration": true }
        },
        "textDocument": {
            "synchronization": { "dynamicRegistration": true, "didSave": true },
            "completion": { "dynamicRegistration": true },
            "hover": { "dynamicRegistration": true, "contentFormat": ["markdown", "plaintext"] },
            "definition": { "dynamicRegistration": true, "linkSupport": true },
            "references": { "dynamicRegistration": true },
            "rename": { "dynamicRegistration": true, "prepareSupport": true },
            "formatting": { "dynamicRegistration": true },
            "publishDiagnostics": { "versionSupport": true }
        },
        "general": { "positionEncodings": ["utf-16", "utf-8"] }
    })
}

fn parse_sync(value: Option<&Value>) -> Result<(Option<SyncKind>, bool, bool), ClientError> {
    let Some(value) = value else {
        return Ok((None, false, false));
    };
    if let Some(number) = value.as_i64() {
        return Ok((sync_kind(number)?, false, false));
    }
    let object = value.as_object().ok_or_else(|| {
        ClientError::InvalidInitializeResult("textDocumentSync must be a number or object".into())
    })?;
    let change = object.get("change").and_then(Value::as_i64).unwrap_or(0);
    let open_close = object
        .get("openClose")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let save = capability_enabled(object.get("save"));
    Ok((sync_kind(change)?, open_close, save))
}

fn sync_kind(value: i64) -> Result<Option<SyncKind>, ClientError> {
    let kind: TextDocumentSyncKind = serde_json::from_value(Value::from(value)).map_err(|_| {
        ClientError::InvalidInitializeResult(format!("invalid textDocumentSync kind {value}"))
    })?;
    if kind == TextDocumentSyncKind::NONE {
        Ok(None)
    } else if kind == TextDocumentSyncKind::FULL {
        Ok(Some(SyncKind::Full))
    } else if kind == TextDocumentSyncKind::INCREMENTAL {
        Ok(Some(SyncKind::Incremental))
    } else {
        Err(ClientError::InvalidInitializeResult(format!(
            "unknown textDocumentSync kind {value}"
        )))
    }
}

fn capability_enabled(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(enabled)) => *enabled,
        Some(Value::Object(_)) => true,
        _ => false,
    }
}

fn parse_server_info(value: &Value) -> Result<ServerInfo, ClientError> {
    let object = value.as_object().ok_or_else(|| {
        ClientError::InvalidInitializeResult("serverInfo must be an object".into())
    })?;
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| {
            ClientError::InvalidInitializeResult("serverInfo.name must be a string".into())
        })?;
    let version = object
        .get("version")
        .map(|version| {
            version.as_str().map(str::to_owned).ok_or_else(|| {
                ClientError::InvalidInitializeResult("serverInfo.version must be a string".into())
            })
        })
        .transpose()?;
    Ok(ServerInfo {
        name: name.to_owned(),
        version,
    })
}

fn is_supported_dynamic_method(method: &str) -> bool {
    matches!(
        method,
        "textDocument/didOpen"
            | "textDocument/didChange"
            | "textDocument/didSave"
            | "textDocument/didClose"
            | "textDocument/completion"
            | "textDocument/hover"
            | "textDocument/definition"
            | "textDocument/references"
            | "textDocument/rename"
            | "textDocument/formatting"
            | "textDocument/diagnostic"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_bool_object_sync_and_server_info() {
        let (capabilities, info) = CapabilitySet::from_initialize_result(&json!({
            "capabilities": {
                "positionEncoding": "utf-8",
                "textDocumentSync": { "openClose": true, "change": 2, "save": {} },
                "completionProvider": {},
                "hoverProvider": true,
                "definitionProvider": false,
                "referencesProvider": {},
                "renameProvider": true,
                "documentFormattingProvider": {},
                "diagnosticProvider": {}
            },
            "serverInfo": { "name": "fixture", "version": "1.2.3" }
        }))
        .unwrap();
        assert_eq!(capabilities.position_encoding, PositionEncoding::Utf8);
        assert!(!capabilities.legacy_position_encoding);
        assert_eq!(capabilities.sync_kind, Some(SyncKind::Incremental));
        assert!(capabilities.open_close && capabilities.save);
        assert!(capabilities.completion && capabilities.hover);
        assert!(!capabilities.definition);
        assert!(capabilities.references && capabilities.rename);
        assert!(capabilities.formatting && capabilities.diagnostics);
        assert_eq!(info.unwrap().name, "fixture");
    }

    #[test]
    fn missing_position_encoding_is_legacy_utf16_and_unknown_is_rejected() {
        let (capabilities, _) =
            CapabilitySet::from_initialize_result(&json!({ "capabilities": {} })).unwrap();
        assert_eq!(capabilities.position_encoding, PositionEncoding::Utf16);
        assert!(capabilities.legacy_position_encoding);
        assert!(CapabilitySet::from_initialize_result(&json!({
            "capabilities": { "positionEncoding": "utf-32" }
        }))
        .is_err());
    }

    #[test]
    fn initialization_options_must_be_an_object() {
        assert!(InitializeConfig::new("0.3.0")
            .with_initialization_options(json!(["unsafe"]))
            .is_err());
        assert!(InitializeConfig::new("0.3.0")
            .with_initialization_options(json!({ "safe": true }))
            .is_ok());
    }

    #[test]
    fn dynamic_capability_allowlist_never_enables_custom_commands() {
        let mut capabilities = CapabilitySet::default();
        capabilities.register("textDocument/hover".into());
        capabilities.register("workspace/executeCommand".into());
        assert!(capabilities.supports("textDocument/hover"));
        assert!(!capabilities.supports("workspace/executeCommand"));
        capabilities.unregister("textDocument/hover");
        assert!(!capabilities.supports("textDocument/hover"));
    }
}
