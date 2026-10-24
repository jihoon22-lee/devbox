use super::*;
use serde::Deserialize;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Lock {
    lockfile_version: Option<String>,
    #[serde(default)]
    importers: BTreeMap<String, Importer>,
    #[serde(default)]
    packages: BTreeMap<String, Package>,
    #[serde(default)]
    snapshots: BTreeMap<String, serde::de::IgnoredAny>,
    #[serde(default)]
    package_manager_dependencies: BTreeMap<String, serde::de::IgnoredAny>,
    #[serde(default)]
    config_dependencies: BTreeMap<String, serde::de::IgnoredAny>,
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Importer {
    #[serde(default)]
    config_dependencies: BTreeMap<String, Reference>,
    #[serde(default)]
    package_manager_dependencies: BTreeMap<String, Reference>,
    #[serde(default)]
    dependencies: BTreeMap<String, Reference>,
    #[serde(default)]
    dev_dependencies: BTreeMap<String, Reference>,
    #[serde(default)]
    optional_dependencies: BTreeMap<String, Reference>,
}
#[derive(Deserialize)]
struct Reference {
    version: String,
}
#[derive(Deserialize)]
struct Package {
    resolution: Resolution,
}
#[derive(Default, Deserialize)]
struct Resolution {
    integrity: Option<String>,
    tarball: Option<String>,
    repo: Option<String>,
    directory: Option<String>,
    revision: Option<u32>,
}
fn split(key: &str) -> Option<(&str, &str)> {
    key.rsplit_once('@')
        .filter(|(n, v)| !n.is_empty() && !v.is_empty())
}
fn plain(version: &str) -> &str {
    version.split('(').next().unwrap_or(version)
}
pub(super) fn parse(text: &str, out: &mut Inventory) -> Result<(), DepsError> {
    let kind = LockKind::Pnpm;
    let docs: Vec<Lock> = yaml(text, kind)?;
    if docs.is_empty() {
        return Err(malformed(kind));
    }
    let mut version_seen = false;
    let mut direct = std::collections::BTreeSet::new();
    for doc in &docs {
        if let Some(v) = &doc.lockfile_version {
            if v != "9.0" {
                return Err(DepsError::UnsupportedLock { lock: kind });
            }
            version_seen = true;
        }
        if !doc.config_dependencies.is_empty() || !doc.package_manager_dependencies.is_empty() {
            return Err(DepsError::UnsupportedLock { lock: kind });
        }
        if doc.packages.len() + doc.snapshots.len() + doc.importers.len() > MAX_ITEMS * 2 {
            return Err(DepsError::LimitExceeded);
        }
        for importer in doc.importers.values() {
            for (name, reference) in importer
                .dependencies
                .iter()
                .chain(&importer.dev_dependencies)
                .chain(&importer.optional_dependencies)
                .chain(&importer.config_dependencies)
                .chain(&importer.package_manager_dependencies)
            {
                let v = plain(&reference.version);
                if let Some(alias) = v.strip_prefix("npm:") {
                    if let Some((name, v)) = split(alias) {
                        direct.insert((name.to_owned(), v.to_owned()));
                    }
                } else {
                    direct.insert((name.to_owned(), v.to_owned()));
                }
            }
        }
    }
    if !version_seen || docs.iter().all(|d| d.importers.is_empty()) {
        return Err(malformed(kind));
    }
    let mut found = std::collections::BTreeSet::new();
    for (name, version) in &direct {
        if version.starts_with("link:") || version.starts_with("file:") {
            out.add(Ecosystem::Npm, name, version, DependencySource::Local, true)?;
            found.insert((name.clone(), version.clone()));
        }
    }
    for doc in docs {
        for (key, p) in doc.packages {
            let (name, version) = split(plain(&key)).ok_or_else(|| malformed(kind))?;
            let r = p.resolution;
            if r.revision.is_some_and(|r| r > 0) {
                return Err(DepsError::UnsupportedLock { lock: kind });
            }
            let source = if r.repo.is_some() {
                DependencySource::Git
            } else if r.directory.is_some() {
                DependencySource::Local
            } else if r.tarball.as_deref().is_some_and(super::node::registry_url)
                || r.tarball.is_none() && r.integrity.is_some()
            {
                DependencySource::Registry
            } else {
                DependencySource::Unknown
            };
            found.insert((name.into(), version.into()));
            out.add(
                Ecosystem::Npm,
                name,
                version,
                source,
                direct.contains(&(name.into(), version.into())),
            )?;
        }
    }
    if !direct.is_subset(&found) {
        return Err(malformed(kind));
    }
    Ok(())
}
