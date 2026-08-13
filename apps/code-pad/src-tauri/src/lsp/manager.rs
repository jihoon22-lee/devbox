//! Application-level language-server sessions and document synchronization.
//!
//! One session is allowed per language id. Starts are reserved before process
//! creation so concurrent commands cannot orphan a duplicate child. Document
//! mutations are staged and committed only after the corresponding JSON-RPC
//! notification has been written successfully.

use super::catalog::{LspConfig, ServerRef};
use super::client::{CapabilitySet, ClientStatus, InitializeConfig, LspClient, ServerInfo};
use super::config::{load_from_app_local_data_dir, save_to_app_local_data_dir, LoadedLspConfig};
use super::documents::{
    DidChange, DidClose, DidOpen, DidSave, DocumentStore, SyncKind, WorkspaceRoot,
};
use super::process::{LspProcess, ProcessState};
use super::runtime::RuntimeResolver;
use serde::Serialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug)]
pub enum LspManagerError {
    Config(String),
    ConfigRecoveryRequired,
    Disabled,
    MissingWorkspace,
    MissingServer(String),
    AlreadyRunning(String),
    StartInProgress(String),
    NotRunning(String),
    Protocol(String),
}

impl fmt::Display for LspManagerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(message) => write!(formatter, "LSP 설정 오류: {message}"),
            Self::ConfigRecoveryRequired => formatter.write_str(
                "기존 LSP 설정 파일이 손상되었습니다. 명시적으로 복구를 선택해야 덮어쓸 수 있습니다",
            ),
            Self::Disabled => formatter.write_str("LSP가 비활성화되어 있습니다"),
            Self::MissingWorkspace => formatter.write_str("LSP 작업 폴더가 설정되지 않았습니다"),
            Self::MissingServer(language_id) => {
                write!(formatter, "{language_id} 언어 서버가 설정되지 않았습니다")
            }
            Self::AlreadyRunning(language_id) => {
                write!(formatter, "{language_id} 언어 서버가 이미 실행 중입니다")
            }
            Self::StartInProgress(language_id) => {
                write!(formatter, "{language_id} 언어 서버 시작이 이미 진행 중입니다")
            }
            Self::NotRunning(language_id) => {
                write!(formatter, "{language_id} 언어 서버가 실행 중이 아닙니다")
            }
            Self::Protocol(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for LspManagerError {}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LanguageServerStatus {
    pub language_id: String,
    pub status: ClientStatus,
    pub process_state: String,
    pub server_info: Option<ServerInfo>,
    pub capabilities: CapabilitySet,
    pub document_count: usize,
    pub stderr: String,
    pub stderr_truncated: bool,
    pub stderr_dropped_bytes: u64,
}

struct LanguageSession {
    client: LspClient,
    process: LspProcess,
    documents: Mutex<DocumentStore>,
}

#[derive(Default)]
struct ManagerState {
    sessions: BTreeMap<String, Arc<LanguageSession>>,
    starting: BTreeMap<String, u64>,
    next_start_token: u64,
}

pub struct LspManager {
    app_local_data_dir: PathBuf,
    app_version: String,
    resolver: RuntimeResolver,
    state: Mutex<ManagerState>,
}

impl LspManager {
    pub fn new(app_local_data_dir: impl Into<PathBuf>, app_version: impl Into<String>) -> Self {
        Self {
            app_local_data_dir: app_local_data_dir.into(),
            app_version: app_version.into(),
            resolver: RuntimeResolver::new(),
            state: Mutex::new(ManagerState::default()),
        }
    }

    pub fn load_config(&self) -> Result<LoadedLspConfig, LspManagerError> {
        load_from_app_local_data_dir(&self.app_local_data_dir)
            .map_err(|error| LspManagerError::Config(error.to_string()))
    }

    pub fn save_config(
        &self,
        config: &LspConfig,
        recover_invalid: bool,
    ) -> Result<(), LspManagerError> {
        let loaded = self.load_config()?;
        if !loaded.persist_allowed && !recover_invalid {
            return Err(LspManagerError::ConfigRecoveryRequired);
        }
        save_to_app_local_data_dir(&self.app_local_data_dir, config)
            .map_err(|error| LspManagerError::Config(error.to_string()))
    }

    pub async fn start(&self, language_id: &str) -> Result<(), LspManagerError> {
        let language_id = normalized_language_id(language_id)?;
        let start_token = self.reserve_start(&language_id).await?;

        let result = self.create_session(&language_id).await;
        match result {
            Ok(session) => {
                let session = Arc::new(session);
                let accepted = {
                    let mut state = self.state.lock().await;
                    if state.starting.get(&language_id) == Some(&start_token) {
                        state.starting.remove(&language_id);
                        state
                            .sessions
                            .insert(language_id.clone(), Arc::clone(&session));
                        true
                    } else {
                        false
                    }
                };
                if accepted {
                    Ok(())
                } else {
                    let _ = session.client.stop().await;
                    Err(LspManagerError::Protocol(format!(
                        "{language_id} 언어 서버 시작이 취소되었습니다"
                    )))
                }
            }
            Err(error) => {
                let mut state = self.state.lock().await;
                if state.starting.get(&language_id) == Some(&start_token) {
                    state.starting.remove(&language_id);
                }
                Err(error)
            }
        }
    }

    async fn reserve_start(&self, language_id: &str) -> Result<u64, LspManagerError> {
        let mut state = self.state.lock().await;
        if state.sessions.contains_key(language_id) {
            return Err(LspManagerError::AlreadyRunning(language_id.to_owned()));
        }
        if state.starting.contains_key(language_id) {
            return Err(LspManagerError::StartInProgress(language_id.to_owned()));
        }
        state.next_start_token = state.next_start_token.wrapping_add(1).max(1);
        let token = state.next_start_token;
        state.starting.insert(language_id.to_owned(), token);
        Ok(token)
    }

    async fn create_session(&self, language_id: &str) -> Result<LanguageSession, LspManagerError> {
        let loaded = self.load_config()?;
        if !loaded.persist_allowed {
            return Err(LspManagerError::Config(
                loaded
                    .error
                    .unwrap_or_else(|| "설정 파일을 복구해야 합니다".into()),
            ));
        }
        let config = loaded.config;
        if !config.enabled {
            return Err(LspManagerError::Disabled);
        }
        if config.workspace_root.is_empty() {
            return Err(LspManagerError::MissingWorkspace);
        }

        let workspace = WorkspaceRoot::new(&config.workspace_root)
            .map_err(|error| LspManagerError::Protocol(error.to_string()))?;
        let resolved = if let Some(server) = config.server_by_language.get(language_id) {
            if matches!(server, ServerRef::Managed { .. }) {
                return Err(LspManagerError::Protocol(
                    "관리형 언어 서버의 설치 확인이 아직 완료되지 않았습니다".into(),
                ));
            }
            self.resolver
                .resolve_server_ref(server, workspace.path())
                .map_err(|error| LspManagerError::Protocol(error.to_string()))?
        } else if let Some(server) = config
            .custom_servers
            .iter()
            .find(|server| server.language_ids.iter().any(|id| id == language_id))
        {
            self.resolver
                .resolve_custom(server, workspace.path())
                .map_err(|error| LspManagerError::Protocol(error.to_string()))?
        } else {
            return Err(LspManagerError::MissingServer(language_id.to_owned()));
        };

        let process = LspProcess::spawn(resolved.process_spec())
            .await
            .map_err(|error| LspManagerError::Protocol(error.to_string()))?;
        let client = LspClient::new(process.clone());
        let capabilities = match client
            .initialize(&workspace, InitializeConfig::new(&self.app_version))
            .await
        {
            Ok(capabilities) => capabilities,
            Err(error) => {
                let _ = client.stop().await;
                return Err(LspManagerError::Protocol(error.to_string()));
            }
        };
        let sync_kind = capabilities.sync_kind.unwrap_or(SyncKind::Full);
        Ok(LanguageSession {
            client,
            process,
            documents: Mutex::new(DocumentStore::new(
                workspace,
                capabilities.position_encoding,
                sync_kind,
            )),
        })
    }

    pub async fn stop(&self, language_id: &str) -> Result<(), LspManagerError> {
        let language_id = normalized_language_id(language_id)?;
        let session = {
            let mut state = self.state.lock().await;
            if state.starting.remove(&language_id).is_some() {
                return Ok(());
            }
            state
                .sessions
                .remove(&language_id)
                .ok_or_else(|| LspManagerError::NotRunning(language_id.clone()))?
        };
        session
            .client
            .stop()
            .await
            .map_err(|error| LspManagerError::Protocol(error.to_string()))
    }

    pub async fn stop_all(&self) -> Result<(), LspManagerError> {
        let sessions = {
            let mut state = self.state.lock().await;
            state.starting.clear();
            std::mem::take(&mut state.sessions)
        };
        let mut first_error = None;
        for session in sessions.into_values() {
            if let Err(error) = session.client.stop().await {
                first_error.get_or_insert_with(|| LspManagerError::Protocol(error.to_string()));
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    pub async fn statuses(&self) -> Vec<LanguageServerStatus> {
        let sessions: Vec<_> = {
            let state = self.state.lock().await;
            state
                .sessions
                .iter()
                .map(|(language_id, session)| (language_id.clone(), Arc::clone(session)))
                .collect()
        };
        let mut statuses = Vec::with_capacity(sessions.len());
        for (language_id, session) in sessions {
            let process_state = process_state_label(session.process.state().await);
            let stderr = session.process.stderr().await;
            let document_count = session.documents.lock().await.len();
            statuses.push(LanguageServerStatus {
                language_id,
                status: session.client.status(),
                process_state,
                server_info: session.client.server_info().await,
                capabilities: session.client.capabilities().await,
                document_count,
                stderr: stderr.text(),
                stderr_truncated: stderr.truncated(),
                stderr_dropped_bytes: stderr.dropped_bytes(),
            });
        }
        statuses
    }

    pub async fn open_document(
        &self,
        language_id: &str,
        path: &Path,
        text: String,
    ) -> Result<DidOpen, LspManagerError> {
        let session = self.session(language_id).await?;
        let mut documents = session.documents.lock().await;
        let mut staged = documents.clone();
        let opened = staged
            .open(path, language_id, text)
            .map_err(|error| LspManagerError::Protocol(error.to_string()))?;
        if session
            .client
            .capabilities()
            .await
            .supports("textDocument/didOpen")
        {
            session
                .process
                .notify(
                    "textDocument/didOpen",
                    Some(json!({
                        "textDocument": {
                            "uri": opened.uri,
                            "languageId": opened.language_id,
                            "version": opened.version,
                            "text": opened.text,
                        }
                    })),
                )
                .await
                .map_err(|error| LspManagerError::Protocol(error.to_string()))?;
        }
        *documents = staged;
        Ok(opened)
    }

    pub async fn change_document(
        &self,
        language_id: &str,
        uri: &str,
        text: String,
        dirty: bool,
    ) -> Result<DidChange, LspManagerError> {
        let session = self.session(language_id).await?;
        let mut documents = session.documents.lock().await;
        let mut staged = documents.clone();
        let changed = staged
            .change(uri, text, dirty)
            .map_err(|error| LspManagerError::Protocol(error.to_string()))?;
        if session
            .client
            .capabilities()
            .await
            .supports("textDocument/didChange")
        {
            session
                .process
                .notify(
                    "textDocument/didChange",
                    Some(json!({
                        "textDocument": { "uri": changed.uri, "version": changed.version },
                        "contentChanges": changed.content_changes,
                    })),
                )
                .await
                .map_err(|error| LspManagerError::Protocol(error.to_string()))?;
        }
        *documents = staged;
        Ok(changed)
    }

    pub async fn save_document(
        &self,
        language_id: &str,
        uri: &str,
    ) -> Result<DidSave, LspManagerError> {
        let session = self.session(language_id).await?;
        let mut documents = session.documents.lock().await;
        let mut staged = documents.clone();
        let saved = staged
            .mark_saved(uri)
            .map_err(|error| LspManagerError::Protocol(error.to_string()))?;
        if session
            .client
            .capabilities()
            .await
            .supports("textDocument/didSave")
        {
            session
                .process
                .notify(
                    "textDocument/didSave",
                    Some(json!({ "textDocument": { "uri": saved.uri } })),
                )
                .await
                .map_err(|error| LspManagerError::Protocol(error.to_string()))?;
        }
        *documents = staged;
        Ok(saved)
    }

    pub async fn close_document(
        &self,
        language_id: &str,
        uri: &str,
    ) -> Result<DidClose, LspManagerError> {
        let session = self.session(language_id).await?;
        let mut documents = session.documents.lock().await;
        let mut staged = documents.clone();
        let closed = staged
            .close(uri)
            .map_err(|error| LspManagerError::Protocol(error.to_string()))?;
        if session
            .client
            .capabilities()
            .await
            .supports("textDocument/didClose")
        {
            session
                .process
                .notify(
                    "textDocument/didClose",
                    Some(json!({ "textDocument": { "uri": closed.uri } })),
                )
                .await
                .map_err(|error| LspManagerError::Protocol(error.to_string()))?;
        }
        *documents = staged;
        Ok(closed)
    }

    async fn session(&self, language_id: &str) -> Result<Arc<LanguageSession>, LspManagerError> {
        let language_id = normalized_language_id(language_id)?;
        self.state
            .lock()
            .await
            .sessions
            .get(&language_id)
            .cloned()
            .ok_or(LspManagerError::NotRunning(language_id))
    }
}

fn normalized_language_id(language_id: &str) -> Result<String, LspManagerError> {
    let language_id = language_id.trim();
    if language_id.is_empty() || language_id.chars().any(char::is_whitespace) {
        return Err(LspManagerError::Protocol(
            "language id는 공백 없는 값이어야 합니다".into(),
        ));
    }
    Ok(language_id.to_owned())
}

fn process_state_label(state: ProcessState) -> String {
    match state {
        ProcessState::Running => "running".into(),
        ProcessState::Stopping => "stopping".into(),
        ProcessState::Exited { code } => format!("exited:{code:?}"),
        ProcessState::Failed { reason } => format!("failed:{reason}"),
    }
}
