use super::*;
use std::collections::BTreeSet;
pub(super) fn parse(text: &str, ms: &Manifests, out: &mut Inventory) -> Result<(), DepsError> {
    let kind = LockKind::Go;
    let go_mod = manifest(ms, "go.mod", kind)?;
    let mut require = false;
    let mut direct = BTreeSet::new();
    let mut required = BTreeSet::new();
    let mut module = false;
    let mut replaced = BTreeSet::new();
    for line in go_mod.lines() {
        let line = line.trim();
        if line.starts_with("module ") {
            module = true;
        }
        if line.starts_with("replace ") {
            let name = line
                .trim_start_matches("replace ")
                .split_whitespace()
                .next()
                .ok_or_else(|| malformed(kind))?;
            if name == "(" {
                return Err(DepsError::UnsupportedLock { lock: kind });
            }
            replaced.insert(name.to_owned());
        }
        if line == "require (" {
            require = true;
            continue;
        }
        if line == ")" {
            require = false;
            continue;
        }
        let entry = if require {
            Some(line)
        } else {
            line.strip_prefix("require ")
        };
        if let Some(entry) = entry {
            let (entry, comment) = entry.split_once("//").unwrap_or((entry, ""));
            if entry.trim().is_empty() {
                continue;
            }
            let words: Vec<_> = entry.split_whitespace().collect();
            if words.len() != 2 || !words[1].starts_with('v') {
                return Err(malformed(kind));
            }
            required.insert((words[0].to_owned(), words[1].to_owned()));
            if comment.trim() != "indirect" {
                direct.insert((words[0].to_owned(), words[1].to_owned()));
            }
        }
    }
    if !module {
        return Err(malformed(kind));
    }
    let mut count = 0;
    let mut found = BTreeSet::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        count += 1;
        if count > MAX_ITEMS * 2 {
            return Err(DepsError::LimitExceeded);
        }
        let words: Vec<_> = line.split_whitespace().collect();
        if words.len() != 3 || !words[1].starts_with('v') || !words[2].starts_with("h1:") {
            return Err(malformed(kind));
        }
        let version = words[1].strip_suffix("/go.mod").unwrap_or(words[1]);
        found.insert((words[0].to_owned(), version.to_owned()));
        let source = if replaced.contains(words[0]) {
            DependencySource::Unknown
        } else {
            DependencySource::Registry
        };
        out.add(
            Ecosystem::Go,
            words[0],
            version,
            source,
            direct.contains(&(words[0].into(), version.into())),
        )?;
    }
    if !required.is_subset(&found) {
        return Err(DepsError::UnsupportedLock { lock: kind });
    }
    Ok(())
}
