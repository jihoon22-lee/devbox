//! Pure data contracts for Code Pad's local language-server integration.
//!
//! This module intentionally contains no process, filesystem, network, or UI
//! code. The manifest and configuration values are validated before a later
//! transport/installer layer is allowed to use them.

pub mod catalog;

pub use catalog::{
    initial_catalog, Artifact, ArtifactKind, CapabilitiesHint, Catalog, CommandSpec, CustomServer,
    InstalledServer, InstalledServerIndex, LanguageSupport, LspConfig, ManifestFiles, RuntimeKind,
    RuntimeSpec, SchemaError, ServerCatalog, ServerManifest, ServerRef, UpdatePolicy,
    ValidationError, LSP_CONFIG_SCHEMA_VERSION, LSP_INSTALLED_SCHEMA_VERSION,
    WINDOWS_X86_64_PLATFORM,
};
