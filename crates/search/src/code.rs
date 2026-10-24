//! Project code search: `ignore` walk (gitignore, user excludes, no hidden or
//! internal files, no symlinks) + `grep-searcher` over files up to 1 MiB.
use crate::model::{CHUNK, SearchHit};
use crate::snippet::{MAX_RANGES, excerpt, find_all};
use grep_regex::RegexMatcher;
use grep_searcher::SearcherBuilder;
use grep_searcher::sinks::Lossy;
use ignore::overrides::OverrideBuilder;
use ignore::{Walk, WalkBuilder};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio_util::sync::CancellationToken;

/// Larger files are skipped.
pub const MAX_FILE_BYTES: u64 = 1 << 20;
/// Line excerpt characters at most, and characters kept before the first match.
pub const MAX_LINE_CHARS: usize = 512;
const LINE_LEAD: usize = 128;

/// A folder to search; `root` is its documents root id (`project:<id>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRoot {
    pub root: String,
    pub path: PathBuf,
}

pub struct CodeSearch<'a> {
    pub matcher: &'a RegexMatcher,
    /// `[search] exclude` globs.
    pub exclude: &'a [String],
    /// Daemon-owned folders that are never read.
    pub private: &'a [PathBuf],
    pub limit: usize,
}

/// The walk every code and file-name search uses: the project's own ignore files
/// (also outside a Git work tree), `!`-prefixed user excludes, no hidden or
/// internal (`.trash`, `.devbox-tmp-*`, `.git`) entries, no private folders, no
/// symlinks followed, files up to [`MAX_FILE_BYTES`], sorted by name.
pub(crate) fn walker(
    root: &Path,
    exclude: &[String],
    private: &[PathBuf],
) -> Result<Walk, ignore::Error> {
    let mut overrides = OverrideBuilder::new(root);
    for glob in exclude {
        overrides.add(&format!("!{glob}"))?;
    }
    let private = private.to_vec();
    Ok(WalkBuilder::new(root)
        .hidden(true)
        .parents(false)
        .require_git(false)
        .follow_links(false)
        .max_filesize(Some(MAX_FILE_BYTES))
        .overrides(overrides.build()?)
        .sort_by_file_name(|a, b| a.cmp(b))
        .filter_entry(move |e| {
            let name = e.file_name().to_str().unwrap_or(".");
            !devbox_documents::is_internal(name) && !private.iter().any(|p| e.path().starts_with(p))
        })
        .build())
}

/// `/`-separated path of a regular (non-symlink) file below `root`.
fn regular(entry: &ignore::DirEntry, root: &Path) -> Option<String> {
    if !entry.file_type().is_some_and(|t| t.is_file()) {
        return None;
    }
    let rel = entry.path().strip_prefix(root).ok()?.to_str()?;
    (!rel.is_empty()).then(|| rel.to_owned())
}

/// The file's bytes when it is still a regular file of at most [`MAX_FILE_BYTES`],
/// opened without following a symlink (or blocking on a FIFO) put there meanwhile.
fn read_regular(path: &Path) -> Option<Vec<u8>> {
    use rustix::fs::OFlags;
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags((OFlags::NOFOLLOW | OFlags::NONBLOCK).bits() as i32)
        .open(path)
        .ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return None;
    }
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= MAX_FILE_BYTES).then_some(bytes)
}

/// Walks `roots` in order and hands hits to `emit` in chunks of [`CHUNK`].
/// Stops when `cancel` fires or `emit` returns `false`. `visited` counts files
/// opened. Binary files (a NUL byte) are skipped. Returns whether `limit` cut the results.
pub fn search(
    roots: &[CodeRoot],
    s: &CodeSearch<'_>,
    cancel: &CancellationToken,
    visited: &AtomicU64,
    emit: &mut dyn FnMut(Vec<SearchHit>) -> bool,
) -> bool {
    let mut searcher = SearcherBuilder::new().line_number(true).build();
    let (mut pending, mut total, mut truncated, mut stop) = (Vec::new(), 0usize, false, false);
    'roots: for root in roots {
        let Ok(walk) = walker(&root.path, s.exclude, s.private) else {
            continue;
        };
        for entry in walk {
            if cancel.is_cancelled() {
                return false;
            }
            let Some(rel) = entry.ok().and_then(|e| regular(&e, &root.path)) else {
                continue;
            };
            visited.fetch_add(1, Ordering::SeqCst);
            let Some(bytes) = read_regular(&root.path.join(&rel)) else {
                continue;
            };
            if bytes.contains(&0) {
                continue;
            }
            let sink = Lossy(|line: u64, text: &str| {
                if total == s.limit {
                    truncated = true;
                    stop = true;
                    return Ok(false);
                }
                let text = text.trim_end_matches(['\n', '\r']);
                let found = find_all(s.matcher, text, MAX_RANGES);
                let (snippet, ranges_utf16) = excerpt(text, &found, MAX_LINE_CHARS, LINE_LEAD);
                pending.push(SearchHit {
                    root: root.root.clone(),
                    path: rel.clone(),
                    title: None,
                    line: u32::try_from(line).ok(),
                    snippet,
                    ranges_utf16,
                });
                total += 1;
                if pending.len() == CHUNK && !emit(std::mem::take(&mut pending)) {
                    stop = true;
                    return Ok(false);
                }
                Ok(!cancel.is_cancelled())
            });
            let _ = searcher.search_slice(s.matcher, &bytes, sink);
            if stop {
                break 'roots;
            }
        }
    }
    if !pending.is_empty() && !cancel.is_cancelled() {
        emit(pending);
    }
    truncated
}

/// Relative `/` paths of the regular files a code search would read below `root`.
pub(crate) fn list(
    root: &Path,
    exclude: &[String],
    private: &[PathBuf],
    max: usize,
) -> (Vec<String>, bool) {
    let Ok(walk) = walker(root, exclude, private) else {
        return (Vec::new(), false);
    };
    let mut out = Vec::new();
    for rel in walk.filter_map(|e| e.ok().and_then(|e| regular(&e, root))) {
        if out.len() == max {
            return (out, true);
        }
        out.push(rel);
    }
    (out, false)
}
