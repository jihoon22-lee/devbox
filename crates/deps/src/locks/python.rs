use super::*;
use serde::Deserialize;
#[derive(Deserialize)]
struct Lock {
    version: Option<u32>,
    revision: Option<u32>,
    metadata: Option<Metadata>,
    package: Vec<Package>,
}
#[derive(Deserialize)]
struct Metadata {
    #[serde(rename = "lock-version")]
    lock_version: String,
}
#[derive(Deserialize)]
struct Package {
    name: String,
    version: Option<String>,
    source: Option<Source>,
    #[serde(default)]
    dependencies: VecOrMap,
    #[serde(default, rename = "optional-dependencies")]
    optional: BTreeMap<String, Vec<Reference>>,
    #[serde(default, rename = "dev-dependencies")]
    dev: BTreeMap<String, Vec<Reference>>,
}
#[derive(Default, Deserialize)]
#[serde(untagged)]
enum VecOrMap {
    List(Vec<Reference>),
    Map(BTreeMap<String, serde::de::IgnoredAny>),
    #[default]
    Empty,
}
#[derive(Deserialize)]
struct Reference {
    name: String,
    version: Option<String>,
}
#[derive(Deserialize)]
struct Source {
    registry: Option<String>,
    git: Option<String>,
    #[serde(rename = "virtual")]
    virtual_path: Option<String>,
    editable: Option<String>,
    directory: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    url: Option<String>,
}
#[derive(Deserialize)]
struct Manifest {
    project: Option<Project>,
    tool: Option<Tool>,
}
#[derive(Deserialize)]
struct Project {
    name: Option<String>,
    #[serde(default)]
    dependencies: Vec<String>,
    #[serde(default, rename = "optional-dependencies")]
    optional: BTreeMap<String, Vec<String>>,
}
#[derive(Deserialize)]
struct Tool {
    poetry: Option<Poetry>,
}
#[derive(Deserialize)]
struct Poetry {
    #[serde(default, rename = "dev-dependencies")]
    legacy_dev: BTreeMap<String, serde::de::IgnoredAny>,
    name: Option<String>,
    #[serde(default)]
    dependencies: BTreeMap<String, serde::de::IgnoredAny>,
    #[serde(default)]
    group: BTreeMap<String, Group>,
}
#[derive(Deserialize)]
struct Group {
    #[serde(default)]
    dependencies: BTreeMap<String, serde::de::IgnoredAny>,
}
pub(super) fn normalize(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if matches!(ch, '-' | '_' | '.') {
            if !out.ends_with('-') {
                out.push('-');
            }
        } else {
            out.push(ch.to_ascii_lowercase());
        }
    }
    out
}
pub(super) fn parse(
    kind: LockKind,
    text: &str,
    ms: &Manifests,
    out: &mut Inventory,
) -> Result<(), DepsError> {
    let lock: Lock = toml::from_str(text).map_err(|_| malformed(kind))?;
    match kind {
        LockKind::Uv if lock.version != Some(1) || lock.revision.is_some_and(|r| r > 3) => {
            return Err(DepsError::UnsupportedLock { lock: kind });
        }
        LockKind::Poetry
            if !lock
                .metadata
                .as_ref()
                .is_some_and(|m| matches!(m.lock_version.as_str(), "1.1" | "2.0" | "2.1")) =>
        {
            return Err(DepsError::UnsupportedLock { lock: kind });
        }
        _ => {}
    }
    if lock.package.len() > MAX_ITEMS {
        return Err(DepsError::LimitExceeded);
    }
    let m: Manifest =
        toml::from_str(manifest(ms, "pyproject.toml", kind)?).map_err(|_| malformed(kind))?;
    let mut roots = std::collections::BTreeSet::new();
    let mut names = std::collections::BTreeSet::new();
    if let Some(p) = m.project {
        if let Some(n) = p.name {
            roots.insert(normalize(&n));
        }
        for req in p
            .dependencies
            .into_iter()
            .chain(p.optional.into_values().flatten())
        {
            let n: String = req
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
                .collect();
            if n.is_empty() {
                return Err(malformed(kind));
            }
            names.insert(normalize(&n));
        }
    }
    if let Some(p) = m.tool.and_then(|t| t.poetry) {
        if let Some(n) = p.name {
            roots.insert(normalize(&n));
        }
        names.extend(
            p.dependencies
                .into_keys()
                .chain(p.legacy_dev.into_keys())
                .chain(
                    p.group
                        .into_values()
                        .flat_map(|g| g.dependencies.into_keys()),
                )
                .filter(|n| n != "python")
                .map(|n| normalize(&n)),
        );
    }
    let mut direct = std::collections::BTreeSet::new();
    if kind == LockKind::Uv {
        let mut found_root = false;
        for p in &lock.package {
            if roots.contains(&normalize(&p.name))
                && p.source
                    .as_ref()
                    .is_some_and(|s| s.virtual_path.is_some() || s.editable.is_some())
            {
                found_root = true;
                let refs = match &p.dependencies {
                    VecOrMap::List(refs) => refs.as_slice(),
                    _ => &[],
                };
                for r in refs
                    .iter()
                    .chain(p.optional.values().flatten())
                    .chain(p.dev.values().flatten())
                {
                    let n = normalize(&r.name);
                    let candidates: Vec<_> = lock
                        .package
                        .iter()
                        .filter(|p| {
                            normalize(&p.name) == n
                                && r.version
                                    .as_ref()
                                    .is_none_or(|v| p.version.as_ref() == Some(v))
                        })
                        .collect();
                    if candidates.len() != 1 {
                        return Err(DepsError::UnsupportedLock { lock: kind });
                    }
                    if let Some(v) = &candidates[0].version {
                        direct.insert((n, v.clone()));
                    }
                }
            }
        }
        if !found_root {
            return Err(DepsError::UnsupportedLock { lock: kind });
        }
    } else {
        for name in names {
            let candidates: Vec<_> = lock
                .package
                .iter()
                .filter(|p| normalize(&p.name) == name)
                .collect();
            if candidates.is_empty() {
                return Err(malformed(kind));
            }
            if candidates.len() > 1 {
                return Err(DepsError::UnsupportedLock { lock: kind });
            }
            if let Some(v) = candidates.first().and_then(|p| p.version.as_ref()) {
                direct.insert((name, v.clone()));
            }
        }
    }
    for p in lock.package {
        if let VecOrMap::Map(ref values) = p.dependencies
            && values.len() > MAX_ITEMS
        {
            return Err(DepsError::LimitExceeded);
        }
        let name = normalize(&p.name);
        let source = match &p.source {
            None if kind == LockKind::Poetry => DependencySource::Registry,
            Some(s) if s.git.is_some() || s.kind.as_deref() == Some("git") => DependencySource::Git,
            Some(s)
                if s.virtual_path.is_some()
                    || s.editable.is_some()
                    || s.directory.is_some()
                    || s.kind.as_deref() == Some("directory") =>
            {
                DependencySource::Local
            }
            Some(s)
                if s.registry
                    .as_deref()
                    .is_some_and(|u| u.trim_end_matches('/') == "https://pypi.org/simple")
                    || s.url
                        .as_deref()
                        .is_some_and(|u| u.trim_end_matches('/') == "https://pypi.org/simple") =>
            {
                DependencySource::Registry
            }
            _ => DependencySource::Unknown,
        };
        if roots.contains(&name) && source == DependencySource::Local {
            continue;
        }
        let version = p.version.ok_or_else(|| malformed(kind))?;
        out.add(
            Ecosystem::Python,
            &name,
            &version,
            source,
            direct.contains(&(name.clone(), version.clone())),
        )?;
    }
    Ok(())
}
