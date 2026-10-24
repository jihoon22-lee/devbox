use super::*;
use serde::Deserialize;
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Manifest {
    pub workspaces: Option<serde::de::IgnoredAny>,
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
    #[serde(default)]
    pub dev_dependencies: BTreeMap<String, String>,
    #[serde(default)]
    pub optional_dependencies: BTreeMap<String, String>,
}
impl Manifest {
    pub fn requirements(&self) -> impl Iterator<Item = (&String, &String)> {
        self.dependencies
            .iter()
            .chain(&self.dev_dependencies)
            .chain(&self.optional_dependencies)
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Lock {
    lockfile_version: u32,
    packages: BTreeMap<String, Package>,
}
#[derive(Default, Deserialize)]
struct Package {
    name: Option<String>,
    version: Option<String>,
    resolved: Option<String>,
    #[serde(default)]
    link: bool,
    #[serde(flatten)]
    manifest: Manifest,
}
pub(super) fn registry_url(value: &str) -> bool {
    value.starts_with("https://registry.npmjs.org/")
        || value.starts_with("https://registry.yarnpkg.com/")
}
pub(super) fn source(resolved: Option<&str>) -> DependencySource {
    match resolved {
        Some(s) if s.starts_with("git") => DependencySource::Git,
        Some(s) if s.starts_with("file:") || s.starts_with("link:") => DependencySource::Local,
        Some(s) if registry_url(s) => DependencySource::Registry,
        _ => DependencySource::Unknown,
    }
}
pub(super) fn parse(text: &str, out: &mut Inventory) -> Result<(), DepsError> {
    let kind = LockKind::Npm;
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Header {
        lockfile_version: u32,
    }
    let h: Header = serde_json::from_str(text).map_err(|_| malformed(kind))?;
    if !matches!(h.lockfile_version, 2 | 3) {
        return Err(DepsError::UnsupportedLock { lock: kind });
    }
    let lock: Lock = serde_json::from_str(text).map_err(|_| malformed(kind))?;
    if lock.packages.len() > MAX_ITEMS {
        return Err(DepsError::LimitExceeded);
    }
    if !lock.packages.contains_key("") {
        return Err(malformed(kind));
    }
    let _ = lock.lockfile_version;
    let mut direct = std::collections::BTreeSet::new();
    for (path, root) in &lock.packages {
        // npm workspaces have real root package records and separate node_modules links.
        if path.split('/').any(|s| s == "node_modules") {
            continue;
        }
        for (name, _) in root.manifest.requirements() {
            let local = if path.is_empty() {
                format!("node_modules/{name}")
            } else {
                format!("{path}/node_modules/{name}")
            };
            if lock.packages.contains_key(&local) {
                direct.insert(local);
            } else {
                direct.insert(format!("node_modules/{name}"));
            }
        }
    }
    if direct.iter().any(|path| !lock.packages.contains_key(path)) {
        return Err(malformed(kind));
    }
    for (path, p) in &lock.packages {
        if path.is_empty() {
            continue;
        }
        let Some((_, name)) = path.rsplit_once("node_modules/") else {
            continue;
        };
        if p.link {
            out.add(
                Ecosystem::Npm,
                p.name.as_deref().unwrap_or(name),
                p.version.as_deref().unwrap_or("local"),
                DependencySource::Local,
                direct.contains(path),
            )?;
            continue;
        }
        let version = p.version.as_deref().ok_or_else(|| malformed(kind))?;
        out.add(
            Ecosystem::Npm,
            p.name.as_deref().unwrap_or(name),
            version,
            source(p.resolved.as_deref()),
            direct.contains(path),
        )?;
    }
    Ok(())
}
