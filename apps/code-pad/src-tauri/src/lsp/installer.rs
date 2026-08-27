//! Exact-version managed language-server installation.
//!
//! Downloads and extraction use nonce-scoped paths. Only a verified staging
//! tree is renamed into the immutable server directory, and that directory is
//! not active until the installed index has been atomically replaced.

use crate::commands::session::atomic_write;
use crate::lsp::catalog::{
    initial_catalog, ArtifactKind, InstallSource, InstalledServer, InstalledServerIndex,
    RuntimeSpec, ServerManifest, WINDOWS_X86_64_PLATFORM,
};
use crate::lsp::node_lock::{
    reviewed_node_lock, NodeDependencyLock, NodeLockError, NodePackageLock,
    REVIEWED_NODE_LOCK_SHA256,
};
use base64::Engine;
use flate2::read::GzDecoder;
use reqwest::header::LOCATION;
use reqwest::{Client, StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256, Sha512};
use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;
use tar::EntryType;
use tokio::io::AsyncWriteExt;

pub const DEFAULT_MAX_INSTALL_BYTES: u64 = 512 * 1024 * 1024;
pub const DEFAULT_MAX_ARCHIVE_ENTRIES: usize = 100_000;
pub const DEFAULT_MAX_ARCHIVE_DEPTH: usize = 32;
const MAX_REDIRECTS: usize = 5;
const INDEX_FILE: &str = "installed.json";
const ARCHIVE_CACHE_DIRECTORY: &str = "cache";

static NONCE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstallLimits {
    pub max_download_bytes: u64,
    pub max_extracted_bytes: u64,
    pub max_archive_entries: usize,
    pub max_archive_depth: usize,
}

impl Default for InstallLimits {
    fn default() -> Self {
        Self {
            max_download_bytes: DEFAULT_MAX_INSTALL_BYTES,
            max_extracted_bytes: DEFAULT_MAX_INSTALL_BYTES,
            max_archive_entries: DEFAULT_MAX_ARCHIVE_ENTRIES,
            max_archive_depth: DEFAULT_MAX_ARCHIVE_DEPTH,
        }
    }
}

impl InstallLimits {
    fn validate(self) -> Result<Self, InstallError> {
        if self.max_download_bytes == 0
            || self.max_extracted_bytes == 0
            || self.max_archive_entries == 0
            || self.max_archive_depth == 0
        {
            return Err(InstallError::InvalidLimits);
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallResult {
    pub server: InstalledServer,
    pub already_installed: bool,
}

/// The only managed-server data that the process-launch boundary receives.
/// The path is derived from the private installed index and the reviewed
/// catalog; it is never accepted from the UI or persisted in `ServerRef`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedInstallResolution {
    pub manifest: ServerManifest,
    pub installed_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedInstallState {
    NotInstalled,
    Installed,
    NeedsReinstall,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedInstallStatus {
    pub manifest_id: String,
    pub version: String,
    pub platform: String,
    pub state: ManagedInstallState,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub installed: Option<InstalledServerMetadata>,
    /// True only when the exact catalog artifact is present in the
    /// app-owned cache and passes both size and SHA-256 verification.
    #[serde(default)]
    pub archive_cached: bool,
}

/// Safe status metadata exposed to the UI. The process keeps the canonical
/// install path in the private index, but never sends it over the Tauri
/// boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledServerMetadata {
    pub manifest_id: String,
    pub version: String,
    pub platform: String,
    pub sha256: String,
    pub source_url: String,
    pub license: String,
    pub artifact_url: String,
    pub entrypoint: String,
    pub runtime: RuntimeSpec,
    pub installed_at: String,
    #[serde(default)]
    pub package_lock_sha256: Option<String>,
    #[serde(default)]
    pub install_source: InstallSource,
    #[serde(default)]
    pub last_verified_at: Option<String>,
}

impl From<&InstalledServer> for InstalledServerMetadata {
    fn from(server: &InstalledServer) -> Self {
        Self {
            manifest_id: server.manifest_id.clone(),
            version: server.version.clone(),
            platform: server.platform.clone(),
            sha256: server.sha256.clone(),
            source_url: server.source_url.clone(),
            license: server.license.clone(),
            artifact_url: server.artifact_url.clone(),
            entrypoint: server.entrypoint.clone(),
            runtime: server.runtime.clone(),
            installed_at: server.installed_at.clone(),
            package_lock_sha256: server.package_lock_sha256.clone(),
            install_source: server.install_source,
            last_verified_at: server.last_verified_at.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NodePackageArchive {
    pub name: String,
    pub version: String,
    pub archive: PathBuf,
}

#[derive(Debug)]
pub enum InstallError {
    InvalidManifest(String),
    InvalidLimits,
    VersionMismatch,
    InsecureUrl,
    RedirectRejected,
    HttpStatus(u16),
    Network(String),
    SizeMismatch {
        expected: u64,
        actual: u64,
    },
    SizeLimitExceeded,
    DigestMismatch,
    DependencyLock(String),
    NodePackageMismatch(String),
    UnsupportedNodePackage(String),
    PackageJsonInvalid(String),
    InvalidArchive(String),
    UnsafeArchivePath,
    UnsupportedArchiveEntry,
    ArchiveDepthExceeded,
    EntrypointMissing,
    InstallConflict,
    InstallBusy,
    IndexCorrupt,
    CatalogManifestNotFound {
        manifest_id: String,
        version: String,
        platform: String,
    },
    NotInstalled,
    MetadataMismatch(String),
    Io {
        operation: &'static str,
        kind: io::ErrorKind,
    },
}

impl InstallError {
    fn io(operation: &'static str, error: io::Error) -> Self {
        Self::Io {
            operation,
            kind: error.kind(),
        }
    }
}

impl fmt::Display for InstallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidManifest(reason) => {
                write!(formatter, "invalid install manifest: {reason}")
            }
            Self::InvalidLimits => formatter.write_str("invalid installer limits"),
            Self::VersionMismatch => {
                formatter.write_str("requested version does not match manifest")
            }
            Self::InsecureUrl => formatter.write_str("artifact URL must use HTTPS"),
            Self::RedirectRejected => formatter.write_str("artifact redirect was rejected"),
            Self::HttpStatus(status) => {
                write!(formatter, "artifact request failed with HTTP {status}")
            }
            Self::Network(reason) => write!(formatter, "artifact request failed: {reason}"),
            Self::SizeMismatch { expected, actual } => {
                write!(
                    formatter,
                    "artifact size mismatch: expected {expected}, got {actual}"
                )
            }
            Self::SizeLimitExceeded => formatter.write_str("installer size limit exceeded"),
            Self::DigestMismatch => formatter.write_str("artifact SHA-256 mismatch"),
            Self::DependencyLock(reason) => {
                write!(formatter, "invalid Node dependency lock: {reason}")
            }
            Self::NodePackageMismatch(reason) => {
                write!(formatter, "Node package does not match its lock: {reason}")
            }
            Self::UnsupportedNodePackage(package) => write!(
                formatter,
                "required Node package is unsupported on this platform: {package}"
            ),
            Self::PackageJsonInvalid(reason) => {
                write!(formatter, "invalid installed package.json: {reason}")
            }
            Self::InvalidArchive(reason) => write!(formatter, "invalid artifact archive: {reason}"),
            Self::UnsafeArchivePath => formatter.write_str("artifact contains an unsafe path"),
            Self::UnsupportedArchiveEntry => {
                formatter.write_str("artifact contains a link or special entry")
            }
            Self::ArchiveDepthExceeded => {
                formatter.write_str("artifact path depth exceeds the installer limit")
            }
            Self::EntrypointMissing => formatter.write_str("artifact entrypoint is missing"),
            Self::InstallConflict => {
                formatter.write_str("immutable install destination already exists")
            }
            Self::InstallBusy => formatter.write_str("another managed installation is active"),
            Self::IndexCorrupt => formatter
                .write_str("installed server index is corrupt; explicit recovery is required"),
            Self::CatalogManifestNotFound {
                manifest_id,
                version,
                platform,
            } => write!(
                formatter,
                "catalog has no exact manifest for {manifest_id}@{version} on {platform}"
            ),
            Self::NotInstalled => formatter.write_str("managed server is not installed"),
            Self::MetadataMismatch(reason) => {
                write!(formatter, "managed installation needs reinstall: {reason}")
            }
            Self::Io { operation, kind } => {
                write!(formatter, "installer I/O failed while {operation}: {kind}")
            }
        }
    }
}

impl std::error::Error for InstallError {}

#[derive(Debug, Clone)]
pub struct ManagedInstaller {
    lsp_root: PathBuf,
    limits: InstallLimits,
    client: Client,
    operation_lock: Arc<tokio::sync::Mutex<()>>,
}

impl ManagedInstaller {
    pub fn new(app_data_root: impl AsRef<Path>) -> Result<Self, InstallError> {
        Self::with_limits(app_data_root, InstallLimits::default())
    }

    pub fn with_limits(
        app_data_root: impl AsRef<Path>,
        limits: InstallLimits,
    ) -> Result<Self, InstallError> {
        let limits = limits.validate()?;
        reject_symlink_tree(app_data_root.as_ref())?;
        let lsp_root = app_data_root.as_ref().join("lsp");
        reject_symlink_tree(&lsp_root)?;
        fs::create_dir_all(lsp_root.join("downloads"))
            .map_err(|error| InstallError::io("creating downloads directory", error))?;
        fs::create_dir_all(lsp_root.join("downloads").join(ARCHIVE_CACHE_DIRECTORY))
            .map_err(|error| InstallError::io("creating archive cache directory", error))?;
        fs::create_dir_all(lsp_root.join("staging"))
            .map_err(|error| InstallError::io("creating staging directory", error))?;
        fs::create_dir_all(lsp_root.join("servers"))
            .map_err(|error| InstallError::io("creating servers directory", error))?;
        reject_symlink_tree(&lsp_root)?;
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(10 * 60))
            .build()
            .map_err(|error| InstallError::Network(error.without_url().to_string()))?;
        Ok(Self {
            lsp_root,
            limits,
            client,
            operation_lock: Arc::new(tokio::sync::Mutex::new(())),
        })
    }

    pub fn lsp_root(&self) -> &Path {
        &self.lsp_root
    }

    /// Return the platform key supported by the current reviewed catalog.
    /// Platform is intentionally derived inside the native process rather
    /// than persisted in a UI-selected `ServerRef`.
    pub const fn current_platform() -> &'static str {
        WINDOWS_X86_64_PLATFORM
    }

    /// Resolve an install request against the reviewed catalog. The caller
    /// supplies only exact lookup keys; the manifest, URL, digest, and all
    /// other install facts always come from this process-owned catalog.
    pub fn catalog_manifest(
        manifest_id: &str,
        version: &str,
        platform: &str,
    ) -> Result<ServerManifest, InstallError> {
        initial_catalog()
            .into_iter()
            .find(|manifest| {
                manifest.id == manifest_id
                    && manifest.version == version
                    && manifest.platform == platform
            })
            .ok_or_else(|| InstallError::CatalogManifestNotFound {
                manifest_id: manifest_id.to_owned(),
                version: version.to_owned(),
                platform: platform.to_owned(),
            })
    }

    /// Resolve one exact, currently supported managed installation for process
    /// launch. Every start re-reads the private index and revalidates the
    /// reviewed manifest, metadata, canonical destination, tree, and
    /// entrypoint. No caller-supplied path, URL, or argv participates.
    pub fn resolve_managed_install(
        &self,
        manifest_id: &str,
        version: &str,
    ) -> Result<ManagedInstallResolution, InstallError> {
        let _operation = self
            .operation_lock
            .try_lock()
            .map_err(|_| InstallError::InstallBusy)?;
        let platform = Self::current_platform();
        let manifest = Self::catalog_manifest(manifest_id, version, platform)?;
        let index = self.read_index()?;
        let server = index
            .servers
            .iter()
            .find(|server| {
                server.manifest_id == manifest_id
                    && server.version == version
                    && server.platform == platform
            })
            .ok_or(InstallError::NotInstalled)?;
        self.validate_installed_entry(&manifest, server)?;
        let installed_path = fs::canonicalize(self.expected_destination(&manifest)?)
            .map_err(|_| InstallError::MetadataMismatch("managed directory is missing".into()))?;
        Ok(ManagedInstallResolution {
            manifest,
            installed_path,
        })
    }

    /// Install the exact reviewed catalog entry. The timestamp is generated
    /// here rather than accepted from the UI.
    pub async fn install_catalog(
        &self,
        manifest_id: &str,
        version: &str,
        platform: &str,
    ) -> Result<InstallResult, InstallError> {
        let manifest = Self::catalog_manifest(manifest_id, version, platform)?;
        self.install(&manifest, &manifest.version, &current_rfc3339())
            .await
    }

    /// Return the reviewed catalog's install state without changing the
    /// on-disk index. A corrupt index is an explicit error; it is never
    /// replaced as a side effect of reading status.
    pub fn installed_status(&self) -> Result<Vec<ManagedInstallStatus>, InstallError> {
        let _operation = self
            .operation_lock
            .try_lock()
            .map_err(|_| InstallError::InstallBusy)?;
        let index = self.read_index()?;
        let catalog = initial_catalog();
        let mut statuses = Vec::with_capacity(catalog.len() + index.servers.len());
        let mut seen = BTreeMap::new();
        for manifest in &catalog {
            let key = manifest_key(&manifest.id, &manifest.version, &manifest.platform);
            let installed = index.servers.iter().find(|server| {
                manifest_key(&server.manifest_id, &server.version, &server.platform) == key
            });
            let status = match installed {
                None => ManagedInstallStatus {
                    manifest_id: manifest.id.clone(),
                    version: manifest.version.clone(),
                    platform: manifest.platform.clone(),
                    state: ManagedInstallState::NotInstalled,
                    reason: None,
                    installed: None,
                    archive_cached: self.cached_archive_is_verified(manifest)?,
                },
                Some(server) => match self.validate_installed_entry(manifest, server) {
                    Ok(()) => ManagedInstallStatus {
                        manifest_id: manifest.id.clone(),
                        version: manifest.version.clone(),
                        platform: manifest.platform.clone(),
                        state: ManagedInstallState::Installed,
                        reason: None,
                        installed: Some(InstalledServerMetadata::from(server)),
                        archive_cached: self.cached_archive_is_verified(manifest)?,
                    },
                    Err(error) => ManagedInstallStatus {
                        manifest_id: manifest.id.clone(),
                        version: manifest.version.clone(),
                        platform: manifest.platform.clone(),
                        state: ManagedInstallState::NeedsReinstall,
                        reason: Some(error.to_string()),
                        installed: Some(InstalledServerMetadata::from(server)),
                        archive_cached: self.cached_archive_is_verified(manifest)?,
                    },
                },
            };
            seen.insert(key, ());
            statuses.push(status);
        }
        for server in &index.servers {
            let key = manifest_key(&server.manifest_id, &server.version, &server.platform);
            if !seen.contains_key(&key) {
                statuses.push(ManagedInstallStatus {
                    manifest_id: server.manifest_id.clone(),
                    version: server.version.clone(),
                    platform: server.platform.clone(),
                    state: ManagedInstallState::NeedsReinstall,
                    reason: Some("installed entry is not present in the reviewed catalog".into()),
                    installed: Some(InstalledServerMetadata::from(server)),
                    archive_cached: false,
                });
            }
        }
        Ok(statuses)
    }

    /// Explicitly recover a corrupt index. This is never called by a status
    /// read or an install attempt; the UI exposes it as a user-confirmed
    /// recovery action.
    pub fn recover_installed_index(&self) -> Result<(), InstallError> {
        let _operation = self
            .operation_lock
            .try_lock()
            .map_err(|_| InstallError::InstallBusy)?;
        self.ensure_index_path_safe()?;
        self.write_index(&InstalledServerIndex::default())
    }

    /// Remove one exact indexed entry. The catalog is intentionally not needed
    /// here: an older or locally missing catalog version must still be
    /// explicitly recoverable. The recorded key and canonical app-owned
    /// destination are the only removal authority; metadata drift is handled
    /// by the separate status validator.
    pub fn uninstall_catalog(
        &self,
        manifest_id: &str,
        version: &str,
        platform: &str,
    ) -> Result<(), InstallError> {
        self.uninstall_indexed(manifest_id, version, platform)
    }

    pub fn uninstall(&self, manifest: &ServerManifest) -> Result<(), InstallError> {
        manifest
            .validate_for_install()
            .map_err(|error| InstallError::InvalidManifest(error.to_string()))?;
        self.uninstall_indexed(&manifest.id, &manifest.version, &manifest.platform)
    }

    /// Remove an exact key from the process-owned installed index. This is the
    /// command boundary used by the UI: callers cannot supply a manifest,
    /// artifact URL, or destination path.
    pub fn uninstall_indexed(
        &self,
        manifest_id: &str,
        version: &str,
        platform: &str,
    ) -> Result<(), InstallError> {
        let _operation = self
            .operation_lock
            .try_lock()
            .map_err(|_| InstallError::InstallBusy)?;
        let mut index = self.read_index()?;
        let position = index.servers.iter().position(|server| {
            server.manifest_id == manifest_id
                && server.version == version
                && server.platform == platform
        });
        let position = position.ok_or(InstallError::NotInstalled)?;
        let server = index.servers[position].clone();
        let destination = self.safe_removal_destination(&server)?;
        let previous = index.clone();
        index.servers.remove(position);
        self.write_index(&index)?;
        match fs::remove_dir_all(&destination) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => {
                let rollback = self.write_index(&previous);
                if rollback.is_err() {
                    return Err(InstallError::io("rolling back uninstall index", error));
                }
                Err(InstallError::io("removing managed installation", error))
            }
        }
    }

    /// Download, verify, extract, and promote the exact version confirmed by
    /// the user. Automatic version selection is intentionally impossible.
    pub async fn install(
        &self,
        manifest: &ServerManifest,
        requested_version: &str,
        installed_at: &str,
    ) -> Result<InstallResult, InstallError> {
        let _operation = self
            .operation_lock
            .try_lock()
            .map_err(|_| InstallError::InstallBusy)?;
        self.validate_request(manifest, requested_version)?;
        self.read_index()?;
        let nonce = unique_nonce();
        if manifest.runtime.kind == crate::lsp::catalog::RuntimeKind::Node {
            let lock = reviewed_node_lock().map_err(node_lock_error)?;
            let download_dir = self.lsp_root.join("downloads").join(&nonce);
            let staging = self.lsp_root.join("staging").join(&nonce);
            let result = self
                .install_node_downloads(manifest, installed_at, &lock, &download_dir, &staging)
                .await;
            if download_dir.exists() {
                let _ = fs::remove_dir_all(&download_dir);
            }
            if staging.exists() {
                let _ = fs::remove_dir_all(&staging);
            }
            return result;
        }
        let partial = self
            .lsp_root
            .join("downloads")
            .join(format!("{nonce}.part"));
        let staging = self.lsp_root.join("staging").join(&nonce);
        let result = async {
            let (archive, source) = self.prepare_archive(manifest, &partial).await?;
            self.install_verified_archive(manifest, installed_at, &archive, &staging, source)
        }
        .await;
        let _ = fs::remove_file(&partial);
        if staging.exists() {
            let _ = fs::remove_dir_all(&staging);
        }
        result
    }

    /// Local archive boundary. The selected file is verified against the
    /// supplied manifest, copied into the app-owned cache, and then follows
    /// the same extraction, entrypoint, promotion, and index checks as a
    /// network artifact. The selected path is never persisted or returned.
    pub fn install_archive(
        &self,
        manifest: &ServerManifest,
        requested_version: &str,
        installed_at: &str,
        archive: impl AsRef<Path>,
    ) -> Result<InstallResult, InstallError> {
        let _operation = self
            .operation_lock
            .try_lock()
            .map_err(|_| InstallError::InstallBusy)?;
        self.validate_request(manifest, requested_version)?;
        self.read_index()?;
        if manifest.runtime.kind == crate::lsp::catalog::RuntimeKind::Node {
            return Err(InstallError::DependencyLock(
                "Node installs require the complete reviewed package archive set".into(),
            ));
        }
        let cached = self.cache_archive(manifest, archive.as_ref())?;
        let staging = self.lsp_root.join("staging").join(unique_nonce());
        let result = self.install_verified_archive(
            manifest,
            installed_at,
            &cached,
            &staging,
            InstallSource::LocalArchive,
        );
        if staging.exists() {
            let _ = fs::remove_dir_all(&staging);
        }
        result
    }

    /// Import exact reviewed catalog artifacts selected by the user. The
    /// catalog and, for Node servers, the dependency lock are process-owned;
    /// the UI can provide only exact lookup keys and native picker paths.
    /// Native archives use the single-file boundary above, while Node imports
    /// match a multi-file `.tgz` set against the complete reviewed closure.
    pub fn import_catalog_archives(
        &self,
        manifest_id: &str,
        version: &str,
        platform: &str,
        archive_paths: &[PathBuf],
    ) -> Result<InstallResult, InstallError> {
        let _operation = self
            .operation_lock
            .try_lock()
            .map_err(|_| InstallError::InstallBusy)?;
        let manifest = Self::catalog_manifest(manifest_id, version, platform)?;
        self.validate_request(&manifest, &manifest.version)?;
        self.read_index()?;
        // The catalog version identifies the artifact; it is not a valid
        // installation timestamp. Capture one trusted wall-clock value for
        // either import path so persisted schema validation cannot fail after
        // an otherwise successful promotion.
        let installed_at = current_rfc3339();
        if manifest.runtime.kind != crate::lsp::catalog::RuntimeKind::Node {
            if archive_paths.len() != 1 {
                return Err(InstallError::DependencyLock(
                    "native imports require exactly one archive".into(),
                ));
            }
            let cached = self.cache_archive(&manifest, &archive_paths[0])?;
            let staging = self.lsp_root.join("staging").join(unique_nonce());
            let result = self.install_verified_archive(
                &manifest,
                &installed_at,
                &cached,
                &staging,
                InstallSource::LocalArchive,
            );
            if staging.exists() {
                let _ = fs::remove_dir_all(&staging);
            }
            return result;
        }

        let lock = reviewed_node_lock().map_err(node_lock_error)?;
        let packages = lock
            .packages_for_server(&manifest.id)
            .map_err(node_lock_error)?;
        let (archives, source) =
            self.resolve_node_archive_set(&manifest.platform, &packages, archive_paths)?;
        let staging = self.lsp_root.join("staging").join(unique_nonce());
        let result = self.install_node_archives_with_lock(
            &manifest,
            &installed_at,
            &lock,
            &archives,
            &staging,
            source,
        );
        if staging.exists() {
            let _ = fs::remove_dir_all(&staging);
        }
        result
    }

    /// Backward-compatible single-file native import helper. Node callers
    /// must use `import_catalog_archives` so a primary tarball cannot bypass
    /// the reviewed dependency-closure check.
    pub fn import_catalog_archive(
        &self,
        manifest_id: &str,
        version: &str,
        platform: &str,
        archive: impl AsRef<Path>,
    ) -> Result<InstallResult, InstallError> {
        let archive_paths = vec![archive.as_ref().to_path_buf()];
        self.import_catalog_archives(manifest_id, version, platform, &archive_paths)
    }

    async fn prepare_archive(
        &self,
        manifest: &ServerManifest,
        partial: &Path,
    ) -> Result<(PathBuf, InstallSource), InstallError> {
        if let Some(cached) = self.cached_archive(manifest)? {
            return Ok((cached, InstallSource::ArchiveCache));
        }
        self.download(manifest, partial).await?;
        let cached = self.cache_archive(manifest, partial)?;
        Ok((cached, InstallSource::Network))
    }

    /// Install a reviewed Node package closure from local fixture archives.
    /// Production downloads use the same extraction path; this boundary keeps
    /// security tests completely independent from the network.
    pub fn install_node_archives(
        &self,
        manifest: &ServerManifest,
        requested_version: &str,
        installed_at: &str,
        archives: &[NodePackageArchive],
    ) -> Result<InstallResult, InstallError> {
        let _operation = self
            .operation_lock
            .try_lock()
            .map_err(|_| InstallError::InstallBusy)?;
        self.validate_request(manifest, requested_version)?;
        self.read_index()?;
        if manifest.runtime.kind != crate::lsp::catalog::RuntimeKind::Node {
            return Err(InstallError::InvalidManifest(
                "Node package archives require a Node runtime".into(),
            ));
        }
        let lock = reviewed_node_lock().map_err(node_lock_error)?;
        let staging = self.lsp_root.join("staging").join(unique_nonce());
        let result = self.install_node_archives_with_lock(
            manifest,
            installed_at,
            &lock,
            archives,
            &staging,
            InstallSource::LocalArchive,
        );
        if staging.exists() {
            let _ = fs::remove_dir_all(&staging);
        }
        result
    }

    async fn install_node_downloads(
        &self,
        manifest: &ServerManifest,
        installed_at: &str,
        lock: &NodeDependencyLock,
        download_dir: &Path,
        staging: &Path,
    ) -> Result<InstallResult, InstallError> {
        fs::create_dir_all(download_dir)
            .map_err(|error| InstallError::io("creating Node download directory", error))?;
        reject_symlink_tree(download_dir)?;
        let mut archives = Vec::new();
        let mut source = InstallSource::ArchiveCache;
        for (index, package) in lock
            .packages_for_server(&manifest.id)
            .map_err(node_lock_error)?
            .into_iter()
            .enumerate()
        {
            if !node_package_supported(package, &manifest.platform) {
                if package.optional {
                    continue;
                }
                return Err(InstallError::UnsupportedNodePackage(format!(
                    "{}@{}",
                    package.name, package.version
                )));
            }
            let archive = if let Some(cached) = self.cached_node_archive(package)? {
                cached
            } else {
                source = InstallSource::Network;
                let partial = download_dir.join(format!("{index}.part"));
                self.download_package(package, &partial).await?;
                self.cache_node_package(package, &partial)?
            };
            archives.push(NodePackageArchive {
                name: package.name.clone(),
                version: package.version.clone(),
                archive,
            });
        }
        self.install_node_archives_with_lock(
            manifest,
            installed_at,
            lock,
            &archives,
            staging,
            source,
        )
    }

    fn install_node_archives_with_lock(
        &self,
        manifest: &ServerManifest,
        installed_at: &str,
        lock: &NodeDependencyLock,
        archives: &[NodePackageArchive],
        staging: &Path,
        install_source: InstallSource,
    ) -> Result<InstallResult, InstallError> {
        lock.validate().map_err(node_lock_error)?;
        let packages = lock
            .packages_for_server(&manifest.id)
            .map_err(node_lock_error)?;
        let mut archive_by_package = BTreeMap::new();
        for archive in archives {
            let key = package_key(&archive.name, &archive.version);
            if archive_by_package.insert(key, archive).is_some() {
                return Err(InstallError::DependencyLock(format!(
                    "duplicate archive for {}@{}",
                    archive.name, archive.version
                )));
            }
        }
        let mut expected_keys = BTreeMap::new();
        let mut extracted_entries = 0_usize;
        let mut extracted_bytes = 0_u64;
        fs::create_dir(staging)
            .map_err(|error| InstallError::io("creating Node staging directory", error))?;
        reject_symlink_tree(staging)?;
        for package in packages {
            let key = package_key(&package.name, &package.version);
            if !node_package_supported(package, &manifest.platform) {
                if package.optional {
                    continue;
                }
                return Err(InstallError::UnsupportedNodePackage(key));
            }
            expected_keys.insert(key.clone(), ());
            let archive = archive_by_package.get(&key).ok_or_else(|| {
                InstallError::DependencyLock(format!("missing archive for {key}"))
            })?;
            let archive_path = self.cache_node_package(package, &archive.archive)?;
            let relative = lock
                .install_path(&manifest.id, package)
                .map_err(node_lock_error)?;
            let (entries, bytes) =
                extract_node_package(package, &archive_path, staging, &relative, self.limits)?;
            extracted_entries = extracted_entries
                .checked_add(entries)
                .ok_or(InstallError::SizeLimitExceeded)?;
            extracted_bytes = extracted_bytes
                .checked_add(bytes)
                .ok_or(InstallError::SizeLimitExceeded)?;
            if extracted_entries > self.limits.max_archive_entries
                || extracted_bytes > self.limits.max_extracted_bytes
            {
                return Err(InstallError::SizeLimitExceeded);
            }
            sanitize_node_package_json(staging, &relative, package)?;
        }
        if archive_by_package
            .keys()
            .any(|key| !expected_keys.contains_key(key))
        {
            return Err(InstallError::DependencyLock(
                "archive set contains a package outside the reviewed closure".into(),
            ));
        }
        self.promote_staging(manifest, installed_at, staging, install_source)
    }

    fn resolve_node_archive_set(
        &self,
        platform: &str,
        packages: &[&NodePackageLock],
        archive_paths: &[PathBuf],
    ) -> Result<(Vec<NodePackageArchive>, InstallSource), InstallError> {
        if archive_paths.is_empty() {
            return Err(InstallError::DependencyLock(
                "at least one local Node archive is required".into(),
            ));
        }

        let mut supported = Vec::new();
        let mut expected_by_sha256: BTreeMap<&str, Vec<&NodePackageLock>> = BTreeMap::new();
        for package in packages {
            if !node_package_supported(package, platform) {
                if package.optional {
                    continue;
                }
                return Err(InstallError::UnsupportedNodePackage(format!(
                    "{}@{}",
                    package.name, package.version
                )));
            }
            supported.push(*package);
            expected_by_sha256
                .entry(package.sha256.as_str())
                .or_default()
                .push(*package);
        }
        if expected_by_sha256
            .values()
            .any(|matches| matches.len() != 1)
        {
            return Err(InstallError::DependencyLock(
                "reviewed Node archive digest is ambiguous".into(),
            ));
        }
        if archive_paths.len() > supported.len() {
            return Err(InstallError::DependencyLock(
                "local Node archive set contains extra archives".into(),
            ));
        }

        let mut seen_paths: Vec<PathBuf> = Vec::with_capacity(archive_paths.len());
        let mut selected = BTreeMap::new();
        for archive in archive_paths {
            validate_node_archive_selection_path(archive)?;
            let canonical = fs::canonicalize(archive).map_err(|_| {
                InstallError::DependencyLock("selected Node archive is unavailable".into())
            })?;
            validate_node_archive_selection_path(&canonical)?;
            if seen_paths.iter().any(|seen| is_same_path(seen, &canonical)) {
                return Err(InstallError::DependencyLock(
                    "local Node archive set contains duplicate archives".into(),
                ));
            }
            seen_paths.push(canonical.clone());

            let digest = hash_archive(&canonical, self.limits.max_download_bytes)?;
            let digest_hex = hex_digest(&digest.sha256);
            let package = match expected_by_sha256.get(digest_hex.as_str()) {
                None => {
                    return Err(InstallError::DependencyLock(
                        "local Node archive set contains an extra archive".into(),
                    ))
                }
                Some(matches) => matches[0],
            };
            if digest.size != package.size_bytes {
                return Err(InstallError::SizeMismatch {
                    expected: package.size_bytes,
                    actual: digest.size,
                });
            }
            verify_digest(&digest.sha256, &package.sha256)?;
            verify_integrity(&digest.sha512, &package.integrity)?;
            let key = package_key(&package.name, &package.version);
            if selected
                .insert(
                    key,
                    NodePackageArchive {
                        name: package.name.clone(),
                        version: package.version.clone(),
                        archive: canonical,
                    },
                )
                .is_some()
            {
                return Err(InstallError::DependencyLock(
                    "local Node archive set contains duplicate packages".into(),
                ));
            }
        }

        let mut source = InstallSource::ArchiveCache;
        let mut archives = Vec::with_capacity(supported.len());
        for package in supported {
            let key = package_key(&package.name, &package.version);
            if let Some(archive) = selected.remove(&key) {
                source = InstallSource::LocalArchive;
                archives.push(archive);
                continue;
            }
            if let Some(archive) = self.cached_node_archive(package)? {
                archives.push(NodePackageArchive {
                    name: package.name.clone(),
                    version: package.version.clone(),
                    archive,
                });
                continue;
            }
            return Err(InstallError::DependencyLock(
                "local Node archive set is missing a reviewed package".into(),
            ));
        }
        if !selected.is_empty() {
            return Err(InstallError::DependencyLock(
                "local Node archive set contains an extra archive".into(),
            ));
        }
        Ok((archives, source))
    }

    /// Return a verified cached copy of the exact catalog artifact, if one is
    /// available. A malformed regular cache file is treated as unavailable so
    /// an explicit install can replace it with a newly verified artifact; a
    /// symlink/reparse target is a hard failure and is never followed.
    fn cached_archive(&self, manifest: &ServerManifest) -> Result<Option<PathBuf>, InstallError> {
        let path = self.archive_cache_path(&manifest.artifact.sha256, manifest.artifact.kind)?;
        if manifest.runtime.kind == crate::lsp::catalog::RuntimeKind::Node {
            let lock = reviewed_node_lock().map_err(node_lock_error)?;
            for package in lock
                .packages_for_server(&manifest.id)
                .map_err(node_lock_error)?
            {
                if !node_package_supported(package, &manifest.platform) {
                    if package.optional {
                        continue;
                    }
                    return Ok(None);
                }
                if self.cached_node_archive(package)?.is_none() {
                    return Ok(None);
                }
            }
            return Ok(Some(path));
        }
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata_has_reparse_point(&metadata) => {
                Err(InstallError::UnsafeArchivePath)
            }
            Ok(metadata) if !metadata.file_type().is_file() => Err(InstallError::UnsafeArchivePath),
            Ok(_) => match self.verify_cached_archive(manifest, &path) {
                Ok(()) => Ok(Some(path)),
                Err(InstallError::DigestMismatch)
                | Err(InstallError::SizeMismatch { .. })
                | Err(InstallError::SizeLimitExceeded) => Ok(None),
                Err(error) => Err(error),
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(InstallError::io("checking archive cache", error)),
        }
    }

    fn verify_cached_archive(
        &self,
        manifest: &ServerManifest,
        archive: &Path,
    ) -> Result<(), InstallError> {
        verify_archive_file(manifest, archive, self.limits.max_download_bytes)
    }

    fn cached_node_archive(
        &self,
        package: &NodePackageLock,
    ) -> Result<Option<PathBuf>, InstallError> {
        let path = self.archive_cache_path(&package.sha256, ArtifactKind::NpmTarball)?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata_has_reparse_point(&metadata) => {
                Err(InstallError::UnsafeArchivePath)
            }
            Ok(metadata) if !metadata.file_type().is_file() => Err(InstallError::UnsafeArchivePath),
            Ok(_) => match verify_node_package_archive(package, &path, self.limits) {
                Ok(()) => Ok(Some(path)),
                Err(InstallError::DigestMismatch)
                | Err(InstallError::SizeMismatch { .. })
                | Err(InstallError::SizeLimitExceeded)
                | Err(InstallError::InvalidArchive(_)) => Ok(None),
                Err(error) => Err(error),
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(InstallError::io("checking Node archive cache", error)),
        }
    }

    fn cached_archive_is_verified(&self, manifest: &ServerManifest) -> Result<bool, InstallError> {
        Ok(self.cached_archive(manifest)?.is_some())
    }

    fn archive_cache_path(
        &self,
        sha256: &str,
        kind: ArtifactKind,
    ) -> Result<PathBuf, InstallError> {
        if decode_sha256(sha256).is_none() {
            return Err(InstallError::InvalidManifest(
                "artifact SHA-256 is invalid".into(),
            ));
        }
        let extension = match kind {
            ArtifactKind::Zip => "zip",
            ArtifactKind::NpmTarball => "tgz",
        };
        let cache_dir = self
            .lsp_root
            .join("downloads")
            .join(ARCHIVE_CACHE_DIRECTORY);
        reject_symlink_tree(&cache_dir)?;
        Ok(cache_dir.join(format!("{sha256}.{extension}")))
    }

    fn cache_archive(
        &self,
        manifest: &ServerManifest,
        archive: &Path,
    ) -> Result<PathBuf, InstallError> {
        validate_external_archive(archive)?;
        verify_archive_file(manifest, archive, self.limits.max_download_bytes)?;
        let destination =
            self.archive_cache_path(&manifest.artifact.sha256, manifest.artifact.kind)?;
        self.copy_verified_to_cache(
            archive,
            &destination,
            manifest
                .artifact
                .size_bytes
                .ok_or_else(|| InstallError::InvalidManifest("artifact size is required".into()))?,
            &manifest.artifact.sha256,
        )
    }

    fn cache_node_package(
        &self,
        package: &NodePackageLock,
        archive: &Path,
    ) -> Result<PathBuf, InstallError> {
        validate_external_archive(archive)?;
        verify_node_package_archive(package, archive, self.limits)?;
        let destination = self.archive_cache_path(&package.sha256, ArtifactKind::NpmTarball)?;
        self.copy_verified_to_cache(archive, &destination, package.size_bytes, &package.sha256)
    }

    fn copy_verified_to_cache(
        &self,
        source: &Path,
        destination: &Path,
        expected_size: u64,
        expected_sha256: &str,
    ) -> Result<PathBuf, InstallError> {
        if source == destination {
            return Ok(destination.to_path_buf());
        }
        let parent = destination
            .parent()
            .ok_or(InstallError::UnsafeArchivePath)?;
        reject_symlink_tree(parent)?;
        let temporary = parent.join(format!(
            ".{}.part-{}",
            destination
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or(InstallError::UnsafeArchivePath)?,
            unique_nonce()
        ));
        let result = (|| {
            let mut input = File::open(source)
                .map_err(|error| InstallError::io("opening archive for cache", error))?;
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|error| InstallError::io("creating archive cache", error))?;
            let mut hasher = Sha256::new();
            let mut copied = 0_u64;
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                let read = input
                    .read(&mut buffer)
                    .map_err(|error| InstallError::io("reading archive for cache", error))?;
                if read == 0 {
                    break;
                }
                copied = copied
                    .checked_add(read as u64)
                    .ok_or(InstallError::SizeLimitExceeded)?;
                if copied > expected_size || copied > self.limits.max_download_bytes {
                    return Err(InstallError::SizeLimitExceeded);
                }
                hasher.update(&buffer[..read]);
                output
                    .write_all(&buffer[..read])
                    .map_err(|error| InstallError::io("writing archive cache", error))?;
            }
            output
                .flush()
                .and_then(|_| output.sync_all())
                .map_err(|error| InstallError::io("syncing archive cache", error))?;
            if copied != expected_size {
                return Err(InstallError::SizeMismatch {
                    expected: expected_size,
                    actual: copied,
                });
            }
            verify_digest(&hasher.finalize(), expected_sha256)?;

            reject_symlink_tree(parent)?;
            match fs::symlink_metadata(destination) {
                Ok(metadata) if metadata_has_reparse_point(&metadata) => {
                    return Err(InstallError::UnsafeArchivePath)
                }
                Ok(metadata) if !metadata.file_type().is_file() => {
                    return Err(InstallError::UnsafeArchivePath)
                }
                Ok(_) => {
                    // Cache files are app-owned. Replacing a regular stale
                    // cache is safe, but never remove a link or directory.
                    fs::remove_file(destination)
                        .map_err(|error| InstallError::io("replacing archive cache", error))?;
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(InstallError::io("checking archive cache target", error)),
            }
            fs::rename(&temporary, destination)
                .map_err(|error| InstallError::io("committing archive cache", error))?;
            Ok(destination.to_path_buf())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    fn validate_request(
        &self,
        manifest: &ServerManifest,
        requested_version: &str,
    ) -> Result<(), InstallError> {
        manifest
            .validate_for_install()
            .map_err(|error| InstallError::InvalidManifest(error.to_string()))?;
        if manifest.version != requested_version {
            return Err(InstallError::VersionMismatch);
        }
        let expected = manifest.artifact.size_bytes.ok_or_else(|| {
            InstallError::InvalidManifest("artifact size is required".to_string())
        })?;
        if expected > self.limits.max_download_bytes {
            return Err(InstallError::SizeLimitExceeded);
        }
        if manifest.runtime.kind == crate::lsp::catalog::RuntimeKind::Node {
            if manifest.artifact.kind != ArtifactKind::NpmTarball
                || manifest.artifact.archive_root != "package"
            {
                return Err(InstallError::DependencyLock(
                    "Node manifests must use a package-root npm tarball".into(),
                ));
            }
            let lock = reviewed_node_lock().map_err(node_lock_error)?;
            let primary = lock.primary_root(&manifest.id).map_err(node_lock_error)?;
            let package = lock
                .package(&primary.name, &primary.version)
                .ok_or_else(|| InstallError::DependencyLock("primary package is missing".into()))?;
            if primary.version != manifest.version
                || package.tarball != manifest.artifact.url
                || package.sha256 != manifest.artifact.sha256
                || package.size_bytes != expected
            {
                return Err(InstallError::DependencyLock(
                    "manifest artifact does not match the reviewed primary package".into(),
                ));
            }
            if manifest.files.package_lock_sha256.as_deref() != Some(REVIEWED_NODE_LOCK_SHA256) {
                return Err(InstallError::DependencyLock(
                    "manifest package lock digest does not match reviewed lock".into(),
                ));
            }
        }
        let url = Url::parse(&manifest.artifact.url)
            .map_err(|_| InstallError::InvalidManifest("artifact URL is invalid".to_string()))?;
        if url.scheme() != "https" || url.host_str().is_none() {
            return Err(InstallError::InsecureUrl);
        }
        Ok(())
    }

    async fn download(
        &self,
        manifest: &ServerManifest,
        partial: &Path,
    ) -> Result<(), InstallError> {
        let expected_size = manifest.artifact.size_bytes.ok_or_else(|| {
            InstallError::InvalidManifest("artifact size is required".to_string())
        })?;
        self.download_verified(
            &manifest.artifact.url,
            expected_size,
            &manifest.artifact.sha256,
            &manifest.artifact.allowed_redirect_hosts,
            partial,
        )
        .await
    }

    async fn download_package(
        &self,
        package: &NodePackageLock,
        partial: &Path,
    ) -> Result<(), InstallError> {
        self.download_verified(
            &package.tarball,
            package.size_bytes,
            &package.sha256,
            &[],
            partial,
        )
        .await
    }

    async fn download_verified(
        &self,
        url: &str,
        expected_size: u64,
        expected_sha256: &str,
        allowed_redirect_hosts: &[String],
        partial: &Path,
    ) -> Result<(), InstallError> {
        // The parent is app-owned and was created by the constructor or the
        // Node download boundary. Recheck it immediately before opening the
        // nonce file so a replaced downloads/cache component cannot redirect
        // a download through a symlink or Windows reparse point.
        reject_symlink_tree(partial.parent().ok_or(InstallError::UnsafeArchivePath)?)?;
        reject_symlink_tree(partial)?;
        let initial = Url::parse(url)
            .map_err(|_| InstallError::InvalidManifest("artifact URL is invalid".to_string()))?;
        let initial_host = initial
            .host_str()
            .ok_or(InstallError::InsecureUrl)?
            .to_string();
        let mut current = initial;
        let mut response = None;
        for redirect_count in 0..=MAX_REDIRECTS {
            let candidate = self
                .client
                .get(current.clone())
                .send()
                .await
                .map_err(|error| InstallError::Network(error.without_url().to_string()))?;
            if candidate.status().is_redirection() {
                if redirect_count == MAX_REDIRECTS {
                    return Err(InstallError::RedirectRejected);
                }
                let location = candidate
                    .headers()
                    .get(LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or(InstallError::RedirectRejected)?;
                let next = current
                    .join(location)
                    .map_err(|_| InstallError::RedirectRejected)?;
                let next_host = next.host_str().ok_or(InstallError::RedirectRejected)?;
                let host_allowed = next_host == initial_host
                    || allowed_redirect_hosts.iter().any(|host| host == next_host);
                if next.scheme() != "https"
                    || !next.username().is_empty()
                    || next.password().is_some()
                    || next.port().is_some()
                    || !host_allowed
                {
                    return Err(InstallError::RedirectRejected);
                }
                current = next;
                continue;
            }
            response = Some(candidate);
            break;
        }
        let mut response = response.ok_or(InstallError::RedirectRejected)?;
        if response.status() != StatusCode::OK {
            return Err(InstallError::HttpStatus(response.status().as_u16()));
        }
        if let Some(length) = response.content_length() {
            if length != expected_size {
                return Err(InstallError::SizeMismatch {
                    expected: expected_size,
                    actual: length,
                });
            }
        }
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(partial)
            .await
            .map_err(|error| InstallError::io("creating partial download", error))?;
        let mut hasher = Sha256::new();
        let mut actual = 0_u64;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| InstallError::Network(error.without_url().to_string()))?
        {
            actual = actual
                .checked_add(chunk.len() as u64)
                .ok_or(InstallError::SizeLimitExceeded)?;
            if actual > expected_size || actual > self.limits.max_download_bytes {
                return Err(InstallError::SizeLimitExceeded);
            }
            hasher.update(&chunk);
            file.write_all(&chunk)
                .await
                .map_err(|error| InstallError::io("writing partial download", error))?;
        }
        file.flush()
            .await
            .map_err(|error| InstallError::io("flushing partial download", error))?;
        let file = file.into_std().await;
        file.sync_all()
            .map_err(|error| InstallError::io("syncing partial download", error))?;
        if actual != expected_size {
            return Err(InstallError::SizeMismatch {
                expected: expected_size,
                actual,
            });
        }
        verify_digest(&hasher.finalize(), expected_sha256)
    }

    fn install_verified_archive(
        &self,
        manifest: &ServerManifest,
        installed_at: &str,
        archive: &Path,
        staging: &Path,
        install_source: InstallSource,
    ) -> Result<InstallResult, InstallError> {
        fs::create_dir(staging)
            .map_err(|error| InstallError::io("creating staging directory", error))?;
        reject_symlink_tree(staging)?;
        extract_archive(manifest, archive, staging, self.limits)?;
        self.promote_staging(manifest, installed_at, staging, install_source)
    }

    fn promote_staging(
        &self,
        manifest: &ServerManifest,
        installed_at: &str,
        staging: &Path,
        install_source: InstallSource,
    ) -> Result<InstallResult, InstallError> {
        let entrypoint = safe_relative_path(&manifest.files.entrypoint)?;
        let entrypoint_path = staging.join(&entrypoint);
        let metadata =
            fs::symlink_metadata(&entrypoint_path).map_err(|_| InstallError::EntrypointMissing)?;
        if !metadata.file_type().is_file() || metadata_has_reparse_point(&metadata) {
            return Err(InstallError::EntrypointMissing);
        }
        let canonical_staging = fs::canonicalize(staging)
            .map_err(|error| InstallError::io("validating staging directory", error))?;
        let canonical_entrypoint =
            fs::canonicalize(&entrypoint_path).map_err(|_| InstallError::EntrypointMissing)?;
        if !canonical_entrypoint.starts_with(&canonical_staging) {
            return Err(InstallError::UnsafeArchivePath);
        }
        validate_manifest_command_files(manifest, &canonical_staging)?;

        let destination = self
            .lsp_root
            .join("servers")
            .join(&manifest.id)
            .join(&manifest.version)
            .join(&manifest.platform);
        let parent = destination
            .parent()
            .ok_or(InstallError::UnsafeArchivePath)?;
        fs::create_dir_all(parent)
            .map_err(|error| InstallError::io("creating version directory", error))?;
        reject_symlink_tree(parent)?;
        if destination.exists() {
            return Err(InstallError::InstallConflict);
        }
        fs::rename(staging, &destination)
            .map_err(|error| InstallError::io("promoting staged installation", error))?;
        let result =
            self.persist_installed(manifest, installed_at, &destination, install_source, false);
        if result.is_err() {
            // Promotion is not active until the index commit succeeds.
            let _ = fs::remove_dir_all(&destination);
        }
        result
    }

    fn index_path(&self) -> PathBuf {
        self.lsp_root.join(INDEX_FILE)
    }

    fn ensure_index_path_safe(&self) -> Result<(), InstallError> {
        reject_symlink_tree(&self.lsp_root)?;
        match fs::symlink_metadata(self.index_path()) {
            Ok(metadata) if metadata_has_reparse_point(&metadata) => {
                Err(InstallError::UnsafeArchivePath)
            }
            Ok(metadata) if !metadata.file_type().is_file() => Err(InstallError::IndexCorrupt),
            Ok(_) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(InstallError::io("checking installed index", error)),
        }
    }

    fn read_index(&self) -> Result<InstalledServerIndex, InstallError> {
        self.ensure_index_path_safe()?;
        match fs::read_to_string(self.index_path()) {
            Ok(json) => {
                InstalledServerIndex::from_json(&json).map_err(|_| InstallError::IndexCorrupt)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Ok(InstalledServerIndex::default())
            }
            Err(error) => Err(InstallError::io("reading installed index", error)),
        }
    }

    fn write_index(&self, index: &InstalledServerIndex) -> Result<(), InstallError> {
        self.ensure_index_path_safe()?;
        index.validate().map_err(|_| InstallError::IndexCorrupt)?;
        let json = index.to_json().map_err(|_| InstallError::IndexCorrupt)?;
        atomic_write(&self.index_path(), json.as_bytes())
            .map_err(|error| InstallError::io("committing installed index", error))
    }

    fn expected_destination(&self, manifest: &ServerManifest) -> Result<PathBuf, InstallError> {
        manifest
            .validate_for_install()
            .map_err(|error| InstallError::InvalidManifest(error.to_string()))?;
        let (servers_root, _canonical_servers) = self.managed_servers_root()?;
        let destination = self
            .lsp_root
            .join("servers")
            .join(&manifest.id)
            .join(&manifest.version)
            .join(&manifest.platform);
        if !destination.starts_with(&servers_root) {
            return Err(InstallError::UnsafeArchivePath);
        }
        Ok(destination)
    }

    fn managed_servers_root(&self) -> Result<(PathBuf, PathBuf), InstallError> {
        let servers_root = self.lsp_root.join("servers");
        let servers_metadata = fs::symlink_metadata(&servers_root)
            .map_err(|error| InstallError::io("checking managed server root", error))?;
        if metadata_has_reparse_point(&servers_metadata) || !servers_metadata.is_dir() {
            return Err(InstallError::UnsafeArchivePath);
        }
        let canonical_lsp_root = fs::canonicalize(&self.lsp_root)
            .map_err(|error| InstallError::io("validating installer root", error))?;
        let canonical_servers = fs::canonicalize(&servers_root)
            .map_err(|error| InstallError::io("validating managed server root", error))?;
        if !canonical_servers.starts_with(&canonical_lsp_root) {
            return Err(InstallError::UnsafeArchivePath);
        }
        Ok((servers_root, canonical_servers))
    }

    /// Validate only the removal boundary. Catalog metadata and entrypoint
    /// drift make a status `needs_reinstall`, but must not strand an otherwise
    /// safe app-owned directory. Path escape and symlinked trees remain hard
    /// failures.
    fn safe_removal_destination(&self, server: &InstalledServer) -> Result<PathBuf, InstallError> {
        server
            .validate()
            .map_err(|error| InstallError::MetadataMismatch(error.to_string()))?;
        let (_servers_root, canonical_servers) = self.managed_servers_root()?;
        let destination = canonical_servers
            .join(&server.manifest_id)
            .join(&server.version)
            .join(&server.platform);
        if !destination.starts_with(&canonical_servers) {
            return Err(InstallError::UnsafeArchivePath);
        }

        let recorded = PathBuf::from(&server.installed_path);
        if !is_same_path(&recorded, &destination) {
            return Err(InstallError::MetadataMismatch(
                "recorded install path is not the managed canonical directory".into(),
            ));
        }

        // All existing components are checked before canonicalization, so a
        // symlink cannot redirect the removal outside the immutable root.
        reject_symlink_tree(&destination)?;
        match fs::symlink_metadata(&destination) {
            Ok(metadata) if metadata_has_reparse_point(&metadata) => {
                return Err(InstallError::UnsafeArchivePath);
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(InstallError::UnsafeArchivePath);
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(destination),
            Err(error) => {
                return Err(InstallError::io("checking managed installation", error));
            }
        }
        let canonical_destination = fs::canonicalize(&destination)
            .map_err(|error| InstallError::io("validating managed installation", error))?;
        if !canonical_destination.starts_with(&canonical_servers)
            || !is_same_path(&canonical_destination, &destination)
        {
            return Err(InstallError::UnsafeArchivePath);
        }
        validate_install_tree(&canonical_destination)?;
        Ok(destination)
    }

    fn validate_installed_entry(
        &self,
        manifest: &ServerManifest,
        server: &InstalledServer,
    ) -> Result<(), InstallError> {
        manifest
            .validate_for_install()
            .map_err(|error| InstallError::InvalidManifest(error.to_string()))?;
        server
            .validate()
            .map_err(|error| InstallError::MetadataMismatch(error.to_string()))?;
        if server.manifest_id != manifest.id
            || server.version != manifest.version
            || server.platform != manifest.platform
            || server.sha256 != manifest.artifact.sha256
            || server.source_url != manifest.source_url
            || server.license != manifest.license
            || server.artifact_url != manifest.artifact.url
            || server.entrypoint != manifest.files.entrypoint
            || server.runtime != manifest.runtime
            || server.package_lock_sha256 != manifest.files.package_lock_sha256
        {
            return Err(InstallError::MetadataMismatch(
                "installed metadata differs from the reviewed catalog".into(),
            ));
        }

        let destination = self.expected_destination(manifest)?;
        reject_symlink_tree(&self.lsp_root)?;
        let destination_metadata = fs::symlink_metadata(&destination)
            .map_err(|_| InstallError::MetadataMismatch("managed directory is missing".into()))?;
        if metadata_has_reparse_point(&destination_metadata) || !destination_metadata.is_dir() {
            return Err(InstallError::UnsafeArchivePath);
        }
        let canonical_destination = fs::canonicalize(&destination)
            .map_err(|_| InstallError::MetadataMismatch("managed directory is missing".into()))?;
        let canonical_servers = fs::canonicalize(self.lsp_root.join("servers"))
            .map_err(|_| InstallError::MetadataMismatch("managed server root is missing".into()))?;
        if !canonical_destination.starts_with(&canonical_servers) {
            return Err(InstallError::UnsafeArchivePath);
        }
        let recorded = PathBuf::from(&server.installed_path);
        let canonical_recorded = fs::canonicalize(&recorded).map_err(|_| {
            InstallError::MetadataMismatch("recorded install path is missing".into())
        })?;
        if !is_same_path(&canonical_recorded, &canonical_destination) {
            return Err(InstallError::MetadataMismatch(
                "recorded install path is not the managed canonical directory".into(),
            ));
        }
        validate_install_tree(&canonical_destination)?;

        let entrypoint = safe_relative_path(&manifest.files.entrypoint)?;
        let entrypoint_path = canonical_destination.join(entrypoint);
        let entrypoint_metadata = fs::symlink_metadata(&entrypoint_path)
            .map_err(|_| InstallError::MetadataMismatch("entrypoint is missing".into()))?;
        if !entrypoint_metadata.file_type().is_file()
            || metadata_has_reparse_point(&entrypoint_metadata)
        {
            return Err(InstallError::MetadataMismatch(
                "entrypoint is not a regular file".into(),
            ));
        }
        let canonical_entrypoint = fs::canonicalize(&entrypoint_path)
            .map_err(|_| InstallError::MetadataMismatch("entrypoint is missing".into()))?;
        if !canonical_entrypoint.starts_with(&canonical_destination) {
            return Err(InstallError::UnsafeArchivePath);
        }
        validate_manifest_command_files(manifest, &canonical_destination)?;
        Ok(())
    }

    fn persist_installed(
        &self,
        manifest: &ServerManifest,
        installed_at: &str,
        destination: &Path,
        install_source: InstallSource,
        already_installed: bool,
    ) -> Result<InstallResult, InstallError> {
        let destination = fs::canonicalize(destination)
            .map_err(|error| InstallError::io("validating install destination", error))?;
        let server = InstalledServer {
            manifest_id: manifest.id.clone(),
            version: manifest.version.clone(),
            platform: manifest.platform.clone(),
            sha256: manifest.artifact.sha256.clone(),
            source_url: manifest.source_url.clone(),
            license: manifest.license.clone(),
            artifact_url: manifest.artifact.url.clone(),
            installed_path: destination.to_string_lossy().into_owned(),
            entrypoint: manifest.files.entrypoint.clone(),
            runtime: manifest.runtime.clone(),
            installed_at: installed_at.to_string(),
            package_lock_sha256: manifest.files.package_lock_sha256.clone(),
            install_source,
            last_verified_at: Some(current_rfc3339()),
        };
        server
            .validate()
            .map_err(|error| InstallError::InvalidManifest(error.to_string()))?;
        let mut index = self.read_index()?;
        index.servers.retain(|entry| {
            !(entry.manifest_id == server.manifest_id
                && entry.version == server.version
                && entry.platform == server.platform)
        });
        index.servers.push(server.clone());
        self.write_index(&index)?;
        Ok(InstallResult {
            server,
            already_installed,
        })
    }
}

/// Every reviewed command, including language-specific overrides, must point
/// to a regular file inside the immutable install tree. This is checked before
/// promotion and again at every status/start boundary so a missing HTML/CSS
/// server cannot silently fall back to another language's executable.
fn validate_manifest_command_files(
    manifest: &ServerManifest,
    root: &Path,
) -> Result<(), InstallError> {
    let mut commands = Vec::with_capacity(manifest.languages.len() + 1);
    commands.push(&manifest.command);
    commands.extend(
        manifest
            .languages
            .iter()
            .filter_map(|language| language.command.as_ref()),
    );
    for command in commands {
        let relative = safe_relative_path(&command.executable)?;
        let path = root.join(relative);
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| InstallError::MetadataMismatch("reviewed command is missing".into()))?;
        if metadata_has_reparse_point(&metadata) {
            return Err(InstallError::UnsafeArchivePath);
        }
        if !metadata.is_file() {
            return Err(InstallError::MetadataMismatch(
                "reviewed command is not a regular file".into(),
            ));
        }
        reject_hard_link(&path)?;
        let canonical = fs::canonicalize(&path)
            .map_err(|_| InstallError::MetadataMismatch("reviewed command is missing".into()))?;
        if !canonical.starts_with(root) {
            return Err(InstallError::UnsafeArchivePath);
        }
    }
    Ok(())
}

fn unique_nonce() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let sequence = NONCE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{}-{nanos}-{sequence}", std::process::id())
}

fn current_rfc3339() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let days = seconds / 86_400;
    let day_seconds = seconds % 86_400;
    let (year, month, day) = civil_date_from_days(days as i64);
    let hour = day_seconds / 3_600;
    let minute = (day_seconds % 3_600) / 60;
    let second = day_seconds % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

// Inverse of the proleptic Gregorian day-count formula. Keeping this tiny
// formatter local avoids adding a date dependency solely for an install
// metadata timestamp.
fn civil_date_from_days(days_since_1970: i64) -> (i64, u32, u32) {
    let shifted = days_since_1970 + 719_468;
    let era = if shifted >= 0 {
        shifted / 146_097
    } else {
        (shifted - 146_096) / 146_097
    };
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = month_part + if month_part < 10 { 3 } else { -9 };
    let year = year + if month <= 2 { 1 } else { 0 };
    (year, month as u32, day as u32)
}

fn validate_external_archive(archive: &Path) -> Result<(), InstallError> {
    validate_absolute_clean_path(archive)?;
    reject_symlink_tree(archive)?;
    let metadata = fs::symlink_metadata(archive)
        .map_err(|error| InstallError::io("reading selected archive", error))?;
    if metadata_has_reparse_point(&metadata) || !metadata.file_type().is_file() {
        return Err(InstallError::InvalidArchive(
            "selected archive is not a regular file".into(),
        ));
    }
    reject_hard_link(archive)?;
    Ok(())
}

fn validate_node_archive_selection_path(archive: &Path) -> Result<(), InstallError> {
    validate_external_archive(archive)?;
    let is_tgz = archive
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("tgz"));
    if !is_tgz {
        return Err(InstallError::DependencyLock(
            "local Node archives must use the .tgz extension".into(),
        ));
    }
    Ok(())
}

fn validate_absolute_clean_path(path: &Path) -> Result<(), InstallError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
        || path
            .as_os_str()
            .to_string_lossy()
            .split(['/', '\\'])
            .any(|component| matches!(component, "." | ".."))
    {
        return Err(InstallError::UnsafeArchivePath);
    }
    Ok(())
}

struct ArchiveDigest {
    size: u64,
    sha256: [u8; 32],
    sha512: [u8; 64],
}

fn hash_archive(archive: &Path, max_bytes: u64) -> Result<ArchiveDigest, InstallError> {
    validate_external_archive(archive)?;
    let mut file =
        File::open(archive).map_err(|error| InstallError::io("opening selected archive", error))?;
    let mut sha256 = Sha256::new();
    let mut sha512 = Sha512::new();
    let mut size = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| InstallError::io("hashing selected archive", error))?;
        if read == 0 {
            break;
        }
        size = size
            .checked_add(read as u64)
            .ok_or(InstallError::SizeLimitExceeded)?;
        if size > max_bytes {
            return Err(InstallError::SizeLimitExceeded);
        }
        sha256.update(&buffer[..read]);
        sha512.update(&buffer[..read]);
    }
    Ok(ArchiveDigest {
        size,
        sha256: sha256.finalize().into(),
        sha512: sha512.finalize().into(),
    })
}

fn verify_archive_file(
    manifest: &ServerManifest,
    archive: &Path,
    max_download_bytes: u64,
) -> Result<(), InstallError> {
    let expected = manifest
        .artifact
        .size_bytes
        .ok_or_else(|| InstallError::InvalidManifest("artifact size is required".to_string()))?;
    validate_external_archive(archive)?;
    let metadata = fs::symlink_metadata(archive)
        .map_err(|error| InstallError::io("reading artifact metadata", error))?;
    if !metadata.file_type().is_file() || metadata.len() > max_download_bytes {
        return Err(InstallError::SizeLimitExceeded);
    }
    if metadata.len() != expected {
        return Err(InstallError::SizeMismatch {
            expected,
            actual: metadata.len(),
        });
    }
    let mut file =
        File::open(archive).map_err(|error| InstallError::io("opening artifact", error))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| InstallError::io("hashing artifact", error))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    verify_digest(&hasher.finalize(), &manifest.artifact.sha256)
}

fn verify_node_package_archive(
    package: &NodePackageLock,
    archive: &Path,
    limits: InstallLimits,
) -> Result<(), InstallError> {
    validate_external_archive(archive)?;
    let metadata = fs::symlink_metadata(archive)
        .map_err(|error| InstallError::io("reading Node package metadata", error))?;
    if !metadata.file_type().is_file() {
        return Err(InstallError::InvalidArchive(format!(
            "{} archive is not a regular file",
            package.name
        )));
    }
    if metadata.len() > limits.max_download_bytes {
        return Err(InstallError::SizeLimitExceeded);
    }
    if metadata.len() != package.size_bytes {
        return Err(InstallError::SizeMismatch {
            expected: package.size_bytes,
            actual: metadata.len(),
        });
    }
    let mut file = File::open(archive)
        .map_err(|error| InstallError::io("opening Node package archive", error))?;
    let mut sha256 = Sha256::new();
    let mut sha512 = Sha512::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| InstallError::io("hashing Node package archive", error))?;
        if read == 0 {
            break;
        }
        sha256.update(&buffer[..read]);
        sha512.update(&buffer[..read]);
    }
    verify_digest(&sha256.finalize(), &package.sha256)?;
    verify_integrity(&sha512.finalize(), &package.integrity)
}

fn node_package_supported(package: &NodePackageLock, platform: &str) -> bool {
    if let Some(os) = &package.os {
        let target = if platform.starts_with("windows-") {
            "win32"
        } else if platform.starts_with("linux-") {
            "linux"
        } else if platform.starts_with("macos-") {
            "darwin"
        } else {
            platform
        };
        if !os.iter().any(|value| value == target) {
            return false;
        }
    }
    if let Some(cpu) = &package.cpu {
        let target = if platform.ends_with("x86_64") {
            "x64"
        } else {
            "arm64"
        };
        if !cpu.iter().any(|value| value == target) {
            return false;
        }
    }
    true
}

fn extract_node_package(
    package: &NodePackageLock,
    archive: &Path,
    staging: &Path,
    install_path: &Path,
    limits: InstallLimits,
) -> Result<(usize, u64), InstallError> {
    let file = File::open(archive)
        .map_err(|error| InstallError::io("opening Node package archive", error))?;
    let decoder = GzDecoder::new(BufReader::new(file));
    let mut tar = tar::Archive::new(decoder);
    let entries = tar
        .entries()
        .map_err(|error| InstallError::InvalidArchive(error.to_string()))?;
    let mut count = 0_usize;
    let mut total = 0_u64;
    let mut saw_package_json = false;
    for entry in entries {
        count = count
            .checked_add(1)
            .ok_or(InstallError::SizeLimitExceeded)?;
        if count > limits.max_archive_entries {
            return Err(InstallError::SizeLimitExceeded);
        }
        let mut entry = entry.map_err(|error| InstallError::InvalidArchive(error.to_string()))?;
        let entry_type = entry.header().entry_type();
        if !matches!(entry_type, EntryType::Regular | EntryType::Directory) {
            return Err(InstallError::UnsupportedArchiveEntry);
        }
        let raw_path = entry.path().map_err(|_| InstallError::UnsafeArchivePath)?;
        let raw_path = raw_path.to_str().ok_or(InstallError::UnsafeArchivePath)?;
        let archive_path = strict_archive_path(raw_path)?;
        enforce_archive_depth(&archive_path, limits)?;
        let Some(relative) = strip_archive_root(&archive_path, "package")? else {
            continue;
        };
        if relative.as_os_str().is_empty() {
            continue;
        }
        enforce_archive_depth(&relative, limits)?;
        if relative
            .components()
            .any(|component| matches!(component, Component::Normal(name) if name == "node_modules"))
        {
            return Err(InstallError::UnsafeArchivePath);
        }
        let destination = checked_destination(staging, &install_path.join(&relative))?;
        if entry_type == EntryType::Directory {
            create_checked_dir_all(staging, &destination)?;
            continue;
        }
        let size = entry.size();
        total = total
            .checked_add(size)
            .ok_or(InstallError::SizeLimitExceeded)?;
        if total > limits.max_extracted_bytes {
            return Err(InstallError::SizeLimitExceeded);
        }
        if relative == Path::new("package.json") {
            saw_package_json = true;
        }
        write_new_entry(staging, &destination, &mut entry, size)?;
    }
    if !saw_package_json {
        return Err(InstallError::PackageJsonInvalid(format!(
            "{}@{} has no package.json",
            package.name, package.version
        )));
    }
    Ok((count, total))
}

fn sanitize_node_package_json(
    staging: &Path,
    install_path: &Path,
    package: &NodePackageLock,
) -> Result<(), InstallError> {
    let package_json = staging.join(install_path).join("package.json");
    let bytes = fs::read(&package_json)
        .map_err(|error| InstallError::io("reading Node package.json", error))?;
    let mut value: Value = serde_json::from_slice(&bytes).map_err(|error| {
        InstallError::PackageJsonInvalid(format!("{}@{}: {error}", package.name, package.version))
    })?;
    let object = value.as_object_mut().ok_or_else(|| {
        InstallError::PackageJsonInvalid(format!(
            "{}@{} package.json must be an object",
            package.name, package.version
        ))
    })?;
    let actual_name = object.get("name").and_then(Value::as_str);
    let actual_version = object.get("version").and_then(Value::as_str);
    if actual_name != Some(package.name.as_str())
        || actual_version != Some(package.version.as_str())
    {
        return Err(InstallError::NodePackageMismatch(format!(
            "expected {}@{}, got {actual_name:?}@{actual_version:?}",
            package.name, package.version
        )));
    }
    // Never run npm. Removing scripts from the staged metadata also prevents
    // a later accidental package-manager invocation from running lifecycle
    // hooks inside an otherwise immutable managed install.
    object.remove("scripts");
    let mut output = serde_json::to_vec_pretty(&value)
        .map_err(|error| InstallError::PackageJsonInvalid(error.to_string()))?;
    output.push(b'\n');
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&package_json)
        .map_err(|error| InstallError::io("sanitizing Node package.json", error))?;
    file.write_all(&output)
        .and_then(|_| file.flush())
        .and_then(|_| file.sync_all())
        .map_err(|error| InstallError::io("sanitizing Node package.json", error))
}

fn package_key(name: &str, version: &str) -> String {
    format!("{name}\u{001f}{version}")
}

fn node_lock_error(error: NodeLockError) -> InstallError {
    InstallError::DependencyLock(error.to_string())
}

fn verify_digest(actual: &[u8], expected_hex: &str) -> Result<(), InstallError> {
    let expected = decode_sha256(expected_hex)
        .ok_or_else(|| InstallError::InvalidManifest("artifact SHA-256 is invalid".to_string()))?;
    if bool::from(actual.ct_eq(expected.as_slice())) {
        Ok(())
    } else {
        Err(InstallError::DigestMismatch)
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn verify_integrity(actual: &[u8], expected_integrity: &str) -> Result<(), InstallError> {
    let encoded = expected_integrity.strip_prefix("sha512-").ok_or_else(|| {
        InstallError::DependencyLock("Node package SHA-512 integrity is invalid".into())
    })?;
    let expected = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()
        .filter(|digest| digest.len() == 64)
        .ok_or_else(|| {
            InstallError::DependencyLock("Node package SHA-512 integrity is invalid".into())
        })?;
    if bool::from(actual.ct_eq(expected.as_slice())) {
        Ok(())
    } else {
        Err(InstallError::DigestMismatch)
    }
}

fn decode_sha256(input: &str) -> Option<[u8; 32]> {
    if input.len() != 64 {
        return None;
    }
    let mut output = [0_u8; 32];
    for (index, pair) in input.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        output[index] = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
    }
    Some(output)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn extract_archive(
    manifest: &ServerManifest,
    archive: &Path,
    staging: &Path,
    limits: InstallLimits,
) -> Result<(), InstallError> {
    match manifest.artifact.kind {
        ArtifactKind::Zip => extract_zip(manifest, archive, staging, limits),
        ArtifactKind::NpmTarball => extract_tar_gz(manifest, archive, staging, limits),
    }
}

fn extract_zip(
    manifest: &ServerManifest,
    archive: &Path,
    staging: &Path,
    limits: InstallLimits,
) -> Result<(), InstallError> {
    let file =
        File::open(archive).map_err(|error| InstallError::io("opening zip artifact", error))?;
    let mut zip = zip::ZipArchive::new(BufReader::new(file))
        .map_err(|error| InstallError::InvalidArchive(error.to_string()))?;
    if zip.len() > limits.max_archive_entries {
        return Err(InstallError::SizeLimitExceeded);
    }
    let mut total = 0_u64;
    for index in 0..zip.len() {
        let mut entry = zip
            .by_index(index)
            .map_err(|error| InstallError::InvalidArchive(error.to_string()))?;
        if entry.is_symlink() || (!entry.is_file() && !entry.is_dir()) {
            return Err(InstallError::UnsupportedArchiveEntry);
        }
        let archive_path = strict_archive_path(entry.name())?;
        enforce_archive_depth(&archive_path, limits)?;
        let Some(relative) = strip_archive_root(&archive_path, &manifest.artifact.archive_root)?
        else {
            continue;
        };
        if relative.as_os_str().is_empty() {
            continue;
        }
        enforce_archive_depth(&relative, limits)?;
        let destination = checked_destination(staging, &relative)?;
        if entry.is_dir() {
            create_checked_dir_all(staging, &destination)?;
            continue;
        }
        let size = entry.size();
        total = total
            .checked_add(size)
            .ok_or(InstallError::SizeLimitExceeded)?;
        if total > limits.max_extracted_bytes {
            return Err(InstallError::SizeLimitExceeded);
        }
        write_new_entry(staging, &destination, &mut entry, size)?;
    }
    Ok(())
}

fn extract_tar_gz(
    manifest: &ServerManifest,
    archive: &Path,
    staging: &Path,
    limits: InstallLimits,
) -> Result<(), InstallError> {
    let file =
        File::open(archive).map_err(|error| InstallError::io("opening tar artifact", error))?;
    let decoder = GzDecoder::new(BufReader::new(file));
    let mut tar = tar::Archive::new(decoder);
    let entries = tar
        .entries()
        .map_err(|error| InstallError::InvalidArchive(error.to_string()))?;
    let mut count = 0_usize;
    let mut total = 0_u64;
    for entry in entries {
        count = count
            .checked_add(1)
            .ok_or(InstallError::SizeLimitExceeded)?;
        if count > limits.max_archive_entries {
            return Err(InstallError::SizeLimitExceeded);
        }
        let mut entry = entry.map_err(|error| InstallError::InvalidArchive(error.to_string()))?;
        let entry_type = entry.header().entry_type();
        if !matches!(entry_type, EntryType::Regular | EntryType::Directory) {
            return Err(InstallError::UnsupportedArchiveEntry);
        }
        let raw_path = entry.path().map_err(|_| InstallError::UnsafeArchivePath)?;
        let raw_path = raw_path.to_str().ok_or(InstallError::UnsafeArchivePath)?;
        let archive_path = strict_archive_path(raw_path)?;
        enforce_archive_depth(&archive_path, limits)?;
        let Some(relative) = strip_archive_root(&archive_path, &manifest.artifact.archive_root)?
        else {
            continue;
        };
        if relative.as_os_str().is_empty() {
            continue;
        }
        enforce_archive_depth(&relative, limits)?;
        let destination = checked_destination(staging, &relative)?;
        if entry_type == EntryType::Directory {
            create_checked_dir_all(staging, &destination)?;
            continue;
        }
        let size = entry.size();
        total = total
            .checked_add(size)
            .ok_or(InstallError::SizeLimitExceeded)?;
        if total > limits.max_extracted_bytes {
            return Err(InstallError::SizeLimitExceeded);
        }
        write_new_entry(staging, &destination, &mut entry, size)?;
    }
    Ok(())
}

fn strict_archive_path(name: &str) -> Result<PathBuf, InstallError> {
    let name = if name.ends_with('/') {
        let trimmed = name
            .strip_suffix('/')
            .ok_or(InstallError::UnsafeArchivePath)?;
        if trimmed.ends_with('/') {
            return Err(InstallError::UnsafeArchivePath);
        }
        trimmed
    } else {
        name
    };
    if name.is_empty()
        || name.contains('\0')
        || name.contains('\\')
        || name.starts_with('/')
        || name
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
        || name
            .split('/')
            .next()
            .is_some_and(|component| component.contains(':'))
    {
        return Err(InstallError::UnsafeArchivePath);
    }
    safe_relative_path(name)
}

fn enforce_archive_depth(path: &Path, limits: InstallLimits) -> Result<(), InstallError> {
    let depth = path
        .components()
        .filter(|component| matches!(component, Component::Normal(_)))
        .count();
    if depth > limits.max_archive_depth {
        return Err(InstallError::ArchiveDepthExceeded);
    }
    Ok(())
}

fn safe_relative_path(path: &str) -> Result<PathBuf, InstallError> {
    let path = PathBuf::from(path);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(InstallError::UnsafeArchivePath);
    }
    Ok(path)
}

fn strip_archive_root(path: &Path, archive_root: &str) -> Result<Option<PathBuf>, InstallError> {
    if archive_root.is_empty() {
        return Ok(Some(path.to_path_buf()));
    }
    let root = safe_relative_path(archive_root)?;
    if path == root {
        return Ok(None);
    }
    path.strip_prefix(&root)
        .map(|relative| Some(relative.to_path_buf()))
        .map_err(|_| InstallError::UnsafeArchivePath)
}

fn checked_destination(staging: &Path, relative: &Path) -> Result<PathBuf, InstallError> {
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(InstallError::UnsafeArchivePath);
    }
    let destination = staging.join(relative);
    if !destination.starts_with(staging) {
        return Err(InstallError::UnsafeArchivePath);
    }
    Ok(destination)
}

fn create_checked_dir_all(staging: &Path, destination: &Path) -> Result<(), InstallError> {
    let relative = destination
        .strip_prefix(staging)
        .map_err(|_| InstallError::UnsafeArchivePath)?;
    let mut current = staging.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(InstallError::UnsafeArchivePath);
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata_has_reparse_point(&metadata) || !metadata.is_dir() => {
                return Err(InstallError::UnsafeArchivePath);
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&current)
                    .map_err(|error| InstallError::io("creating archive directory", error))?;
            }
            Err(error) => {
                return Err(InstallError::io("checking archive directory", error));
            }
        }
    }
    Ok(())
}

fn write_new_entry(
    staging: &Path,
    destination: &Path,
    reader: &mut impl Read,
    expected_size: u64,
) -> Result<(), InstallError> {
    let parent = destination
        .parent()
        .ok_or(InstallError::UnsafeArchivePath)?;
    create_checked_dir_all(staging, parent)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| InstallError::io("creating archive entry", error))?;
    let copied = io::copy(&mut reader.take(expected_size.saturating_add(1)), &mut file)
        .map_err(|error| InstallError::io("extracting archive entry", error))?;
    if copied != expected_size {
        return Err(InstallError::InvalidArchive(
            "entry size mismatch".to_string(),
        ));
    }
    file.flush()
        .and_then(|_| file.sync_all())
        .map_err(|error| InstallError::io("syncing archive entry", error))
}

fn reject_symlink_tree(path: &Path) -> Result<(), InstallError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if matches!(component, Component::Prefix(_) | Component::RootDir) {
            // On Windows, probing the Prefix component alone (for example
            // C:) is not a valid filesystem path. Check only after the
            // drive/UNC root has been joined with a normal component.
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata_has_reparse_point(&metadata) => {
                return Err(InstallError::UnsafeArchivePath);
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(InstallError::io("validating installer path", error));
            }
        }
    }
    Ok(())
}

fn validate_install_tree(root: &Path) -> Result<(), InstallError> {
    let metadata = fs::symlink_metadata(root)
        .map_err(|_| InstallError::MetadataMismatch("managed directory is missing".into()))?;
    if metadata_has_reparse_point(&metadata) || !metadata.is_dir() {
        return Err(InstallError::UnsafeArchivePath);
    }
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)
            .map_err(|error| InstallError::io("reading managed install tree", error))?
        {
            let entry =
                entry.map_err(|error| InstallError::io("reading managed install entry", error))?;
            let metadata = fs::symlink_metadata(entry.path())
                .map_err(|error| InstallError::io("checking managed install entry", error))?;
            if metadata_has_reparse_point(&metadata) {
                return Err(InstallError::UnsafeArchivePath);
            }
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_file() {
                reject_hard_link(&entry.path())?;
            } else {
                return Err(InstallError::UnsupportedArchiveEntry);
            }
        }
    }
    Ok(())
}

fn metadata_has_reparse_point(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Managed trees are immutable regular-file trees. A regular file with more
/// than one link could alias content outside the tree and make an otherwise
/// local removal mutate unrelated user data, so all platforms fail closed
/// unless the platform can prove the link count is exactly one.
fn reject_hard_link(path: &Path) -> Result<(), InstallError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        let metadata = fs::symlink_metadata(path)
            .map_err(|error| InstallError::io("checking managed file links", error))?;
        if metadata.nlink() != 1 {
            return Err(InstallError::UnsafeArchivePath);
        }
        Ok(())
    }

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::Storage::FileSystem::{
            CreateFileW, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
            FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
            OPEN_EXISTING,
        };

        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let handle = unsafe {
            CreateFileW(
                PCWSTR(wide.as_ptr()),
                FILE_READ_ATTRIBUTES.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
                None,
            )
        }
        .map_err(|_| InstallError::UnsafeArchivePath)?;
        let mut information = BY_HANDLE_FILE_INFORMATION::default();
        let result = unsafe { GetFileInformationByHandle(handle, &mut information) };
        let close_result = unsafe { CloseHandle(handle) };
        if result.is_err()
            || close_result.is_err()
            || information.nNumberOfLinks != 1
            || information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
        {
            return Err(InstallError::UnsafeArchivePath);
        }
        Ok(())
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Err(InstallError::UnsafeArchivePath)
    }
}

fn is_same_path(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn manifest_key(manifest_id: &str, version: &str, platform: &str) -> String {
    format!("{manifest_id}\u{001f}{version}\u{001f}{platform}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::catalog::{
        Artifact, CommandSpec, LanguageSupport, ManifestFiles, RuntimeKind, RuntimeSpec,
        WINDOWS_X86_64_PLATFORM,
    };
    use crate::lsp::node_lock::NodePackageRef;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Cursor;
    use tar::{Builder, Header};
    use tempfile::TempDir;
    use zip::write::SimpleFileOptions;

    fn digest(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn integrity(bytes: &[u8]) -> String {
        format!(
            "sha512-{}",
            base64::engine::general_purpose::STANDARD.encode(Sha512::digest(bytes))
        )
    }

    fn manifest(bytes: &[u8], entrypoint: &str) -> ServerManifest {
        ServerManifest {
            id: "fixture-server".to_string(),
            version: "1.2.3".to_string(),
            platform: WINDOWS_X86_64_PLATFORM.to_string(),
            languages: vec![LanguageSupport {
                language_id: "fixture".to_string(),
                extensions: vec![".fixture".to_string()],
                command: None,
            }],
            source_url: "https://example.com/source".to_string(),
            license: "MIT".to_string(),
            artifact: Artifact {
                kind: ArtifactKind::Zip,
                url: "https://example.com/server.zip".to_string(),
                sha256: digest(bytes),
                size_bytes: Some(bytes.len() as u64),
                allowed_redirect_hosts: Vec::new(),
                archive_root: String::new(),
            },
            runtime: RuntimeSpec {
                kind: RuntimeKind::Native,
                executable: entrypoint.to_string(),
                min_version: None,
            },
            command: CommandSpec {
                executable: entrypoint.to_string(),
                args: vec![],
            },
            files: ManifestFiles {
                entrypoint: entrypoint.to_string(),
                package_lock_sha256: None,
            },
            capabilities_hint: None,
            generated_at: "2026-08-13T00:00:00Z".to_string(),
        }
    }

    fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut output = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut output);
            for (name, contents) in entries {
                writer
                    .start_file(*name, SimpleFileOptions::default())
                    .unwrap();
                writer.write_all(contents).unwrap();
            }
            writer.finish().unwrap();
        }
        output.into_inner()
    }

    fn zip_with_directory(directory: &str, file: &str, contents: &[u8]) -> Vec<u8> {
        let mut output = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut output);
            writer
                .add_directory(directory, SimpleFileOptions::default())
                .unwrap();
            writer
                .start_file(file, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(contents).unwrap();
            writer.finish().unwrap();
        }
        output.into_inner()
    }

    fn write_fixture(temp: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
        let path = temp.path().join(name);
        fs::write(&path, bytes).unwrap();
        path
    }

    fn tar_gz_file(path: &str, contents: &[u8]) -> Vec<u8> {
        tar_gz_files(&[(path, contents)])
    }

    fn tar_gz_files(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let encoder = GzEncoder::new(Vec::new(), Compression::default());
        let mut builder = Builder::new(encoder);
        for (path, contents) in entries {
            let mut header = Header::new_gnu();
            header.set_entry_type(EntryType::Regular);
            header.set_mode(0o644);
            header.set_size(contents.len() as u64);
            header.set_cksum();
            builder
                .append_data(&mut header, *path, Cursor::new(*contents))
                .unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    fn tar_gz_symlink(path: &str, target: &str) -> Vec<u8> {
        let encoder = GzEncoder::new(Vec::new(), Compression::default());
        let mut builder = Builder::new(encoder);
        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Symlink);
        header.set_mode(0o777);
        header.set_size(0);
        header.set_link_name(target).unwrap();
        header.set_cksum();
        builder.append_data(&mut header, path, io::empty()).unwrap();
        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn exact_archive_is_promoted_and_indexed_atomically() {
        let archive = zip(&[("server.exe", b"fixture")]);
        let manifest = manifest(&archive, "server.exe");
        let temp = TempDir::new().unwrap();
        let archive_path = write_fixture(&temp, "fixture.zip", &archive);
        let installer = ManagedInstaller::new(temp.path().join("data")).unwrap();

        let result = installer
            .install_archive(&manifest, "1.2.3", "2026-08-13T01:02:03Z", &archive_path)
            .unwrap();

        assert!(!result.already_installed);
        assert_eq!(result.server.install_source, InstallSource::LocalArchive);
        assert!(result.server.last_verified_at.is_some());
        assert_eq!(
            fs::read(Path::new(&result.server.installed_path).join("server.exe")).unwrap(),
            b"fixture"
        );
        let cache_path = installer
            .archive_cache_path(&manifest.artifact.sha256, manifest.artifact.kind)
            .unwrap();
        assert_eq!(fs::read(cache_path).unwrap(), archive);
        assert!(installer.cached_archive(&manifest).unwrap().is_some());
        let index = InstalledServerIndex::from_json(
            &fs::read_to_string(installer.lsp_root().join(INDEX_FILE)).unwrap(),
        )
        .unwrap();
        assert_eq!(index.servers, vec![result.server]);
        assert!(fs::read_dir(installer.lsp_root().join("staging"))
            .unwrap()
            .next()
            .is_none());
    }

    #[tokio::test]
    async fn verified_archive_cache_is_reused_without_network() {
        let archive = zip(&[("server.exe", b"offline fixture")]);
        let mut manifest = manifest(&archive, "server.exe");
        // This URL is intentionally unreachable. A cache miss would try the
        // network and fail; a verified cache hit must complete offline.
        manifest.artifact.url = "https://127.0.0.1:9/unreachable.zip".into();
        let temp = TempDir::new().unwrap();
        let archive_path = write_fixture(&temp, "selected.zip", &archive);
        let installer = ManagedInstaller::new(temp.path().join("data")).unwrap();

        installer
            .install_archive(&manifest, "1.2.3", "2026-08-13T01:02:03Z", &archive_path)
            .unwrap();
        installer
            .uninstall_catalog(&manifest.id, &manifest.version, &manifest.platform)
            .unwrap();

        let partial = installer.lsp_root().join("downloads/offline.part");
        let (cached, source) = installer
            .prepare_archive(&manifest, &partial)
            .await
            .unwrap();
        assert_eq!(source, InstallSource::ArchiveCache);
        assert!(cached.is_file());
        assert!(!partial.exists());

        let result = installer
            .install(&manifest, "1.2.3", "2026-08-13T01:02:03Z")
            .await
            .unwrap();
        assert_eq!(result.server.install_source, InstallSource::ArchiveCache);
        assert_eq!(
            fs::read(Path::new(&result.server.installed_path).join("server.exe")).unwrap(),
            b"offline fixture"
        );
    }

    #[test]
    fn local_archive_selection_is_not_persisted_or_reflected() {
        let archive = zip(&[("server.exe", b"fixture")]);
        let manifest = manifest(&archive, "server.exe");
        let temp = TempDir::new().unwrap();
        let archive_path = write_fixture(&temp, "user-private-selection.zip", &archive);
        let installer = ManagedInstaller::new(temp.path().join("data")).unwrap();

        installer
            .install_archive(&manifest, "1.2.3", "2026-08-13T01:02:03Z", &archive_path)
            .unwrap();

        let index = fs::read_to_string(installer.lsp_root().join(INDEX_FILE)).unwrap();
        assert!(!index.contains("user-private-selection.zip"));
    }

    #[test]
    fn wrong_size_digest_and_version_never_promote() {
        let archive = zip(&[("server.exe", b"fixture")]);
        let temp = TempDir::new().unwrap();
        let archive_path = write_fixture(&temp, "fixture.zip", &archive);
        let installer = ManagedInstaller::new(temp.path().join("data")).unwrap();

        let mut wrong_size = manifest(&archive, "server.exe");
        wrong_size.artifact.size_bytes = Some(archive.len() as u64 + 1);
        assert!(matches!(
            installer.install_archive(&wrong_size, "1.2.3", "2026-08-13T01:02:03Z", &archive_path),
            Err(InstallError::SizeMismatch { .. })
        ));
        let mut wrong_digest = manifest(&archive, "server.exe");
        wrong_digest.artifact.sha256 = "00".repeat(32);
        assert!(matches!(
            installer.install_archive(
                &wrong_digest,
                "1.2.3",
                "2026-08-13T01:02:03Z",
                &archive_path
            ),
            Err(InstallError::DigestMismatch)
        ));
        assert!(matches!(
            installer.install_archive(
                &manifest(&archive, "server.exe"),
                "latest",
                "2026-08-13T01:02:03Z",
                &archive_path
            ),
            Err(InstallError::VersionMismatch)
        ));
        assert!(!installer.lsp_root().join(INDEX_FILE).exists());
    }

    #[test]
    fn unsafe_paths_and_missing_entrypoint_are_rejected() {
        for unsafe_name in [
            "../escape",
            "/absolute",
            "C:/drive",
            "a\\b",
            "a//b",
            "a/./b",
        ] {
            assert!(matches!(
                strict_archive_path(unsafe_name),
                Err(InstallError::UnsafeArchivePath)
            ));
        }
        let archive = zip(&[("other.exe", b"fixture")]);
        let temp = TempDir::new().unwrap();
        let archive_path = write_fixture(&temp, "fixture.zip", &archive);
        let installer = ManagedInstaller::new(temp.path().join("data")).unwrap();
        assert!(matches!(
            installer.install_archive(
                &manifest(&archive, "server.exe"),
                "1.2.3",
                "2026-08-13T01:02:03Z",
                &archive_path
            ),
            Err(InstallError::EntrypointMissing)
        ));
    }

    #[test]
    fn ordinary_zip_directory_entries_are_supported() {
        let archive = zip_with_directory("bin/", "bin/server.exe", b"fixture");
        let manifest = manifest(&archive, "bin/server.exe");
        let temp = TempDir::new().unwrap();
        let archive_path = write_fixture(&temp, "fixture.zip", &archive);
        let installer = ManagedInstaller::new(temp.path().join("data")).unwrap();

        let result = installer
            .install_archive(&manifest, "1.2.3", "2026-08-13T01:02:03Z", &archive_path)
            .unwrap();
        assert_eq!(
            fs::read(Path::new(&result.server.installed_path).join("bin/server.exe")).unwrap(),
            b"fixture"
        );
    }

    #[test]
    fn archive_root_is_stripped_and_outside_entries_are_rejected() {
        let archive = zip(&[("package/server.exe", b"fixture")]);
        let mut rooted_manifest = manifest(&archive, "server.exe");
        rooted_manifest.artifact.archive_root = "package".to_string();
        let temp = TempDir::new().unwrap();
        let archive_path = write_fixture(&temp, "fixture.zip", &archive);
        let installer = ManagedInstaller::new(temp.path().join("data")).unwrap();
        let result = installer
            .install_archive(
                &rooted_manifest,
                "1.2.3",
                "2026-08-13T01:02:03Z",
                &archive_path,
            )
            .unwrap();
        assert!(Path::new(&result.server.installed_path)
            .join("server.exe")
            .is_file());

        let mixed = zip(&[("package/server.exe", b"fixture"), ("outside.txt", b"bad")]);
        let mut mixed_manifest = manifest(&mixed, "server.exe");
        mixed_manifest.id = "mixed-server".to_string();
        mixed_manifest.artifact.archive_root = "package".to_string();
        let mixed_path = write_fixture(&temp, "mixed.zip", &mixed);
        assert!(matches!(
            installer.install_archive(
                &mixed_manifest,
                "1.2.3",
                "2026-08-13T01:02:03Z",
                &mixed_path
            ),
            Err(InstallError::UnsafeArchivePath)
        ));
    }

    #[test]
    fn extraction_limits_and_existing_conflicts_fail_closed() {
        let archive = zip(&[("server.exe", b"0123456789")]);
        let manifest = manifest(&archive, "server.exe");
        let temp = TempDir::new().unwrap();
        let archive_path = write_fixture(&temp, "fixture.zip", &archive);
        let installer = ManagedInstaller::with_limits(
            temp.path().join("data"),
            InstallLimits {
                max_download_bytes: archive.len() as u64,
                max_extracted_bytes: 4,
                max_archive_entries: 1,
                max_archive_depth: DEFAULT_MAX_ARCHIVE_DEPTH,
            },
        )
        .unwrap();
        assert!(matches!(
            installer.install_archive(&manifest, "1.2.3", "2026-08-13T01:02:03Z", &archive_path),
            Err(InstallError::SizeLimitExceeded)
        ));

        let installer = ManagedInstaller::new(temp.path().join("other-data")).unwrap();
        let destination = installer
            .lsp_root()
            .join("servers/fixture-server/1.2.3/windows-x86_64");
        fs::create_dir_all(&destination).unwrap();
        assert!(matches!(
            installer.install_archive(&manifest, "1.2.3", "2026-08-13T01:02:03Z", &archive_path),
            Err(InstallError::InstallConflict)
        ));
    }

    #[test]
    fn archive_depth_bound_fails_before_promotion() {
        let archive = zip(&[("one/two/three/server.exe", b"fixture")]);
        let manifest = manifest(&archive, "one/two/three/server.exe");
        let temp = TempDir::new().unwrap();
        let archive_path = write_fixture(&temp, "deep.zip", &archive);
        let installer = ManagedInstaller::with_limits(
            temp.path().join("data"),
            InstallLimits {
                max_download_bytes: archive.len() as u64,
                max_extracted_bytes: 1024,
                max_archive_entries: 10,
                max_archive_depth: 2,
            },
        )
        .unwrap();

        assert!(matches!(
            installer.install_archive(&manifest, "1.2.3", "2026-08-13T01:02:03Z", &archive_path),
            Err(InstallError::ArchiveDepthExceeded)
        ));
        assert!(!installer
            .lsp_root()
            .join("servers/fixture-server/1.2.3/windows-x86_64")
            .exists());
        assert!(!installer.lsp_root().join(INDEX_FILE).exists());
    }

    #[test]
    fn failed_new_archive_preserves_previous_active_install_and_index() {
        let old_archive = zip(&[("server.exe", b"old fixture")]);
        let new_archive = zip(&[("server.exe", b"new fixture")]);
        let old_manifest = manifest(&old_archive, "server.exe");
        let new_manifest = manifest(&new_archive, "server.exe");
        let temp = TempDir::new().unwrap();
        let old_path = write_fixture(&temp, "old.zip", &old_archive);
        let new_path = write_fixture(&temp, "new.zip", &new_archive);
        let installer = ManagedInstaller::new(temp.path().join("data")).unwrap();
        let old_result = installer
            .install_archive(&old_manifest, "1.2.3", "2026-08-13T01:02:03Z", &old_path)
            .unwrap();
        let old_index = fs::read_to_string(installer.lsp_root().join(INDEX_FILE)).unwrap();

        assert!(matches!(
            installer.install_archive(&new_manifest, "1.2.3", "2026-08-13T01:02:03Z", &new_path),
            Err(InstallError::InstallConflict)
        ));
        assert_eq!(
            fs::read(Path::new(&old_result.server.installed_path).join("server.exe")).unwrap(),
            b"old fixture"
        );
        assert_eq!(
            fs::read_to_string(installer.lsp_root().join(INDEX_FILE)).unwrap(),
            old_index
        );
    }

    #[cfg(unix)]
    #[test]
    fn selected_archive_symlink_is_rejected_without_cache_or_install() {
        use std::os::unix::fs::symlink;

        let archive = zip(&[("server.exe", b"fixture")]);
        let manifest = manifest(&archive, "server.exe");
        let temp = TempDir::new().unwrap();
        let outside = write_fixture(&temp, "outside.zip", &archive);
        let selected = temp.path().join("selected.zip");
        symlink(&outside, &selected).unwrap();
        let installer = ManagedInstaller::new(temp.path().join("data")).unwrap();

        assert!(matches!(
            installer.install_archive(&manifest, "1.2.3", "2026-08-13T01:02:03Z", &selected),
            Err(InstallError::UnsafeArchivePath)
        ));
        assert!(!installer.lsp_root().join(INDEX_FILE).exists());
        assert!(fs::read_dir(installer.lsp_root().join("downloads/cache"))
            .unwrap()
            .next()
            .is_none());
    }

    #[cfg(unix)]
    #[test]
    fn cache_symlink_is_never_followed_or_replaced() {
        use std::os::unix::fs::symlink;

        let archive = zip(&[("server.exe", b"fixture")]);
        let manifest = manifest(&archive, "server.exe");
        let temp = TempDir::new().unwrap();
        let archive_path = write_fixture(&temp, "selected.zip", &archive);
        let outside = write_fixture(&temp, "outside.bin", b"outside");
        let installer = ManagedInstaller::new(temp.path().join("data")).unwrap();
        let cache_path = installer
            .archive_cache_path(&manifest.artifact.sha256, manifest.artifact.kind)
            .unwrap();
        symlink(&outside, &cache_path).unwrap();

        assert!(matches!(
            installer.install_archive(&manifest, "1.2.3", "2026-08-13T01:02:03Z", &archive_path),
            Err(InstallError::UnsafeArchivePath)
        ));
        assert_eq!(fs::read(&outside).unwrap(), b"outside");
        assert!(!installer.lsp_root().join(INDEX_FILE).exists());
        assert!(fs::symlink_metadata(cache_path)
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[test]
    fn selected_archive_requires_an_absolute_clean_regular_file() {
        let archive = zip(&[("server.exe", b"fixture")]);
        assert!(matches!(
            validate_external_archive(Path::new("selected.zip")),
            Err(InstallError::UnsafeArchivePath)
        ));

        let temp = TempDir::new().unwrap();
        let archive_path = write_fixture(&temp, "selected.zip", &archive);
        let cur_dir = temp.path().join(".").join("selected.zip");
        let parent_dir = temp.path().join("nested").join("..").join("selected.zip");
        assert!(matches!(
            validate_external_archive(&cur_dir),
            Err(InstallError::UnsafeArchivePath)
        ));
        assert!(matches!(
            validate_external_archive(&parent_dir),
            Err(InstallError::UnsafeArchivePath)
        ));
        assert!(validate_external_archive(&archive_path).is_ok());

        let directory = temp.path().join("directory.tgz");
        fs::create_dir(&directory).unwrap();
        assert!(matches!(
            validate_external_archive(&directory),
            Err(InstallError::InvalidArchive(_))
        ));

        #[cfg(unix)]
        {
            let hard_link = temp.path().join("hard-link.zip");
            fs::hard_link(&archive_path, &hard_link).unwrap();
            assert!(matches!(
                validate_external_archive(&hard_link),
                Err(InstallError::UnsafeArchivePath)
            ));
        }
    }

    #[cfg(unix)]
    #[test]
    fn status_fails_closed_on_a_symlinked_reviewed_cache() {
        use std::os::unix::fs::symlink;

        let manifest = initial_catalog()
            .into_iter()
            .find(|manifest| manifest.id == "rust-analyzer")
            .unwrap();
        let temp = TempDir::new().unwrap();
        let installer = ManagedInstaller::new(temp.path().join("data")).unwrap();
        let outside = write_fixture(&temp, "outside.bin", b"outside");
        let cache_path = installer
            .archive_cache_path(&manifest.artifact.sha256, manifest.artifact.kind)
            .unwrap();
        symlink(&outside, &cache_path).unwrap();

        assert!(matches!(
            installer.installed_status(),
            Err(InstallError::UnsafeArchivePath)
        ));
        assert_eq!(fs::read(outside).unwrap(), b"outside");
    }

    #[test]
    fn npm_tarball_root_is_stripped_but_links_are_rejected() {
        let archive = tar_gz_file("package/server.js", b"fixture");
        let mut tar_manifest = manifest(&archive, "server.js");
        tar_manifest.artifact.kind = ArtifactKind::NpmTarball;
        tar_manifest.artifact.archive_root = "package".to_string();
        let temp = TempDir::new().unwrap();
        let archive_path = write_fixture(&temp, "fixture.tgz", &archive);
        let installer = ManagedInstaller::new(temp.path().join("data")).unwrap();
        let result = installer
            .install_archive(
                &tar_manifest,
                "1.2.3",
                "2026-08-13T01:02:03Z",
                &archive_path,
            )
            .unwrap();
        assert_eq!(
            fs::read(Path::new(&result.server.installed_path).join("server.js")).unwrap(),
            b"fixture"
        );

        let linked = tar_gz_symlink("package/server.js", "../../outside");
        let mut linked_manifest = manifest(&linked, "server.js");
        linked_manifest.id = "linked-server".to_string();
        linked_manifest.artifact.kind = ArtifactKind::NpmTarball;
        linked_manifest.artifact.archive_root = "package".to_string();
        let linked_path = write_fixture(&temp, "linked.tgz", &linked);
        assert!(matches!(
            installer.install_archive(
                &linked_manifest,
                "1.2.3",
                "2026-08-13T01:02:03Z",
                &linked_path
            ),
            Err(InstallError::UnsupportedArchiveEntry)
        ));
    }

    #[test]
    fn node_fixture_installs_under_node_modules_without_lifecycle_scripts() {
        let package_json = br#"{
  "name": "fixture-server",
  "version": "1.2.3",
  "bin": { "fixture-server": "bin/server.js" },
  "dependencies": { "fixture-dependency": "0.1.0" },
  "scripts": { "install": "echo SHOULD_NOT_RUN > marker.txt" }
}"#;
        let archive = tar_gz_files(&[
            ("package/package.json", package_json),
            ("package/bin/server.js", b"fixture"),
        ]);
        let dependency_archive = tar_gz_files(&[
            (
                "package/package.json",
                br#"{
  "name": "fixture-dependency",
  "version": "0.1.0",
  "scripts": { "install": "echo SHOULD_NOT_RUN > dependency-marker.txt" }
}"#,
            ),
            ("package/lib/dependency.js", b"dependency"),
        ]);
        let temp = TempDir::new().unwrap();
        let archive_path = write_fixture(&temp, "fixture.tgz", &archive);
        let dependency_archive_path =
            write_fixture(&temp, "fixture-dependency.tgz", &dependency_archive);
        let package = NodePackageLock {
            name: "fixture-server".into(),
            version: "1.2.3".into(),
            path: "node_modules/fixture-server".into(),
            tarball: "https://registry.npmjs.org/fixture-server/-/fixture-server-1.2.3.tgz".into(),
            sha256: digest(&archive),
            size_bytes: archive.len() as u64,
            integrity: integrity(&archive),
            dependencies: BTreeMap::from([("fixture-dependency".into(), "0.1.0".into())]),
            optional_dependencies: BTreeMap::new(),
            optional: false,
            os: None,
            cpu: None,
            has_install_script: true,
        };
        let dependency_package = NodePackageLock {
            name: "fixture-dependency".into(),
            version: "0.1.0".into(),
            path: "node_modules/fixture-dependency".into(),
            tarball: "https://registry.npmjs.org/fixture-dependency/-/fixture-dependency-0.1.0.tgz"
                .into(),
            sha256: digest(&dependency_archive),
            size_bytes: dependency_archive.len() as u64,
            integrity: integrity(&dependency_archive),
            dependencies: BTreeMap::new(),
            optional_dependencies: BTreeMap::new(),
            optional: false,
            os: None,
            cpu: None,
            has_install_script: true,
        };
        let lock = NodeDependencyLock {
            schema: 1,
            generated_at: "2026-08-13T00:00:00Z".into(),
            registry: "https://registry.npmjs.org".into(),
            roots: BTreeMap::from([(
                "fixture-server".into(),
                vec![NodePackageRef {
                    name: "fixture-server".into(),
                    version: "1.2.3".into(),
                    path: "node_modules/fixture-server".into(),
                    primary: true,
                }],
            )]),
            packages: vec![package, dependency_package],
        };
        let mut manifest = manifest(&archive, "bin/server.js");
        manifest.id = "fixture-server".into();
        manifest.runtime.kind = RuntimeKind::Node;
        manifest.runtime.executable = "node".into();
        manifest.artifact.kind = ArtifactKind::NpmTarball;
        manifest.artifact.archive_root = "package".into();
        manifest.artifact.url =
            "https://registry.npmjs.org/fixture-server/-/fixture-server-1.2.3.tgz".into();
        manifest.files.package_lock_sha256 = Some(REVIEWED_NODE_LOCK_SHA256.into());
        let installer = ManagedInstaller::new(temp.path().join("data")).unwrap();
        let staging = installer.lsp_root().join("staging/fixture");
        let result = installer
            .install_node_archives_with_lock(
                &manifest,
                "2026-08-13T01:02:03Z",
                &lock,
                &[
                    NodePackageArchive {
                        name: "fixture-server".into(),
                        version: "1.2.3".into(),
                        archive: archive_path.clone(),
                    },
                    NodePackageArchive {
                        name: "fixture-dependency".into(),
                        version: "0.1.0".into(),
                        archive: dependency_archive_path,
                    },
                ],
                &staging,
                InstallSource::LocalArchive,
            )
            .unwrap();
        let mut wrong_integrity = lock.packages[0].clone();
        wrong_integrity.integrity = integrity(&dependency_archive);
        assert!(matches!(
            verify_node_package_archive(&wrong_integrity, &archive_path, InstallLimits::default()),
            Err(InstallError::DigestMismatch)
        ));
        let root = Path::new(&result.server.installed_path);
        assert_eq!(fs::read(root.join("bin/server.js")).unwrap(), b"fixture");
        assert_eq!(
            fs::read(root.join("node_modules/fixture-dependency/lib/dependency.js")).unwrap(),
            b"dependency"
        );
        let installed_json: Value =
            serde_json::from_slice(&fs::read(root.join("package.json")).unwrap()).unwrap();
        assert!(installed_json.get("scripts").is_none());
        assert!(!root.join("marker.txt").exists());
        let dependency_json: Value = serde_json::from_slice(
            &fs::read(root.join("node_modules/fixture-dependency/package.json")).unwrap(),
        )
        .unwrap();
        assert!(dependency_json.get("scripts").is_none());
        assert!(!root
            .join("node_modules/fixture-dependency/dependency-marker.txt")
            .exists());
    }

    #[test]
    fn node_local_archive_set_matches_lock_by_digest_and_rejects_duplicates() {
        let archive = tar_gz_files(&[
            (
                "package/package.json",
                br#"{"name":"fixture-server","version":"1.2.3"}"#,
            ),
            ("package/bin/server.js", b"fixture"),
        ]);
        let temp = TempDir::new().unwrap();
        let archive_path = write_fixture(&temp, "fixture.tgz", &archive);
        let dependency_archive = tar_gz_files(&[(
            "package/package.json",
            br#"{"name":"fixture-dependency","version":"0.1.0"}"#,
        )]);
        let dependency_archive_path =
            write_fixture(&temp, "fixture-dependency.tgz", &dependency_archive);
        let package = NodePackageLock {
            name: "fixture-server".into(),
            version: "1.2.3".into(),
            path: "node_modules/fixture-server".into(),
            tarball: "https://registry.npmjs.org/fixture-server/-/fixture-server-1.2.3.tgz".into(),
            sha256: digest(&archive),
            size_bytes: archive.len() as u64,
            integrity: integrity(&archive),
            dependencies: BTreeMap::new(),
            optional_dependencies: BTreeMap::new(),
            optional: false,
            os: None,
            cpu: None,
            has_install_script: false,
        };
        let dependency_package = NodePackageLock {
            name: "fixture-dependency".into(),
            version: "0.1.0".into(),
            path: "node_modules/fixture-dependency".into(),
            tarball: "https://registry.npmjs.org/fixture-dependency/-/fixture-dependency-0.1.0.tgz"
                .into(),
            sha256: digest(&dependency_archive),
            size_bytes: dependency_archive.len() as u64,
            integrity: integrity(&dependency_archive),
            dependencies: BTreeMap::new(),
            optional_dependencies: BTreeMap::new(),
            optional: false,
            os: None,
            cpu: None,
            has_install_script: false,
        };
        let installer = ManagedInstaller::new(temp.path().join("data")).unwrap();
        installer
            .cache_node_package(&dependency_package, &dependency_archive_path)
            .unwrap();
        let packages = vec![&package, &dependency_package];
        let (archives, source) = installer
            .resolve_node_archive_set(
                WINDOWS_X86_64_PLATFORM,
                &packages,
                std::slice::from_ref(&archive_path),
            )
            .unwrap();
        assert_eq!(source, InstallSource::LocalArchive);
        assert_eq!(archives.len(), 2);
        assert_eq!(archives[0].name, package.name);
        assert_eq!(archives[0].version, package.version);
        // `TempDir` may expose an 8.3 path on Windows while canonicalization
        // returns the extended, long-name form. Compare against the same
        // canonical contract returned by `resolve_node_archive_set`.
        assert_eq!(
            archives[0].archive,
            fs::canonicalize(&archive_path).unwrap()
        );
        assert_eq!(archives[1].name, dependency_package.name);
        assert_eq!(archives[1].version, dependency_package.version);
        assert_eq!(
            archives[1].archive,
            installer
                .archive_cache_path(&dependency_package.sha256, ArtifactKind::NpmTarball,)
                .unwrap()
        );

        let duplicate = write_fixture(&temp, "duplicate.tgz", &archive);
        assert!(matches!(
            installer.resolve_node_archive_set(
                WINDOWS_X86_64_PLATFORM,
                &packages,
                &[archive_path, duplicate],
            ),
            Err(InstallError::DependencyLock(_))
        ));
        assert!(matches!(
            installer.resolve_node_archive_set(WINDOWS_X86_64_PLATFORM, &packages, &[]),
            Err(InstallError::DependencyLock(_))
        ));
    }

    #[test]
    fn node_manifest_cannot_bypass_reviewed_dependency_lock() {
        let archive = zip(&[("server.js", b"fixture")]);
        let mut manifest = manifest(&archive, "server.js");
        manifest.runtime.kind = RuntimeKind::Node;
        manifest.runtime.executable = "node".to_string();
        manifest.files.package_lock_sha256 = Some("11".repeat(32));
        let temp = TempDir::new().unwrap();
        let archive_path = write_fixture(&temp, "node.zip", &archive);
        let installer = ManagedInstaller::new(temp.path().join("data")).unwrap();
        assert!(matches!(
            installer.install_archive(&manifest, "1.2.3", "2026-08-13T01:02:03Z", &archive_path),
            Err(InstallError::DependencyLock(_))
        ));
    }

    #[test]
    fn catalog_install_lookup_accepts_only_exact_process_owned_keys() {
        let manifest = ManagedInstaller::catalog_manifest(
            "rust-analyzer",
            "2026-08-10.1",
            WINDOWS_X86_64_PLATFORM,
        )
        .unwrap();
        assert_eq!(manifest.artifact.url, "https://github.com/rust-lang/rust-analyzer/releases/download/2026-08-10.1/rust-analyzer-x86_64-pc-windows-msvc.zip");
        for (manifest_id, version, platform) in [
            (
                "rust-analyzer; invoke",
                "2026-08-10.1",
                WINDOWS_X86_64_PLATFORM,
            ),
            ("rust-analyzer", "latest", WINDOWS_X86_64_PLATFORM),
            ("rust-analyzer", "2026-08-10.1", "windows-x86_64;url"),
        ] {
            assert!(matches!(
                ManagedInstaller::catalog_manifest(manifest_id, version, platform),
                Err(InstallError::CatalogManifestNotFound { .. })
            ));
        }
    }

    #[test]
    fn corrupt_index_requires_explicit_recovery_and_is_not_overwritten_by_status() {
        let temp = TempDir::new().unwrap();
        let installer = ManagedInstaller::new(temp.path().join("data")).unwrap();
        let index_path = installer.lsp_root().join(INDEX_FILE);
        fs::write(&index_path, b"{not-json").unwrap();
        assert!(matches!(
            installer.installed_status(),
            Err(InstallError::IndexCorrupt)
        ));
        assert_eq!(fs::read(&index_path).unwrap(), b"{not-json");
        installer.recover_installed_index().unwrap();
        let statuses = installer.installed_status().unwrap();
        assert!(statuses
            .iter()
            .all(|status| status.state == ManagedInstallState::NotInstalled));
    }

    #[test]
    fn status_marks_metadata_drift_but_safe_removal_allows_reinstall() {
        let archive = zip(&[("server.exe", b"fixture")]);
        let manifest = manifest(&archive, "server.exe");
        let temp = TempDir::new().unwrap();
        let archive_path = write_fixture(&temp, "fixture.zip", &archive);
        let installer = ManagedInstaller::new(temp.path().join("data")).unwrap();
        let result = installer
            .install_archive(&manifest, "1.2.3", "2026-08-13T01:02:03Z", &archive_path)
            .unwrap();
        installer
            .validate_installed_entry(&manifest, &result.server)
            .unwrap();

        let mut index = installer.read_index().unwrap();
        index.servers[0].sha256 = "00".repeat(32);
        atomic_write(
            &installer.lsp_root().join(INDEX_FILE),
            index.to_json().unwrap().as_bytes(),
        )
        .unwrap();
        let status = installer
            .installed_status()
            .unwrap()
            .into_iter()
            .find(|status| status.manifest_id == manifest.id)
            .unwrap();
        assert_eq!(status.state, ManagedInstallState::NeedsReinstall);
        assert!(status.reason.is_some());
        assert!(matches!(
            installer.validate_installed_entry(&manifest, &index.servers[0]),
            Err(InstallError::MetadataMismatch(_))
        ));
        installer
            .uninstall_indexed(&manifest.id, &manifest.version, &manifest.platform)
            .unwrap();
        assert!(!Path::new(&result.server.installed_path).exists());
        assert!(installer.read_index().unwrap().servers.is_empty());

        // Removing the drifted indexed entry clears the immutable destination,
        // so the exact reviewed version can be installed again explicitly.
        installer
            .install_archive(&manifest, "1.2.3", "2026-08-13T01:04:03Z", &archive_path)
            .unwrap();
        assert!(installer
            .lsp_root()
            .join("servers/fixture-server/1.2.3/windows-x86_64/server.exe")
            .is_file());

        let mut index = installer.read_index().unwrap();
        index.servers[0].entrypoint = "missing.exe".into();
        atomic_write(
            &installer.lsp_root().join(INDEX_FILE),
            index.to_json().unwrap().as_bytes(),
        )
        .unwrap();
        let status = installer
            .installed_status()
            .unwrap()
            .into_iter()
            .find(|status| status.manifest_id == manifest.id)
            .unwrap();
        assert_eq!(status.state, ManagedInstallState::NeedsReinstall);
        installer
            .uninstall_indexed(&manifest.id, &manifest.version, &manifest.platform)
            .unwrap();
        assert!(!installer
            .lsp_root()
            .join("servers/fixture-server/1.2.3/windows-x86_64")
            .exists());
    }

    #[test]
    fn missing_language_command_override_marks_install_needs_reinstall() {
        let archive = zip(&[("server.exe", b"fixture"), ("html-server.exe", b"html")]);
        let mut manifest = manifest(&archive, "server.exe");
        manifest.languages[0].command = Some(CommandSpec {
            executable: "html-server.exe".into(),
            args: vec!["--stdio".into()],
        });
        let temp = TempDir::new().unwrap();
        let archive_path = write_fixture(&temp, "fixture.zip", &archive);
        let installer = ManagedInstaller::new(temp.path().join("data")).unwrap();
        installer
            .install_archive(&manifest, "1.2.3", "2026-08-13T01:05:03Z", &archive_path)
            .unwrap();
        fs::remove_file(
            installer
                .lsp_root()
                .join("servers/fixture-server/1.2.3/windows-x86_64/html-server.exe"),
        )
        .unwrap();
        let status = installer
            .installed_status()
            .unwrap()
            .into_iter()
            .find(|status| status.manifest_id == manifest.id)
            .unwrap();
        assert_eq!(status.state, ManagedInstallState::NeedsReinstall);
        assert!(status.reason.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_install_destination_is_never_removed() {
        use std::os::unix::fs::symlink;
        let archive = zip(&[("server.exe", b"fixture")]);
        let manifest = manifest(&archive, "server.exe");
        let temp = TempDir::new().unwrap();
        let archive_path = write_fixture(&temp, "fixture.zip", &archive);
        let installer = ManagedInstaller::new(temp.path().join("data")).unwrap();
        installer
            .install_archive(&manifest, "1.2.3", "2026-08-13T01:02:03Z", &archive_path)
            .unwrap();
        let destination = installer
            .lsp_root()
            .join("servers/fixture-server/1.2.3/windows-x86_64");
        let outside = temp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::remove_dir_all(&destination).unwrap();
        symlink(&outside, &destination).unwrap();
        assert!(matches!(
            installer.uninstall(&manifest),
            Err(InstallError::UnsafeArchivePath)
        ));
        assert!(fs::symlink_metadata(&destination)
            .unwrap()
            .file_type()
            .is_symlink());
        assert!(outside.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_managed_server_root_is_never_followed() {
        use std::os::unix::fs::symlink;
        let archive = zip(&[("server.exe", b"fixture")]);
        let manifest = manifest(&archive, "server.exe");
        let temp = TempDir::new().unwrap();
        let archive_path = write_fixture(&temp, "fixture.zip", &archive);
        let installer = ManagedInstaller::new(temp.path().join("data")).unwrap();
        installer
            .install_archive(&manifest, "1.2.3", "2026-08-13T01:02:03Z", &archive_path)
            .unwrap();
        let servers = installer.lsp_root().join("servers");
        let outside = temp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        let moved = temp.path().join("moved-servers");
        fs::rename(&servers, &moved).unwrap();
        symlink(&outside, &servers).unwrap();
        assert!(matches!(
            installer.uninstall(&manifest),
            Err(InstallError::UnsafeArchivePath)
        ));
        assert!(moved
            .join("fixture-server/1.2.3/windows-x86_64/server.exe")
            .is_file());
        assert!(outside.is_dir());
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn hardlinked_install_file_is_never_removed() {
        use std::fs::hard_link;

        let archive = zip(&[("server.exe", b"fixture")]);
        let manifest = manifest(&archive, "server.exe");
        let temp = TempDir::new().unwrap();
        let archive_path = write_fixture(&temp, "fixture.zip", &archive);
        let installer = ManagedInstaller::new(temp.path().join("data")).unwrap();
        let result = installer
            .install_archive(&manifest, "1.2.3", "2026-08-13T01:02:03Z", &archive_path)
            .unwrap();
        let entrypoint = Path::new(&result.server.installed_path).join("server.exe");
        let alias = Path::new(&result.server.installed_path).join("alias.exe");
        hard_link(&entrypoint, &alias).unwrap();

        let status = installer
            .installed_status()
            .unwrap()
            .into_iter()
            .find(|status| status.manifest_id == manifest.id)
            .unwrap();
        assert_eq!(status.state, ManagedInstallState::NeedsReinstall);
        assert!(matches!(
            installer.uninstall(&manifest),
            Err(InstallError::UnsafeArchivePath)
        ));
        assert!(entrypoint.is_file());
        assert!(alias.is_file());
        assert_eq!(installer.read_index().unwrap().servers.len(), 1);
    }

    #[test]
    fn uninstall_commits_one_exact_version_and_preserves_other_versions() {
        let archive_v1 = zip(&[("server.exe", b"v1")]);
        let archive_v2 = zip(&[("server.exe", b"v2")]);
        let manifest_v1 = manifest(&archive_v1, "server.exe");
        let mut manifest_v2 = manifest(&archive_v2, "server.exe");
        manifest_v2.version = "1.2.4".into();
        manifest_v2.artifact.url = "https://example.com/server-v2.zip".into();
        let temp = TempDir::new().unwrap();
        let archive_v1_path = write_fixture(&temp, "fixture-v1.zip", &archive_v1);
        let archive_v2_path = write_fixture(&temp, "fixture-v2.zip", &archive_v2);
        let installer = ManagedInstaller::new(temp.path().join("data")).unwrap();
        installer
            .install_archive(
                &manifest_v1,
                "1.2.3",
                "2026-08-13T01:02:03Z",
                &archive_v1_path,
            )
            .unwrap();
        installer
            .install_archive(
                &manifest_v2,
                "1.2.4",
                "2026-08-13T01:03:03Z",
                &archive_v2_path,
            )
            .unwrap();
        installer.uninstall(&manifest_v1).unwrap();
        let index = installer.read_index().unwrap();
        assert_eq!(index.servers.len(), 1);
        assert_eq!(index.servers[0].version, manifest_v2.version);
        installer
            .validate_installed_entry(&manifest_v2, &index.servers[0])
            .unwrap();
        assert!(!installer
            .lsp_root()
            .join("servers/fixture-server/1.2.3/windows-x86_64")
            .exists());
        assert!(installer
            .lsp_root()
            .join("servers/fixture-server/1.2.4/windows-x86_64/server.exe")
            .is_file());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_installer_root_is_rejected() {
        use std::os::unix::fs::symlink;
        let temp = TempDir::new().unwrap();
        let outside = temp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        let data = temp.path().join("data");
        fs::create_dir(&data).unwrap();
        symlink(&outside, data.join("lsp")).unwrap();
        assert!(matches!(
            ManagedInstaller::new(&data),
            Err(InstallError::UnsafeArchivePath)
        ));
    }
}
