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
    DidChange, DidClose, DidOpen, DidSave, DocumentSnapshot, DocumentStore, SyncKind, WorkspaceRoot,
};
use super::features::{
    apply_workspace_edit, build_completion_params, build_definition_params,
    build_formatting_params, build_hover_params, build_pull_diagnostics_params,
    build_reference_params, build_rename_params, filter_definition_response,
    filter_reference_locations, parse_completion_response, parse_definition_response,
    parse_hover_response, parse_publish_diagnostics, parse_pull_diagnostics,
    parse_reference_locations, preflight_formatting_edits, preflight_workspace_edit,
    sanitize_hover, validate_completion_response, validate_pull_diagnostics, CompletionResult,
    DiagnosticResult, DiagnosticStore, FeatureError, FeatureResponse, FilteredLocations,
    SanitizedHover, WorkspaceEditPlan,
};
use super::installer::ManagedInstaller;
use super::logs::{LanguageServerLog, LspLogLevel, LspLogStore, StderrLineSanitizer};
use super::positions::{position_to_offset, LspPosition, PositionEncoding};
use super::process::{IncomingMessage, LspProcess, ProcessState};
use super::runtime::RuntimeResolver;
use super::transport::RequestCancellation;
use lsp_types as lsp;
use serde::Serialize;
use serde_json::json;
use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, Mutex};

const PULL_DIAGNOSTICS_TIMEOUT: Duration = Duration::from_secs(5);
const COMPLETION_TIMEOUT: Duration = Duration::from_secs(2);
const HOVER_TIMEOUT: Duration = Duration::from_secs(2);
const DEFINITION_TIMEOUT: Duration = Duration::from_secs(5);
const REFERENCES_TIMEOUT: Duration = Duration::from_secs(5);
const MUTATION_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_RENAME_BYTES: usize = 1_024;
const RESTART_WINDOW: Duration = Duration::from_secs(5 * 60);
const RESTART_DELAYS: [Duration; 6] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
    Duration::from_secs(16),
    Duration::from_secs(30),
];

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
    UnsupportedFeature { language_id: String, method: String },
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
            Self::UnsupportedFeature {
                language_id,
                method,
            } => write!(
                formatter,
                "{language_id} 언어 서버가 {method} 기능을 협상하지 않았습니다"
            ),
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
    pub restart_attempt: u32,
    pub restart_failures: u32,
    pub restart_delay_ms: Option<u64>,
    pub auto_restart_disabled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LspDiagnosticsEvent {
    pub language_id: String,
    pub response: FeatureResponse<DiagnosticResult>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LspStatusEvent {
    pub language_id: String,
    pub status: LanguageServerStatus,
    pub reason: Option<String>,
    pub restarting: bool,
}

#[derive(Debug, Clone)]
pub enum LspEvent {
    Diagnostics(LspDiagnosticsEvent),
    Status(LspStatusEvent),
}

/// Complete buffers returned after one atomic rename or formatting action.
/// The frontend replaces these buffers together and deliberately does not
/// save them; every returned document is dirty in the LSP document store.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppliedDocumentEdits {
    pub documents: Vec<EditedDocument>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EditedDocument {
    pub uri: String,
    pub version: i32,
    pub text: String,
}

struct LanguageSession {
    client: LspClient,
    process: LspProcess,
    documents: Mutex<DocumentStore>,
    diagnostics: Mutex<DiagnosticStore>,
    cancelable_requests: Mutex<CancelableRequests>,
    failure_handled: AtomicBool,
    /// Explicit stop/restart owns this session until its child has been
    /// reaped. Failure monitors must not replace it while that ownership is
    /// active.
    stopping: AtomicBool,
}

#[derive(Debug, Clone, Copy)]
enum CancelableFeature {
    Completion,
    Hover,
}

struct ActiveCancelableRequest {
    token: u64,
    cancellation: RequestCancellation,
}

#[derive(Default)]
struct CancelableRequests {
    next_token: u64,
    completion: Option<ActiveCancelableRequest>,
    hover: Option<ActiveCancelableRequest>,
}

#[derive(Debug, Clone, Default)]
struct RestartTracker {
    failures: VecDeque<Instant>,
    attempt: u32,
    next_restart_at: Option<Instant>,
    disabled: bool,
    reason: Option<String>,
}

struct FeatureRequestContext {
    session: Arc<LanguageSession>,
    snapshot: DocumentSnapshot,
    metadata: super::features::RequestMetadata,
    workspace: WorkspaceRoot,
    encoding: PositionEncoding,
}

struct MutationRequestContext {
    session: Arc<LanguageSession>,
    snapshot: DocumentSnapshot,
    request_documents: DocumentStore,
    metadata: super::features::RequestMetadata,
    encoding: PositionEncoding,
}

#[derive(Default)]
struct ManagerState {
    sessions: BTreeMap<String, Arc<LanguageSession>>,
    starting: BTreeMap<String, u64>,
    next_start_token: u64,
    restart: BTreeMap<String, RestartTracker>,
}

struct StartActivity {
    count: Arc<std::sync::atomic::AtomicUsize>,
    finished: Arc<tokio::sync::Notify>,
}

impl Drop for StartActivity {
    fn drop(&mut self) {
        if self.count.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.finished.notify_one();
        }
    }
}

struct StartReservation {
    state: Arc<Mutex<ManagerState>>,
    language_id: String,
    token: u64,
    activity: Option<StartActivity>,
    completed: bool,
}

impl StartReservation {
    fn token(&self) -> u64 {
        self.token
    }

    fn complete(mut self) {
        self.completed = true;
        self.activity.take();
    }
}

impl Drop for StartReservation {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        let state = Arc::clone(&self.state);
        let language_id = self.language_id.clone();
        let token = self.token;
        if let Ok(mut state) = state.try_lock() {
            if state.starting.get(&language_id) == Some(&token) {
                state.starting.remove(&language_id);
            }
            return;
        }
        let cleanup = async move {
            let mut state = state.lock().await;
            if state.starting.get(&language_id) == Some(&token) {
                state.starting.remove(&language_id);
            }
        };
        // A canceled start cannot await its reservation cleanup in Drop. The
        // task is intentionally tiny and runs before a subsequent start can
        // observe the old token.
        tokio::spawn(cleanup);
    }
}

#[derive(Clone)]
pub struct LspManager {
    app_local_data_dir: PathBuf,
    app_version: String,
    resolver: RuntimeResolver,
    installer: Arc<ManagedInstaller>,
    state: Arc<Mutex<ManagerState>>,
    logs: Arc<Mutex<LspLogStore>>,
    events: broadcast::Sender<LspEvent>,
    active_starts: Arc<std::sync::atomic::AtomicUsize>,
    start_finished: Arc<tokio::sync::Notify>,
    shutting_down: Arc<AtomicBool>,
}

impl LspManager {
    pub fn new(app_local_data_dir: impl Into<PathBuf>, app_version: impl Into<String>) -> Self {
        let app_local_data_dir = app_local_data_dir.into();
        let installer = Arc::new(
            ManagedInstaller::new(&app_local_data_dir)
                .expect("app-local LSP installer state must be creatable"),
        );
        Self::with_installer(app_local_data_dir, app_version, installer)
    }

    pub fn with_installer(
        app_local_data_dir: impl Into<PathBuf>,
        app_version: impl Into<String>,
        installer: Arc<ManagedInstaller>,
    ) -> Self {
        let (events, _) = broadcast::channel(128);
        Self {
            app_local_data_dir: app_local_data_dir.into(),
            app_version: app_version.into(),
            resolver: RuntimeResolver::new(),
            installer,
            state: Arc::new(Mutex::new(ManagerState::default())),
            logs: Arc::new(Mutex::new(LspLogStore::default())),
            events,
            active_starts: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            start_finished: Arc::new(tokio::sync::Notify::new()),
            shutting_down: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<LspEvent> {
        self.events.subscribe()
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
        self.append_log(
            &language_id,
            LspLogLevel::Info,
            "start-requested",
            "언어 서버 시작을 요청했습니다",
        )
        .await;
        let result = match self.reserve_start(&language_id).await {
            Ok(reservation) => {
                let start_token = reservation.token();
                let result = self.start_reserved(&language_id, start_token).await;
                reservation.complete();
                result
            }
            Err(error) => Err(error),
        };
        match &result {
            Ok(()) => {
                self.append_log(
                    &language_id,
                    LspLogLevel::Info,
                    "server-ready",
                    "언어 서버가 준비되었습니다",
                )
                .await;
            }
            Err(_) => {
                self.append_log(
                    &language_id,
                    LspLogLevel::Error,
                    "start-failed",
                    "언어 서버를 시작하지 못했습니다",
                )
                .await;
            }
        }
        result
    }

    async fn start_reserved(
        &self,
        language_id: &str,
        start_token: u64,
    ) -> Result<(), LspManagerError> {
        let result = self.create_session(language_id).await;
        match result {
            Ok(session) => {
                let session = Arc::new(session);
                let accepted = {
                    let mut state = self.state.lock().await;
                    if !self.shutting_down.load(Ordering::Acquire)
                        && state.starting.get(language_id) == Some(&start_token)
                    {
                        state.starting.remove(language_id);
                        state
                            .sessions
                            .insert(language_id.to_owned(), Arc::clone(&session));
                        true
                    } else {
                        false
                    }
                };
                if accepted {
                    self.spawn_session_monitor(language_id, Arc::clone(&session));
                    self.emit_status(language_id, None, false).await;
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
                if state.starting.get(language_id) == Some(&start_token) {
                    state.starting.remove(language_id);
                }
                Err(error)
            }
        }
    }

    async fn reserve_start(&self, language_id: &str) -> Result<StartReservation, LspManagerError> {
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(LspManagerError::Protocol(
                "LSP 서버 종료가 진행 중입니다".into(),
            ));
        }
        let mut state = self.state.lock().await;
        // Recheck after taking the state lock. This is the linearization
        // point shared with stop_all/shutdown_for_exit, so a shutdown racing
        // the initial atomic read cannot reserve a new child.
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(LspManagerError::Protocol(
                "LSP 서버 종료가 진행 중입니다".into(),
            ));
        }
        if state.sessions.contains_key(language_id) {
            return Err(LspManagerError::AlreadyRunning(language_id.to_owned()));
        }
        if state.starting.contains_key(language_id) {
            return Err(LspManagerError::StartInProgress(language_id.to_owned()));
        }
        state.next_start_token = state.next_start_token.wrapping_add(1).max(1);
        let token = state.next_start_token;
        state.starting.insert(language_id.to_owned(), token);
        self.active_starts.fetch_add(1, Ordering::AcqRel);
        Ok(StartReservation {
            state: Arc::clone(&self.state),
            language_id: language_id.to_owned(),
            token,
            activity: Some(StartActivity {
                count: Arc::clone(&self.active_starts),
                finished: Arc::clone(&self.start_finished),
            }),
            completed: false,
        })
    }

    fn begin_start_activity(&self) -> StartActivity {
        self.active_starts.fetch_add(1, Ordering::AcqRel);
        StartActivity {
            count: Arc::clone(&self.active_starts),
            finished: Arc::clone(&self.start_finished),
        }
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
            match server {
                ServerRef::Managed {
                    manifest_id,
                    version,
                    node_path,
                } => {
                    let installation = self
                        .installer
                        .resolve_managed_install(manifest_id, version)
                        .map_err(|error| LspManagerError::Protocol(error.to_string()))?;
                    self.resolver
                        .resolve_managed(
                            &installation.manifest,
                            language_id,
                            &installation.installed_path,
                            node_path.as_deref(),
                            workspace.path(),
                        )
                        .await
                        .map_err(|error| LspManagerError::Protocol(error.to_string()))?
                }
                _ => self
                    .resolver
                    .resolve_server_ref(server, workspace.path())
                    .map_err(|error| LspManagerError::Protocol(error.to_string()))?,
            }
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
        self.spawn_stderr_monitor(language_id.to_owned(), process.subscribe_stderr());
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
            diagnostics: Mutex::new(DiagnosticStore::new()),
            cancelable_requests: Mutex::new(CancelableRequests::default()),
            failure_handled: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
        })
    }

    pub async fn stop(&self, language_id: &str) -> Result<(), LspManagerError> {
        let language_id = normalized_language_id(language_id)?;
        self.append_log(
            &language_id,
            LspLogLevel::Info,
            "stop-requested",
            "언어 서버 중지를 요청했습니다",
        )
        .await;
        let selection = {
            let mut state = self.state.lock().await;
            state.restart.remove(&language_id);
            if let Some(session) = state.sessions.get(&language_id).cloned() {
                // Linearize ownership before releasing the manager lock. An
                // auto-restart attempt uses the same starting map and must
                // observe this flag before it can create a replacement.
                session.stopping.store(true, Ordering::Release);
                state.starting.remove(&language_id);
                Ok(Some(session))
            } else if state.starting.remove(&language_id).is_some() {
                Ok(None)
            } else {
                Err(LspManagerError::NotRunning(language_id.clone()))
            }
        };
        let result = match selection {
            Ok(Some(session)) => self.terminate_session(&language_id, &session).await,
            Ok(None) => Ok(()),
            Err(error) => Err(error),
        };
        self.append_log(
            &language_id,
            if result.is_ok() {
                LspLogLevel::Info
            } else {
                LspLogLevel::Error
            },
            if result.is_ok() {
                "server-stopped"
            } else {
                "stop-failed"
            },
            if result.is_ok() {
                "언어 서버를 중지했습니다"
            } else {
                "언어 서버를 중지하지 못했습니다"
            },
        )
        .await;
        result
    }

    /// Explicit restart clears the automatic backoff circuit and starts a
    /// fresh session. The frontend's buffers remain untouched.
    pub async fn restart(&self, language_id: &str) -> Result<(), LspManagerError> {
        let language_id = normalized_language_id(language_id)?;
        self.append_log(
            &language_id,
            LspLogLevel::Warning,
            "manual-retry",
            "사용자가 언어 서버 다시 시도를 요청했습니다",
        )
        .await;
        let session = {
            let mut state = self.state.lock().await;
            state.restart.remove(&language_id);
            let session = state.sessions.get(&language_id).cloned();
            if let Some(session) = &session {
                session.stopping.store(true, Ordering::Release);
                state.starting.remove(&language_id);
            }
            session
        };
        if let Some(session) = session {
            if let Err(error) = self.terminate_session(&language_id, &session).await {
                self.append_log(
                    &language_id,
                    LspLogLevel::Error,
                    "manual-retry-failed",
                    "언어 서버 다시 시도를 시작하지 못했습니다",
                )
                .await;
                return Err(error);
            }
        }
        self.start(&language_id).await
    }

    pub async fn stop_all(&self) -> Result<(), LspManagerError> {
        let sessions = {
            let mut state = self.state.lock().await;
            state.starting.clear();
            state.restart.clear();
            state
                .sessions
                .iter()
                .map(|(language_id, session)| {
                    session.stopping.store(true, Ordering::Release);
                    (language_id.clone(), Arc::clone(session))
                })
                .collect::<Vec<_>>()
        };
        let mut first_error = None;
        for (language_id, session) in sessions {
            let result = self.terminate_session(&language_id, &session).await;
            self.append_log(
                &language_id,
                if result.is_ok() {
                    LspLogLevel::Info
                } else {
                    LspLogLevel::Error
                },
                if result.is_ok() {
                    "server-stopped"
                } else {
                    "stop-failed"
                },
                if result.is_ok() {
                    "언어 서버를 중지했습니다"
                } else {
                    "언어 서버를 중지하지 못했습니다"
                },
            )
            .await;
            if let Err(error) = result {
                first_error.get_or_insert_with(|| LspManagerError::Protocol(error.to_string()));
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    /// Stop a session while retaining ownership until the process wait task
    /// confirms that the child is reaped. A failed/slow kill leaves the
    /// session in the manager, so neither a replacement start nor an exit
    /// race can orphan the old child.
    async fn terminate_session(
        &self,
        language_id: &str,
        session: &Arc<LanguageSession>,
    ) -> Result<(), LspManagerError> {
        let _ = session.client.stop().await;
        if !session
            .process
            .wait_for_exit(Duration::from_millis(250))
            .await
        {
            return Err(LspManagerError::Protocol(
                "language server termination was not confirmed".into(),
            ));
        }
        self.emit_status_override(language_id, None, false, Some(ClientStatus::Stopped))
            .await;
        let mut state = self.state.lock().await;
        if state
            .sessions
            .get(language_id)
            .is_some_and(|current| Arc::ptr_eq(current, session))
        {
            state.sessions.remove(language_id);
        }
        Ok(())
    }

    /// Stop every child during application exit and wait for any reserved
    /// start to either publish its session or hit the cancellation cleanup
    /// path. Normal config changes use [`Self::stop_all`] and remain reusable.
    pub async fn shutdown_for_exit(&self) -> Result<(), LspManagerError> {
        self.shutting_down.store(true, Ordering::Release);
        let result = self.stop_all().await;
        while self.active_starts.load(Ordering::Acquire) != 0 {
            let notified = self.start_finished.notified();
            if self.active_starts.load(Ordering::Acquire) == 0 {
                break;
            }
            notified.await;
        }
        result
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
        let restart_state: BTreeMap<_, _> = {
            let state = self.state.lock().await;
            state
                .restart
                .iter()
                .map(|(language_id, tracker)| (language_id.clone(), tracker.clone()))
                .collect()
        };
        let mut statuses = Vec::with_capacity(sessions.len());
        for (language_id, session) in sessions {
            let process = session.process.state().await;
            let process_state = process_state_label(process.clone());
            let document_count = session.documents.lock().await.len();
            statuses.push(LanguageServerStatus {
                language_id: language_id.clone(),
                // The process state is the authoritative failure boundary.
                // The client consumer and manager monitor receive the same
                // broadcast independently, so ClientStatus alone can race an
                // exit notification and publish Ready for a crashed child.
                status: effective_client_status(session.client.status(), &process),
                process_state,
                // The initialize response is controlled by the third-party
                // server and can put arbitrary path or credential text in
                // serverInfo. Runtime identity is derived from reviewed config
                // metadata in the UI, so do not forward this untrusted label.
                server_info: None,
                capabilities: session.client.capabilities().await,
                document_count,
                restart_attempt: restart_state
                    .get(&language_id)
                    .map_or(0, |tracker| tracker.attempt),
                restart_failures: restart_state
                    .get(&language_id)
                    .map_or(0, |tracker| tracker.failures.len() as u32),
                restart_delay_ms: restart_state
                    .get(&language_id)
                    .and_then(|tracker| tracker.next_restart_at)
                    .map(|at| at.saturating_duration_since(Instant::now()).as_millis() as u64),
                auto_restart_disabled: restart_state
                    .get(&language_id)
                    .is_some_and(|tracker| tracker.disabled),
            });
        }
        statuses
    }

    pub async fn logs(&self) -> Vec<LanguageServerLog> {
        self.logs.lock().await.snapshots()
    }

    async fn append_log(
        &self,
        language_id: &str,
        level: LspLogLevel,
        code: &'static str,
        message: &'static str,
    ) {
        self.logs
            .lock()
            .await
            .append(language_id, level, code, message);
    }

    fn spawn_stderr_monitor(
        &self,
        language_id: String,
        mut receiver: broadcast::Receiver<super::process::StderrEvent>,
    ) {
        let logs = Arc::clone(&self.logs);
        tokio::spawn(async move {
            let mut sanitizer = StderrLineSanitizer::default();
            loop {
                match receiver.recv().await {
                    Ok(event) => {
                        let lines = sanitizer.push(&event.bytes);
                        let mut store = logs.lock().await;
                        store.record_stderr_state(
                            &language_id,
                            event.dropped_bytes,
                            event.truncated,
                        );
                        for line in lines {
                            store.append(&language_id, LspLogLevel::Warning, "server-stderr", line);
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        logs.lock().await.append(
                            &language_id,
                            LspLogLevel::Warning,
                            "stderr-events-dropped",
                            "서버 진단 출력 일부를 처리하지 못했습니다",
                        );
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            if let Some(line) = sanitizer.finish() {
                logs.lock()
                    .await
                    .append(&language_id, LspLogLevel::Warning, "server-stderr", line);
            }
        });
    }

    pub async fn open_document(
        &self,
        language_id: &str,
        path: &Path,
        text: String,
    ) -> Result<DidOpen, LspManagerError> {
        let session = self.session(language_id).await?;
        // Commit the authoritative document store before notifying the server:
        // a slow or dead child must not delay the store that a replacement
        // session later replays from.
        let opened = {
            let mut documents = session.documents.lock().await;
            let mut staged = documents.clone();
            let opened = staged
                .open(path, language_id, text)
                .map_err(|error| LspManagerError::Protocol(error.to_string()))?;
            *documents = staged;
            opened
        };
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
        let changed = {
            let mut documents = session.documents.lock().await;
            let mut staged = documents.clone();
            let changed = staged
                .change(uri, text, dirty)
                .map_err(|error| LspManagerError::Protocol(error.to_string()))?;
            *documents = staged;
            changed
        };
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
        Ok(changed)
    }

    pub async fn reload_document(
        &self,
        language_id: &str,
        uri: &str,
        text: String,
    ) -> Result<DidChange, LspManagerError> {
        let session = self.session(language_id).await?;
        let changed = {
            let mut documents = session.documents.lock().await;
            let mut staged = documents.clone();
            let changed = staged
                .reload(uri, text)
                .map_err(|error| LspManagerError::Protocol(error.to_string()))?;
            *documents = staged;
            changed
        };
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
        Ok(changed)
    }

    pub async fn save_document(
        &self,
        language_id: &str,
        uri: &str,
    ) -> Result<DidSave, LspManagerError> {
        let session = self.session(language_id).await?;
        let saved = {
            let mut documents = session.documents.lock().await;
            let mut staged = documents.clone();
            let saved = staged
                .mark_saved(uri)
                .map_err(|error| LspManagerError::Protocol(error.to_string()))?;
            *documents = staged;
            saved
        };
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
        Ok(saved)
    }

    pub async fn close_document(
        &self,
        language_id: &str,
        uri: &str,
    ) -> Result<DidClose, LspManagerError> {
        let session = self.session(language_id).await?;
        let closed = {
            let mut documents = session.documents.lock().await;
            let mut staged = documents.clone();
            let closed = staged
                .close(uri)
                .map_err(|error| LspManagerError::Protocol(error.to_string()))?;
            *documents = staged;
            closed
        };
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
        Ok(closed)
    }

    /// Pull diagnostics for one open document.  The version is captured before
    /// the request is sent and is compared again after the server responds;
    /// stale results are returned for observability but are never treated as
    /// current by the feature adapter.
    pub async fn pull_diagnostics(
        &self,
        language_id: &str,
        uri: &str,
    ) -> Result<FeatureResponse<DiagnosticResult>, LspManagerError> {
        let context = self
            .feature_context(language_id, uri, "textDocument/diagnostic")
            .await?;
        let raw = context
            .session
            .client
            .request(
                "textDocument/diagnostic",
                build_pull_diagnostics_params(&context.metadata),
                PULL_DIAGNOSTICS_TIMEOUT,
            )
            .await
            .map_err(|error| LspManagerError::Protocol(error.to_string()))?;
        let parsed = parse_pull_diagnostics(&raw, &context.metadata).map_err(feature_error)?;
        let (current, document_is_stale) = current_feature_metadata(&context).await;
        let mut response = validate_pull_diagnostics(parsed, &current).map_err(feature_error)?;
        response.metadata = context.metadata;
        response.stale |= document_is_stale;
        response.value.stale = response.stale;
        let response = {
            let mut diagnostics = context.session.diagnostics.lock().await;
            diagnostics.apply(response)
        };
        self.emit_diagnostics(language_id, response.clone());
        Ok(response)
    }

    pub async fn completion(
        &self,
        language_id: &str,
        uri: &str,
        position: LspPosition,
    ) -> Result<FeatureResponse<CompletionResult>, LspManagerError> {
        let context = self
            .feature_context(language_id, uri, "textDocument/completion")
            .await?;
        validate_request_position(&context.snapshot, position, context.encoding)?;
        let raw = cancelable_feature_request(
            &context.session,
            CancelableFeature::Completion,
            "textDocument/completion",
            build_completion_params(&context.metadata, position),
            COMPLETION_TIMEOUT,
        )
        .await?;
        let parsed = parse_completion_response(&raw).map_err(feature_error)?;
        let value = validate_completion_response(
            parsed,
            &context.snapshot.text,
            position,
            context.encoding,
        )
        .map_err(feature_error)?;
        let stale = current_feature_metadata(&context).await.1;
        Ok(FeatureResponse::new(context.metadata, value, stale))
    }

    pub async fn hover(
        &self,
        language_id: &str,
        uri: &str,
        position: LspPosition,
    ) -> Result<FeatureResponse<Option<SanitizedHover>>, LspManagerError> {
        let context = self
            .feature_context(language_id, uri, "textDocument/hover")
            .await?;
        validate_request_position(&context.snapshot, position, context.encoding)?;
        let raw = cancelable_feature_request(
            &context.session,
            CancelableFeature::Hover,
            "textDocument/hover",
            build_hover_params(&context.metadata, position),
            HOVER_TIMEOUT,
        )
        .await?;
        let parsed = parse_hover_response(&raw).map_err(feature_error)?;
        let value = parsed.map(sanitize_hover);
        let stale = current_feature_metadata(&context).await.1;
        Ok(FeatureResponse::new(context.metadata, value, stale))
    }

    pub async fn definition(
        &self,
        language_id: &str,
        uri: &str,
        position: LspPosition,
    ) -> Result<FeatureResponse<FilteredLocations>, LspManagerError> {
        let context = self
            .feature_context(language_id, uri, "textDocument/definition")
            .await?;
        validate_request_position(&context.snapshot, position, context.encoding)?;
        let raw = context
            .session
            .client
            .request(
                "textDocument/definition",
                build_definition_params(&context.metadata, position),
                DEFINITION_TIMEOUT,
            )
            .await
            .map_err(|error| LspManagerError::Protocol(error.to_string()))?;
        let parsed = parse_definition_response(&raw).map_err(feature_error)?;
        let value = filter_definition_response(&context.workspace, &parsed);
        let stale = current_feature_metadata(&context).await.1;
        Ok(FeatureResponse::new(context.metadata, value, stale))
    }

    pub async fn references(
        &self,
        language_id: &str,
        uri: &str,
        position: LspPosition,
        include_declaration: bool,
    ) -> Result<FeatureResponse<FilteredLocations>, LspManagerError> {
        let context = self
            .feature_context(language_id, uri, "textDocument/references")
            .await?;
        validate_request_position(&context.snapshot, position, context.encoding)?;
        let raw = context
            .session
            .client
            .request(
                "textDocument/references",
                build_reference_params(&context.metadata, position, include_declaration),
                REFERENCES_TIMEOUT,
            )
            .await
            .map_err(|error| LspManagerError::Protocol(error.to_string()))?;
        let parsed = parse_reference_locations(&raw).map_err(feature_error)?;
        let value = filter_reference_locations(&context.workspace, &parsed);
        let stale = current_feature_metadata(&context).await.1;
        Ok(FeatureResponse::new(context.metadata, value, stale))
    }

    /// Request and atomically apply a text-only WorkspaceEdit. Every open
    /// document version is captured before the request, preflighted against
    /// that snapshot, and checked again against the live store at commit.
    pub async fn rename(
        &self,
        language_id: &str,
        uri: &str,
        position: LspPosition,
        new_name: String,
    ) -> Result<AppliedDocumentEdits, LspManagerError> {
        validate_rename_name(&new_name)?;
        let context = self
            .mutation_context(language_id, uri, "textDocument/rename")
            .await?;
        validate_request_position(&context.snapshot, position, context.encoding)?;
        let raw = context
            .session
            .client
            .request(
                "textDocument/rename",
                build_rename_params(&context.metadata, position, new_name),
                MUTATION_TIMEOUT,
            )
            .await
            .map_err(|error| LspManagerError::Protocol(error.to_string()))?;
        if raw.is_null() {
            return Ok(AppliedDocumentEdits {
                documents: Vec::new(),
            });
        }
        let edit: lsp::WorkspaceEdit = serde_json::from_value(raw).map_err(|error| {
            LspManagerError::Protocol(format!("invalid rename WorkspaceEdit: {error}"))
        })?;
        let plan =
            preflight_workspace_edit(&context.request_documents, &edit).map_err(feature_error)?;
        self.apply_mutation_plan(language_id, &context.session, &plan)
            .await
    }

    /// Formatting is only run for an explicit command. A null response is a
    /// successful no-op and malformed/out-of-range edits fail closed.
    pub async fn formatting(
        &self,
        language_id: &str,
        uri: &str,
        tab_size: u32,
        insert_spaces: bool,
    ) -> Result<AppliedDocumentEdits, LspManagerError> {
        if tab_size == 0 || tab_size > 32 {
            return Err(LspManagerError::Protocol(
                "formatting tab size must be between 1 and 32".into(),
            ));
        }
        let context = self
            .mutation_context(language_id, uri, "textDocument/formatting")
            .await?;
        let raw = context
            .session
            .client
            .request(
                "textDocument/formatting",
                build_formatting_params(&context.metadata, (tab_size, insert_spaces)),
                MUTATION_TIMEOUT,
            )
            .await
            .map_err(|error| LspManagerError::Protocol(error.to_string()))?;
        let edits = if raw.is_null() { json!([]) } else { raw };
        let plan = preflight_formatting_edits(
            &context.request_documents,
            uri,
            context.metadata.version,
            edits,
        )
        .map_err(feature_error)?;
        self.apply_mutation_plan(language_id, &context.session, &plan)
            .await
    }

    async fn feature_context(
        &self,
        language_id: &str,
        uri: &str,
        method: &str,
    ) -> Result<FeatureRequestContext, LspManagerError> {
        let language_id = normalized_language_id(language_id)?;
        let session = self.session(&language_id).await?;
        if !session.client.capabilities().await.supports(method) {
            return Err(LspManagerError::UnsupportedFeature {
                language_id,
                method: method.to_owned(),
            });
        }
        let (snapshot, workspace, encoding) = {
            let documents = session.documents.lock().await;
            let snapshot = documents
                .snapshot(uri)
                .ok_or_else(|| LspManagerError::Protocol(format!("document is not open: {uri}")))?;
            (
                snapshot,
                documents.workspace().clone(),
                documents.position_encoding(),
            )
        };
        Ok(FeatureRequestContext {
            session,
            metadata: super::features::RequestMetadata::new(snapshot.uri.clone(), snapshot.version),
            snapshot,
            workspace,
            encoding,
        })
    }

    async fn apply_mutation_plan(
        &self,
        language_id: &str,
        session: &Arc<LanguageSession>,
        plan: &WorkspaceEditPlan,
    ) -> Result<AppliedDocumentEdits, LspManagerError> {
        let result = apply_mutation_plan(session, plan).await;
        if let Err(error) = &result {
            // A notification can fail after an earlier document in the same
            // workspace edit was already written. The authoritative store is
            // intentionally still uncommitted, but the server mirror may be
            // partial. Treat the session as failed so recovery tears down that
            // mirror and replays the old snapshot atomically.
            self.handle_session_failure(
                language_id,
                session,
                format!("LSP mutation notification failed: {error}"),
            )
            .await;
        }
        result
    }

    async fn mutation_context(
        &self,
        language_id: &str,
        uri: &str,
        method: &str,
    ) -> Result<MutationRequestContext, LspManagerError> {
        let language_id = normalized_language_id(language_id)?;
        let session = self.session(&language_id).await?;
        if !session.client.capabilities().await.supports(method) {
            return Err(LspManagerError::UnsupportedFeature {
                language_id,
                method: method.to_owned(),
            });
        }
        let (snapshot, request_documents, encoding) = {
            let documents = session.documents.lock().await;
            let snapshot = documents
                .snapshot(uri)
                .ok_or_else(|| LspManagerError::Protocol(format!("document is not open: {uri}")))?;
            (snapshot, documents.clone(), documents.position_encoding())
        };
        Ok(MutationRequestContext {
            session,
            metadata: super::features::RequestMetadata::new(snapshot.uri.clone(), snapshot.version),
            snapshot,
            request_documents,
            encoding,
        })
    }

    fn spawn_session_monitor(&self, language_id: &str, session: Arc<LanguageSession>) {
        let manager = self.clone();
        let language_id = language_id.to_owned();
        let mut incoming = session.process.subscribe();
        tokio::spawn(async move {
            // Initialization can fail before the manager gets a chance to
            // subscribe. Check the process state immediately, then continue
            // consuming the broadcast stream for notifications and exits.
            if matches!(
                session.process.state().await,
                ProcessState::Exited { .. } | ProcessState::Failed { .. }
            ) {
                manager
                    .handle_session_failure(
                        &language_id,
                        &session,
                        "language server exited during startup".into(),
                    )
                    .await;
                return;
            }
            loop {
                let message = match incoming.recv().await {
                    Ok(message) => message,
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        if matches!(
                            session.process.state().await,
                            ProcessState::Exited { .. } | ProcessState::Failed { .. }
                        ) {
                            manager
                                .handle_session_failure(
                                    &language_id,
                                    &session,
                                    "language server exited after monitor lag".into(),
                                )
                                .await;
                            break;
                        }
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        if !matches!(session.process.state().await, ProcessState::Stopping) {
                            manager
                                .handle_session_failure(
                                    &language_id,
                                    &session,
                                    "language server monitor closed".into(),
                                )
                                .await;
                        }
                        break;
                    }
                };
                match message {
                    IncomingMessage::Message(super::transport::JsonRpcMessage::Notification {
                        method,
                        params,
                    }) if method == "textDocument/publishDiagnostics" => {
                        manager
                            .handle_push_diagnostics(&language_id, &session, params)
                            .await;
                    }
                    IncomingMessage::ProtocolError(reason) => {
                        manager
                            .handle_session_failure(&language_id, &session, reason)
                            .await;
                        break;
                    }
                    IncomingMessage::Exited { code } => {
                        let reason = format!("language server exited with code {code:?}");
                        manager
                            .handle_session_failure(&language_id, &session, reason)
                            .await;
                        break;
                    }
                    IncomingMessage::Eof => {
                        manager
                            .handle_session_failure(
                                &language_id,
                                &session,
                                "language server closed its stdio stream".into(),
                            )
                            .await;
                        break;
                    }
                    IncomingMessage::Message(_) | IncomingMessage::UnknownResponse(_) => {}
                }
            }
        });
    }

    async fn handle_push_diagnostics(
        &self,
        language_id: &str,
        session: &Arc<LanguageSession>,
        params: Option<serde_json::Value>,
    ) {
        let Some(params) = params else {
            return;
        };
        let Ok(result) = parse_publish_diagnostics(&params) else {
            return;
        };
        let Some(snapshot) = session.documents.lock().await.snapshot(&result.uri) else {
            return;
        };
        // A versionless push cannot be tied to the current editor generation:
        // after a local change it could be an arbitrarily late report. Pull
        // diagnostics remain usable because their request metadata supplies
        // the version. Fail closed for every versionless push, including the
        // initial document notification.
        if result.version.is_none() {
            return;
        }
        let current = super::features::RequestMetadata::new(snapshot.uri, snapshot.version);
        let Ok(response) = session
            .diagnostics
            .lock()
            .await
            .accept_push(result, &current)
        else {
            return;
        };
        self.emit_diagnostics(language_id, response);
    }

    fn emit_diagnostics(&self, language_id: &str, response: FeatureResponse<DiagnosticResult>) {
        let _ = self.events.send(LspEvent::Diagnostics(LspDiagnosticsEvent {
            language_id: language_id.to_owned(),
            response,
        }));
    }

    async fn emit_status(&self, language_id: &str, reason: Option<String>, restarting: bool) {
        self.emit_status_override(language_id, reason, restarting, None)
            .await;
    }

    async fn emit_status_override(
        &self,
        language_id: &str,
        reason: Option<String>,
        restarting: bool,
        override_status: Option<ClientStatus>,
    ) {
        let Some(status) = self
            .statuses()
            .await
            .into_iter()
            .find(|status| status.language_id == language_id)
        else {
            return;
        };
        let mut status = status;
        if let Some(override_status) = override_status {
            status.status = override_status;
        }
        let _ = self.events.send(LspEvent::Status(LspStatusEvent {
            language_id: language_id.to_owned(),
            status,
            // JSON-RPC/OS errors can contain executable paths, arguments, or
            // server stderr. Keep that detail in the native diagnostic path;
            // the public event only carries a stable, user-facing message.
            reason: safe_status_reason(reason.as_deref(), restarting),
            restarting,
        }));
    }

    async fn handle_session_failure(
        &self,
        language_id: &str,
        session: &Arc<LanguageSession>,
        reason: String,
    ) {
        if session.stopping.load(Ordering::Acquire) {
            return;
        }
        if session
            .failure_handled
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let snapshots = {
            let state = self.state.lock().await;
            let Some(current) = state.sessions.get(language_id) else {
                return;
            };
            if !Arc::ptr_eq(current, session) {
                return;
            }
            drop(state);
            session.documents.lock().await.clone()
        };
        let stale_events = {
            let mut diagnostics = session.diagnostics.lock().await;
            diagnostics.mark_all_stale(
                &snapshots
                    .snapshots()
                    .into_iter()
                    .map(|snapshot| (snapshot.uri, snapshot.version))
                    .collect::<Vec<_>>(),
            )
        };
        for response in stale_events {
            self.emit_diagnostics(language_id, response);
        }
        // EOF can race the process wait task: the manager's monitor receives
        // the stream-close message before the child exit code is published.
        // Synchronize that boundary so a non-zero exit is reported as
        // crashed rather than a transient Ready status. If the child remains
        // alive after EOF, classify the failure as stopped below.
        if matches!(session.process.state().await, ProcessState::Running) {
            let _ = session
                .process
                .wait_for_exit(Duration::from_millis(250))
                .await;
        }
        let failure_status = match session.process.state().await {
            ProcessState::Exited { code: Some(0) } | ProcessState::Stopping => {
                ClientStatus::Stopped
            }
            ProcessState::Exited { .. } => ClientStatus::Crashed,
            ProcessState::Failed { .. } => ClientStatus::Degraded,
            // EOF with a still-running child is an unusable protocol
            // session, not a user-requested stop. Keep the restart control
            // visible and classify it as degraded.
            ProcessState::Running => ClientStatus::Degraded,
        };
        if session.stopping.load(Ordering::Acquire) {
            return;
        }
        let (delay, disabled) = self
            .record_restart_failure(language_id, reason.clone())
            .await;
        self.emit_status_override(
            language_id,
            Some(reason.clone()),
            !disabled,
            Some(failure_status),
        )
        .await;
        if let Some(delay) = delay {
            let manager = self.clone();
            let language_id = language_id.to_owned();
            let session = Arc::clone(session);
            tokio::spawn(async move {
                manager
                    .restart_after_failure(&language_id, &session, delay)
                    .await;
            });
        }
    }

    async fn restart_after_failure(
        &self,
        language_id: &str,
        failed_session: &Arc<LanguageSession>,
        mut delay: Duration,
    ) {
        loop {
            tokio::time::sleep(delay).await;
            if self.shutting_down.load(Ordering::Acquire) {
                return;
            }
            let _activity = self.begin_start_activity();
            let replacement_token = {
                let mut state = self.state.lock().await;
                let Some(current) = state.sessions.get(language_id) else {
                    return;
                };
                if !Arc::ptr_eq(current, failed_session)
                    || failed_session.stopping.load(Ordering::Acquire)
                    || self.shutting_down.load(Ordering::Acquire)
                    || state
                        .restart
                        .get(language_id)
                        .is_some_and(|tracker| tracker.disabled)
                {
                    return;
                }
                state.next_start_token = state.next_start_token.wrapping_add(1).max(1);
                let token = state.next_start_token;
                state.starting.insert(language_id.to_owned(), token);
                token
            };
            let still_reserved =
                self.state.lock().await.starting.get(language_id) == Some(&replacement_token);
            if !still_reserved {
                self.clear_start_reservation(language_id, replacement_token)
                    .await;
                return;
            }
            let result = self.create_session(language_id).await;
            match result {
                Ok(session) => {
                    let session = Arc::new(session);
                    // Lifecycle commands continue to commit the authoritative
                    // document store while this session is unavailable. Capture
                    // the snapshot as late as possible — after the replacement
                    // child has spawned and initialized — so edits, opens, and
                    // closes made during backoff and startup are reflected.
                    let snapshots = failed_session.documents.lock().await.clone();
                    // Replay is staged entirely in the replacement session.
                    // Do not publish it as the current session until every
                    // didOpen has been written successfully.
                    if let Err(error) = self.replay_documents(&session, &snapshots).await {
                        self.clear_start_reservation(language_id, replacement_token)
                            .await;
                        let reason = error.to_string();
                        let _ = session.client.stop().await;
                        let (next_delay, disabled) = self
                            .record_restart_failure(language_id, reason.clone())
                            .await;
                        self.emit_status(language_id, Some(reason), !disabled).await;
                        let Some(next_delay) = next_delay else {
                            return;
                        };
                        delay = next_delay;
                        continue;
                    }
                    if matches!(
                        session.process.state().await,
                        ProcessState::Exited { .. } | ProcessState::Failed { .. }
                    ) {
                        self.clear_start_reservation(language_id, replacement_token)
                            .await;
                        let reason =
                            "replacement language server exited during document replay".to_owned();
                        let _ = session.client.stop().await;
                        let (next_delay, disabled) = self
                            .record_restart_failure(language_id, reason.clone())
                            .await;
                        self.emit_status(language_id, Some(reason), !disabled).await;
                        let Some(next_delay) = next_delay else {
                            return;
                        };
                        delay = next_delay;
                        continue;
                    }
                    let accepted = {
                        let mut state = self.state.lock().await;
                        match state.sessions.get(language_id) {
                            Some(current)
                                if Arc::ptr_eq(current, failed_session)
                                    && !self.shutting_down.load(Ordering::Acquire)
                                    && !failed_session.stopping.load(Ordering::Acquire)
                                    && state.starting.get(language_id)
                                        == Some(&replacement_token) =>
                            {
                                state.starting.remove(language_id);
                                state
                                    .sessions
                                    .insert(language_id.to_owned(), Arc::clone(&session));
                                if let Some(tracker) = state.restart.get_mut(language_id) {
                                    tracker.next_restart_at = None;
                                    tracker.reason = None;
                                }
                                true
                            }
                            _ => false,
                        }
                    };
                    if !accepted {
                        self.clear_start_reservation(language_id, replacement_token)
                            .await;
                        let _ = session.client.stop().await;
                        return;
                    }
                    self.spawn_session_monitor(language_id, Arc::clone(&session));
                    self.emit_status(language_id, None, false).await;
                    self.append_log(
                        language_id,
                        LspLogLevel::Info,
                        "auto-restart-ready",
                        "언어 서버 자동 재시작이 완료되었습니다",
                    )
                    .await;
                    return;
                }
                Err(error) => {
                    self.clear_start_reservation(language_id, replacement_token)
                        .await;
                    let reason = error.to_string();
                    let (next_delay, disabled) = self
                        .record_restart_failure(language_id, reason.clone())
                        .await;
                    self.emit_status(language_id, Some(reason), !disabled).await;
                    let Some(next_delay) = next_delay else {
                        return;
                    };
                    delay = next_delay;
                }
            }
        }
    }

    async fn record_restart_failure(
        &self,
        language_id: &str,
        reason: String,
    ) -> (Option<Duration>, bool) {
        let outcome = {
            let mut state = self.state.lock().await;
            let tracker = state.restart.entry(language_id.to_owned()).or_default();
            let now = Instant::now();
            while tracker
                .failures
                .front()
                .is_some_and(|failure| now.duration_since(*failure) > RESTART_WINDOW)
            {
                tracker.failures.pop_front();
            }
            if tracker.failures.is_empty() {
                tracker.attempt = 0;
                tracker.disabled = false;
                tracker.reason = None;
            }
            tracker.failures.push_back(now);
            tracker.reason = Some(reason);
            if tracker.failures.len() >= 3 {
                tracker.disabled = true;
                tracker.next_restart_at = None;
                (None, true)
            } else {
                let index = tracker.attempt.min((RESTART_DELAYS.len() - 1) as u32) as usize;
                let delay = RESTART_DELAYS[index];
                tracker.attempt = tracker.attempt.saturating_add(1);
                tracker.next_restart_at = Some(now + delay);
                (Some(delay), false)
            }
        };
        self.append_log(
            language_id,
            if outcome.1 {
                LspLogLevel::Error
            } else {
                LspLogLevel::Warning
            },
            if outcome.1 {
                "auto-restart-disabled"
            } else {
                "auto-restart-scheduled"
            },
            if outcome.1 {
                "반복 실패로 자동 재시작을 중지했습니다"
            } else {
                "언어 서버 실패 후 자동 재시작을 예약했습니다"
            },
        )
        .await;
        outcome
    }

    async fn clear_start_reservation(&self, language_id: &str, token: u64) {
        let mut state = self.state.lock().await;
        if state.starting.get(language_id) == Some(&token) {
            state.starting.remove(language_id);
        }
    }

    async fn replay_documents(
        &self,
        session: &Arc<LanguageSession>,
        snapshots: &DocumentStore,
    ) -> Result<(), LspManagerError> {
        let mut staged = session.documents.lock().await.clone();
        for snapshot in snapshots.snapshots() {
            let opened = staged
                .open_snapshot(&snapshot)
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
        }
        *session.documents.lock().await = staged;
        Ok(())
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

async fn apply_mutation_plan(
    session: &Arc<LanguageSession>,
    plan: &WorkspaceEditPlan,
) -> Result<AppliedDocumentEdits, LspManagerError> {
    if plan.changes.is_empty() {
        return Ok(AppliedDocumentEdits {
            documents: Vec::new(),
        });
    }

    let mut documents = session.documents.lock().await;
    let mut staged = documents.clone();
    let applied = apply_workspace_edit(&mut staged, plan).map_err(feature_error)?;

    if session
        .client
        .capabilities()
        .await
        .supports("textDocument/didChange")
    {
        for change in &applied.changes {
            session
                .process
                .notify(
                    "textDocument/didChange",
                    Some(json!({
                        "textDocument": { "uri": change.uri, "version": change.version },
                        "contentChanges": change.content_changes,
                    })),
                )
                .await
                .map_err(|error| LspManagerError::Protocol(error.to_string()))?;
            // A child can consume a notification and exit before the writer
            // observes a broken pipe. Give the process wait task a bounded
            // chance to publish that failure before sending the next edit;
            // this keeps a partial server mirror from being committed as a
            // successful atomic mutation.
            let _ = session
                .process
                .wait_for_exit(Duration::from_millis(10))
                .await;
            if !matches!(session.process.state().await, ProcessState::Running) {
                return Err(LspManagerError::Protocol(
                    "language server exited during workspace edit notification".into(),
                ));
            }
        }
    }

    let edited = plan
        .changes
        .iter()
        .map(|change| {
            let snapshot = staged.snapshot(&change.uri).ok_or_else(|| {
                LspManagerError::Protocol(format!(
                    "edited document disappeared before commit: {}",
                    change.uri
                ))
            })?;
            Ok(EditedDocument {
                uri: snapshot.uri,
                version: snapshot.version,
                text: snapshot.text,
            })
        })
        .collect::<Result<Vec<_>, LspManagerError>>()?;
    *documents = staged;
    Ok(AppliedDocumentEdits { documents: edited })
}

async fn cancelable_feature_request(
    session: &Arc<LanguageSession>,
    feature: CancelableFeature,
    method: &str,
    params: serde_json::Value,
    timeout: Duration,
) -> Result<serde_json::Value, LspManagerError> {
    let (token, cancellation) = {
        let mut requests = session.cancelable_requests.lock().await;
        requests.next_token = requests.next_token.wrapping_add(1).max(1);
        let token = requests.next_token;
        let slot = match feature {
            CancelableFeature::Completion => &mut requests.completion,
            CancelableFeature::Hover => &mut requests.hover,
        };
        if let Some(previous) = slot.take() {
            previous.cancellation.cancel();
        }
        let cancellation = RequestCancellation::new();
        *slot = Some(ActiveCancelableRequest {
            token,
            cancellation: cancellation.clone(),
        });
        (token, cancellation)
    };

    let result = session
        .client
        .request_with_cancel(method, params, timeout, cancellation)
        .await
        .map_err(|error| LspManagerError::Protocol(error.to_string()));

    let mut requests = session.cancelable_requests.lock().await;
    let slot = match feature {
        CancelableFeature::Completion => &mut requests.completion,
        CancelableFeature::Hover => &mut requests.hover,
    };
    if slot.as_ref().is_some_and(|active| active.token == token) {
        *slot = None;
    }
    result
}

async fn current_feature_metadata(
    context: &FeatureRequestContext,
) -> (super::features::RequestMetadata, bool) {
    let documents = context.session.documents.lock().await;
    let Some(snapshot) = documents.snapshot(&context.metadata.uri) else {
        return (context.metadata.clone(), true);
    };
    let stale = snapshot.version != context.metadata.version;
    (
        super::features::RequestMetadata::new(snapshot.uri, snapshot.version),
        stale,
    )
}

fn validate_request_position(
    snapshot: &DocumentSnapshot,
    position: LspPosition,
    encoding: PositionEncoding,
) -> Result<(), LspManagerError> {
    position_to_offset(&snapshot.text, position, encoding)
        .map(|_| ())
        .map_err(|error| LspManagerError::Protocol(format!("invalid LSP position: {error}")))
}

fn feature_error(error: FeatureError) -> LspManagerError {
    LspManagerError::Protocol(error.to_string())
}

fn validate_rename_name(new_name: &str) -> Result<(), LspManagerError> {
    if new_name.is_empty() {
        return Err(LspManagerError::Protocol(
            "rename target cannot be empty".into(),
        ));
    }
    if new_name.len() > MAX_RENAME_BYTES {
        return Err(LspManagerError::Protocol(format!(
            "rename target exceeds {MAX_RENAME_BYTES} UTF-8 bytes"
        )));
    }
    if new_name
        .chars()
        .any(|value| matches!(value, '\0' | '\r' | '\n'))
    {
        return Err(LspManagerError::Protocol(
            "rename target cannot contain NUL or line breaks".into(),
        ));
    }
    Ok(())
}

fn normalized_language_id(language_id: &str) -> Result<String, LspManagerError> {
    let language_id = language_id.trim();
    if language_id.is_empty()
        || language_id.len() > 64
        || !language_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+'))
    {
        return Err(LspManagerError::Protocol(
            "language id 형식이 올바르지 않습니다".into(),
        ));
    }
    Ok(language_id.to_owned())
}

fn process_state_label(state: ProcessState) -> String {
    match state {
        ProcessState::Running => "running".into(),
        ProcessState::Stopping => "stopping".into(),
        ProcessState::Exited { .. } => "exited".into(),
        ProcessState::Failed { .. } => "failed".into(),
    }
}

fn safe_status_reason(reason: Option<&str>, restarting: bool) -> Option<String> {
    reason.map(|_| {
        if restarting {
            "언어 서버가 중단되어 재시작합니다".into()
        } else {
            "언어 서버를 사용할 수 없습니다".into()
        }
    })
}

fn effective_client_status(client: ClientStatus, process: &ProcessState) -> ClientStatus {
    match process {
        // A stop request is still in flight. Treat it as degraded so the UI
        // does not offer Start while the owned child may still be alive.
        ProcessState::Stopping => ClientStatus::Degraded,
        ProcessState::Exited { code: Some(0) } => ClientStatus::Stopped,
        ProcessState::Exited { .. } => ClientStatus::Crashed,
        ProcessState::Failed { .. } => ClientStatus::Degraded,
        ProcessState::Running => client,
    }
}

#[cfg(test)]
mod status_tests {
    use super::*;

    #[test]
    fn unconfirmed_stop_is_not_reported_as_stopped() {
        assert_eq!(
            effective_client_status(ClientStatus::Ready, &ProcessState::Stopping),
            ClientStatus::Degraded
        );
        assert_eq!(
            effective_client_status(
                ClientStatus::Degraded,
                &ProcessState::Exited { code: Some(0) }
            ),
            ClientStatus::Stopped
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_language_id_uses_the_same_safe_identifier_boundary_as_config() {
        assert_eq!(normalized_language_id("rust").unwrap(), "rust");
        assert_eq!(normalized_language_id(" c-sharp ").unwrap(), "c-sharp");
        for invalid in ["C:\\private", "../../secret", "token value", "한글"] {
            assert!(normalized_language_id(invalid).is_err());
        }
        assert!(normalized_language_id(&"a".repeat(65)).is_err());
    }

    #[tokio::test]
    async fn restart_backoff_resets_after_the_failure_window() {
        let manager = LspManager::new("/tmp/code-pad-lsp-test", "test");
        assert_eq!(
            manager.record_restart_failure("rust", "one".into()).await.0,
            Some(Duration::from_secs(1))
        );
        assert_eq!(
            manager.record_restart_failure("rust", "two".into()).await.0,
            Some(Duration::from_secs(2))
        );
        {
            let mut state = manager.state.lock().await;
            let tracker = state.restart.get_mut("rust").unwrap();
            for failure in &mut tracker.failures {
                *failure = Instant::now() - RESTART_WINDOW - Duration::from_secs(1);
            }
        }
        let (delay, disabled) = manager
            .record_restart_failure("rust", "new sequence".into())
            .await;
        assert_eq!(delay, Some(Duration::from_secs(1)));
        assert!(!disabled);
    }

    #[tokio::test]
    async fn restart_backoff_opens_after_three_recent_failures() {
        let manager = LspManager::new("/tmp/code-pad-lsp-test", "test");
        assert!(!manager.record_restart_failure("rust", "one".into()).await.1);
        assert!(!manager.record_restart_failure("rust", "two".into()).await.1);
        let (delay, disabled) = manager.record_restart_failure("rust", "three".into()).await;
        assert!(delay.is_none());
        assert!(disabled);
        let logs = manager.logs().await;
        let entries = &logs[0].entries;
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[2].code, "auto-restart-disabled");
        assert!(!entries.iter().any(|entry| entry.message.contains("three")));
    }
}
