//! Exact-version managed language-server installation.
//!
//! Downloads and extraction use nonce-scoped paths. Only a verified staging
//! tree is renamed into the immutable server directory, and that directory is
//! not active until the installed index has been atomically replaced.

use crate::commands::session::atomic_write;
use crate::lsp::catalog::{ArtifactKind, InstalledServer, InstalledServerIndex, ServerManifest};
use crate::lsp::node_lock::{
    reviewed_node_lock, NodeDependencyLock, NodeLockError, NodePackageLock,
    REVIEWED_NODE_LOCK_SHA256,
};
use flate2::read::GzDecoder;
use reqwest::header::LOCATION;
use reqwest::{Client, StatusCode, Url};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;
use tar::EntryType;
use tokio::io::AsyncWriteExt;

pub const DEFAULT_MAX_INSTALL_BYTES: u64 = 512 * 1024 * 1024;
pub const DEFAULT_MAX_ARCHIVE_ENTRIES: usize = 100_000;
const MAX_REDIRECTS: usize = 5;
const INDEX_FILE: &str = "installed.json";

static NONCE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstallLimits {
    pub max_download_bytes: u64,
    pub max_extracted_bytes: u64,
    pub max_archive_entries: usize,
}

impl Default for InstallLimits {
    fn default() -> Self {
        Self {
            max_download_bytes: DEFAULT_MAX_INSTALL_BYTES,
            max_extracted_bytes: DEFAULT_MAX_INSTALL_BYTES,
            max_archive_entries: DEFAULT_MAX_ARCHIVE_ENTRIES,
        }
    }
}

impl InstallLimits {
    fn validate(self) -> Result<Self, InstallError> {
        if self.max_download_bytes == 0
            || self.max_extracted_bytes == 0
            || self.max_archive_entries == 0
        {
            return Err(InstallError::InvalidLimits);
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallResult {
    pub server: InstalledServer,
    pub already_installed: bool,
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
    EntrypointMissing,
    InstallConflict,
    InstallBusy,
    IndexCorrupt,
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
            Self::EntrypointMissing => formatter.write_str("artifact entrypoint is missing"),
            Self::InstallConflict => {
                formatter.write_str("immutable install destination already exists")
            }
            Self::InstallBusy => formatter.write_str("another managed installation is active"),
            Self::IndexCorrupt => formatter.write_str("installed server index is corrupt"),
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
        fs::create_dir_all(lsp_root.join("staging"))
            .map_err(|error| InstallError::io("creating staging directory", error))?;
        fs::create_dir_all(lsp_root.join("servers"))
            .map_err(|error| InstallError::io("creating servers directory", error))?;
        reject_symlink_tree(&lsp_root)?;
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
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
            self.download(manifest, &partial).await?;
            self.install_verified_archive(manifest, installed_at, &partial, &staging)
        }
        .await;
        let _ = fs::remove_file(&partial);
        if staging.exists() {
            let _ = fs::remove_dir_all(&staging);
        }
        result
    }

    /// Local fixture boundary. The archive still passes all size, digest,
    /// extraction, entrypoint, promotion, and index checks.
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
        if manifest.runtime.kind == crate::lsp::catalog::RuntimeKind::Node {
            return Err(InstallError::DependencyLock(
                "Node installs require the complete reviewed package archive set".into(),
            ));
        }
        verify_archive_file(manifest, archive.as_ref(), self.limits.max_download_bytes)?;
        let staging = self.lsp_root.join("staging").join(unique_nonce());
        let result =
            self.install_verified_archive(manifest, installed_at, archive.as_ref(), &staging);
        if staging.exists() {
            let _ = fs::remove_dir_all(&staging);
        }
        result
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
        if manifest.runtime.kind != crate::lsp::catalog::RuntimeKind::Node {
            return Err(InstallError::InvalidManifest(
                "Node package archives require a Node runtime".into(),
            ));
        }
        let lock = reviewed_node_lock().map_err(node_lock_error)?;
        let staging = self.lsp_root.join("staging").join(unique_nonce());
        let result =
            self.install_node_archives_with_lock(manifest, installed_at, &lock, archives, &staging);
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
            let archive = download_dir.join(format!("{index}.tgz"));
            self.download_package(package, &archive).await?;
            archives.push(NodePackageArchive {
                name: package.name.clone(),
                version: package.version.clone(),
                archive,
            });
        }
        self.install_node_archives_with_lock(manifest, installed_at, lock, &archives, staging)
    }

    fn install_node_archives_with_lock(
        &self,
        manifest: &ServerManifest,
        installed_at: &str,
        lock: &NodeDependencyLock,
        archives: &[NodePackageArchive],
        staging: &Path,
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
            verify_node_package_archive(package, &archive.archive, self.limits)?;
            let relative = lock
                .install_path(&manifest.id, package)
                .map_err(node_lock_error)?;
            let (entries, bytes) =
                extract_node_package(package, &archive.archive, staging, &relative, self.limits)?;
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
        self.promote_staging(manifest, installed_at, staging)
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
    ) -> Result<InstallResult, InstallError> {
        fs::create_dir(staging)
            .map_err(|error| InstallError::io("creating staging directory", error))?;
        reject_symlink_tree(staging)?;
        extract_archive(manifest, archive, staging, self.limits)?;
        self.promote_staging(manifest, installed_at, staging)
    }

    fn promote_staging(
        &self,
        manifest: &ServerManifest,
        installed_at: &str,
        staging: &Path,
    ) -> Result<InstallResult, InstallError> {
        let entrypoint = safe_relative_path(&manifest.files.entrypoint)?;
        let entrypoint_path = staging.join(&entrypoint);
        let metadata =
            fs::symlink_metadata(&entrypoint_path).map_err(|_| InstallError::EntrypointMissing)?;
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Err(InstallError::EntrypointMissing);
        }
        let canonical_staging = fs::canonicalize(staging)
            .map_err(|error| InstallError::io("validating staging directory", error))?;
        let canonical_entrypoint =
            fs::canonicalize(&entrypoint_path).map_err(|_| InstallError::EntrypointMissing)?;
        if !canonical_entrypoint.starts_with(&canonical_staging) {
            return Err(InstallError::UnsafeArchivePath);
        }

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
        let result = self.persist_installed(manifest, installed_at, &destination, false);
        if result.is_err() {
            // Promotion is not active until the index commit succeeds.
            let _ = fs::remove_dir_all(&destination);
        }
        result
    }

    fn persist_installed(
        &self,
        manifest: &ServerManifest,
        installed_at: &str,
        destination: &Path,
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
        };
        server
            .validate()
            .map_err(|error| InstallError::InvalidManifest(error.to_string()))?;
        let index_path = self.lsp_root.join(INDEX_FILE);
        let mut index = match fs::read_to_string(&index_path) {
            Ok(json) => {
                InstalledServerIndex::from_json(&json).map_err(|_| InstallError::IndexCorrupt)?
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                InstalledServerIndex::default()
            }
            Err(error) => return Err(InstallError::io("reading installed index", error)),
        };
        index.servers.retain(|entry| {
            !(entry.manifest_id == server.manifest_id
                && entry.version == server.version
                && entry.platform == server.platform)
        });
        index.servers.push(server.clone());
        index.validate().map_err(|_| InstallError::IndexCorrupt)?;
        let json = index.to_json().map_err(|_| InstallError::IndexCorrupt)?;
        atomic_write(&index_path, json.as_bytes())
            .map_err(|error| InstallError::io("committing installed index", error))?;
        Ok(InstallResult {
            server,
            already_installed,
        })
    }
}

fn unique_nonce() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let sequence = NONCE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{}-{nanos}-{sequence}", std::process::id())
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
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| InstallError::io("hashing Node package archive", error))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    verify_digest(&hasher.finalize(), &package.sha256)
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
        let Some(relative) = strip_archive_root(&archive_path, "package")? else {
            continue;
        };
        if relative.as_os_str().is_empty() {
            continue;
        }
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

fn decode_sha256(input: &str) -> Option<[u8; 32]> {
    if input.len() != 64 {
        return None;
    }
    let mut output = [0_u8; 32];
    for (index, pair) in input.as_bytes().chunks_exact(2).enumerate() {
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
        let Some(relative) = strip_archive_root(&archive_path, &manifest.artifact.archive_root)?
        else {
            continue;
        };
        if relative.as_os_str().is_empty() {
            continue;
        }
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
        let Some(relative) = strip_archive_root(&archive_path, &manifest.artifact.archive_root)?
        else {
            continue;
        };
        if relative.as_os_str().is_empty() {
            continue;
        }
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
    let name = name.strip_suffix('/').unwrap_or(name);
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
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
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
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
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

    fn manifest(bytes: &[u8], entrypoint: &str) -> ServerManifest {
        ServerManifest {
            id: "fixture-server".to_string(),
            version: "1.2.3".to_string(),
            platform: WINDOWS_X86_64_PLATFORM.to_string(),
            languages: vec![LanguageSupport {
                language_id: "fixture".to_string(),
                extensions: vec![".fixture".to_string()],
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
        assert_eq!(
            fs::read(Path::new(&result.server.installed_path).join("server.exe")).unwrap(),
            b"fixture"
        );
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
            integrity: "sha512-fixture".into(),
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
            integrity: "sha512-fixture".into(),
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
                        archive: archive_path,
                    },
                    NodePackageArchive {
                        name: "fixture-dependency".into(),
                        version: "0.1.0".into(),
                        archive: dependency_archive_path,
                    },
                ],
                &staging,
            )
            .unwrap();
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
