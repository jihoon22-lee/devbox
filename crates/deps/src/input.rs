//! The host supplies registered-project, bounded reads; no package manager runs.
use crate::{
    locks::{self, Manifests},
    model::*,
};
use sha2::{Digest, Sha256};
use std::{future::Future, pin::Pin, sync::Arc};
pub type ReadFuture = Pin<Box<dyn Future<Output = Result<Option<Vec<u8>>, DepsError>> + Send>>;
pub type Reader = Arc<dyn Fn(String) -> ReadFuture + Send + Sync>;
const LOCKS: [(&str, LockKind); 7] = [
    ("pnpm-lock.yaml", LockKind::Pnpm),
    ("package-lock.json", LockKind::Npm),
    ("yarn.lock", LockKind::Yarn),
    ("Cargo.lock", LockKind::Cargo),
    ("go.sum", LockKind::Go),
    ("poetry.lock", LockKind::Poetry),
    ("uv.lock", LockKind::Uv),
];
pub async fn load(reader: Reader) -> Result<DependencyInventory, DepsError> {
    let mut inputs = Manifests::new();
    let mut kinds = Vec::new();
    let mut bytes = 0;
    for (path, kind) in LOCKS {
        if let Some(value) = read(&reader, path, &mut bytes).await? {
            inputs.insert(path.into(), value);
            kinds.push(kind);
        }
    }
    if kinds.is_empty() {
        return Err(DepsError::NoLockfile);
    }
    let mut manifests = Manifests::new();
    for (kind, path) in [
        (LockKind::Yarn, "package.json"),
        (LockKind::Cargo, "Cargo.toml"),
        (LockKind::Go, "go.mod"),
        (LockKind::Poetry, "pyproject.toml"),
        (LockKind::Uv, "pyproject.toml"),
    ] {
        if kinds.contains(&kind)
            && !manifests.contains_key(path)
            && let Some(value) = read(&reader, path, &mut bytes).await?
        {
            manifests.insert(path.into(), value);
        }
    }
    if let Some(root) = manifests.get("Cargo.toml") {
        for path in crate::locks::cargo_manifests(root)? {
            if let Some(value) = read(&reader, &path, &mut bytes).await? {
                manifests.insert(path, value);
            }
        }
    }
    let fingerprint = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&(&inputs, &manifests)).map_err(|_| DepsError::ReadFailed)?
        )
    );
    tokio::task::spawn_blocking(move || {
        let mut merged = std::collections::BTreeMap::new();
        for (path, kind) in LOCKS {
            if let Some(text) = inputs.get(path) {
                for dependency in locks::parse(kind, text, &manifests)? {
                    for version in dependency.versions {
                        *merged
                            .entry((
                                dependency.ecosystem,
                                dependency.name.clone(),
                                version.version,
                                version.source,
                            ))
                            .or_insert(false) |= version.direct;
                    }
                }
            }
        }
        if merged.len() > locks::MAX_ITEMS {
            return Err(DepsError::LimitExceeded);
        }
        let mut dependencies: Vec<Dependency> = Vec::new();
        for ((ecosystem, name, version, source), direct) in merged {
            if let Some(last) = dependencies
                .last_mut()
                .filter(|d| d.ecosystem == ecosystem && d.name == name)
            {
                last.direct |= direct;
                last.versions.push(DependencyVersion {
                    version,
                    source,
                    direct,
                });
            } else {
                dependencies.push(Dependency {
                    ecosystem,
                    name,
                    versions: vec![DependencyVersion {
                        version,
                        source,
                        direct,
                    }],
                    direct,
                });
            }
        }
        Ok(DependencyInventory {
            fingerprint,
            locks: kinds,
            dependencies,
        })
    })
    .await
    .map_err(|_| DepsError::ReadFailed)?
}
async fn read(reader: &Reader, path: &str, total: &mut usize) -> Result<Option<String>, DepsError> {
    if path.starts_with('/')
        || path.contains('\\')
        || path
            .split('/')
            .any(|c| c.is_empty() || c == "." || c == "..")
    {
        return Err(DepsError::UnsafePath);
    }
    let value = reader(path.into()).await?;
    if let Some(value) = value {
        *total += value.len();
        if value.len() > locks::MAX_BYTES || *total > 16 * 1024 * 1024 {
            return Err(DepsError::LimitExceeded);
        }
        Ok(Some(
            String::from_utf8(value).map_err(|_| DepsError::ReadFailed)?,
        ))
    } else {
        Ok(None)
    }
}
