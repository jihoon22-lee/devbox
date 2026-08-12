//! Local language-server contracts, transport, and process lifecycle.
//!
//! The transport/process code deliberately stays generic over the catalog and
//! document schema.  The schema can therefore evolve without putting protocol
//! parsing or child lifecycle policy behind catalog-specific types.

pub mod catalog;
pub mod process;
pub mod transport;

pub use catalog::{
    initial_catalog, Artifact, ArtifactKind, CapabilitiesHint, Catalog, CommandSpec, CustomServer,
    InstalledServer, InstalledServerIndex, LanguageSupport, LspConfig, ManifestFiles, RuntimeKind,
    RuntimeSpec, SchemaError, ServerCatalog, ServerManifest, ServerRef, UpdatePolicy,
    ValidationError, LSP_CONFIG_SCHEMA_VERSION, LSP_INSTALLED_SCHEMA_VERSION,
    WINDOWS_X86_64_PLATFORM,
};
pub use process::{
    BoundedStderr, IncomingMessage, LspProcess, ProcessError, ProcessSpec, ProcessState,
    RequestError, StderrEvent,
};
pub use transport::{
    FrameLimits, JsonRpcMessage, JsonRpcReader, JsonRpcWriter, PendingError, PendingRequests,
    RequestCancellation, RequestId, RpcError, RpcId, TransportError, DEFAULT_MAX_HEADER_BYTES,
    DEFAULT_MAX_MESSAGE_BYTES,
};
