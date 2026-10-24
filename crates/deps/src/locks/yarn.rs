use super::node::{Manifest, source};
use super::*;
fn scalar(value: &str) -> Option<String> {
    if value.starts_with('"') {
        serde_json::from_str(value).ok()
    } else if !value.is_empty() && !value.contains(['\n', '\r', '"']) {
        Some(value.into())
    } else {
        None
    }
}
fn selectors(line: &str) -> Option<Vec<String>> {
    line.strip_suffix(':')?.split(", ").map(scalar).collect()
}
pub(super) fn parse(text: &str, ms: &Manifests, out: &mut Inventory) -> Result<(), DepsError> {
    let kind = LockKind::Yarn;
    if !text.lines().take(5).any(|l| l == "# yarn lockfile v1") {
        return Err(DepsError::UnsupportedLock { lock: kind });
    }
    let m: Manifest =
        serde_json::from_str(manifest(ms, "package.json", kind)?).map_err(|_| malformed(kind))?;
    if m.workspaces.is_some() {
        return Err(DepsError::UnsupportedLock { lock: kind });
    }
    let requirements: std::collections::BTreeSet<_> =
        m.requirements().map(|(n, v)| format!("{n}@{v}")).collect();
    let mut found = std::collections::BTreeSet::new();
    let mut keys = Vec::<String>::new();
    let mut version = None;
    let mut resolved = None;
    let mut count = 0;
    let emit = |keys: &[String],
                version: &Option<String>,
                resolved: &Option<String>,
                out: &mut Inventory|
     -> Result<(), DepsError> {
        if keys.is_empty() {
            return Ok(());
        }
        let version = version.as_deref().ok_or_else(|| malformed(kind))?;
        let mut name = None;
        for key in keys {
            let pos = key
                .char_indices()
                .find(|(i, c)| *i > 0 && *c == '@')
                .map(|(i, _)| i)
                .ok_or_else(|| malformed(kind))?;
            let n = &key[..pos];
            if name.is_some_and(|prev| prev != n) {
                return Err(DepsError::UnsupportedLock { lock: kind });
            }
            name = Some(n);
        }
        let name = name.ok_or_else(|| malformed(kind))?;
        if keys.iter().any(|key| key.contains("@npm:")) {
            return Err(DepsError::UnsupportedLock { lock: kind });
        }
        out.add(
            Ecosystem::Npm,
            name,
            version,
            source(resolved.as_deref()),
            keys.iter().any(|k| requirements.contains(k)),
        )
    };
    for line in text.lines() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        if !line.starts_with(' ') {
            emit(&keys, &version, &resolved, out)?;
            keys = selectors(line).ok_or_else(|| malformed(kind))?;
            found.extend(keys.iter().filter(|k| requirements.contains(*k)).cloned());
            if keys.iter().any(|k| k.len() < 3) {
                return Err(malformed(kind));
            }
            version = None;
            resolved = None;
            count += 1;
            if count > MAX_ITEMS {
                return Err(DepsError::LimitExceeded);
            }
        } else if let Some(value) = line.strip_prefix("  version ") {
            if version.is_some() {
                return Err(malformed(kind));
            }
            version = Some(scalar(value).ok_or_else(|| malformed(kind))?);
        } else if let Some(value) = line.strip_prefix("  resolved ") {
            if resolved.is_some() {
                return Err(malformed(kind));
            }
            resolved = Some(scalar(value).ok_or_else(|| malformed(kind))?);
        } else if keys.is_empty()
            || (!line.starts_with("    ")
                && !line.starts_with("  integrity ")
                && !line.starts_with("  dependencies:")
                && !line.starts_with("  optionalDependencies:"))
        {
            return Err(malformed(kind));
        }
    }
    if !requirements.is_subset(&found) {
        return Err(malformed(kind));
    }
    emit(&keys, &version, &resolved, out)
}
