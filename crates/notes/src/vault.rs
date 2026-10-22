//! Vault folder rules: creation, the read-only scan for indexing, note names and
//! trash paths. Writes go through `devbox-documents`; this module only reads.
use crate::templates::safe_relative_path;
use std::path::Path;

/// Notes indexed and listed at most (the tree's last page reports `truncated`).
pub const MAX_NOTES: usize = 20_000;
pub const TRASH: &str = ".trash";
pub const MAX_TITLE_BYTES: usize = 256;
const MAX_NAME_CHARS: usize = 120;

/// True when a component is hidden (`.git`, `.trash`, `.devbox-tmp-*`, dotfiles)
/// or internal to devbox (CV-F06).
pub fn hidden(rel: &str) -> bool {
    devbox_documents::is_internal(rel) || rel.split('/').any(|c| c.starts_with('.'))
}
pub fn is_note(rel: &str) -> bool {
    rel.len() > 3 && rel.to_ascii_lowercase().ends_with(".md")
}
/// A vault-relative note path the screen may name.
pub fn valid_note_path(rel: &str) -> bool {
    safe_relative_path(rel) && is_note(rel) && !hidden(rel) && rel.len() <= 1024
}
/// `""` (vault root) or a visible relative folder.
pub fn valid_folder(rel: &str) -> bool {
    rel.is_empty() || (safe_relative_path(rel) && !hidden(rel) && rel.len() <= 900)
}
pub fn join(folder: &str, name: &str) -> String {
    if folder.is_empty() {
        name.into()
    } else {
        format!("{folder}/{name}")
    }
}
pub fn file_stem(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.len().checked_sub(3) {
        Some(cut) if name[cut..].eq_ignore_ascii_case(".md") => name[..cut].into(),
        _ => name.into(),
    }
}

/// A file name for `title`: characters Windows or links cannot carry are dropped,
/// leading/trailing spaces and dots trimmed. `None` when nothing is left.
pub fn note_file_name(title: &str) -> Option<String> {
    let cleaned: String = title
        .chars()
        .filter(|c| !c.is_control() && !"/\\:*?\"<>|#[]^".contains(*c))
        .take(MAX_NAME_CHARS)
        .collect();
    let cleaned = cleaned.trim_matches(|c: char| c.is_whitespace() || c == '.');
    (!cleaned.is_empty()).then(|| format!("{cleaned}.md"))
}
/// `a/b.md` → `a/b 2.md` for the `n`th try (`n == 1` keeps the name).
pub fn with_suffix(path: &str, n: usize) -> String {
    if n <= 1 {
        return path.into();
    }
    match path.rsplit_once('.') {
        Some((stem, ext)) if !stem.ends_with('/') && !stem.is_empty() => {
            format!("{stem} {n}.{ext}")
        }
        _ => format!("{path} {n}"),
    }
}
pub fn trash_path(date: &str, path: &str, n: usize) -> String {
    format!("{TRASH}/{date}/{}", with_suffix(path, n))
}

/// Creates the vault folder when missing; `true` when this call created it.
pub fn ensure(vault: &Path) -> std::io::Result<bool> {
    if vault.is_dir() {
        return Ok(false);
    }
    if let Some(parent) = vault.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match std::fs::create_dir(vault) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(e),
    }
}

/// Visible `.md` files below the vault, sorted, without following symlinks.
/// Returns relative paths and whether the cap was hit.
pub fn scan(vault: &Path) -> (Vec<String>, bool) {
    let mut out = Vec::new();
    let mut stack = vec![(String::new(), vault.to_path_buf())];
    let mut truncated = false;
    while let Some((rel, dir)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            let child = join(&rel, &name);
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                stack.push((child, entry.path()));
            } else if kind.is_file() && is_note(&child) && !hidden(&child) {
                if out.len() >= MAX_NOTES {
                    truncated = true;
                    continue;
                }
                out.push(child);
            }
        }
    }
    out.sort();
    (out, truncated)
}

/// Reads `vault/rel` for indexing (read only). Every folder on the way must be a
/// real folder and the note a regular file: nothing reached through a symlink is
/// indexed. `None`: not such a file (any more). Text is `None` when too large or not UTF-8.
pub fn read_note(vault: &Path, rel: &str) -> Option<(String, Option<String>)> {
    use sha2::{Digest, Sha256};
    let parts: Vec<&str> = rel.split('/').collect();
    let mut abs = vault.to_path_buf();
    let mut meta = None;
    for (i, part) in parts.iter().enumerate() {
        abs.push(part);
        let m = std::fs::symlink_metadata(&abs).ok()?;
        let last = i + 1 == parts.len();
        if (last && !m.is_file()) || (!last && !m.is_dir()) {
            return None;
        }
        meta = Some(m);
    }
    let meta = meta?;
    if meta.len() > devbox_documents::NOTE_LIMIT {
        let hash = format!("large:{}", meta.len());
        return Some((hash, None));
    }
    let bytes = std::fs::read(&abs).ok()?;
    let hash = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Some((hash, String::from_utf8(bytes).ok()))
}
