use super::*;
use serde::Deserialize;
#[derive(Deserialize)]
struct Lock {
    version: u32,
    package: Vec<Package>,
}
#[derive(Deserialize)]
struct Package {
    name: String,
    version: String,
    source: Option<String>,
    #[serde(default)]
    dependencies: Vec<String>,
}
#[derive(Deserialize)]
struct Manifest {
    package: Option<Identity>,
    workspace: Option<Workspace>,
    #[serde(flatten)]
    declarations: Declarations,
    #[serde(default)]
    target: BTreeMap<String, Declarations>,
}
#[derive(Deserialize)]
struct Identity {
    name: String,
}
#[derive(Deserialize)]
struct Workspace {
    #[serde(default)]
    members: Vec<String>,
    #[serde(default)]
    dependencies: BTreeMap<String, Declaration>,
}
#[derive(Default, Deserialize)]
struct Declarations {
    #[serde(default)]
    dependencies: BTreeMap<String, Declaration>,
    #[serde(default, rename = "dev-dependencies")]
    dev: BTreeMap<String, Declaration>,
    #[serde(default, rename = "build-dependencies")]
    build: BTreeMap<String, Declaration>,
}
impl Declarations {
    fn entries(&self) -> impl Iterator<Item = (&String, &Declaration)> {
        self.dependencies.iter().chain(&self.dev).chain(&self.build)
    }
}
#[derive(Deserialize)]
#[serde(untagged)]
enum Declaration {
    Version(String),
    Detail {
        package: Option<String>,
        workspace: Option<bool>,
        version: Option<String>,
        path: Option<String>,
        git: Option<String>,
    },
}
fn declared_name<'a>(
    alias: &'a str,
    declaration: &'a Declaration,
    workspace: Option<&'a Workspace>,
) -> Result<&'a str, DepsError> {
    let kind = LockKind::Cargo;
    let declaration = match declaration {
        Declaration::Detail {
            workspace: Some(true),
            package: None,
            version: None,
            path: None,
            git: None,
        } => workspace
            .and_then(|w| w.dependencies.get(alias))
            .ok_or(DepsError::UnsupportedLock { lock: kind })?,
        Declaration::Detail {
            workspace: Some(true),
            ..
        } => return Err(DepsError::UnsupportedLock { lock: kind }),
        other => other,
    };
    match declaration {
        Declaration::Version(version) if !version.is_empty() => Ok(alias),
        Declaration::Detail {
            package,
            workspace,
            version,
            path,
            git,
        } if workspace != &Some(true) && (version.is_some() || path.is_some() || git.is_some()) => {
            Ok(package.as_deref().unwrap_or(alias))
        }
        _ => Err(DepsError::UnsupportedLock { lock: kind }),
    }
}
pub(super) fn parse(text: &str, ms: &Manifests, out: &mut Inventory) -> Result<(), DepsError> {
    let kind = LockKind::Cargo;
    #[derive(Deserialize)]
    struct Header {
        version: u32,
    }
    let header: Header = toml::from_str(text).map_err(|_| malformed(kind))?;
    if !matches!(header.version, 3 | 4) {
        return Err(DepsError::UnsupportedLock { lock: kind });
    }
    let lock: Lock = toml::from_str(text).map_err(|_| malformed(kind))?;
    let _ = lock.version;
    if lock.package.len() > MAX_ITEMS {
        return Err(DepsError::LimitExceeded);
    }
    let manifest: Manifest =
        toml::from_str(manifest(ms, "Cargo.toml", kind)?).map_err(|_| malformed(kind))?;
    let mut manifests = vec![manifest];
    let members = manifests[0]
        .workspace
        .as_ref()
        .map(|w| w.members.clone())
        .unwrap_or_default();
    if members.len() > 64 {
        return Err(DepsError::LimitExceeded);
    }
    for member in members {
        if member.contains(['*', '?']) {
            return Err(DepsError::UnsupportedLock { lock: kind });
        }
        let path = format!("{member}/Cargo.toml");
        let m: Manifest =
            toml::from_str(super::manifest(ms, &path, kind)?).map_err(|_| malformed(kind))?;
        if m.package.is_none() {
            return Err(malformed(kind));
        }
        manifests.push(m);
    }
    let roots: std::collections::BTreeSet<_> = manifests
        .iter()
        .filter_map(|m| m.package.as_ref().map(|p| p.name.clone()))
        .collect();
    for m in &manifests {
        let Some(identity) = &m.package else { continue };
        let root = lock
            .package
            .iter()
            .find(|p| p.name == identity.name && p.source.is_none())
            .ok_or_else(|| malformed(kind))?;
        if root.dependencies.len() > MAX_ITEMS {
            return Err(DepsError::LimitExceeded);
        }
        let references: std::collections::BTreeSet<_> = root
            .dependencies
            .iter()
            .filter_map(|r| r.split_whitespace().next())
            .collect();
        for (count, (alias, declaration)) in m
            .declarations
            .entries()
            .chain(m.target.values().flat_map(Declarations::entries))
            .enumerate()
        {
            if count >= MAX_ITEMS {
                return Err(DepsError::LimitExceeded);
            }
            let name = declared_name(alias, declaration, manifests[0].workspace.as_ref())?;
            if !references.contains(name) {
                return Err(malformed(kind));
            }
        }
    }
    if roots.is_empty()
        || roots.iter().any(|n| {
            !lock
                .package
                .iter()
                .any(|p| p.name == *n && p.source.is_none())
        })
    {
        return Err(malformed(kind));
    }
    let direct: Vec<&str> = lock
        .package
        .iter()
        .filter(|p| p.source.is_none() && roots.contains(&p.name))
        .flat_map(|p| p.dependencies.iter().map(String::as_str))
        .collect();
    let mut direct_indices = std::collections::BTreeSet::new();
    for reference in direct {
        let parts: Vec<_> = reference.split_whitespace().collect();
        if parts.is_empty() || parts.len() > 3 {
            return Err(malformed(kind));
        }
        let candidates: Vec<_> = lock
            .package
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                p.name == parts[0]
                    && parts.get(1).is_none_or(|v| *v == p.version)
                    && parts.get(2).is_none_or(|source| {
                        Some(source.trim_matches(['(', ')'])) == p.source.as_deref()
                    })
            })
            .map(|(i, _)| i)
            .collect();
        if candidates.len() != 1 {
            return Err(malformed(kind));
        }
        direct_indices.insert(candidates[0]);
    }
    for (index, p) in lock.package.iter().enumerate() {
        if p.source.is_none() && roots.contains(&p.name) {
            continue;
        }
        let source = match p.source.as_deref() {
            None => DependencySource::Local,
            Some(s) if s.starts_with("git+") => DependencySource::Git,
            Some(
                "registry+https://github.com/rust-lang/crates.io-index"
                | "sparse+https://index.crates.io/",
            ) => DependencySource::Registry,
            _ => DependencySource::Unknown,
        };
        let is_direct = direct_indices.contains(&index);
        out.add(Ecosystem::Cargo, &p.name, &p.version, source, is_direct)?;
    }
    Ok(())
}
pub(super) fn manifests(text: &str) -> Result<Vec<String>, DepsError> {
    let kind = LockKind::Cargo;
    let m: Manifest = toml::from_str(text).map_err(|_| malformed(kind))?;
    let members = m.workspace.map(|w| w.members).unwrap_or_default();
    if members.len() > 64 {
        return Err(DepsError::LimitExceeded);
    }
    members
        .into_iter()
        .map(|m| {
            if m.contains(['*', '?']) {
                Err(DepsError::UnsupportedLock { lock: kind })
            } else {
                Ok(format!("{m}/Cargo.toml"))
            }
        })
        .collect()
}
