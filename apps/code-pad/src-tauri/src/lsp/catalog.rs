//! Versioned LSP manifests, server selections, and the reviewed catalog.
//!
//! The catalog is deliberately data-only. In particular, validating a
//! manifest never checks the local filesystem, downloads an artifact, starts
//! a runtime, or resolves a version range. Those operations belong to later
//! Phase 2 features.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use url::Url;

pub const LSP_CONFIG_SCHEMA_VERSION: u32 = 1;
pub const LSP_INSTALLED_SCHEMA_VERSION: u32 = 1;
pub const WINDOWS_X86_64_PLATFORM: &str = "windows-x86_64";

/// The only update policy supported by the design. An explicit enum prevents
/// an accidental future "latest"/automatic policy from being accepted by old
/// clients.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdatePolicy {
    #[default]
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Zip,
    #[serde(rename = "npm_tarball")]
    NpmTarball,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeKind {
    Native,
    Node,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanguageSupport {
    pub language_id: String,
    pub extensions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    pub kind: ArtifactKind,
    pub url: String,
    pub sha256: String,
    /// The design requires this for an installable manifest. The checked-in
    /// design snapshot does not publish byte sizes, so the initial catalog
    /// keeps this explicitly absent rather than inventing a value. The
    /// installer gate validate_for_install rejects an absent size.
    #[serde(default)]
    pub size_bytes: Option<u64>,
    /// Additional HTTPS hosts reviewed for redirects from the artifact URL.
    /// The artifact URL's own host is always allowed and need not be listed.
    #[serde(default)]
    pub allowed_redirect_hosts: Vec<String>,
    /// Archive-relative root. An empty root means entries are directly below
    /// the staging root (as with the rust-analyzer zip).
    pub archive_root: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeSpec {
    pub kind: RuntimeKind,
    pub executable: String,
    #[serde(default)]
    pub min_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandSpec {
    pub executable: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestFiles {
    pub entrypoint: String,
    #[serde(default)]
    pub package_lock_sha256: Option<String>,
}

/// UI-only hints. The initialize response from a real server remains the
/// authority; these fields must never be used to enable an LSP request.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitiesHint {
    #[serde(default)]
    pub diagnostics: bool,
    #[serde(default)]
    pub completion: bool,
    #[serde(default)]
    pub hover: bool,
    #[serde(default)]
    pub definition: bool,
    #[serde(default)]
    pub references: bool,
    #[serde(default)]
    pub rename: bool,
    #[serde(default)]
    pub formatting: bool,
}

/// A catalog entry describes one exact, managed server release. It is not a
/// process command and does not imply that the artifact has been installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerManifest {
    pub id: String,
    pub version: String,
    pub platform: String,
    pub languages: Vec<LanguageSupport>,
    pub source_url: String,
    pub license: String,
    pub artifact: Artifact,
    pub runtime: RuntimeSpec,
    pub command: CommandSpec,
    pub files: ManifestFiles,
    #[serde(default)]
    pub capabilities_hint: Option<CapabilitiesHint>,
    pub generated_at: String,
}

/// User-selected server metadata. A custom server is never a download source:
/// source/license/version are informational values supplied by the user and
/// are retained for trust/status UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomServer {
    pub language_ids: Vec<String>,
    pub executable: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub runtime: RuntimeSpec,
    pub source: String,
    pub license: String,
    pub version: String,
}

/// A language-to-server selection in LspConfig.
///
/// The tagged representation intentionally mirrors the persisted design:
/// { "kind": "managed", ... }, { "kind": "local", ... }, or
/// { "kind": "custom", ... }.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServerRef {
    Managed {
        manifest_id: String,
        version: String,
        #[serde(default)]
        installed_path: Option<String>,
    },
    Local {
        installed_path: String,
        #[serde(default)]
        executable: Option<String>,
        #[serde(default)]
        args: Vec<String>,
    },
    Custom {
        executable: String,
        #[serde(default)]
        args: Vec<String>,
    },
}

impl ServerRef {
    pub fn managed(manifest_id: impl Into<String>, version: impl Into<String>) -> Self {
        Self::Managed {
            manifest_id: manifest_id.into(),
            version: version.into(),
            installed_path: None,
        }
    }

    pub fn local(installed_path: impl Into<String>) -> Self {
        Self::Local {
            installed_path: installed_path.into(),
            executable: None,
            args: Vec::new(),
        }
    }

    pub fn custom(executable: impl Into<String>, args: Vec<String>) -> Self {
        Self::Custom {
            executable: executable.into(),
            args,
        }
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Managed {
                manifest_id,
                version,
                installed_path,
            } => {
                validate_identifier("server_ref.manifest_id", manifest_id)?;
                validate_exact_version("server_ref.version", version)?;
                if let Some(path) = installed_path {
                    validate_canonical_path("server_ref.installed_path", path)?;
                }
            }
            Self::Local {
                installed_path,
                executable,
                args,
            } => {
                validate_canonical_path("server_ref.installed_path", installed_path)?;
                if let Some(executable) = executable {
                    validate_argv_component("server_ref.executable", executable)?;
                }
                validate_args("server_ref.args", args)?;
            }
            Self::Custom { executable, args } => {
                validate_argv_component("server_ref.executable", executable)?;
                validate_args("server_ref.args", args)?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspConfig {
    pub version: u32,
    pub enabled: bool,
    /// Empty means that no workspace has been selected yet. A non-empty value
    /// must already be canonical and absolute; validation is lexical and
    /// therefore remains IO-free.
    pub workspace_root: String,
    pub server_by_language: BTreeMap<String, ServerRef>,
    #[serde(default)]
    pub custom_servers: Vec<CustomServer>,
    pub update_policy: UpdatePolicy,
}

impl Default for LspConfig {
    fn default() -> Self {
        Self::empty()
    }
}

impl LspConfig {
    pub fn empty() -> Self {
        Self {
            version: LSP_CONFIG_SCHEMA_VERSION,
            enabled: false,
            workspace_root: String::new(),
            server_by_language: BTreeMap::new(),
            custom_servers: Vec::new(),
            update_policy: UpdatePolicy::Manual,
        }
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(input: &str) -> Result<Self, SchemaError> {
        let config: Self = serde_json::from_str(input).map_err(SchemaError::Json)?;
        config.validate().map_err(SchemaError::Validation)?;
        Ok(config)
    }

    /// Corrupt, unsupported, or invalid persisted settings are disposable
    /// state. Callers can log the parse error separately if desired.
    pub fn load(input: &str) -> Self {
        Self::from_json(input).unwrap_or_else(|_| Self::empty())
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.version != LSP_CONFIG_SCHEMA_VERSION {
            return Err(ValidationError::new(
                "version",
                format!(
                    "unsupported schema version {}; expected {LSP_CONFIG_SCHEMA_VERSION}",
                    self.version
                ),
            ));
        }
        if self.workspace_root.is_empty() {
            if self.enabled
                || !self.server_by_language.is_empty()
                || !self.custom_servers.is_empty()
            {
                return Err(ValidationError::new(
                    "workspace_root",
                    "a workspace is required when LSP is configured",
                ));
            }
        } else {
            validate_canonical_path("workspace_root", &self.workspace_root)?;
        }

        for (language_id, server) in &self.server_by_language {
            validate_language_id("server_by_language key", language_id)?;
            server.validate()?;
        }

        let mut custom_language_ids = BTreeSet::new();
        for (index, custom) in self.custom_servers.iter().enumerate() {
            custom.validate_at(index)?;
            for language_id in &custom.language_ids {
                if self.server_by_language.contains_key(language_id) {
                    return Err(ValidationError::new(
                        format!("custom_servers[{index}].language_ids"),
                        format!("language id {language_id:?} is already configured"),
                    ));
                }
                if !custom_language_ids.insert(language_id) {
                    return Err(ValidationError::new(
                        format!("custom_servers[{index}].language_ids"),
                        format!("duplicate language id {language_id:?}"),
                    ));
                }
            }
        }
        Ok(())
    }
}

impl CustomServer {
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.validate_at(0)
    }

    fn validate_at(&self, index: usize) -> Result<(), ValidationError> {
        let prefix = format!("custom_servers[{index}]");
        if self.language_ids.is_empty() {
            return Err(ValidationError::new(
                format!("{prefix}.language_ids"),
                "at least one language id is required",
            ));
        }
        let mut languages = BTreeSet::new();
        for language_id in &self.language_ids {
            validate_language_id(&format!("{prefix}.language_ids"), language_id)?;
            if !languages.insert(language_id) {
                return Err(ValidationError::new(
                    format!("{prefix}.language_ids"),
                    format!("duplicate language id {language_id:?}"),
                ));
            }
        }
        validate_argv_component(&format!("{prefix}.executable"), &self.executable)?;
        validate_args(&format!("{prefix}.args"), &self.args)?;
        self.runtime.validate_at(&format!("{prefix}.runtime"))?;
        validate_user_metadata(&format!("{prefix}.source"), &self.source)?;
        validate_user_license(&format!("{prefix}.license"), &self.license)?;
        validate_user_version(&format!("{prefix}.version"), &self.version)?;
        Ok(())
    }
}

impl RuntimeSpec {
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.validate_at("runtime")
    }

    fn validate_at(&self, field: &str) -> Result<(), ValidationError> {
        validate_argv_component(&format!("{field}.executable"), &self.executable)?;
        if let Some(min_version) = &self.min_version {
            validate_runtime_version(&format!("{field}.min_version"), min_version)?;
        }
        Ok(())
    }
}

impl CommandSpec {
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_argv_component("command.executable", &self.executable)?;
        validate_args("command.args", &self.args)
    }
}

impl ManifestFiles {
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_relative_path("files.entrypoint", &self.entrypoint, false)?;
        if let Some(digest) = &self.package_lock_sha256 {
            validate_sha256("files.package_lock_sha256", digest)?;
        }
        Ok(())
    }
}

impl Artifact {
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_https_url("artifact.url", &self.url)?;
        validate_sha256("artifact.sha256", &self.sha256)?;
        if self.size_bytes == Some(0) {
            return Err(ValidationError::new(
                "artifact.size_bytes",
                "must be positive when supplied",
            ));
        }
        let mut redirect_hosts = BTreeSet::new();
        for (index, host) in self.allowed_redirect_hosts.iter().enumerate() {
            validate_redirect_host(&format!("artifact.allowed_redirect_hosts[{index}]"), host)?;
            if !redirect_hosts.insert(host) {
                return Err(ValidationError::new(
                    format!("artifact.allowed_redirect_hosts[{index}]"),
                    "duplicate redirect host",
                ));
            }
        }
        validate_relative_path("artifact.archive_root", &self.archive_root, true)
    }
}

impl ServerManifest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_identifier("id", &self.id)?;
        validate_exact_version("version", &self.version)?;
        validate_platform(&self.platform)?;
        if self.languages.is_empty() {
            return Err(ValidationError::new(
                "languages",
                "at least one language mapping is required",
            ));
        }
        let mut language_ids = BTreeSet::new();
        let mut extensions = BTreeSet::new();
        for (index, language) in self.languages.iter().enumerate() {
            let field = format!("languages[{index}]");
            validate_language_id(&format!("{field}.language_id"), &language.language_id)?;
            if !language_ids.insert(&language.language_id) {
                return Err(ValidationError::new(
                    format!("{field}.language_id"),
                    "duplicate language id",
                ));
            }
            if language.extensions.is_empty() {
                return Err(ValidationError::new(
                    format!("{field}.extensions"),
                    "at least one extension is required",
                ));
            }
            for extension in &language.extensions {
                validate_extension(&format!("{field}.extensions"), extension)?;
                if !extensions.insert(extension.to_ascii_lowercase()) {
                    return Err(ValidationError::new(
                        format!("{field}.extensions"),
                        format!("duplicate extension {extension:?}"),
                    ));
                }
            }
        }
        validate_https_url("source_url", &self.source_url)?;
        validate_spdx("license", &self.license)?;
        self.artifact.validate()?;
        self.runtime.validate_at("runtime")?;
        self.command.validate()?;
        self.files.validate()?;
        validate_rfc3339("generated_at", &self.generated_at)?;
        Ok(())
    }

    /// A second gate used by a future installer. The structural schema can
    /// represent the design snapshot's missing artifact sizes/lock digests,
    /// while installation must refuse to proceed until those reviewed facts
    /// are present.
    pub fn validate_for_install(&self) -> Result<(), ValidationError> {
        self.validate()?;
        if self.artifact.size_bytes.is_none() {
            return Err(ValidationError::new(
                "artifact.size_bytes",
                "an exact artifact size is required for installation",
            ));
        }
        if self.runtime.kind == RuntimeKind::Node && self.files.package_lock_sha256.is_none() {
            return Err(ValidationError::new(
                "files.package_lock_sha256",
                "a reviewed dependency lock digest is required for Node servers",
            ));
        }
        Ok(())
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(input: &str) -> Result<Self, SchemaError> {
        let manifest: Self = serde_json::from_str(input).map_err(SchemaError::Json)?;
        manifest.validate().map_err(SchemaError::Validation)?;
        Ok(manifest)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledServer {
    pub manifest_id: String,
    pub version: String,
    pub platform: String,
    pub sha256: String,
    pub source_url: String,
    pub license: String,
    pub artifact_url: String,
    pub installed_path: String,
    pub entrypoint: String,
    pub runtime: RuntimeSpec,
    pub installed_at: String,
    #[serde(default)]
    pub package_lock_sha256: Option<String>,
}

impl InstalledServer {
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_identifier("manifest_id", &self.manifest_id)?;
        validate_exact_version("version", &self.version)?;
        validate_platform(&self.platform)?;
        validate_sha256("sha256", &self.sha256)?;
        validate_https_url("source_url", &self.source_url)?;
        validate_spdx("license", &self.license)?;
        validate_https_url("artifact_url", &self.artifact_url)?;
        validate_canonical_path("installed_path", &self.installed_path)?;
        validate_relative_path("entrypoint", &self.entrypoint, false)?;
        self.runtime.validate_at("runtime")?;
        validate_rfc3339("installed_at", &self.installed_at)?;
        if let Some(digest) = &self.package_lock_sha256 {
            validate_sha256("package_lock_sha256", digest)?;
        } else if self.runtime.kind == RuntimeKind::Node {
            return Err(ValidationError::new(
                "package_lock_sha256",
                "a reviewed dependency lock digest is required for Node servers",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledServerIndex {
    pub version: u32,
    pub servers: Vec<InstalledServer>,
}

impl Default for InstalledServerIndex {
    fn default() -> Self {
        Self {
            version: LSP_INSTALLED_SCHEMA_VERSION,
            servers: Vec::new(),
        }
    }
}

impl InstalledServerIndex {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(input: &str) -> Result<Self, SchemaError> {
        let index: Self = serde_json::from_str(input).map_err(SchemaError::Json)?;
        index.validate().map_err(SchemaError::Validation)?;
        Ok(index)
    }

    pub fn load(input: &str) -> Self {
        Self::from_json(input).unwrap_or_default()
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.version != LSP_INSTALLED_SCHEMA_VERSION {
            return Err(ValidationError::new(
                "version",
                format!(
                    "unsupported schema version {}; expected {LSP_INSTALLED_SCHEMA_VERSION}",
                    self.version
                ),
            ));
        }
        let mut keys = BTreeSet::new();
        for (index, server) in self.servers.iter().enumerate() {
            server
                .validate()
                .map_err(|error| error.with_prefix(&format!("servers[{index}]")))?;
            let key = format!(
                "{}\u{001f}{}\u{001f}{}",
                server.manifest_id, server.version, server.platform
            );
            if !keys.insert(key) {
                return Err(ValidationError::new(
                    format!("servers[{index}]"),
                    "duplicate manifest/version/platform entry",
                ));
            }
        }
        Ok(())
    }
}

/// A pure catalog container. The production catalog is the fixed snapshot
/// returned by initial_catalog; callers can validate an updated fixture
/// before exposing it to UI or an installer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerCatalog {
    pub manifests: Vec<ServerManifest>,
}

pub type Catalog = ServerCatalog;

impl ServerCatalog {
    pub fn initial() -> Self {
        Self {
            manifests: initial_catalog(),
        }
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(input: &str) -> Result<Self, SchemaError> {
        let catalog: Self = serde_json::from_str(input).map_err(SchemaError::Json)?;
        catalog.validate().map_err(SchemaError::Validation)?;
        Ok(catalog)
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        let mut keys = BTreeSet::new();
        for (index, manifest) in self.manifests.iter().enumerate() {
            manifest
                .validate()
                .map_err(|error| error.with_prefix(&format!("manifests[{index}]")))?;
            let key = format!(
                "{}\u{001f}{}\u{001f}{}",
                manifest.id, manifest.version, manifest.platform
            );
            if !keys.insert(key) {
                return Err(ValidationError::new(
                    format!("manifests[{index}]"),
                    "duplicate id/version/platform entry",
                ));
            }
        }
        Ok(())
    }

    pub fn find(&self, id: &str, version: &str, platform: &str) -> Option<&ServerManifest> {
        self.manifests.iter().find(|manifest| {
            manifest.id == id && manifest.version == version && manifest.platform == platform
        })
    }
}

/// The 2026-08-12 design snapshot. Artifact sizes and reviewed npm lock
/// digests are intentionally absent because the specification did not pin
/// those facts; validate_for_install keeps them from being treated as ready.
pub fn initial_catalog() -> Vec<ServerManifest> {
    vec![
        ServerManifest {
            id: "rust-analyzer".into(),
            version: "2026-08-10.1".into(),
            platform: WINDOWS_X86_64_PLATFORM.into(),
            languages: vec![LanguageSupport {
                language_id: "rust".into(),
                extensions: vec![".rs".into()],
            }],
            source_url: "https://github.com/rust-lang/rust-analyzer".into(),
            license: "MIT OR Apache-2.0".into(),
            artifact: Artifact {
                kind: ArtifactKind::Zip,
                url: "https://github.com/rust-lang/rust-analyzer/releases/download/2026-08-10.1/rust-analyzer-x86_64-pc-windows-msvc.zip".into(),
                sha256: "f667620d3af202f480faf9e407374509ebddef3b8611922e463aeaa7e6985fc8".into(),
                size_bytes: None,
                allowed_redirect_hosts: vec!["release-assets.githubusercontent.com".into()],
                archive_root: String::new(),
            },
            runtime: RuntimeSpec {
                kind: RuntimeKind::Native,
                executable: "rust-analyzer.exe".into(),
                min_version: None,
            },
            command: CommandSpec {
                executable: "rust-analyzer.exe".into(),
                args: Vec::new(),
            },
            files: ManifestFiles {
                entrypoint: "rust-analyzer.exe".into(),
                package_lock_sha256: None,
            },
            capabilities_hint: None,
            generated_at: "2026-08-12T00:00:00Z".into(),
        },
        ServerManifest {
            id: "typescript-language-server".into(),
            version: "5.3.0".into(),
            platform: WINDOWS_X86_64_PLATFORM.into(),
            languages: vec![
                LanguageSupport {
                    language_id: "typescript".into(),
                    extensions: vec![".ts".into(), ".tsx".into()],
                },
                LanguageSupport {
                    language_id: "javascript".into(),
                    extensions: vec![".js".into(), ".jsx".into()],
                },
            ],
            source_url: "https://github.com/typescript-language-server/typescript-language-server".into(),
            license: "Apache-2.0".into(),
            artifact: Artifact {
                kind: ArtifactKind::NpmTarball,
                url: "https://registry.npmjs.org/typescript-language-server/-/typescript-language-server-5.3.0.tgz".into(),
                sha256: "398cacc17fff2108652e7b4050e3182008d17063246b3fea7dcf5fae2ce1560e".into(),
                size_bytes: None,
                allowed_redirect_hosts: Vec::new(),
                archive_root: "package".into(),
            },
            runtime: RuntimeSpec {
                kind: RuntimeKind::Node,
                executable: "node".into(),
                min_version: Some(">=20".into()),
            },
            command: CommandSpec {
                executable: "typescript-language-server".into(),
                args: vec!["--stdio".into()],
            },
            files: ManifestFiles {
                entrypoint: "lib/cli.mjs".into(),
                package_lock_sha256: None,
            },
            capabilities_hint: None,
            generated_at: "2026-08-12T00:00:00Z".into(),
        },
        ServerManifest {
            id: "basedpyright".into(),
            version: "1.39.9".into(),
            platform: WINDOWS_X86_64_PLATFORM.into(),
            languages: vec![LanguageSupport {
                language_id: "python".into(),
                extensions: vec![".py".into(), ".pyi".into()],
            }],
            source_url: "https://github.com/DetachHead/basedpyright".into(),
            license: "MIT".into(),
            artifact: Artifact {
                kind: ArtifactKind::NpmTarball,
                url: "https://registry.npmjs.org/basedpyright/-/basedpyright-1.39.9.tgz".into(),
                sha256: "5e92f462d04d91fe1370d65cbb1ac241c0c62b3f2c893c4e0b1bf9a82c9e99b2".into(),
                size_bytes: None,
                allowed_redirect_hosts: Vec::new(),
                archive_root: "package".into(),
            },
            runtime: RuntimeSpec {
                kind: RuntimeKind::Node,
                executable: "node".into(),
                min_version: Some(">=14".into()),
            },
            command: CommandSpec {
                executable: "basedpyright-langserver".into(),
                args: vec!["--stdio".into()],
            },
            files: ManifestFiles {
                entrypoint: "index.js".into(),
                package_lock_sha256: None,
            },
            capabilities_hint: None,
            generated_at: "2026-08-12T00:00:00Z".into(),
        },
        ServerManifest {
            id: "vscode-langservers-extracted".into(),
            version: "4.10.0".into(),
            platform: WINDOWS_X86_64_PLATFORM.into(),
            languages: vec![
                LanguageSupport {
                    language_id: "json".into(),
                    extensions: vec![".json".into(), ".jsonc".into()],
                },
                LanguageSupport {
                    language_id: "html".into(),
                    extensions: vec![".html".into(), ".htm".into()],
                },
                LanguageSupport {
                    language_id: "css".into(),
                    extensions: vec![".css".into(), ".scss".into(), ".less".into()],
                },
            ],
            source_url: "https://github.com/hrsh7th/vscode-langservers-extracted".into(),
            license: "MIT".into(),
            artifact: Artifact {
                kind: ArtifactKind::NpmTarball,
                url: "https://registry.npmjs.org/vscode-langservers-extracted/-/vscode-langservers-extracted-4.10.0.tgz".into(),
                sha256: "d6e2d090d09c4b91daa74e9e7462a3d3f244efb96aa5111004cfffa49d6dc9ef".into(),
                size_bytes: None,
                allowed_redirect_hosts: Vec::new(),
                archive_root: "package".into(),
            },
            runtime: RuntimeSpec {
                kind: RuntimeKind::Node,
                executable: "node".into(),
                min_version: None,
            },
            command: CommandSpec {
                executable: "vscode-json-language-server".into(),
                args: vec!["--stdio".into()],
            },
            files: ManifestFiles {
                entrypoint: "bin/vscode-json-language-server".into(),
                package_lock_sha256: None,
            },
            capabilities_hint: None,
            generated_at: "2026-08-12T00:00:00Z".into(),
        },
    ]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub field: String,
    pub reason: String,
}

impl ValidationError {
    fn new(field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            reason: reason.into(),
        }
    }

    fn with_prefix(self, prefix: &str) -> Self {
        Self {
            field: format!("{prefix}.{}", self.field),
            reason: self.reason,
        }
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid {}: {}", self.field, self.reason)
    }
}

impl std::error::Error for ValidationError {}

#[derive(Debug)]
pub enum SchemaError {
    Json(serde_json::Error),
    Validation(ValidationError),
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(f, "invalid LSP schema JSON: {error}"),
            Self::Validation(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for SchemaError {}

fn validate_identifier(field: &str, value: &str) -> Result<(), ValidationError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+'))
        || value.starts_with('.')
        || value.ends_with('.')
    {
        return Err(ValidationError::new(
            field,
            "must be a short identifier without path separators or whitespace",
        ));
    }
    Ok(())
}

fn validate_language_id(field: &str, value: &str) -> Result<(), ValidationError> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+'))
    {
        return Err(ValidationError::new(
            field,
            "must be a non-empty language identifier",
        ));
    }
    Ok(())
}

fn validate_extension(field: &str, value: &str) -> Result<(), ValidationError> {
    if value.len() < 2
        || !value.starts_with('.')
        || value.contains('/')
        || value.contains('\\')
        || value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(ValidationError::new(
            field,
            "must be a dot-prefixed extension without path separators",
        ));
    }
    Ok(())
}

fn validate_platform(platform: &str) -> Result<(), ValidationError> {
    if platform != WINDOWS_X86_64_PLATFORM
        || !platform.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
    {
        return Err(ValidationError::new(
            "platform",
            format!("unsupported platform {platform:?}; expected {WINDOWS_X86_64_PLATFORM}"),
        ));
    }
    Ok(())
}

fn validate_exact_version(field: &str, value: &str) -> Result<(), ValidationError> {
    let lower = value.to_ascii_lowercase();
    if value.is_empty()
        || value != value.trim()
        || lower == "latest"
        || lower == "next"
        || value.contains("..")
        || value.chars().any(|character| {
            !character.is_ascii_alphanumeric() && !matches!(character, '.' | '-' | '_' | '+')
        })
        || !value.chars().any(|character| character.is_ascii_digit())
    {
        return Err(ValidationError::new(
            field,
            "must be one exact version (ranges, wildcards, and latest are not allowed)",
        ));
    }
    Ok(())
}

fn validate_user_version(field: &str, value: &str) -> Result<(), ValidationError> {
    if is_unknown_metadata(value) {
        return Ok(());
    }
    validate_exact_version(field, value)
}

fn validate_redirect_host(field: &str, value: &str) -> Result<(), ValidationError> {
    if value.is_empty()
        || value != value.to_ascii_lowercase()
        || value.starts_with('.')
        || value.ends_with('.')
        || value.split('.').any(|label| {
            label.is_empty()
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
    {
        return Err(ValidationError::new(
            field,
            "must be a lowercase DNS host without a port",
        ));
    }
    Ok(())
}

fn validate_https_url(field: &str, value: &str) -> Result<(), ValidationError> {
    let parsed =
        Url::parse(value).map_err(|_| ValidationError::new(field, "must be a valid URL"))?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(ValidationError::new(
            field,
            "must be an HTTPS URL with a host and no embedded credentials",
        ));
    }
    Ok(())
}

fn validate_sha256(field: &str, value: &str) -> Result<(), ValidationError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ValidationError::new(
            field,
            "must be a lowercase 64-character SHA-256 digest",
        ));
    }
    Ok(())
}

fn validate_relative_path(
    field: &str,
    value: &str,
    allow_empty: bool,
) -> Result<(), ValidationError> {
    if value.is_empty() {
        if allow_empty {
            return Ok(());
        }
        return Err(ValidationError::new(
            field,
            "must be a non-empty relative path",
        ));
    }
    if value.starts_with('/')
        || value.starts_with('\\')
        || (value.len() >= 2 && value.as_bytes()[1] == b':')
        || value.chars().any(|character| character.is_control())
    {
        return Err(ValidationError::new(
            field,
            "must be a relative archive path",
        ));
    }
    let mut components = 0;
    for component in value.split('/') {
        if component.is_empty() || component == "." || component == ".." || component.contains('\\')
        {
            return Err(ValidationError::new(
                field,
                "must not contain empty, dot, parent, or backslash components",
            ));
        }
        components += 1;
    }
    if components == 0 {
        return Err(ValidationError::new(field, "must contain a path component"));
    }
    Ok(())
}

fn validate_argv_component(field: &str, value: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty() || contains_control_or_shell(value) || is_shell_executable(value) {
        return Err(ValidationError::new(
            field,
            "must be a non-empty argv value without shell syntax",
        ));
    }
    Ok(())
}

fn validate_args(field: &str, args: &[String]) -> Result<(), ValidationError> {
    for (index, arg) in args.iter().enumerate() {
        validate_argv_component(&format!("{field}[{index}]"), arg)?;
        let lower = arg.to_ascii_lowercase();
        if lower.contains("tcp://") || lower.contains("ws://") || lower.contains("wss://") {
            return Err(ValidationError::new(
                format!("{field}[{index}]"),
                "remote TCP/WebSocket endpoints are not allowed",
            ));
        }
    }
    Ok(())
}

fn contains_control_or_shell(value: &str) -> bool {
    value.chars().any(|character| {
        character.is_control() || matches!(character, '|' | ';' | '&' | '<' | '>' | '\u{60}')
    }) || value.contains("$(")
        || (value.contains('$') && value.contains('{'))
}

fn is_shell_executable(value: &str) -> bool {
    let normalized = value.replace('\\', "/").to_ascii_lowercase();
    matches!(
        normalized.rsplit('/').next(),
        Some("cmd")
            | Some("cmd.exe")
            | Some("powershell")
            | Some("powershell.exe")
            | Some("pwsh")
            | Some("pwsh.exe")
    )
}

fn validate_user_metadata(field: &str, value: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty() || value.chars().any(|character| character.is_control()) {
        return Err(ValidationError::new(field, "must be non-empty metadata"));
    }
    Ok(())
}

fn validate_user_license(field: &str, value: &str) -> Result<(), ValidationError> {
    if is_unknown_metadata(value) {
        return Ok(());
    }
    validate_spdx(field, value)
}

fn is_unknown_metadata(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "unknown" | "unverified" | "user-provided" | "user-provided/unchecked"
    )
}

fn validate_spdx(field: &str, value: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty() || value.chars().any(|character| character.is_control()) {
        return Err(ValidationError::new(
            field,
            "must be an SPDX license expression",
        ));
    }
    let mut saw_identifier = false;
    let mut expect_identifier = true;
    for raw in value.split_whitespace() {
        let token = raw.trim_matches(['(', ')']);
        if token.is_empty() {
            continue;
        }
        if matches!(token, "AND" | "OR" | "WITH") {
            if expect_identifier {
                return Err(ValidationError::new(field, "malformed SPDX expression"));
            }
            expect_identifier = true;
            continue;
        }
        if !token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+' | b'_'))
        {
            return Err(ValidationError::new(field, "malformed SPDX expression"));
        }
        if !expect_identifier {
            return Err(ValidationError::new(field, "malformed SPDX expression"));
        }
        saw_identifier = true;
        expect_identifier = false;
    }
    if !saw_identifier || expect_identifier {
        return Err(ValidationError::new(field, "malformed SPDX expression"));
    }
    Ok(())
}

fn validate_runtime_version(field: &str, value: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty()
        || value != value.trim()
        || !value.chars().any(|character| character.is_ascii_digit())
        || value.chars().any(|character| {
            !character.is_ascii_alphanumeric()
                && !matches!(
                    character,
                    '.' | '-' | '_' | '+' | '<' | '>' | '=' | '^' | '~' | '*'
                )
        })
    {
        return Err(ValidationError::new(
            field,
            "must be a non-empty runtime version constraint",
        ));
    }
    Ok(())
}

fn validate_canonical_path(field: &str, value: &str) -> Result<(), ValidationError> {
    if value.is_empty() || value.chars().any(|character| character.is_control()) {
        return Err(ValidationError::new(
            field,
            "must be an absolute canonical path",
        ));
    }
    let normalized = value.replace('\\', "/");
    let rest = if let Some(stripped) = normalized.strip_prefix("//") {
        let parts: Vec<&str> = stripped.split('/').collect();
        if parts.len() < 2 || parts[0].is_empty() || parts[1].is_empty() {
            return Err(ValidationError::new(
                field,
                "UNC path must include server and share",
            ));
        }
        stripped
    } else if let Some(stripped) = normalized.strip_prefix('/') {
        stripped
    } else if normalized.len() >= 3
        && normalized.as_bytes()[0].is_ascii_alphabetic()
        && normalized.as_bytes()[1] == b':'
        && normalized.as_bytes()[2] == b'/'
    {
        &normalized[3..]
    } else {
        return Err(ValidationError::new(
            field,
            "must be an absolute Unix, drive, or UNC path",
        ));
    };

    for component in rest.split('/') {
        if component == "." || component == ".." {
            return Err(ValidationError::new(
                field,
                "canonical paths must not contain dot components",
            ));
        }
        if component.is_empty() && !rest.is_empty() {
            return Err(ValidationError::new(
                field,
                "canonical paths must not contain repeated separators",
            ));
        }
    }
    Ok(())
}

fn validate_rfc3339(field: &str, value: &str) -> Result<(), ValidationError> {
    let bytes = value.as_bytes();
    if bytes.len() < 20
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
    {
        return Err(ValidationError::new(field, "must be RFC3339"));
    }
    let year = parse_digits(bytes, 0, 4);
    let month = parse_digits(bytes, 5, 2);
    let day = parse_digits(bytes, 8, 2);
    let hour = parse_digits(bytes, 11, 2);
    let minute = parse_digits(bytes, 14, 2);
    let second = parse_digits(bytes, 17, 2);
    let (Some(year), Some(month), Some(day), Some(hour), Some(minute), Some(second)) =
        (year, month, day, hour, minute, second)
    else {
        return Err(ValidationError::new(
            field,
            "must contain numeric date/time fields",
        ));
    };
    if !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return Err(ValidationError::new(
            field,
            "contains an out-of-range date/time",
        ));
    }
    let mut index = 19;
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == start {
            return Err(ValidationError::new(
                field,
                "fractional seconds require digits",
            ));
        }
    }
    match bytes.get(index) {
        Some(b'Z') => {
            index += 1;
        }
        Some(b'+') | Some(b'-') => {
            if bytes.len() < index + 6
                || bytes.get(index + 3) != Some(&b':')
                || parse_digits(bytes, index + 1, 2).is_none()
                || parse_digits(bytes, index + 4, 2).is_none()
            {
                return Err(ValidationError::new(field, "invalid timezone offset"));
            }
            let offset_hour = parse_digits(bytes, index + 1, 2).unwrap_or_default();
            let offset_minute = parse_digits(bytes, index + 4, 2).unwrap_or_default();
            if offset_hour > 23 || offset_minute > 59 {
                return Err(ValidationError::new(field, "invalid timezone offset"));
            }
            index += 6;
        }
        _ => return Err(ValidationError::new(field, "timezone must be Z or ±HH:MM")),
    }
    if index != bytes.len() {
        return Err(ValidationError::new(
            field,
            "trailing data is not valid RFC3339",
        ));
    }
    Ok(())
}

fn parse_digits(bytes: &[u8], start: usize, count: usize) -> Option<u32> {
    if start.checked_add(count)? > bytes.len() {
        return None;
    }
    let mut value = 0u32;
    for byte in &bytes[start..start + count] {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value * 10 + u32::from(byte - b'0');
    }
    Some(value)
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_manifest() -> ServerManifest {
        initial_catalog().remove(0)
    }

    fn valid_config() -> LspConfig {
        let mut config = LspConfig::empty();
        config.enabled = true;
        config.workspace_root = "/work/project".into();
        config.server_by_language.insert(
            "rust".into(),
            ServerRef::managed("rust-analyzer", "2026-08-10.1"),
        );
        config
    }

    #[test]
    fn initial_catalog_has_pinned_entries_and_validates_structurally() {
        let catalog = ServerCatalog::initial();
        assert_eq!(catalog.manifests.len(), 4);
        catalog.validate().unwrap();
        assert_eq!(
            ServerCatalog::from_json(&catalog.to_json().unwrap()).unwrap(),
            catalog
        );
        assert_eq!(catalog.manifests[0].artifact.sha256.len(), 64);
        assert!(catalog
            .find("rust-analyzer", "2026-08-10.1", WINDOWS_X86_64_PLATFORM)
            .is_some());
        assert!(catalog
            .manifests
            .iter()
            .all(|manifest| manifest.validate_for_install().is_err()));
    }

    #[test]
    fn manifest_json_roundtrips_without_normalizing_pinned_values() {
        let manifest = valid_manifest();
        let json = manifest.to_json().unwrap();
        let decoded = ServerManifest::from_json(&json).unwrap();
        assert_eq!(decoded, manifest);
        assert!(json.contains("\"source_url\""));
        assert!(json.contains("\"sha256\""));
    }

    #[test]
    fn redirect_hosts_are_explicit_unique_lowercase_dns_names() {
        let mut manifest = valid_manifest();
        assert_eq!(
            manifest.artifact.allowed_redirect_hosts,
            vec!["release-assets.githubusercontent.com"]
        );
        for invalid in [
            "HTTPS://example.com",
            "Example.com",
            "example.com:443",
            ".example.com",
            "example..com",
            "-example.com",
        ] {
            manifest.artifact.allowed_redirect_hosts = vec![invalid.to_string()];
            assert!(manifest.validate().is_err(), "accepted {invalid:?}");
        }
        manifest.artifact.allowed_redirect_hosts = vec!["cdn.example.com".to_string(); 2];
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn install_gate_requires_size_and_node_lock_but_schema_does_not_invent_them() {
        let mut manifest = valid_manifest();
        assert!(manifest.validate().is_ok());
        assert!(matches!(
            manifest.validate_for_install(),
            Err(ValidationError { field, .. }) if field == "artifact.size_bytes"
        ));
        manifest.artifact.size_bytes = Some(1);
        assert!(manifest.validate_for_install().is_ok());
        manifest.runtime.kind = RuntimeKind::Node;
        assert!(matches!(
            manifest.validate_for_install(),
            Err(ValidationError { field, .. }) if field == "files.package_lock_sha256"
        ));
    }

    #[test]
    fn exact_version_rejects_ranges_latest_and_invalid_characters() {
        for version in ["latest", "^1.2.3", ">=1", "1.*", "", " 1.2.3", "1..2"] {
            assert!(
                validate_exact_version("version", version).is_err(),
                "{version}"
            );
        }
        for version in ["1.2.3", "2026-08-10.1", "1.2.3-beta.1"] {
            validate_exact_version("version", version).unwrap();
        }
    }

    #[test]
    fn manifest_validation_covers_https_license_hash_paths_platform_and_time() {
        let mut manifest = valid_manifest();
        manifest.source_url = "http://example.com/source".into();
        assert!(manifest.validate().is_err());
        manifest = valid_manifest();
        manifest.license = "not a license".into();
        assert!(manifest.validate().is_err());
        manifest = valid_manifest();
        manifest.artifact.sha256 = "A".repeat(64);
        assert!(manifest.validate().is_err());
        manifest = valid_manifest();
        manifest.files.entrypoint = "../escape.exe".into();
        assert!(manifest.validate().is_err());
        manifest = valid_manifest();
        manifest.platform = "linux-x86_64".into();
        assert!(manifest.validate().is_err());
        manifest = valid_manifest();
        manifest.generated_at = "2026-02-30T00:00:00Z".into();
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn config_roundtrips_and_unknown_schema_starts_empty() {
        let config = valid_config();
        let json = config.to_json().unwrap();
        assert_eq!(LspConfig::from_json(&json).unwrap(), config);
        assert_eq!(LspConfig::load("not-json"), LspConfig::empty());
        let mut unknown = config;
        unknown.version += 1;
        assert_eq!(
            LspConfig::load(&unknown.to_json().unwrap()),
            LspConfig::empty()
        );
    }

    #[test]
    fn config_requires_workspace_and_valid_server_selection() {
        let mut config = LspConfig::empty();
        config.enabled = true;
        assert!(config.validate().is_err());
        config.enabled = false;
        config.workspace_root = "relative/project".into();
        assert!(config.validate().is_err());
        config.workspace_root = r"C:\work\project".into();
        config
            .server_by_language
            .insert("rust".into(), ServerRef::managed("rust-analyzer", "latest"));
        assert!(config.validate().is_err());
        config.server_by_language.insert(
            "rust".into(),
            ServerRef::custom("powershell.exe", vec!["-Command".into()]),
        );
        assert!(config.validate().is_err());
        config.server_by_language.insert(
            "rust".into(),
            ServerRef::custom("my-server.exe", vec!["--stdio".into()]),
        );
        config.validate().unwrap();
    }

    #[test]
    fn server_ref_variants_roundtrip_with_explicit_tags() {
        let values = [
            ServerRef::Managed {
                manifest_id: "rust-analyzer".into(),
                version: "2026-08-10.1".into(),
                installed_path: Some(r"C:\CodePad\rust-analyzer".into()),
            },
            ServerRef::Local {
                installed_path: r"C:\Tools\language-server.exe".into(),
                executable: Some("language-server.exe".into()),
                args: vec!["--stdio".into()],
            },
            ServerRef::Custom {
                executable: "custom-language-server.exe".into(),
                args: vec!["--stdio".into()],
            },
        ];
        for value in values {
            let json = serde_json::to_string(&value).unwrap();
            assert!(json.contains("\"kind\""));
            assert_eq!(serde_json::from_str::<ServerRef>(&json).unwrap(), value);
            value.validate().unwrap();
        }
    }

    #[test]
    fn canonical_paths_cover_unix_drive_and_unc_and_reject_traversal() {
        for path in [
            "/work/project",
            r"C:\work\project",
            r"\\server\share\project",
        ] {
            validate_canonical_path("path", path).unwrap();
        }
        for path in [
            "relative",
            "/work/../outside",
            r"C:\work\.\project",
            r"\\server",
        ] {
            assert!(validate_canonical_path("path", path).is_err(), "{path}");
        }
    }

    #[test]
    fn custom_servers_require_unique_languages_and_no_remote_or_shell_argv() {
        let runtime = RuntimeSpec {
            kind: RuntimeKind::Native,
            executable: "custom-server.exe".into(),
            min_version: None,
        };
        let custom = CustomServer {
            language_ids: vec!["rust".into()],
            executable: "custom-server.exe".into(),
            args: vec!["--stdio".into()],
            runtime,
            source: "user-provided/unchecked".into(),
            license: "unknown".into(),
            version: "local-1".into(),
        };
        custom.validate().unwrap();
        let mut unknown_version = custom.clone();
        unknown_version.version = "unknown".into();
        unknown_version.validate().unwrap();
        let mut config = valid_config();
        config.custom_servers = vec![custom.clone(), custom];
        assert!(config.validate().is_err());
        config.custom_servers[1].language_ids = vec!["python".into()];
        config.custom_servers[1].args = vec!["tcp://127.0.0.1:9000".into()];
        assert!(config.validate().is_err());
    }

    #[test]
    fn installed_index_roundtrips_and_rejects_duplicate_or_unknown_version() {
        let manifest = valid_manifest();
        let installed = InstalledServer {
            manifest_id: manifest.id,
            version: manifest.version,
            platform: manifest.platform,
            sha256: manifest.artifact.sha256,
            source_url: manifest.source_url,
            license: manifest.license,
            artifact_url: manifest.artifact.url,
            installed_path: r"C:\Users\me\AppData\Local\CodePad\lsp\rust".into(),
            entrypoint: "rust-analyzer.exe".into(),
            runtime: manifest.runtime,
            installed_at: "2026-08-12T00:00:00Z".into(),
            package_lock_sha256: None,
        };
        let index = InstalledServerIndex {
            version: LSP_INSTALLED_SCHEMA_VERSION,
            servers: vec![installed.clone()],
        };
        assert_eq!(
            InstalledServerIndex::from_json(&index.to_json().unwrap()).unwrap(),
            index
        );
        let duplicate = InstalledServerIndex {
            servers: vec![installed.clone(), installed],
            ..index.clone()
        };
        assert!(duplicate.validate().is_err());
        let mut unknown = index;
        unknown.version += 1;
        assert_eq!(
            InstalledServerIndex::load(&unknown.to_json().unwrap()),
            InstalledServerIndex::default()
        );
    }

    #[test]
    fn rfc3339_validation_handles_fraction_offsets_and_bad_dates() {
        for value in [
            "2026-08-12T00:00:00Z",
            "2024-02-29T23:59:60.123+09:00",
            "2026-08-12T00:00:00-05:30",
        ] {
            validate_rfc3339("time", value).unwrap();
        }
        for value in [
            "2026-02-29T00:00:00Z",
            "2026-08-12 00:00:00Z",
            "2026-08-12T00:00:00",
            "2026-08-12T00:00:00+24:00",
        ] {
            assert!(validate_rfc3339("time", value).is_err(), "{value}");
        }
    }
}
