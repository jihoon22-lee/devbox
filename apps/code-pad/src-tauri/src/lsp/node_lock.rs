//! Reviewed, exact npm dependency locks for managed Node language servers.
//!
//! The lock is data rather than an npm project. Every package is identified by
//! an exact version and carries the registry tarball URL, SHA-256, compressed
//! byte size, and resolved dependency versions. The installer never invokes
//! npm or a package lifecycle script; it only downloads and extracts the
//! reviewed tarballs into the paths described by this lock.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use url::Url;

pub const NODE_LOCK_SCHEMA_VERSION: u32 = 1;
pub const REVIEWED_NODE_LOCK_SHA256: &str =
    "eda1314445a932cfa1a6c6662ce068700212a0bdb051d31c23bf30697f04a4dd";

const REVIEWED_NODE_LOCK: &[u8] = include_bytes!("node-lock.json");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodePackageRef {
    pub name: String,
    pub version: String,
    pub path: String,
    #[serde(default)]
    pub primary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodePackageLock {
    pub name: String,
    pub version: String,
    /// The npm lockfile path. The root package is remapped to a dot by the
    /// installer; all other paths are relative to the managed server root.
    pub path: String,
    pub tarball: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub integrity: String,
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
    #[serde(default)]
    pub optional_dependencies: BTreeMap<String, String>,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub os: Option<Vec<String>>,
    #[serde(default)]
    pub cpu: Option<Vec<String>>,
    /// This is review metadata. It is deliberately not a reason to execute
    /// the script: all package scripts are ignored and removed from the
    /// installed package.json by the installer.
    #[serde(default)]
    pub has_install_script: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeDependencyLock {
    pub schema: u32,
    pub generated_at: String,
    pub registry: String,
    pub roots: BTreeMap<String, Vec<NodePackageRef>>,
    pub packages: Vec<NodePackageLock>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeLockError {
    InvalidJson(String),
    DigestMismatch,
    Invalid(String),
    UnknownServer(String),
    MissingPackage { name: String, version: String },
}

impl fmt::Display for NodeLockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson(error) => {
                write!(formatter, "invalid reviewed Node lock JSON: {error}")
            }
            Self::DigestMismatch => formatter.write_str("reviewed Node lock SHA-256 mismatch"),
            Self::Invalid(reason) => write!(formatter, "invalid reviewed Node lock: {reason}"),
            Self::UnknownServer(server) => {
                write!(formatter, "reviewed Node lock has no root for {server}")
            }
            Self::MissingPackage { name, version } => {
                write!(formatter, "reviewed Node lock is missing {name}@{version}")
            }
        }
    }
}

impl std::error::Error for NodeLockError {}

pub fn reviewed_node_lock() -> Result<NodeDependencyLock, NodeLockError> {
    let digest = Sha256::digest(REVIEWED_NODE_LOCK);
    if hex_digest(&digest) != REVIEWED_NODE_LOCK_SHA256 {
        return Err(NodeLockError::DigestMismatch);
    }
    let lock: NodeDependencyLock = serde_json::from_slice(REVIEWED_NODE_LOCK)
        .map_err(|error| NodeLockError::InvalidJson(error.to_string()))?;
    lock.validate()?;
    Ok(lock)
}

impl NodeDependencyLock {
    pub fn validate(&self) -> Result<(), NodeLockError> {
        if self.schema != NODE_LOCK_SCHEMA_VERSION {
            return Err(NodeLockError::Invalid(format!(
                "unsupported schema {}; expected {NODE_LOCK_SCHEMA_VERSION}",
                self.schema
            )));
        }
        if self.registry != "https://registry.npmjs.org" {
            return Err(NodeLockError::Invalid(
                "registry must be the official npm registry".into(),
            ));
        }
        if self.roots.is_empty() || self.packages.is_empty() {
            return Err(NodeLockError::Invalid(
                "at least one root and package are required".into(),
            ));
        }
        let mut package_keys = BTreeSet::new();
        let mut package_paths = BTreeSet::new();
        for package in &self.packages {
            validate_package(package)?;
            let key = package_key(&package.name, &package.version);
            if !package_keys.insert(key) {
                return Err(NodeLockError::Invalid(format!(
                    "duplicate package {}@{}",
                    package.name, package.version
                )));
            }
            if !package_paths.insert(&package.path) {
                return Err(NodeLockError::Invalid(format!(
                    "duplicate package install path {}",
                    package.path
                )));
            }
        }
        for (server, roots) in &self.roots {
            if roots.is_empty() {
                return Err(NodeLockError::Invalid(format!(
                    "root {server} has no packages"
                )));
            }
            for root in roots {
                validate_package_ref(root)?;
                let package = self.package(&root.name, &root.version).ok_or_else(|| {
                    NodeLockError::MissingPackage {
                        name: root.name.clone(),
                        version: root.version.clone(),
                    }
                })?;
                if package.path != root.path {
                    return Err(NodeLockError::Invalid(format!(
                        "root {}@{} path does not match package path",
                        root.name, root.version
                    )));
                }
            }
            if roots.iter().filter(|root| root.primary).count() != 1 {
                return Err(NodeLockError::Invalid(format!(
                    "root {server} must have exactly one primary package"
                )));
            }
            self.packages_for_server(server)?;
        }
        Ok(())
    }

    pub fn root_packages(&self, server: &str) -> Result<&[NodePackageRef], NodeLockError> {
        self.roots
            .get(server)
            .map(Vec::as_slice)
            .ok_or_else(|| NodeLockError::UnknownServer(server.to_owned()))
    }

    pub fn primary_root(&self, server: &str) -> Result<&NodePackageRef, NodeLockError> {
        self.root_packages(server)?
            .iter()
            .find(|root| root.primary)
            .ok_or_else(|| NodeLockError::Invalid(format!("root {server} has no primary package")))
    }

    pub fn install_path(
        &self,
        server: &str,
        package: &NodePackageLock,
    ) -> Result<std::path::PathBuf, NodeLockError> {
        let primary = self.primary_root(server)?;
        if primary.name == package.name
            && primary.version == package.version
            && primary.path == package.path
        {
            Ok(std::path::PathBuf::new())
        } else {
            std::path::PathBuf::from(package.path.strip_prefix("node_modules/").ok_or_else(
                || NodeLockError::Invalid(format!("invalid npm install path {}", package.path)),
            )?)
            .components()
            .try_fold(
                std::path::PathBuf::from("node_modules"),
                |path, component| match component {
                    std::path::Component::Normal(name) => Ok(path.join(name)),
                    _ => Err(NodeLockError::Invalid(format!(
                        "invalid npm install path {}",
                        package.path
                    ))),
                },
            )
        }
    }

    pub fn package(&self, name: &str, version: &str) -> Option<&NodePackageLock> {
        self.packages
            .iter()
            .find(|package| package.name == name && package.version == version)
    }

    /// Return the transitive closure for one managed server in deterministic
    /// path order. Optional platform-specific packages remain in the closure
    /// so their exact tarball facts are still reviewed in the lock.
    pub fn packages_for_server(
        &self,
        server: &str,
    ) -> Result<Vec<&NodePackageLock>, NodeLockError> {
        let roots = self.root_packages(server)?;
        let mut queue = VecDeque::new();
        let mut seen = BTreeSet::new();
        for root in roots {
            queue.push_back((root.name.clone(), root.version.clone()));
        }
        while let Some((name, version)) = queue.pop_front() {
            let key = package_key(&name, &version);
            if !seen.insert(key) {
                continue;
            }
            let package = self
                .package(&name, &version)
                .ok_or(NodeLockError::MissingPackage { name, version })?;
            for (dependency, dependency_version) in package
                .dependencies
                .iter()
                .chain(package.optional_dependencies.iter())
            {
                queue.push_back((dependency.clone(), dependency_version.clone()));
            }
        }
        let mut packages = self
            .packages
            .iter()
            .filter(|package| seen.contains(&package_key(&package.name, &package.version)))
            .collect::<Vec<_>>();
        packages.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(packages)
    }
}

fn validate_package_ref(package: &NodePackageRef) -> Result<(), NodeLockError> {
    validate_package_name(&package.name)?;
    validate_exact_version(&package.version)?;
    validate_node_path(&package.path, &package.name)
}

fn validate_package(package: &NodePackageLock) -> Result<(), NodeLockError> {
    validate_package_name(&package.name)?;
    validate_exact_version(&package.version)?;
    validate_node_path(&package.path, &package.name)?;
    let url = Url::parse(&package.tarball)
        .map_err(|_| NodeLockError::Invalid(format!("invalid tarball URL for {}", package.name)))?;
    let package_file_name = package.name.rsplit('/').next().unwrap_or(&package.name);
    let expected_path = format!(
        "/{}/-/{}-{}.tgz",
        package.name, package_file_name, package.version
    );
    if url.scheme() != "https"
        || url.host_str() != Some("registry.npmjs.org")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.path() != expected_path
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(NodeLockError::Invalid(format!(
            "tarball for {} must use its exact official HTTPS registry URL",
            package.name
        )));
    }
    if package.size_bytes == 0 {
        return Err(NodeLockError::Invalid(format!(
            "tarball size for {} must be positive",
            package.name
        )));
    }
    validate_sha256(&package.sha256)?;
    if !package.integrity.starts_with("sha512-") {
        return Err(NodeLockError::Invalid(format!(
            "integrity for {} must be npm SHA-512",
            package.name
        )));
    }
    for (dependency, version) in package
        .dependencies
        .iter()
        .chain(package.optional_dependencies.iter())
    {
        validate_package_name(dependency)?;
        validate_exact_version(version)?;
    }
    Ok(())
}

fn validate_package_name(name: &str) -> Result<(), NodeLockError> {
    if name.is_empty()
        || name.contains('\\')
        || name.starts_with('/')
        || name
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || (name.starts_with('@') && name.split('/').count() != 2)
    {
        return Err(NodeLockError::Invalid(format!(
            "invalid npm package name {name:?}"
        )));
    }
    Ok(())
}

fn validate_node_path(path: &str, package_name: &str) -> Result<(), NodeLockError> {
    if path.is_empty()
        || path.contains('\\')
        || path.starts_with('/')
        || path
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
        || !path.starts_with("node_modules/")
    {
        return Err(NodeLockError::Invalid(format!(
            "invalid npm install path {path:?}"
        )));
    }
    let expected_suffix = format!("node_modules/{package_name}");
    if path != expected_suffix && !path.ends_with(&format!("/{expected_suffix}")) {
        return Err(NodeLockError::Invalid(format!(
            "install path {path:?} does not contain package {package_name:?}"
        )));
    }
    Ok(())
}

fn validate_exact_version(version: &str) -> Result<(), NodeLockError> {
    if version.is_empty()
        || version == "latest"
        || version.chars().any(|character| {
            character.is_whitespace()
                || matches!(character, '^' | '~' | '*' | '<' | '>' | '=' | '|' | '/')
        })
    {
        return Err(NodeLockError::Invalid(format!(
            "dependency version is not exact: {version:?}"
        )));
    }
    Ok(())
}

fn validate_sha256(value: &str) -> Result<(), NodeLockError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(NodeLockError::Invalid(
            "SHA-256 must be 64 lowercase hex characters".into(),
        ));
    }
    Ok(())
}

fn package_key(name: &str, version: &str) -> String {
    format!("{name}\u{001f}{version}")
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reviewed_lock_has_exact_server_closures_and_paired_typescript() {
        let lock = reviewed_node_lock().unwrap();
        assert_eq!(lock.packages.len(), 36);
        assert_eq!(
            lock.packages_for_server("typescript-language-server")
                .unwrap()
                .len(),
            2
        );
        assert_eq!(lock.packages_for_server("basedpyright").unwrap().len(), 2);
        assert_eq!(
            lock.packages_for_server("vscode-langservers-extracted")
                .unwrap()
                .len(),
            32
        );
        let roots = lock.root_packages("typescript-language-server").unwrap();
        assert_eq!(roots.iter().filter(|root| root.primary).count(), 1);
        assert_eq!(roots[1].name, "typescript");
        assert!(!roots[1].primary);
        assert_eq!(
            lock.primary_root("typescript-language-server")
                .unwrap()
                .name,
            "typescript-language-server"
        );
    }

    #[test]
    fn lock_rejects_ranges_and_non_registry_tarballs() {
        let mut lock = reviewed_node_lock().unwrap();
        lock.packages[0].version = "^1.2.3".into();
        assert!(matches!(lock.validate(), Err(NodeLockError::Invalid(_))));

        let mut lock = reviewed_node_lock().unwrap();
        lock.packages[0].tarball = "http://registry.npmjs.org/package.tgz".into();
        assert!(matches!(lock.validate(), Err(NodeLockError::Invalid(_))));
    }

    #[test]
    fn lock_rejects_a_package_path_for_another_name() {
        let mut lock = reviewed_node_lock().unwrap();
        lock.packages[0].path = "node_modules/not-the-package".into();
        assert!(matches!(
            lock.validate(),
            Err(NodeLockError::Invalid(reason)) if reason.contains("does not contain package")
        ));
    }

    #[test]
    fn lock_rejects_a_registry_url_for_another_package() {
        let mut lock = reviewed_node_lock().unwrap();
        lock.packages[0].tarball =
            "https://registry.npmjs.org/not-the-package/-/not-the-package-1.0.0.tgz".into();
        assert!(matches!(
            lock.validate(),
            Err(NodeLockError::Invalid(reason)) if reason.contains("exact official")
        ));
    }
}
