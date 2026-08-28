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
    DidChange, DidClose, DidOpen, DidSave, DocumentSnapshot, DocumentStore, RequestSnapshot,
    SyncKind, WorkspaceRoot,
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
use super::positions::{position_to_offset, LspPosition, LspRange, PositionEncoding};
use super::process::{IncomingMessage, LspProcess, ProcessState};
use super::runtime::RuntimeResolver;
use super::transport::RequestCancellation;
use crate::commands::file as file_commands;
use crate::core::encoding::Encoding;
use crate::core::line_ending::LineEnding;
use devbox_filesystem::FilesystemIdentity;
use lsp_types as lsp;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, Mutex};
use url::Url;
use uuid::Uuid;

const PULL_DIAGNOSTICS_TIMEOUT: Duration = Duration::from_secs(5);
const COMPLETION_TIMEOUT: Duration = Duration::from_secs(2);
const HOVER_TIMEOUT: Duration = Duration::from_secs(2);
const DEFINITION_TIMEOUT: Duration = Duration::from_secs(5);
const REFERENCES_TIMEOUT: Duration = Duration::from_secs(5);
const MUTATION_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_RENAME_BYTES: usize = 1_024;
const MAX_RENAME_FILES: usize = 256;
const MAX_RENAME_TOTAL_BYTES: usize = 2 * 1024 * 1024;
const MAX_RENAME_URIS: usize = 256;
const MAX_RENAME_URI_BYTES: usize = 32 * 1024;
const MAX_RENAME_EDITS: usize = 4 * 1024;
const MAX_RENAME_NEW_TEXT_BYTES: usize = 2 * 1024 * 1024;
const MAX_RENAME_PREVIEW_BYTES: usize = 16 * 1024;
const MAX_PENDING_RENAMES: usize = 16;
const MAX_RENAME_JOURNAL_BYTES: u64 = 512 * 1024;
const MAX_RENAME_JOURNAL_ENTRIES: usize = MAX_RENAME_FILES;
const MAX_RENAME_RECOVERY_BYTES: u64 = 16 * 1024 * 1024;
/// Startup recovery is native synchronous work. Bound the total target and
/// backup bytes it may inspect across all stale transactions, not only per
/// journal, so a directory full of valid-but-stale journals cannot create a
/// burst of unbounded disk I/O or allocations.
const MAX_RENAME_RECOVERY_SCAN_BYTES: u64 = 64 * 1024 * 1024;
const RENAME_PLAN_TTL: Duration = Duration::from_secs(5 * 60);
const RENAME_APPLY_TIMEOUT: Duration = Duration::from_secs(30);
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

/// Complete buffers returned after one atomic formatting/rename mirror action.
/// Formatting callers keep the returned buffers dirty; a successful
/// disk-backed rename marks them clean only after the native save boundary.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppliedDocumentEdits {
    pub documents: Vec<EditedDocument>,
}

/// A disk-backed rename is intentionally split into request/preview and
/// apply.  The opaque plan id is the only handle the frontend receives; the
/// native side retains absolute paths, snapshots, encodings, and full buffers.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RenamePreview {
    pub plan_id: String,
    pub files: Vec<RenamePreviewFile>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RenamePreviewFile {
    /// Workspace-relative display path. Absolute paths never cross this IPC
    /// boundary as part of a rename preview.
    pub path: String,
    pub ranges: Vec<RenamePreviewRange>,
    /// Bounded before/after excerpts used by the diff preview. The native
    /// pending plan retains complete text for the eventual write.
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RenamePreviewRange {
    pub range: LspRange,
    pub new_text: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RenameFileStatus {
    Applied,
    RolledBack,
    Failed,
    NotApplied,
    Conflict,
    RollbackFailed,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RenameFileResult {
    /// Workspace-relative display path; see [`RenamePreviewFile::path`].
    pub path: String,
    pub status: RenameFileStatus,
    pub mtime_nanos: Option<String>,
    pub size: Option<u64>,
    pub content_hash: Option<String>,
    /// User-safe category only; OS/path/protocol details stay native.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RenameApplyResult {
    pub plan_id: String,
    pub success: bool,
    pub rolled_back: bool,
    pub files: Vec<RenameFileResult>,
    /// Only workspace-relative paths cross the rename IPC boundary. The
    /// frontend resolves them against its current workspace and never receives
    /// the native file URI or absolute path from this result.
    pub documents: Vec<RenamedDocument>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RenamedDocument {
    pub path: String,
    pub version: i32,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EditedDocument {
    pub uri: String,
    pub version: i32,
    pub text: String,
}

struct LanguageSession {
    generation: u64,
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

#[derive(Debug, Clone)]
struct PendingRenameFile {
    path: PathBuf,
    display_path: String,
    before_text: String,
    after_text: String,
    encoding: Encoding,
    line_ending: LineEnding,
    expected_mtime: i64,
    expected_size: u64,
    expected_content_hash: String,
    expected_identity: FilesystemIdentity,
    ranges: Vec<RenamePreviewRange>,
}

#[derive(Debug, Clone)]
struct PendingRenameDocument {
    uri: String,
    version: i32,
    text: String,
    dirty: bool,
}

#[derive(Clone)]
struct PendingRename {
    language_id: String,
    session: Arc<LanguageSession>,
    session_generation: u64,
    workspace_root: PathBuf,
    created_at: Instant,
    open_documents: Vec<PendingRenameDocument>,
    plan: WorkspaceEditPlan,
    files: Vec<PendingRenameFile>,
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
    pending_renames: Arc<Mutex<BTreeMap<String, PendingRename>>>,
    next_rename_id: Arc<AtomicU64>,
    rename_epoch: Arc<AtomicU64>,
    next_session_generation: Arc<AtomicU64>,
    /// Cancellation tokens are registered as soon as a preview is published,
    /// not only when disk I/O starts. This closes the apply/cancel handoff
    /// race while the native command is validating its pending plan.
    active_rename_cancellations: Arc<Mutex<BTreeMap<String, Arc<AtomicBool>>>>,
    rename_backup_root: PathBuf,
    document_mutation_gate: Arc<Mutex<()>>,
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
        let app_local_data_dir = app_local_data_dir.into();
        let rename_backup_root = app_local_data_dir.join("rename-backups");
        let (events, _) = broadcast::channel(128);
        let manager = Self {
            app_local_data_dir,
            app_version: app_version.into(),
            resolver: RuntimeResolver::new(),
            installer,
            state: Arc::new(Mutex::new(ManagerState::default())),
            logs: Arc::new(Mutex::new(LspLogStore::default())),
            events,
            active_starts: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            start_finished: Arc::new(tokio::sync::Notify::new()),
            shutting_down: Arc::new(AtomicBool::new(false)),
            pending_renames: Arc::new(Mutex::new(BTreeMap::new())),
            next_rename_id: Arc::new(AtomicU64::new(0)),
            rename_epoch: Arc::new(AtomicU64::new(0)),
            next_session_generation: Arc::new(AtomicU64::new(0)),
            active_rename_cancellations: Arc::new(Mutex::new(BTreeMap::new())),
            rename_backup_root,
            document_mutation_gate: Arc::new(Mutex::new(())),
        };
        recover_rename_journals(&manager.rename_backup_root);
        manager
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<LspEvent> {
        self.events.subscribe()
    }

    fn next_rename_plan_id(&self) -> String {
        // UUID v4 is generated independently from the monotonic counter used
        // only for diagnostics, so a renderer cannot guess another approval
        // handle by observing previous plans.
        let _ = self.next_rename_id.fetch_add(1, Ordering::Relaxed);
        format!("rename-{}", Uuid::new_v4().simple())
    }

    async fn prune_pending_renames(&self) {
        let now = Instant::now();
        let expired = {
            let mut pending = self.pending_renames.lock().await;
            let expired = pending
                .iter()
                .filter(|(_, plan)| now.duration_since(plan.created_at) >= RENAME_PLAN_TTL)
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            for id in &expired {
                pending.remove(id);
            }
            expired
        };
        if !expired.is_empty() {
            let mut cancellations = self.active_rename_cancellations.lock().await;
            for id in expired {
                cancellations.remove(&id);
            }
        }
    }

    async fn invalidate_renames_for_session(&self, session: &Arc<LanguageSession>) {
        self.rename_epoch.fetch_add(1, Ordering::AcqRel);
        let removed = {
            let mut pending = self.pending_renames.lock().await;
            let ids = pending
                .iter()
                .filter(|(_, plan)| Arc::ptr_eq(&plan.session, session))
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            for id in &ids {
                pending.remove(id);
            }
            ids
        };
        let mut cancellations = self.active_rename_cancellations.lock().await;
        for cancellation in cancellations.values() {
            cancellation.store(true, Ordering::Release);
        }
        for id in removed {
            cancellations.remove(&id);
        }
    }

    async fn invalidate_all_renames(&self) {
        self.rename_epoch.fetch_add(1, Ordering::AcqRel);
        self.pending_renames.lock().await.clear();
        let mut cancellations = self.active_rename_cancellations.lock().await;
        for cancellation in cancellations.values() {
            cancellation.store(true, Ordering::Release);
        }
        cancellations.clear();
    }

    async fn clear_rename_cancellation(&self, plan_id: &str) {
        self.active_rename_cancellations
            .lock()
            .await
            .remove(plan_id);
    }

    /// Explicitly discard a preview that the user cancelled or that the UI no
    /// longer owns. Discard is intentionally idempotent at the command edge.
    pub async fn discard_rename(&self, plan_id: &str) -> bool {
        let removed = self.pending_renames.lock().await.remove(plan_id).is_some();
        if let Some(cancellation) = self
            .active_rename_cancellations
            .lock()
            .await
            .remove(plan_id)
        {
            cancellation.store(true, Ordering::Release);
        }
        removed
    }

    /// Request cancellation of a pending preview or active disk transaction.
    /// The worker checks this flag between every filesystem operation and
    /// always attempts the already-created backups before returning.
    pub async fn cancel_rename(&self, plan_id: &str) -> bool {
        let Some(cancellation) = self
            .active_rename_cancellations
            .lock()
            .await
            .get(plan_id)
            .cloned()
        else {
            return false;
        };
        cancellation.store(true, Ordering::Release);
        true
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
        let generation = self
            .next_session_generation
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(1);
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
            generation,
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
        self.invalidate_all_renames().await;
        let _mutation_guard = self.document_mutation_gate.lock().await;
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
        self.invalidate_all_renames().await;
        let _mutation_guard = self.document_mutation_gate.lock().await;
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
        self.invalidate_all_renames().await;
        let _mutation_guard = self.document_mutation_gate.lock().await;
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
        let _mutation_guard = self.document_mutation_gate.lock().await;
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
        let _mutation_guard = self.document_mutation_gate.lock().await;
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
        let _mutation_guard = self.document_mutation_gate.lock().await;
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
        let _mutation_guard = self.document_mutation_gate.lock().await;
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
        let _mutation_guard = self.document_mutation_gate.lock().await;
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

    /// Request a rename and stage a bounded, disk-backed plan.  No editor
    /// buffer or file is changed until the frontend has rendered the returned
    /// ranges/diff and explicitly calls [`Self::apply_rename`].
    pub async fn rename(
        &self,
        language_id: &str,
        uri: &str,
        position: LspPosition,
        new_name: String,
    ) -> Result<RenamePreview, LspManagerError> {
        self.prune_pending_renames().await;
        let rename_epoch = self.rename_epoch.load(Ordering::Acquire);
        let normalized_id = normalized_language_id(language_id)?;
        validate_rename_name(&new_name)?;
        let context = self
            .mutation_context(&normalized_id, uri, "textDocument/rename")
            .await?;
        if !context
            .session
            .client
            .capabilities()
            .await
            .supports("textDocument/didChange")
        {
            // A disk-backed rename must keep the server mirror and editor
            // generation in lockstep. There is no safe fallback when a
            // server explicitly negotiated textDocumentSync: none.
            return Err(LspManagerError::UnsupportedFeature {
                language_id: normalized_id,
                method: "textDocument/didChange".into(),
            });
        }
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
            return Ok(RenamePreview {
                plan_id: String::new(),
                files: Vec::new(),
            });
        }
        let edit: lsp::WorkspaceEdit = serde_json::from_value(raw).map_err(|error| {
            LspManagerError::Protocol(format!("invalid rename WorkspaceEdit: {error}"))
        })?;
        validate_rename_edit_bounds(&edit)?;
        if edit.changes.is_some() && edit.document_changes.is_some() {
            return Err(LspManagerError::Protocol(
                "이름 변경 WorkspaceEdit가 changes와 documentChanges를 함께 포함합니다".into(),
            ));
        }
        if !context
            .session
            .documents
            .lock()
            .await
            .is_current(&RequestSnapshot {
                uri: context.metadata.uri.clone(),
                version: context.metadata.version,
            })
        {
            return Err(LspManagerError::Protocol(
                "LSP 이름 변경 결과가 최신 문서 상태와 맞지 않습니다".into(),
            ));
        }

        let (request_documents, disk_files) =
            load_rename_documents(&normalized_id, &context, &edit)?;
        let plan = preflight_workspace_edit(&request_documents, &edit).map_err(feature_error)?;
        let files = pending_rename_files(&request_documents, &disk_files, &plan)?;
        if files.is_empty() {
            return Ok(RenamePreview {
                plan_id: String::new(),
                files: Vec::new(),
            });
        }

        // A response can race a local change in any affected document, not
        // only the document at the cursor. Recheck all open versions before
        // making the plan available to the UI.
        {
            let documents = context.session.documents.lock().await;
            for change in &plan.changes {
                let Some(requested) = context.request_documents.snapshot(&change.uri) else {
                    continue;
                };
                let Some(current) = documents.snapshot(&change.uri) else {
                    return Err(LspManagerError::Protocol(
                        "이름 변경 대상 문서가 닫혔습니다".into(),
                    ));
                };
                if current.version != requested.version || current.text != requested.text {
                    return Err(LspManagerError::Protocol(
                        "LSP 이름 변경 결과가 최신 문서 상태와 맞지 않습니다".into(),
                    ));
                }
            }
        }

        let plan_id = self.next_rename_plan_id();
        let preview = RenamePreview {
            plan_id: plan_id.clone(),
            files: files.iter().map(RenamePreviewFile::from_pending).collect(),
        };
        let affected_uris = plan
            .changes
            .iter()
            .map(|change| change.uri.as_str())
            .collect::<BTreeSet<_>>();
        let open_documents = context
            .request_documents
            .snapshots()
            .into_iter()
            .filter(|snapshot| affected_uris.contains(snapshot.uri.as_str()))
            .map(|snapshot| PendingRenameDocument {
                uri: snapshot.uri,
                version: snapshot.version,
                text: snapshot.text,
                dirty: snapshot.dirty,
            })
            .collect::<Vec<_>>();
        let pending = PendingRename {
            language_id: normalized_id,
            session: Arc::clone(&context.session),
            session_generation: context.session.generation,
            workspace_root: context.request_documents.workspace().path().to_path_buf(),
            created_at: Instant::now(),
            open_documents,
            plan,
            files,
        };
        let mut pending_renames = self.pending_renames.lock().await;
        if self.rename_epoch.load(Ordering::Acquire) != rename_epoch {
            return Err(LspManagerError::Protocol(
                "언어 서버 또는 작업 폴더가 변경되어 이름 변경 미리보기가 만료되었습니다".into(),
            ));
        }
        let mut evicted = Vec::new();
        while pending_renames.len() >= MAX_PENDING_RENAMES {
            let Some(oldest) = pending_renames
                .iter()
                .min_by_key(|(_, plan)| plan.created_at)
                .map(|(id, _)| id.clone())
            else {
                break;
            };
            pending_renames.remove(&oldest);
            evicted.push(oldest);
        }
        // Register the cancellation token while the pending-plan lock is held.
        // The UI can press Cancel immediately after the preview arrives; this
        // makes that request win even if apply_rename has not reached its
        // worker yet. `discard_rename` uses the same pending-then-token lock
        // order, so no token can be orphaned between the two maps.
        let cancellation = Arc::new(AtomicBool::new(false));
        let mut cancellations = self.active_rename_cancellations.lock().await;
        for id in evicted {
            cancellations.remove(&id);
        }
        cancellations.insert(plan_id.clone(), cancellation);
        pending_renames.insert(plan_id, pending);
        Ok(preview)
    }

    /// Apply a previously previewed rename.  Native writes are completed
    /// under per-file snapshot guards, then the server mirror is updated. If
    /// either phase fails, transaction-private backups restore every file that was
    /// committed and the caller receives a file-by-file outcome.
    pub async fn apply_rename(&self, plan_id: &str) -> Result<RenameApplyResult, LspManagerError> {
        self.prune_pending_renames().await;
        if !valid_rename_plan_id(plan_id) {
            return Err(LspManagerError::Protocol(
                "이름 변경 미리보기가 만료되었습니다".into(),
            ));
        }
        let _mutation_guard = self.document_mutation_gate.lock().await;
        let pending = self
            .pending_renames
            .lock()
            .await
            .remove(plan_id)
            .ok_or_else(|| {
                LspManagerError::Protocol("이름 변경 미리보기가 만료되었습니다".into())
            })?;
        let cancellation = self
            .active_rename_cancellations
            .lock()
            .await
            .entry(plan_id.to_owned())
            .or_insert_with(|| Arc::new(AtomicBool::new(false)))
            .clone();
        let current_session = self
            .state
            .lock()
            .await
            .sessions
            .get(&pending.language_id)
            .is_some_and(|current| {
                Arc::ptr_eq(current, &pending.session)
                    && current.generation == pending.session_generation
                    && !current.stopping.load(Ordering::Acquire)
            });
        let current_workspace = pending
            .session
            .documents
            .lock()
            .await
            .workspace()
            .path()
            .to_path_buf();
        if !current_session
            || current_workspace != pending.workspace_root
            || !matches!(pending.session.process.state().await, ProcessState::Running)
        {
            self.clear_rename_cancellation(plan_id).await;
            return Err(LspManagerError::NotRunning(pending.language_id));
        }

        {
            let documents = pending.session.documents.lock().await;
            if let Err(error) = validate_pending_rename_documents(&documents, &pending) {
                self.clear_rename_cancellation(plan_id).await;
                return Err(error);
            }
        }

        let backup_root = self.rename_backup_root.clone();
        let files = pending.files.clone();
        let workspace_root = pending.workspace_root.clone();
        let plan_id_for_worker = plan_id.to_owned();
        let deadline = Instant::now() + RENAME_APPLY_TIMEOUT;
        let worker_cancellation = Arc::clone(&cancellation);
        let worker_result = tokio::task::spawn_blocking(move || {
            apply_pending_rename_files(
                &files,
                &backup_root,
                &plan_id_for_worker,
                &workspace_root,
                worker_cancellation,
                deadline,
            )
        })
        .await;
        let mut disk = match worker_result {
            Ok(outcome) => outcome,
            Err(_) => {
                // A panic/abort in the blocking worker cannot leave an
                // approval handle cancellable forever. The native task has
                // already been joined at this point.
                self.clear_rename_cancellation(plan_id).await;
                return Err(LspManagerError::Protocol(
                    "이름 변경 작업이 중단되었습니다".into(),
                ));
            }
        };
        if !disk.success {
            self.clear_rename_cancellation(plan_id).await;
            return Ok(RenameApplyResult {
                plan_id: plan_id.to_owned(),
                success: false,
                rolled_back: disk.rolled_back,
                files: disk.files,
                documents: Vec::new(),
                error: disk.error,
            });
        }

        if cancellation.load(Ordering::Acquire) {
            disk = rollback_disk_rename_blocking(disk).await;
            let rolled_back = disk.rolled_back;
            self.clear_rename_cancellation(plan_id).await;
            return Ok(RenameApplyResult {
                plan_id: plan_id.to_owned(),
                success: false,
                rolled_back,
                files: disk.files,
                documents: Vec::new(),
                error: Some(if rolled_back {
                    "이름 변경을 취소해 모든 파일을 되돌렸습니다".into()
                } else {
                    "이름 변경 취소와 되돌리기에 실패했습니다. 백업을 확인하세요".into()
                }),
            });
        }

        {
            let documents = pending.session.documents.lock().await;
            if let Err(error) = validate_pending_rename_documents(&documents, &pending) {
                drop(documents);
                disk = rollback_disk_rename_blocking(disk).await;
                let rolled_back = disk.rolled_back;
                self.clear_rename_cancellation(plan_id).await;
                return Ok(RenameApplyResult {
                    plan_id: plan_id.to_owned(),
                    success: false,
                    rolled_back,
                    files: disk.files,
                    documents: Vec::new(),
                    error: Some(if rolled_back {
                        error.to_string()
                    } else {
                        "문서 상태 검증과 되돌리기에 실패했습니다. 백업 journal을 확인하세요".into()
                    }),
                });
            }
        }

        let open_plan = {
            let documents = pending.session.documents.lock().await;
            let changes = pending
                .plan
                .changes
                .iter()
                .filter(|change| documents.snapshot(&change.uri).is_some())
                .cloned()
                .collect::<Vec<_>>();
            let edits = pending
                .plan
                .edits
                .iter()
                .filter(|edit| changes.iter().any(|change| change.uri == edit.uri))
                .cloned()
                .collect::<Vec<_>>();
            WorkspaceEditPlan { changes, edits }
        };

        let documents = match apply_saved_mutation_plan(
            &pending.session,
            &open_plan,
            &cancellation,
            deadline,
        )
        .await
        {
            Ok(documents) => documents,
            Err(error) => {
                // Cancellation can arrive after one or more notifications
                // have already reached the server. Tear down/replay that
                // mirror just like an I/O failure before restoring disk;
                // otherwise rollback would leave the server on a partial
                // post-rename view.
                let mirror_may_be_partial = matches!(&error, SavedMutationError::Server(_))
                    || matches!(
                        &error,
                        SavedMutationError::Cancelled {
                            mirror_may_be_partial: true
                        }
                    );
                if mirror_may_be_partial {
                    self.handle_session_failure(
                        &pending.language_id,
                        &pending.session,
                        format!("LSP rename notification failed: {error}"),
                    )
                    .await;
                    // Stop the old mirror before restoring disk. The
                    // automatic restart task will replay the authoritative
                    // pre-rename snapshots; leaving this process alive could
                    // let a queued didChange observe the rolled-back bytes.
                    let _ = pending.session.client.stop().await;
                }
                disk = rollback_disk_rename_blocking(disk).await;
                let rolled_back = disk.rolled_back;
                self.clear_rename_cancellation(plan_id).await;
                return Ok(RenameApplyResult {
                    plan_id: plan_id.to_owned(),
                    success: false,
                    rolled_back,
                    files: disk.files,
                    documents: Vec::new(),
                    error: Some(
                        if matches!(error, SavedMutationError::Cancelled { .. }) && rolled_back {
                            "이름 변경을 취소해 모든 파일을 되돌렸습니다".into()
                        } else if rolled_back && mirror_may_be_partial {
                            "언어 서버 반영에 실패해 모든 파일을 되돌렸습니다".into()
                        } else if rolled_back {
                            "이름 변경 대상 문서가 최신 상태가 아니어서 모든 파일을 되돌렸습니다"
                                .into()
                        } else {
                            "언어 서버 반영과 파일 되돌리기에 실패했습니다. 백업을 확인하세요"
                                .into()
                        },
                    ),
                });
            }
        };
        let rename_documents = documents
            .documents
            .into_iter()
            .filter_map(|edited| {
                pending
                    .files
                    .iter()
                    .find(|file| {
                        super::documents::file_uri_from_absolute_path(&file.path)
                            .map(|uri| uri.as_str() == edited.uri)
                            .unwrap_or(false)
                    })
                    .map(|file| RenamedDocument {
                        path: file.display_path.clone(),
                        version: edited.version,
                        text: edited.text,
                    })
            })
            .collect::<Vec<_>>();
        let journal_committed = mark_rename_journal_committed(&disk);
        self.clear_rename_cancellation(plan_id).await;
        cleanup_rename_backups(&disk.backups, disk.backup_dir.as_deref(), journal_committed);
        Ok(RenameApplyResult {
            plan_id: plan_id.to_owned(),
            success: true,
            rolled_back: false,
            files: disk.files,
            documents: rename_documents,
            error: (!journal_committed).then(|| {
                "이름 변경은 완료되었지만 transaction journal 정리가 지연되었습니다".into()
            }),
        })
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
        let _mutation_guard = self.document_mutation_gate.lock().await;
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
        self.invalidate_renames_for_session(session).await;
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

impl RenamePreviewFile {
    fn from_pending(file: &PendingRenameFile) -> Self {
        Self {
            path: file.display_path.clone(),
            ranges: file.ranges.clone(),
            before: bounded_rename_excerpt(&file.before_text),
            after: bounded_rename_excerpt(&file.after_text),
        }
    }
}

/// Gather all URI keys before running the normal WorkspaceEdit preflight. The
/// existing LSP document store contains open files only; rename may legitimately
/// include a workspace file that has not been opened in a tab yet.
fn workspace_edit_uris(edit: &lsp::WorkspaceEdit) -> Result<Vec<String>, LspManagerError> {
    let mut uris = BTreeSet::new();
    let mut insert_uri = |uri: String| -> Result<(), LspManagerError> {
        if uri.len() > MAX_RENAME_URI_BYTES {
            return Err(LspManagerError::Protocol(
                "이름 변경 URI가 허용 범위를 초과했습니다".into(),
            ));
        }
        uris.insert(uri);
        if uris.len() > MAX_RENAME_URIS {
            return Err(LspManagerError::Protocol(
                "이름 변경 대상 파일 수가 허용 범위를 초과했습니다".into(),
            ));
        }
        Ok(())
    };
    if let Some(changes) = &edit.changes {
        for uri in changes.keys() {
            insert_uri(uri.as_str().to_owned())?;
        }
    }
    if let Some(document_changes) = &edit.document_changes {
        match document_changes {
            lsp::DocumentChanges::Edits(edits) => {
                for edit in edits {
                    insert_uri(edit.text_document.uri.as_str().to_owned())?;
                }
            }
            lsp::DocumentChanges::Operations(operations) => {
                for operation in operations {
                    match operation {
                        lsp::DocumentChangeOperation::Edit(edit) => {
                            insert_uri(edit.text_document.uri.as_str().to_owned())?;
                        }
                        lsp::DocumentChangeOperation::Op(operation) => {
                            return Err(LspManagerError::Protocol(format!(
                                "LSP 이름 변경이 지원하지 않는 리소스 작업을 반환했습니다: {}",
                                match operation {
                                    lsp::ResourceOp::Create(_) => "create",
                                    lsp::ResourceOp::Rename(_) => "rename",
                                    lsp::ResourceOp::Delete(_) => "delete",
                                }
                            )));
                        }
                    }
                }
            }
        }
    }
    Ok(uris.into_iter().collect())
}

fn validate_rename_edit_bounds(edit: &lsp::WorkspaceEdit) -> Result<(), LspManagerError> {
    let mut edit_count = 0usize;
    let mut replacement_bytes = 0usize;
    let mut account = |text_edits: &[lsp::TextEdit]| -> Result<(), LspManagerError> {
        edit_count = edit_count
            .checked_add(text_edits.len())
            .ok_or_else(|| LspManagerError::Protocol("이름 변경 edit 수가 너무 큽니다".into()))?;
        if edit_count > MAX_RENAME_EDITS {
            return Err(LspManagerError::Protocol(
                "이름 변경 edit 수가 허용 범위를 초과했습니다".into(),
            ));
        }
        for text_edit in text_edits {
            replacement_bytes = replacement_bytes
                .checked_add(text_edit.new_text.len())
                .ok_or_else(|| {
                    LspManagerError::Protocol("이름 변경 replacement 크기가 너무 큽니다".into())
                })?;
            if replacement_bytes > MAX_RENAME_NEW_TEXT_BYTES {
                return Err(LspManagerError::Protocol(
                    "이름 변경 replacement 크기가 허용 범위를 초과했습니다".into(),
                ));
            }
        }
        Ok(())
    };
    if let Some(changes) = &edit.changes {
        for text_edits in changes.values() {
            account(text_edits)?;
        }
    }
    if let Some(document_changes) = &edit.document_changes {
        match document_changes {
            lsp::DocumentChanges::Edits(edits) => {
                for document_edit in edits {
                    for edit in &document_edit.edits {
                        let text_edit = match edit {
                            lsp::OneOf::Left(edit) => edit,
                            lsp::OneOf::Right(edit) => &edit.text_edit,
                        };
                        account(std::slice::from_ref(text_edit))?;
                    }
                }
            }
            lsp::DocumentChanges::Operations(operations) => {
                for operation in operations {
                    if let lsp::DocumentChangeOperation::Edit(document_edit) = operation {
                        for edit in &document_edit.edits {
                            let text_edit = match edit {
                                lsp::OneOf::Left(edit) => edit,
                                lsp::OneOf::Right(edit) => &edit.text_edit,
                            };
                            account(std::slice::from_ref(text_edit))?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn is_sensitive_rename_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return true;
    };
    let lower = name.to_ascii_lowercase();
    let sensitive_directory = path.components().any(|component| {
        matches!(
            component
                .as_os_str()
                .to_str()
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some(".ssh") | Some(".aws") | Some(".gnupg") | Some(".kube") | Some(".docker")
        )
    });
    sensitive_directory
        || lower == ".env"
        || lower.starts_with(".env.")
        || lower.starts_with("id_rsa")
        || lower.starts_with("id_ed25519")
        || lower == "credentials"
        || lower.starts_with("credentials.")
        || lower.contains("credential")
        || lower.contains("secret")
        || lower.contains("token")
        || lower.contains("password")
        || lower.contains("passwd")
        || lower == ".git-credentials"
        || lower == ".npmrc"
        || lower == ".pypirc"
        || lower == ".netrc"
        || lower.ends_with(".pem")
        || lower.ends_with(".key")
        || lower.ends_with(".p12")
        || lower.ends_with(".pfx")
}

/// Build an augmented store for the WorkspaceEdit. Open documents are read
/// again so the disk snapshot, encoding, and LSP buffer must all agree before a
/// plan can be shown. Unopened workspace files are loaded into this temporary
/// store only; they are never opened in the user's UI as a side effect.
fn load_rename_documents(
    language_id: &str,
    context: &MutationRequestContext,
    edit: &lsp::WorkspaceEdit,
) -> Result<(DocumentStore, BTreeMap<String, file_commands::OpenedFile>), LspManagerError> {
    let mut store = context.request_documents.clone();
    let mut disk_files = BTreeMap::new();
    let mut source_bytes = 0u64;
    for raw_uri in workspace_edit_uris(edit)? {
        let uri = Url::parse(&raw_uri).map_err(|_| {
            LspManagerError::Protocol("LSP 이름 변경 URI가 올바르지 않습니다".into())
        })?;
        let path = store.workspace().resolve_uri(&uri).map_err(|_| {
            LspManagerError::Protocol("LSP 이름 변경 대상이 작업 폴더 밖에 있습니다".into())
        })?;
        let canonical_uri = super::documents::file_uri_from_absolute_path(&path)
            .map_err(|_| {
                LspManagerError::Protocol("LSP 이름 변경 URI를 정규화하지 못했습니다".into())
            })?
            .as_str()
            .to_owned();

        if is_sensitive_rename_path(&path) {
            return Err(LspManagerError::Protocol(
                "보안상 민감한 파일은 이름 변경 대상에 포함할 수 없습니다".into(),
            ));
        }
        if !disk_files.contains_key(&canonical_uri) {
            let size = file_commands::preflight_size(&path).map_err(|_| {
                LspManagerError::Protocol("이름 변경 대상 파일 크기를 확인하지 못했습니다".into())
            })?;
            if size > MAX_RENAME_TOTAL_BYTES as u64 {
                return Err(LspManagerError::Protocol(
                    "이름 변경 대상 파일 크기가 허용 범위를 초과했습니다".into(),
                ));
            }
            source_bytes = source_bytes.checked_add(size).ok_or_else(|| {
                LspManagerError::Protocol("이름 변경 대상 크기가 너무 큽니다".into())
            })?;
            if source_bytes > MAX_RENAME_TOTAL_BYTES as u64 {
                return Err(LspManagerError::Protocol(
                    "이름 변경 대상 전체 크기가 허용 범위를 초과했습니다".into(),
                ));
            }
        }

        let opened = if let Some(snapshot) = store.snapshot(&canonical_uri) {
            if snapshot.dirty {
                return Err(LspManagerError::Protocol(
                    "저장되지 않은 문서가 이름 변경 대상에 포함되어 있습니다. 먼저 저장하세요"
                        .into(),
                ));
            }
            let opened =
                file_commands::open_path_limited(&snapshot.path, MAX_RENAME_TOTAL_BYTES as u64)
                    .map_err(|_| {
                        LspManagerError::Protocol(
                            "이름 변경 대상 파일을 안전하게 읽지 못했습니다".into(),
                        )
                    })?;
            if opened.lossy || opened.read_only || opened.text != snapshot.text {
                return Err(LspManagerError::Protocol(
                    "이름 변경 대상 문서가 디스크 상태와 일치하지 않습니다".into(),
                ));
            }
            opened
        } else {
            let opened = file_commands::open_path_limited(&path, MAX_RENAME_TOTAL_BYTES as u64)
                .map_err(|_| {
                    LspManagerError::Protocol(
                        "이름 변경 대상 파일을 안전하게 읽지 못했습니다".into(),
                    )
                })?;
            if opened.lossy || opened.read_only {
                return Err(LspManagerError::Protocol(
                    "읽기 전용 또는 손실 디코딩 파일은 이름 변경으로 저장할 수 없습니다".into(),
                ));
            }
            store
                .open(&opened.path, language_id, opened.text.clone())
                .map_err(|_| {
                    LspManagerError::Protocol(
                        "LSP 이름 변경 대상 문서를 준비하지 못했습니다".into(),
                    )
                })?;
            opened
        };
        disk_files.insert(canonical_uri, opened);
    }
    Ok((store, disk_files))
}

fn pending_rename_files(
    store: &DocumentStore,
    disk_files: &BTreeMap<String, file_commands::OpenedFile>,
    plan: &WorkspaceEditPlan,
) -> Result<Vec<PendingRenameFile>, LspManagerError> {
    if plan.changes.len() > MAX_RENAME_FILES {
        return Err(LspManagerError::Protocol(
            "이름 변경 범위가 너무 커서 미리보기를 만들 수 없습니다".into(),
        ));
    }
    let mut total_bytes = 0usize;
    // The preview is bounded in normalized UTF-8, but the selected source
    // encoding (UTF-16/UTF-32) and restored line endings can expand the bytes
    // written to disk. Keep a second aggregate bound for the actual encoded
    // payload so approval cannot reserve a transaction larger than the worker
    // is prepared to read/write.
    let mut encoded_total_bytes = 0usize;
    let mut files = Vec::with_capacity(plan.changes.len());
    for change in &plan.changes {
        let snapshot = store.snapshot(&change.uri).ok_or_else(|| {
            LspManagerError::Protocol("이름 변경 대상 문서가 준비되지 않았습니다".into())
        })?;
        let disk = disk_files.get(&change.uri).ok_or_else(|| {
            LspManagerError::Protocol("이름 변경 대상 파일 스냅샷이 없습니다".into())
        })?;
        if snapshot.dirty || snapshot.text != disk.text {
            return Err(LspManagerError::Protocol(
                "이름 변경 대상 문서가 최신 디스크 상태와 일치하지 않습니다".into(),
            ));
        }
        // A no-op edit is harmless but does not need a backup or a preview row.
        if snapshot.text == change.text {
            continue;
        }
        total_bytes = total_bytes
            .checked_add(snapshot.text.len())
            .and_then(|value| value.checked_add(change.text.len()))
            .ok_or_else(|| {
                LspManagerError::Protocol("이름 변경 미리보기 크기가 너무 큽니다".into())
            })?;
        if total_bytes > MAX_RENAME_TOTAL_BYTES {
            return Err(LspManagerError::Protocol(
                "이름 변경 미리보기 크기가 너무 큽니다".into(),
            ));
        }
        validate_encoded_rename_output(
            &change.text,
            disk.encoding,
            disk.line_ending,
            &mut encoded_total_bytes,
        )?;
        let path = PathBuf::from(&disk.path);
        let display_path = store
            .workspace()
            .relative_path(&path)
            .filter(|relative| !relative.as_os_str().is_empty())
            .map(|relative| relative.to_string_lossy().replace('\\', "/"))
            .ok_or_else(|| {
                LspManagerError::Protocol("이름 변경 대상이 작업 폴더 밖에 있습니다".into())
            })?;
        let ranges = plan
            .edits
            .iter()
            .filter(|edit| edit.uri == change.uri)
            .map(|edit| RenamePreviewRange {
                range: edit.range,
                new_text: edit.new_text.clone(),
            })
            .collect::<Vec<_>>();
        files.push(PendingRenameFile {
            path,
            display_path,
            before_text: disk.text.clone(),
            after_text: change.text.clone(),
            encoding: disk.encoding,
            line_ending: disk.line_ending,
            expected_mtime: disk.mtime,
            expected_size: disk.size,
            expected_content_hash: disk.content_hash.clone(),
            expected_identity: disk.identity,
            ranges,
        });
    }
    Ok(files)
}

fn validate_encoded_rename_output(
    text: &str,
    encoding: Encoding,
    line_ending: LineEnding,
    total_bytes: &mut usize,
) -> Result<usize, LspManagerError> {
    let encoded_size = file_commands::encode_for_save(text, encoding, line_ending)
        .map_err(|_| {
            LspManagerError::Protocol(
                "이름 변경 결과를 저장 가능한 인코딩으로 변환하지 못했습니다".into(),
            )
        })?
        .len();
    *total_bytes = total_bytes
        .checked_add(encoded_size)
        .ok_or_else(|| LspManagerError::Protocol("이름 변경 저장 크기가 너무 큽니다".into()))?;
    if encoded_size > MAX_RENAME_TOTAL_BYTES || *total_bytes > MAX_RENAME_TOTAL_BYTES {
        return Err(LspManagerError::Protocol(
            "이름 변경 저장 크기가 너무 커서 미리보기를 만들 수 없습니다".into(),
        ));
    }
    Ok(encoded_size)
}

fn bounded_rename_excerpt(text: &str) -> String {
    if text.len() <= MAX_RENAME_PREVIEW_BYTES {
        return text.to_owned();
    }
    const MARKER: &str = "\n… 미리보기 생략 …\n";
    // Keep the marker in the public bound too. The previous half-bytes
    // calculation bounded the source slices but not the marker itself, so a
    // large replacement could make the renderer payload exceed its contract.
    let content_budget = MAX_RENAME_PREVIEW_BYTES.saturating_sub(MARKER.len());
    let leading_budget = content_budget / 2;
    let trailing_budget = content_budget.saturating_sub(leading_budget);
    let leading_end = text
        .char_indices()
        .take_while(|(index, character)| *index + character.len_utf8() <= leading_budget)
        .map(|(index, character)| index + character.len_utf8())
        .last()
        .unwrap_or(0);
    let trailing_start = text.len().saturating_sub(trailing_budget);
    let trailing_start = text
        .char_indices()
        .find(|(index, _)| *index >= trailing_start)
        .map(|(index, _)| index)
        .unwrap_or(text.len());
    format!(
        "{}{}{}",
        &text[..leading_end],
        MARKER,
        &text[trailing_start..]
    )
}

fn write_rename_journal(directory: &Path, journal: &RenameJournal) -> Result<PathBuf, String> {
    let path = directory.join("journal.json");
    let bytes =
        serde_json::to_vec(journal).map_err(|_| "rename journal encoding failed".to_owned())?;
    if bytes.len() as u64 > MAX_RENAME_JOURNAL_BYTES {
        return Err("rename journal exceeds its size bound".to_owned());
    }
    file_commands::write_private_atomic(&path, &bytes)
        .map_err(|_| "rename journal could not be written".to_owned())?;
    Ok(path)
}

fn read_bounded_journal(path: &Path) -> Result<Vec<u8>, ()> {
    file_commands::read_stable_limited(path, Some(MAX_RENAME_JOURNAL_BYTES))
        .map(|(_, bytes)| bytes)
        .map_err(|_| ())
}

fn valid_rename_journal(journal: &RenameJournal) -> bool {
    journal.schema == 1
        && !journal.plan_id.is_empty()
        && journal.plan_id.len() <= 128
        && journal
            .plan_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        && !journal.workspace_root.is_empty()
        && journal.workspace_root.len() <= 32 * 1024
        && Path::new(&journal.workspace_root).is_absolute()
        && journal.entries.len() <= MAX_RENAME_JOURNAL_ENTRIES
        && journal.entries.iter().all(|entry| {
            !entry.target.is_empty()
                && !entry.backup.is_empty()
                && entry.target.len() <= 32 * 1024
                && entry.backup.len() <= 32 * 1024
                && entry.before_size <= MAX_RENAME_TOTAL_BYTES as u64
                && entry.after_size <= MAX_RENAME_TOTAL_BYTES as u64
                && is_hex_hash(&entry.before_hash)
                && is_hex_hash(&entry.after_hash)
        })
        && journal
            .entries
            .iter()
            .try_fold(0u64, |total, entry| {
                total
                    .checked_add(entry.before_size)
                    .and_then(|total| total.checked_add(entry.after_size))
            })
            .is_some_and(|total| total <= MAX_RENAME_RECOVERY_BYTES)
}

fn is_hex_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_rename_plan_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn restore_or_remove_recovered_journal(directory: &Path, journal: &RenameJournal) -> bool {
    if !valid_rename_journal(journal)
        || devbox_filesystem::filesystem_identity(directory, true).is_err()
        || directory.file_name().and_then(|name| name.to_str()) != Some(journal.plan_id.as_str())
    {
        return false;
    }
    if journal.state == RenameJournalState::Committed {
        return fs::remove_dir_all(directory).is_ok();
    }
    let Ok(workspace_root) = WorkspaceRoot::new(&journal.workspace_root) else {
        return false;
    };
    let mut complete = true;
    for entry in &journal.entries {
        let target = PathBuf::from(&entry.target);
        let backup = PathBuf::from(&entry.backup);
        // Journal recovery is allowed to replace only the exact regular file
        // named when the transaction was prepared. A target path that became
        // a symlink/reparse point (even one pointing back inside the workspace)
        // is left for explicit user recovery instead of being followed.
        if !target.is_absolute() || !backup.is_absolute() {
            complete = false;
            continue;
        }
        let Ok(target_identity) = devbox_filesystem::filesystem_identity(&target, false) else {
            complete = false;
            continue;
        };
        let Ok(canonical_target) = target.canonicalize() else {
            complete = false;
            continue;
        };
        if !canonical_target.starts_with(workspace_root.path())
            || backup.parent() != Some(directory)
        {
            complete = false;
            continue;
        }
        let Some(backup_name) = backup.file_name().and_then(|name| name.to_str()) else {
            complete = false;
            continue;
        };
        if !backup_name.starts_with("backup-") || !backup_name.ends_with(".bak") {
            complete = false;
            continue;
        }
        let Ok((metadata, bytes)) = file_commands::read_stable_limited(
            &canonical_target,
            Some(MAX_RENAME_TOTAL_BYTES as u64),
        ) else {
            complete = false;
            continue;
        };
        let hash = file_commands::content_hash(&bytes);
        if metadata.len() == entry.before_size && hash == entry.before_hash {
            continue;
        }
        if metadata.len() != entry.after_size || hash != entry.after_hash {
            // A user/external writer owns this newer content. Never overwrite
            // it as part of automatic startup recovery.
            complete = false;
            continue;
        }
        let Ok(identity) = devbox_filesystem::filesystem_identity(&backup, false) else {
            complete = false;
            continue;
        };
        let descriptor = file_commands::CreatedBackup {
            path: backup,
            identity,
            size: entry.before_size,
            content_hash: entry.before_hash.clone(),
        };
        let Ok(target_mtime) = file_commands::modified_epoch_nanos(&metadata) else {
            complete = false;
            continue;
        };
        if file_commands::restore_sibling_backup_if_current_limited(
            &canonical_target,
            &descriptor,
            Some(file_commands::ExpectedFileSnapshot {
                mtime: target_mtime,
                size: metadata.len(),
                content_hash: &entry.after_hash,
                identity: Some(target_identity),
            }),
            Some(MAX_RENAME_TOTAL_BYTES as u64),
        )
        .is_err()
        {
            complete = false;
        }
    }
    complete && fs::remove_dir_all(directory).is_ok()
}

/// Recover only transactions whose target still contains the exact post-write
/// bytes recorded in the private journal. Anything changed by a user is left
/// untouched and the journal remains for an explicit future recovery attempt.
fn recover_rename_journals(root: &Path) {
    if devbox_filesystem::filesystem_identity(root, true).is_err() {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let mut inspected = 0;
    let mut remaining_scan_bytes = MAX_RENAME_RECOVERY_SCAN_BYTES;
    for entry in entries.flatten() {
        if inspected >= MAX_PENDING_RENAMES {
            break;
        }
        let directory = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        inspected += 1;
        if devbox_filesystem::filesystem_identity(&directory, true).is_err() {
            continue;
        }
        let journal_path = directory.join("journal.json");
        let Ok(bytes) = read_bounded_journal(&journal_path) else {
            continue;
        };
        let journal_bytes = bytes.len() as u64;
        if journal_bytes > remaining_scan_bytes {
            break;
        }
        remaining_scan_bytes -= journal_bytes;
        let Ok(journal) = serde_json::from_slice::<RenameJournal>(&bytes) else {
            continue;
        };
        // Recovery reads each target once to classify it and the restore helper
        // may read the post-write target twice plus the backup once. Reserve a
        // conservative cost before touching any recorded absolute path.
        let Some(estimated_bytes) = journal.entries.iter().try_fold(0_u64, |total, entry| {
            total
                .checked_add(entry.before_size)
                .and_then(|total| total.checked_add(entry.after_size.checked_mul(3)?))
        }) else {
            continue;
        };
        if estimated_bytes > remaining_scan_bytes {
            continue;
        }
        remaining_scan_bytes -= estimated_bytes;
        let _ = restore_or_remove_recovered_journal(&directory, &journal);
    }
}

fn validate_pending_rename_documents(
    documents: &DocumentStore,
    pending: &PendingRename,
) -> Result<(), LspManagerError> {
    let guarded = pending
        .open_documents
        .iter()
        .map(|document| document.uri.as_str())
        .collect::<BTreeSet<_>>();
    for expected in &pending.open_documents {
        let Some(current) = documents.snapshot(&expected.uri) else {
            return Err(LspManagerError::Protocol(
                "이름 변경 대상 문서가 닫혔습니다".into(),
            ));
        };
        if current.version != expected.version
            || current.text != expected.text
            || current.dirty != expected.dirty
        {
            return Err(LspManagerError::Protocol(
                "이름 변경 미리보기 이후 문서가 변경되었습니다".into(),
            ));
        }
    }
    // A file that was unopened at preview time must not be silently adopted if
    // the user opened it while the approval dialog was waiting. Requiring the
    // same snapshot closes the close/reopen generation hole as well.
    for change in &pending.plan.changes {
        if guarded.contains(change.uri.as_str()) {
            continue;
        }
        if documents.snapshot(&change.uri).is_some() {
            return Err(LspManagerError::Protocol(
                "이름 변경 대상 문서가 미리보기 이후 열렸습니다".into(),
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct RenameBackup {
    target: PathBuf,
    backup: file_commands::CreatedBackup,
    applied_mtime: Option<i64>,
    applied_size: Option<u64>,
    applied_content_hash: Option<String>,
    applied_identity: Option<FilesystemIdentity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenameJournal {
    schema: u8,
    plan_id: String,
    workspace_root: String,
    state: RenameJournalState,
    entries: Vec<RenameJournalEntry>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum RenameJournalState {
    Applying,
    Committed,
    RollbackFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenameJournalEntry {
    target: String,
    backup: String,
    before_size: u64,
    before_hash: String,
    after_size: u64,
    after_hash: String,
}

#[derive(Clone)]
struct DiskRenameOutcome {
    success: bool,
    rolled_back: bool,
    files: Vec<RenameFileResult>,
    backups: Vec<RenameBackup>,
    backup_dir: Option<PathBuf>,
    error: Option<String>,
}

enum SavedMutationError {
    Preflight(LspManagerError),
    Server(LspManagerError),
    Cancelled { mirror_may_be_partial: bool },
}

impl fmt::Display for SavedMutationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Preflight(error) | Self::Server(error) => error.fmt(formatter),
            Self::Cancelled { .. } => formatter.write_str("이름 변경이 취소되었습니다"),
        }
    }
}

fn rename_checkpoint(cancellation: &AtomicBool, deadline: Instant) -> Result<(), &'static str> {
    if cancellation.load(Ordering::Acquire) {
        return Err("이름 변경이 취소되었습니다");
    }
    if Instant::now() >= deadline {
        return Err("이름 변경 작업 시간이 초과되었습니다");
    }
    Ok(())
}

/// Write a mutation notification without allowing a stalled language-server
/// pipe to bypass the rename cancellation/deadline contract. Dropping the
/// pending write releases the process writer; the caller will tear down and
/// replay the mirror when a notification was already partially delivered.
async fn notify_rename_with_control(
    process: &LspProcess,
    method: &'static str,
    params: serde_json::Value,
    cancellation: &AtomicBool,
    deadline: Instant,
    mirror_may_be_partial: bool,
) -> Result<(), SavedMutationError> {
    let mut notify = Box::pin(process.notify(method, Some(params)));
    loop {
        if cancellation.load(Ordering::Acquire) {
            return Err(SavedMutationError::Cancelled {
                mirror_may_be_partial,
            });
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(SavedMutationError::Cancelled {
                mirror_may_be_partial,
            });
        }
        let poll_interval = remaining.min(Duration::from_millis(25));
        tokio::select! {
            result = &mut notify => {
                return result
                    .map_err(|error| SavedMutationError::Server(LspManagerError::Protocol(error.to_string())));
            }
            _ = tokio::time::sleep(poll_interval) => {}
        }
    }
}

fn apply_pending_rename_files(
    files: &[PendingRenameFile],
    backup_root: &Path,
    plan_id: &str,
    workspace_root: &Path,
    cancellation: Arc<AtomicBool>,
    deadline: Instant,
) -> DiskRenameOutcome {
    let mut results = files
        .iter()
        .map(|file| RenameFileResult {
            path: file.display_path.clone(),
            status: RenameFileStatus::NotApplied,
            mtime_nanos: None,
            size: None,
            content_hash: None,
            error: None,
        })
        .collect::<Vec<_>>();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let mut backups = Vec::with_capacity(files.len());
    let mut backup_dir = None;

    // Recheck every file before creating the first backup. This is intentionally
    // repeated after the UI-side document gate and uses identity in addition to
    // mtime/size/hash for delete-and-recreate races.
    let mut current_bytes = Vec::with_capacity(files.len());
    for (index, file) in files.iter().enumerate() {
        if let Err(error) = rename_checkpoint(&cancellation, deadline) {
            results[index].error = Some(error.into());
            return DiskRenameOutcome {
                success: false,
                rolled_back: false,
                files: results,
                backups,
                backup_dir,
                error: Some(error.into()),
            };
        }
        let current = match file_commands::read_stable_limited(
            &file.path,
            Some(MAX_RENAME_TOTAL_BYTES as u64),
        ) {
            Ok(current) => current,
            Err(_) => {
                results[index].status = RenameFileStatus::Conflict;
                results[index].error = Some("파일 스냅샷을 확인하지 못했습니다".into());
                return DiskRenameOutcome {
                    success: false,
                    rolled_back: false,
                    files: results,
                    backups,
                    backup_dir,
                    error: Some("파일이 변경되었거나 읽을 수 없어 적용을 중단했습니다".into()),
                };
            }
        };
        let Ok(mtime) = file_commands::modified_epoch_nanos(&current.0) else {
            results[index].status = RenameFileStatus::Conflict;
            results[index].error = Some("파일 스냅샷을 확인하지 못했습니다".into());
            return DiskRenameOutcome {
                success: false,
                rolled_back: false,
                files: results,
                backups,
                backup_dir,
                error: Some("파일 스냅샷을 확인하지 못해 적용을 중단했습니다".into()),
            };
        };
        let Ok(identity) = devbox_filesystem::filesystem_identity(&file.path, false) else {
            results[index].status = RenameFileStatus::Conflict;
            results[index].error = Some("파일 identity를 확인하지 못했습니다".into());
            return DiskRenameOutcome {
                success: false,
                rolled_back: false,
                files: results,
                backups,
                backup_dir,
                error: Some("파일 identity를 확인하지 못해 적용을 중단했습니다".into()),
            };
        };
        if identity != file.expected_identity
            || mtime != file.expected_mtime
            || current.0.len() != file.expected_size
            || file_commands::content_hash(&current.1) != file.expected_content_hash
        {
            results[index].status = RenameFileStatus::Conflict;
            results[index].error = Some("적용 전 파일이 변경되었습니다".into());
            return DiskRenameOutcome {
                success: false,
                rolled_back: false,
                files: results,
                backups,
                backup_dir,
                error: Some("적용 전 파일이 변경되어 이름 변경을 중단했습니다".into()),
            };
        }
        current_bytes.push(current);
    }

    let mut after_snapshots = Vec::with_capacity(files.len());
    let mut encoded_total_bytes = 0usize;
    for (index, file) in files.iter().enumerate() {
        if let Err(error) = rename_checkpoint(&cancellation, deadline) {
            results[index].error = Some(error.into());
            return DiskRenameOutcome {
                success: false,
                rolled_back: false,
                files: results,
                backups,
                backup_dir,
                error: Some(error.into()),
            };
        }
        let Ok(bytes) =
            file_commands::encode_for_save(&file.after_text, file.encoding, file.line_ending)
        else {
            results[index].error =
                Some("이름 변경 결과를 저장 가능한 인코딩으로 변환하지 못했습니다".into());
            return DiskRenameOutcome {
                success: false,
                rolled_back: false,
                files: results,
                backups,
                backup_dir,
                error: Some("이름 변경 결과를 저장 가능한 인코딩으로 변환하지 못했습니다".into()),
            };
        };
        encoded_total_bytes = match encoded_total_bytes.checked_add(bytes.len()) {
            Some(total)
                if bytes.len() <= MAX_RENAME_TOTAL_BYTES && total <= MAX_RENAME_TOTAL_BYTES =>
            {
                total
            }
            _ => {
                results[index].error = Some("이름 변경 저장 크기가 너무 큽니다".into());
                return DiskRenameOutcome {
                    success: false,
                    rolled_back: false,
                    files: results,
                    backups,
                    backup_dir,
                    error: Some("이름 변경 저장 크기가 너무 커서 적용을 중단했습니다".into()),
                };
            }
        };
        after_snapshots.push((bytes.len() as u64, file_commands::content_hash(&bytes)));
    }

    let directory = match file_commands::create_private_backup_dir(backup_root, plan_id) {
        Ok(directory) => directory,
        Err(_) => {
            return DiskRenameOutcome {
                success: false,
                rolled_back: false,
                files: results,
                backups,
                backup_dir,
                error: Some("이름 변경용 보안 백업 영역을 만들지 못했습니다".into()),
            }
        }
    };
    backup_dir = Some(directory.clone());

    for (index, file) in files.iter().enumerate() {
        if let Err(error) = rename_checkpoint(&cancellation, deadline) {
            let _ = fs::remove_dir_all(&directory);
            results[index].error = Some(error.into());
            return DiskRenameOutcome {
                success: false,
                rolled_back: false,
                files: results,
                backups,
                backup_dir: None,
                error: Some(error.into()),
            };
        }
        let backup = match file_commands::create_sibling_backup(
            &directory,
            &file.path,
            &current_bytes[index].1,
            &current_bytes[index].0.permissions(),
            nonce,
            index,
        ) {
            Ok(backup) => backup,
            Err(_) => {
                results[index].status = RenameFileStatus::Failed;
                results[index].error = Some("백업을 만들지 못했습니다".into());
                let _ = fs::remove_dir_all(&directory);
                return DiskRenameOutcome {
                    success: false,
                    rolled_back: false,
                    files: results,
                    backups: Vec::new(),
                    backup_dir: None,
                    error: Some("백업을 만들지 못해 이름 변경을 시작하지 않았습니다".into()),
                };
            }
        };
        backups.push(RenameBackup {
            target: file.path.clone(),
            backup,
            applied_mtime: None,
            applied_size: None,
            applied_content_hash: None,
            applied_identity: None,
        });
    }

    let journal = RenameJournal {
        schema: 1,
        plan_id: plan_id.to_owned(),
        workspace_root: workspace_root.to_string_lossy().into_owned(),
        state: RenameJournalState::Applying,
        entries: files
            .iter()
            .zip(backups.iter())
            .zip(after_snapshots.iter())
            .map(
                |((file, backup), (after_size, after_hash))| RenameJournalEntry {
                    target: file.path.to_string_lossy().into_owned(),
                    backup: backup.backup.path.to_string_lossy().into_owned(),
                    before_size: file.expected_size,
                    before_hash: file.expected_content_hash.clone(),
                    after_size: *after_size,
                    after_hash: after_hash.clone(),
                },
            )
            .collect(),
    };
    if write_rename_journal(&directory, &journal).is_err() {
        let _ = fs::remove_dir_all(&directory);
        return DiskRenameOutcome {
            success: false,
            rolled_back: false,
            files: results,
            backups: Vec::new(),
            backup_dir: None,
            error: Some("이름 변경 transaction journal을 만들지 못했습니다".into()),
        };
    }

    for (index, file) in files.iter().enumerate() {
        if let Err(error) = rename_checkpoint(&cancellation, deadline) {
            let mut outcome = DiskRenameOutcome {
                success: false,
                rolled_back: false,
                files: results,
                backups,
                backup_dir,
                error: Some(error.into()),
            };
            let rolled_back = rollback_disk_rename(&mut outcome);
            outcome.error = Some(if rolled_back {
                error.into()
            } else {
                "이름 변경 취소와 되돌리기에 실패했습니다. 백업 journal을 확인하세요".into()
            });
            return outcome;
        }
        let saved = match file_commands::save_path_limited(
            &file.path,
            &file.after_text,
            file.encoding,
            file.line_ending,
            file_commands::ExpectedFileSnapshot {
                mtime: file.expected_mtime,
                size: file.expected_size,
                content_hash: &file.expected_content_hash,
                identity: Some(file.expected_identity),
            },
            false,
            Some(MAX_RENAME_TOTAL_BYTES as u64),
        ) {
            Ok(saved) => saved,
            Err(_) => {
                results[index].status = RenameFileStatus::Failed;
                results[index].error = Some("파일을 쓰지 못했습니다".into());
                let mut outcome = DiskRenameOutcome {
                    success: false,
                    rolled_back: false,
                    files: results,
                    backups,
                    backup_dir,
                    error: Some("파일 쓰기에 실패했습니다".into()),
                };
                let rolled_back = rollback_disk_rename(&mut outcome);
                outcome.error = Some(if rolled_back {
                    "파일 쓰기에 실패해 변경된 파일을 되돌렸습니다".into()
                } else {
                    "파일 쓰기와 되돌리기에 실패했습니다. 백업 journal을 확인하세요".into()
                });
                return outcome;
            }
        };
        let saved_content_hash = saved.content_hash.clone();
        results[index] = RenameFileResult {
            path: file.display_path.clone(),
            status: RenameFileStatus::Applied,
            mtime_nanos: Some(saved.mtime.to_string()),
            size: Some(saved.size),
            content_hash: Some(saved_content_hash.clone()),
            error: None,
        };
        let backup = &mut backups[index];
        backup.applied_mtime = Some(saved.mtime);
        backup.applied_size = Some(saved.size);
        backup.applied_content_hash = Some(saved_content_hash);
        backup.applied_identity = saved.identity;
    }

    if let Err(error) = rename_checkpoint(&cancellation, deadline) {
        let mut outcome = DiskRenameOutcome {
            success: false,
            rolled_back: false,
            files: results,
            backups,
            backup_dir,
            error: Some(error.into()),
        };
        let rolled_back = rollback_disk_rename(&mut outcome);
        outcome.error = Some(if rolled_back {
            error.into()
        } else {
            "이름 변경 취소와 되돌리기에 실패했습니다. 백업 journal을 확인하세요".into()
        });
        return outcome;
    }

    DiskRenameOutcome {
        success: true,
        rolled_back: false,
        files: results,
        backups,
        backup_dir,
        error: None,
    }
}

fn rollback_disk_rename(outcome: &mut DiskRenameOutcome) -> bool {
    let rolled_back = rollback_rename_backups(&outcome.backups, outcome.backups.len());
    update_rename_results_after_rollback(&mut outcome.files, rolled_back);
    cleanup_rename_backups(&outcome.backups, outcome.backup_dir.as_deref(), rolled_back);
    outcome.rolled_back = rolled_back;
    if rolled_back {
        outcome.backup_dir = None;
    }
    rolled_back
}

/// Rollback can read and atomically replace every file in a transaction. Keep
/// that synchronous work off the async runtime even when it is triggered by a
/// cancellation or a server notification failure after the worker returned.
async fn rollback_disk_rename_blocking(mut outcome: DiskRenameOutcome) -> DiskRenameOutcome {
    let fallback = outcome.clone();
    match tokio::task::spawn_blocking(move || {
        rollback_disk_rename(&mut outcome);
        outcome
    })
    .await
    {
        Ok(outcome) => outcome,
        Err(_) => {
            let mut fallback = fallback;
            update_rename_results_after_rollback(&mut fallback.files, false);
            fallback.rolled_back = false;
            fallback.error =
                Some("이름 변경 되돌리기 작업이 중단되었습니다. 백업 journal을 확인하세요".into());
            fallback
        }
    }
}

fn rollback_rename_backups(backups: &[RenameBackup], count: usize) -> bool {
    let mut success = true;
    for backup in backups.iter().take(count).rev() {
        let (Some(expected_mtime), Some(expected_size), Some(expected_hash)) = (
            backup.applied_mtime,
            backup.applied_size,
            backup.applied_content_hash.as_deref(),
        ) else {
            continue;
        };
        let current = match file_commands::read_stable_limited(
            &backup.target,
            Some(MAX_RENAME_TOTAL_BYTES as u64),
        ) {
            Ok(current) => current,
            Err(_) => {
                success = false;
                continue;
            }
        };
        let current_mtime = match file_commands::modified_epoch_nanos(&current.0) {
            Ok(mtime) => mtime,
            Err(_) => {
                success = false;
                continue;
            }
        };
        let identity_matches = backup.applied_identity.is_none_or(|identity| {
            devbox_filesystem::filesystem_identity(&backup.target, false)
                .map(|current| current == identity)
                .unwrap_or(false)
        });
        if !identity_matches
            || current_mtime != expected_mtime
            || current.0.len() != expected_size
            || file_commands::content_hash(&current.1) != expected_hash
        {
            success = false;
            continue;
        }
        if file_commands::restore_sibling_backup_if_current(
            &backup.target,
            &backup.backup,
            Some(file_commands::ExpectedFileSnapshot {
                mtime: expected_mtime,
                size: expected_size,
                content_hash: expected_hash,
                identity: backup.applied_identity,
            }),
        )
        .is_err()
        {
            success = false;
        }
    }
    success
}

fn cleanup_rename_backups(backups: &[RenameBackup], backup_dir: Option<&Path>, success: bool) {
    if !success {
        return;
    }
    if let Some(directory) = backup_dir {
        let _ = fs::remove_dir_all(directory);
    } else {
        for backup in backups {
            let _ = fs::remove_file(&backup.backup.path);
        }
    }
}

fn mark_rename_journal_committed(outcome: &DiskRenameOutcome) -> bool {
    let Some(directory) = outcome.backup_dir.as_deref() else {
        return true;
    };
    let path = directory.join("journal.json");
    let Ok(bytes) = read_bounded_journal(&path) else {
        return false;
    };
    let Ok(mut journal) = serde_json::from_slice::<RenameJournal>(&bytes) else {
        return false;
    };
    if !valid_rename_journal(&journal)
        || devbox_filesystem::filesystem_identity(directory, true).is_err()
    {
        return false;
    }
    journal.state = RenameJournalState::Committed;
    write_rename_journal(directory, &journal).is_ok()
}

fn update_rename_results_after_rollback(results: &mut [RenameFileResult], success: bool) {
    for result in results {
        if result.status != RenameFileStatus::Applied {
            continue;
        }
        result.status = if success {
            RenameFileStatus::RolledBack
        } else {
            RenameFileStatus::RollbackFailed
        };
        result.mtime_nanos = None;
        result.size = None;
        result.content_hash = None;
        result.error = Some(if success {
            "되돌렸습니다".into()
        } else {
            "되돌리기에 실패했습니다".into()
        });
    }
}

/// Apply a plan to the server mirror after native files are committed. The
/// mirror remains unchanged until all didChange and didSave notifications have
/// succeeded, preserving the same all-or-rollback boundary as the disk layer.
async fn apply_saved_mutation_plan(
    session: &Arc<LanguageSession>,
    plan: &WorkspaceEditPlan,
    cancellation: &AtomicBool,
    deadline: Instant,
) -> Result<AppliedDocumentEdits, SavedMutationError> {
    if plan.changes.is_empty() {
        return Ok(AppliedDocumentEdits {
            documents: Vec::new(),
        });
    }

    let mut documents = session.documents.lock().await;
    let mut staged = documents.clone();
    let applied = apply_workspace_edit(&mut staged, plan)
        .map_err(|error| SavedMutationError::Preflight(feature_error(error)))?;
    let capabilities = session.client.capabilities().await;
    if !capabilities.supports("textDocument/didChange") {
        return Err(SavedMutationError::Server(LspManagerError::Protocol(
            "이름 변경을 반영할 didChange capability가 없습니다".into(),
        )));
    }
    let mut mirror_may_be_partial = false;
    if capabilities.supports("textDocument/didChange") {
        for change in &applied.changes {
            if rename_checkpoint(cancellation, deadline).is_err() {
                return Err(SavedMutationError::Cancelled {
                    mirror_may_be_partial,
                });
            }
            // A notify can reach the child and still return a transport error,
            // so mark the mirror as potentially changed before writing it.
            mirror_may_be_partial = true;
            notify_rename_with_control(
                &session.process,
                "textDocument/didChange",
                json!({
                    "textDocument": { "uri": change.uri, "version": change.version },
                    "contentChanges": change.content_changes,
                }),
                cancellation,
                deadline,
                mirror_may_be_partial,
            )
            .await?;
            let _ = session
                .process
                .wait_for_exit(Duration::from_millis(10))
                .await;
            if !matches!(session.process.state().await, ProcessState::Running) {
                return Err(SavedMutationError::Server(LspManagerError::Protocol(
                    "language server exited during workspace edit notification".into(),
                )));
            }
        }
    }
    if capabilities.supports("textDocument/didSave") {
        for change in &applied.changes {
            if rename_checkpoint(cancellation, deadline).is_err() {
                return Err(SavedMutationError::Cancelled {
                    mirror_may_be_partial,
                });
            }
            mirror_may_be_partial = true;
            notify_rename_with_control(
                &session.process,
                "textDocument/didSave",
                json!({ "textDocument": { "uri": change.uri } }),
                cancellation,
                deadline,
                mirror_may_be_partial,
            )
            .await?;
            let _ = session
                .process
                .wait_for_exit(Duration::from_millis(10))
                .await;
            if !matches!(session.process.state().await, ProcessState::Running) {
                return Err(SavedMutationError::Server(LspManagerError::Protocol(
                    "language server exited during workspace save notification".into(),
                )));
            }
        }
    }
    if rename_checkpoint(cancellation, deadline).is_err() {
        return Err(SavedMutationError::Cancelled {
            mirror_may_be_partial,
        });
    }
    for change in &applied.changes {
        staged.mark_saved(&change.uri).map_err(|error| {
            SavedMutationError::Preflight(LspManagerError::Protocol(error.to_string()))
        })?;
    }
    let edited = plan
        .changes
        .iter()
        .map(|change| {
            let snapshot = staged.snapshot(&change.uri).ok_or_else(|| {
                SavedMutationError::Preflight(LspManagerError::Protocol(format!(
                    "edited document disappeared before commit: {}",
                    change.uri
                )))
            })?;
            Ok(EditedDocument {
                uri: snapshot.uri,
                version: snapshot.version,
                text: snapshot.text,
            })
        })
        .collect::<Result<Vec<_>, SavedMutationError>>()?;
    if rename_checkpoint(cancellation, deadline).is_err() {
        return Err(SavedMutationError::Cancelled {
            mirror_may_be_partial,
        });
    }
    *documents = staged;
    Ok(AppliedDocumentEdits { documents: edited })
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

    #[test]
    fn rename_workspace_edit_bounds_cover_uri_edits_and_replacements() {
        let uri = "file:///workspace/main.rs".parse::<lsp::Uri>().unwrap();
        let edit = lsp::WorkspaceEdit {
            changes: Some(std::collections::HashMap::from([(
                uri,
                vec![lsp::TextEdit {
                    range: lsp::Range {
                        start: lsp::Position::new(0, 0),
                        end: lsp::Position::new(0, 1),
                    },
                    new_text: "ok".into(),
                }],
            )])),
            document_changes: None,
            change_annotations: None,
        };
        validate_rename_edit_bounds(&edit).unwrap();

        let too_many = lsp::WorkspaceEdit {
            changes: Some(std::collections::HashMap::from([(
                "file:///workspace/main.rs".parse::<lsp::Uri>().unwrap(),
                (0..=MAX_RENAME_EDITS)
                    .map(|_| lsp::TextEdit {
                        range: lsp::Range {
                            start: lsp::Position::new(0, 0),
                            end: lsp::Position::new(0, 0),
                        },
                        new_text: String::new(),
                    })
                    .collect(),
            )])),
            document_changes: None,
            change_annotations: None,
        };
        assert!(validate_rename_edit_bounds(&too_many).is_err());

        let too_much_text = lsp::WorkspaceEdit {
            changes: Some(std::collections::HashMap::from([(
                "file:///workspace/main.rs".parse::<lsp::Uri>().unwrap(),
                vec![lsp::TextEdit {
                    range: lsp::Range {
                        start: lsp::Position::new(0, 0),
                        end: lsp::Position::new(0, 0),
                    },
                    new_text: "x".repeat(MAX_RENAME_NEW_TEXT_BYTES + 1),
                }],
            )])),
            document_changes: None,
            change_annotations: None,
        };
        assert!(validate_rename_edit_bounds(&too_much_text).is_err());

        let too_many_uris = lsp::WorkspaceEdit {
            changes: Some(
                (0..=MAX_RENAME_URIS)
                    .map(|index| {
                        (
                            format!("file:///workspace/{index}.rs")
                                .parse::<lsp::Uri>()
                                .unwrap(),
                            Vec::new(),
                        )
                    })
                    .collect(),
            ),
            document_changes: None,
            change_annotations: None,
        };
        assert!(workspace_edit_uris(&too_many_uris).is_err());

        let too_long_uri = lsp::WorkspaceEdit {
            changes: Some(std::collections::HashMap::from([(
                format!("file:///workspace/{}.rs", "a".repeat(MAX_RENAME_URI_BYTES))
                    .parse::<lsp::Uri>()
                    .unwrap(),
                Vec::new(),
            )])),
            document_changes: None,
            change_annotations: None,
        };
        assert!(workspace_edit_uris(&too_long_uri).is_err());
    }

    #[test]
    fn rename_preview_excerpt_is_utf8_safe_and_stays_within_wire_bound() {
        let text = "한".repeat(MAX_RENAME_PREVIEW_BYTES);
        let excerpt = bounded_rename_excerpt(&text);
        assert!(excerpt.len() <= MAX_RENAME_PREVIEW_BYTES);
        assert!(std::str::from_utf8(excerpt.as_bytes()).is_ok());
        assert!(excerpt.contains("미리보기 생략"));
    }

    #[test]
    fn rename_encoded_output_can_be_larger_than_normalized_preview_text() {
        let source = "x";
        let output = "x".repeat(MAX_RENAME_TOTAL_BYTES - source.len());
        let encoded =
            file_commands::encode_for_save(&output, Encoding::utf16_le(false), LineEnding::Lf)
                .unwrap();
        assert_eq!(source.len() + output.len(), MAX_RENAME_TOTAL_BYTES);
        assert!(encoded.len() > MAX_RENAME_TOTAL_BYTES);
        let mut encoded_total = 0;
        assert!(validate_encoded_rename_output(
            &output,
            Encoding::utf16_le(false),
            LineEnding::Lf,
            &mut encoded_total,
        )
        .is_err());
    }

    #[test]
    fn rename_rejects_sensitive_and_credential_like_paths() {
        for path in [
            "/workspace/.env",
            "/workspace/.env.local",
            "/workspace/.ssh/id_ed25519",
            "/workspace/.aws/credentials",
            "/workspace/client.pem",
            "/workspace/service.p12",
            "/workspace/access_token.json",
        ] {
            assert!(is_sensitive_rename_path(Path::new(path)), "{path}");
        }
        assert!(!is_sensitive_rename_path(Path::new(
            "/workspace/src/main.rs"
        )));
    }

    #[test]
    fn rename_journal_recovers_only_the_recorded_postwrite_state() {
        let workspace = tempfile::tempdir().unwrap();
        let target = workspace.path().join("main.rs");
        fs::write(&target, b"before\n").unwrap();
        let before = fs::read(&target).unwrap();
        let before_metadata = fs::metadata(&target).unwrap();
        let before_hash = file_commands::content_hash(&before);
        let backup_root = tempfile::tempdir().unwrap();
        let directory =
            file_commands::create_private_backup_dir(backup_root.path(), "rename-test").unwrap();
        let backup = file_commands::create_sibling_backup(
            &directory,
            &target,
            &before,
            &before_metadata.permissions(),
            31,
            0,
        )
        .unwrap();
        fs::write(&target, b"after\n").unwrap();
        let after = fs::read(&target).unwrap();
        let journal = RenameJournal {
            schema: 1,
            plan_id: "rename-test".into(),
            workspace_root: workspace.path().to_string_lossy().into_owned(),
            state: RenameJournalState::Applying,
            entries: vec![RenameJournalEntry {
                target: target.to_string_lossy().into_owned(),
                backup: backup.path.to_string_lossy().into_owned(),
                before_size: before.len() as u64,
                before_hash,
                after_size: after.len() as u64,
                after_hash: file_commands::content_hash(&after),
            }],
        };
        write_rename_journal(&directory, &journal).unwrap();
        recover_rename_journals(backup_root.path());
        assert_eq!(fs::read(&target).unwrap(), before);
        assert!(!directory.exists());
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

    #[test]
    fn disk_rename_rolls_back_a_prior_write_when_a_later_file_is_read_only() {
        let directory = tempfile::tempdir().unwrap();
        let first_path = directory.path().join("first.rs");
        let second_path = directory.path().join("second.rs");
        std::fs::write(&first_path, b"one\n").unwrap();
        std::fs::write(&second_path, b"two\n").unwrap();
        let first = file_commands::open_path(&first_path).unwrap();
        let second = file_commands::open_path(&second_path).unwrap();
        let mut read_only = std::fs::metadata(&second_path).unwrap().permissions();
        read_only.set_readonly(true);
        std::fs::set_permissions(&second_path, read_only).unwrap();

        let files = vec![
            PendingRenameFile {
                path: first_path.clone(),
                display_path: "first.rs".into(),
                before_text: first.text.clone(),
                after_text: "renamed-one\n".into(),
                encoding: first.encoding,
                line_ending: first.line_ending,
                expected_mtime: first.mtime,
                expected_size: first.size,
                expected_content_hash: first.content_hash.clone(),
                expected_identity: first.identity,
                ranges: Vec::new(),
            },
            PendingRenameFile {
                path: second_path.clone(),
                display_path: "second.rs".into(),
                before_text: second.text.clone(),
                after_text: "renamed-two\n".into(),
                encoding: second.encoding,
                line_ending: second.line_ending,
                expected_mtime: second.mtime,
                expected_size: second.size,
                expected_content_hash: second.content_hash.clone(),
                expected_identity: second.identity,
                ranges: Vec::new(),
            },
        ];

        let backup_root = directory.path().join("app-data");
        let outcome = apply_pending_rename_files(
            &files,
            &backup_root,
            "rename-test",
            directory.path(),
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(10),
        );
        assert!(!outcome.success);
        assert!(outcome.rolled_back);
        assert_eq!(outcome.files[0].status, RenameFileStatus::RolledBack);
        assert_eq!(outcome.files[1].status, RenameFileStatus::Failed);
        assert_eq!(std::fs::read(&first_path).unwrap(), b"one\n");
        assert_eq!(std::fs::read(&second_path).unwrap(), b"two\n");
        assert!(backup_root
            .read_dir()
            .unwrap()
            .filter_map(Result::ok)
            .next()
            .is_none());

        let mut writable = std::fs::metadata(&second_path).unwrap().permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            writable.set_mode(writable.mode() | 0o200);
        }
        #[cfg(not(unix))]
        {
            // This is test-fixture cleanup for a file that this test just made
            // readonly. The lint's Unix world-writable warning does not apply
            // to this non-Unix branch; Windows exposes no mode-bit alternative.
            #[allow(clippy::permissions_set_readonly_false)]
            writable.set_readonly(false);
        }
        std::fs::set_permissions(&second_path, writable).unwrap();
    }
}
