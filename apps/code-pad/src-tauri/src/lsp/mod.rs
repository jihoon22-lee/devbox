//! Local language-server contracts, transport, and process lifecycle.
//!
//! The transport/process code deliberately stays generic over the catalog and
//! document schema.  The schema can therefore evolve without putting protocol
//! parsing or child lifecycle policy behind catalog-specific types.

pub mod catalog;
pub mod client;
pub mod config;
pub mod documents;
pub mod features;
pub mod installer;
pub mod logs;
pub mod manager;
pub mod node_lock;
pub mod positions;
pub mod process;
pub mod runtime;
pub mod transport;

pub use catalog::{
    initial_catalog, Artifact, ArtifactKind, CapabilitiesHint, Catalog, CommandSpec, CustomServer,
    InstalledServer, InstalledServerIndex, LanguageSupport, LspConfig, ManifestFiles, RuntimeKind,
    RuntimeSpec, SchemaError, ServerCatalog, ServerManifest, ServerRef, UpdatePolicy,
    ValidationError, LSP_CONFIG_SCHEMA_VERSION, LSP_INSTALLED_SCHEMA_VERSION,
    WINDOWS_X86_64_PLATFORM,
};
pub use client::{
    CapabilitySet, ClientError, ClientStatus, InitializeConfig, LspClient, ServerInfo,
    INITIALIZE_TIMEOUT,
};
pub use config::{
    config_path_from_app_local_data_dir, load_from_app_local_data_dir, load_from_path,
    load_from_path_with_status, lsp_config_path, save_to_app_local_data_dir, save_to_path,
    LoadedLspConfig, LspConfigLoadError, LspConfigSaveError, LSP_CONFIG_DIRECTORY,
    LSP_CONFIG_FILE_NAME,
};
pub use documents::{
    AtomicDocumentChange, DidChange, DidClose, DidOpen, DidSave, DocumentError, DocumentSnapshot,
    DocumentStore, RequestSnapshot, SyncKind, TextChange, WorkspaceRoot,
};
pub use features::{
    apply_formatting_edits, apply_workspace_edit, build_completion_params, build_definition_params,
    build_formatting_params, build_hover_params, build_pull_diagnostics_params,
    build_reference_params, build_rename_params, cancel_request, filter_definition_response,
    filter_reference_locations, parse_completion_response, parse_definition_response,
    parse_hover_response, parse_publish_diagnostics, parse_pull_diagnostics,
    parse_reference_locations, preflight_formatting_edits, preflight_workspace_edit,
    sanitize_hover, validate_completion_response, validate_pull_diagnostics,
    validate_push_diagnostics, AppliedWorkspaceEdit, CancelRequestId, CompletionResult,
    DiagnosticOrigin, DiagnosticResult, DiagnosticStore, FeatureError, FeatureResponse,
    FilteredLocations, FormattingEditsInput, FormattingOptionsInput, HoverResult, HoverText,
    LocationTarget, RequestMetadata, RequestTarget, SanitizedHover, ValidatedDiagnostic,
    ValidatedTextEdit, WorkspaceEditPlan,
};
pub use installer::{
    InstallError, InstallLimits, InstallResult, InstalledServerMetadata, ManagedInstallResolution,
    ManagedInstallState, ManagedInstallStatus, ManagedInstaller, NodePackageArchive,
    DEFAULT_MAX_ARCHIVE_ENTRIES, DEFAULT_MAX_INSTALL_BYTES,
};
pub use logs::{LanguageServerLog, LspLogEntry, LspLogLevel};
pub use manager::{
    AppliedDocumentEdits, EditedDocument, LanguageServerStatus, LspDiagnosticsEvent, LspEvent,
    LspManager, LspManagerError, LspStatusEvent,
};
pub use node_lock::{
    reviewed_node_lock, NodeDependencyLock, NodeLockError, NodePackageLock, NodePackageRef,
    NODE_LOCK_SCHEMA_VERSION, REVIEWED_NODE_LOCK_SHA256,
};
pub use positions::{
    offset_to_position, position_to_offset, position_to_offset_clamped, LspPosition, LspRange,
    PositionEncoding, PositionError,
};
pub use process::{
    BoundedStderr, IncomingMessage, LspProcess, ProcessError, ProcessSpec, ProcessState,
    RequestError, StderrEvent,
};
pub use runtime::{
    canonical_workspace_root, EnvironmentAllowlist, ResolvedProcess, ResolvedRuntime, RuntimeError,
    RuntimeResolver, RuntimeVersion, VersionOperator, VersionRequirement,
};
pub use transport::{
    FrameLimits, JsonRpcMessage, JsonRpcReader, JsonRpcWriter, PendingError, PendingRequests,
    RequestCancellation, RequestId, RpcError, RpcId, TransportError, DEFAULT_MAX_HEADER_BYTES,
    DEFAULT_MAX_MESSAGE_BYTES,
};
