//! Write scope: every document operation resolves to a parent directory fd opened
//! through an `O_NOFOLLOW|O_DIRECTORY` chain from a registered root, plus one final
//! name. Later `*at` calls use that fd, so the checked path is never re-resolved.
use crate::model::DocKey;
use rustix::fs::{AtFlags, CWD, FileType, Mode, OFlags, mkdirat, openat, readlinkat, statat};
use rustix::io::Errno;
use std::ffi::OsString;
use std::os::fd::OwnedFd;
use std::os::unix::ffi::OsStringExt;
use std::path::{Component, Path, PathBuf};

const MAX_LINKS: usize = 8;
const MAX_PATH: usize = 4096;

/// A registered root as the daemon composes it (project, agents folder, vault, data).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root {
    pub id: String,
    pub path: PathBuf,
}

/// Canonical roots. Roots that do not exist are skipped instead of failing requests.
#[derive(Debug, Clone, Default)]
pub struct WriteScope {
    roots: Vec<(String, PathBuf)>,
    /// Daemon-owned folders: refused (and their ancestors not movable) via any root.
    private: Vec<PathBuf>,
}

/// A checked location: `dir` is the opened parent, `name` one path component.
#[derive(Debug)]
pub struct Resolved {
    pub key: DocKey,
    pub dir: OwnedFd,
    pub dir_path: PathBuf,
    pub name: String,
    pub kind: Option<FileType>,
}
impl Resolved {
    pub fn abs(&self) -> PathBuf {
        self.dir_path.join(&self.name)
    }
}

/// Display never includes a path: internal errors are logged by the router.
#[derive(Debug, thiserror::Error)]
pub enum ScopeError {
    #[error("document path is invalid")]
    Invalid,
    #[error("document is outside the registered roots")]
    OutOfScope,
    /// The folder chain does not exist yet; carries the canonical key it would have.
    #[error("document folder is missing")]
    Missing(DocKey),
    #[error("document folder is missing")]
    NoFolder,
    #[error("document folder could not be opened: {0}")]
    Io(Errno),
}
fn opened(e: Errno) -> ScopeError {
    match e {
        Errno::NOENT => ScopeError::NoFolder,
        Errno::LOOP => ScopeError::OutOfScope,
        Errno::NOTDIR => ScopeError::Invalid,
        e => ScopeError::Io(e),
    }
}

/// True when any component is a devbox temp file, `.trash` or `.git`. Trees, watch,
/// note indexes, search and link rewriting all filter with this (CV-F06).
pub fn is_internal(rel: &str) -> bool {
    rel.split('/')
        .any(|c| c.starts_with(".devbox-tmp-") || c == ".trash" || c == ".git")
}

/// `.trash/<rest>` where `<rest>` has no other internal component.
pub fn in_trash(rel: &str) -> bool {
    rel.strip_prefix(".trash/")
        .is_some_and(|rest| !rest.is_empty() && !is_internal(rest))
}

pub(crate) fn validate(key: &DocKey) -> Result<(), ScopeError> {
    let path = key.path.as_str();
    let malformed = key.root.is_empty()
        || key.root.len() > 128
        || path.is_empty()
        || path.len() > MAX_PATH
        || path.contains('\0');
    if malformed {
        return Err(ScopeError::Invalid);
    }
    // Absolute paths and `..` try to leave the root: that is a scope refusal.
    if path.starts_with('/') || path.split('/').any(|c| c == "..") {
        return Err(ScopeError::OutOfScope);
    }
    if path
        .split('/')
        .any(|c| c.is_empty() || c == "." || c.starts_with(".devbox-tmp-"))
    {
        return Err(ScopeError::Invalid);
    }
    Ok(())
}

fn dir_flags() -> OFlags {
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC
}

impl WriteScope {
    pub fn new(roots: Vec<Root>) -> Self {
        let roots = roots
            .into_iter()
            .filter_map(|root| {
                let real = std::fs::canonicalize(&root.path).ok()?;
                real.is_dir().then_some((root.id, real))
            })
            .collect();
        Self {
            roots,
            private: Vec::new(),
        }
    }
    /// Refuses the daemon's own folders even when a root (e.g. `~`) contains them.
    pub fn deny(mut self, folders: &[PathBuf]) -> Self {
        // Both the resolved path and "resolved parent + name", so a symlink entry
        // (e.g. a systemd `.wants` link) is refused as itself, not only its target.
        for f in folders {
            self.private
                .push(std::fs::canonicalize(f).unwrap_or_else(|_| f.clone()));
            if let (Some(parent), Some(name)) = (f.parent(), f.file_name())
                && let Ok(parent) = std::fs::canonicalize(parent)
            {
                self.private.push(parent.join(name));
            }
        }
        self
    }
    pub fn root(&self, id: &str) -> Option<&Path> {
        self.roots
            .iter()
            .find(|(rid, _)| rid == id)
            .map(|(_, p)| p.as_path())
    }
    pub fn roots(&self) -> impl Iterator<Item = (&str, &Path)> {
        self.roots.iter().map(|(id, p)| (id.as_str(), p.as_path()))
    }
    /// Maps a real absolute path back to its key (deepest containing root).
    pub fn locate(&self, abs: &Path) -> Option<DocKey> {
        let (id, root) = self
            .roots
            .iter()
            .filter(|(_, root)| abs.starts_with(root) && abs != root)
            .max_by_key(|(_, root)| root.as_os_str().len())?;
        let rel = abs.strip_prefix(root).ok()?.to_str()?;
        Some(DocKey {
            root: id.clone(),
            path: rel.into(),
        })
    }
    /// Resolves a document, following a final symlink whose target stays in scope.
    pub fn resolve(&self, key: &DocKey, create: bool) -> Result<Resolved, ScopeError> {
        self.resolve_with(key, create, true)
    }
    /// Resolves the entry itself (a final symlink is moved, not followed).
    pub fn resolve_entry(&self, key: &DocKey, create: bool) -> Result<Resolved, ScopeError> {
        self.resolve_with(key, create, false)
    }
    fn resolve_with(
        &self,
        key: &DocKey,
        create: bool,
        follow: bool,
    ) -> Result<Resolved, ScopeError> {
        validate(key)?;
        let root = self.root(&key.root).ok_or(ScopeError::OutOfScope)?;
        let mut abs = root.join(&key.path);
        let mut preferred = key.root.clone();
        for _ in 0..MAX_LINKS {
            let (id, parents, name) = self.place(&abs, &preferred)?;
            let root = self.root(&id).ok_or(ScopeError::OutOfScope)?.to_path_buf();
            let mut path = parents.join("/");
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(&name);
            let key = DocKey { root: id, path };
            let dir = match open_chain(&root, &parents, create) {
                Err(ScopeError::NoFolder) => return Err(ScopeError::Missing(key)),
                other => other?,
            };
            let dir_path = parents.iter().fold(root, |p, c| p.join(c));
            let kind = match statat(&dir, name.as_str(), AtFlags::SYMLINK_NOFOLLOW) {
                Ok(stat) => Some(FileType::from_raw_mode(stat.st_mode)),
                Err(Errno::NOENT) => None,
                Err(e) => return Err(ScopeError::Io(e)),
            };
            if follow && kind == Some(FileType::Symlink) {
                let target = readlinkat(&dir, name.as_str(), Vec::new()).map_err(ScopeError::Io)?;
                let target = PathBuf::from(OsString::from_vec(target.into_bytes()));
                abs = if target.is_absolute() {
                    target
                } else {
                    dir_path.join(target)
                };
                preferred = key.root;
                continue;
            }
            return Ok(Resolved {
                key,
                dir,
                dir_path,
                name,
                kind,
            });
        }
        Err(ScopeError::OutOfScope)
    }
    /// Canonicalizes the existing part of the parent and picks the containing root.
    fn place(
        &self,
        abs: &Path,
        preferred: &str,
    ) -> Result<(String, Vec<String>, String), ScopeError> {
        let name = match abs.components().next_back() {
            Some(Component::Normal(name)) => name.to_str().ok_or(ScopeError::Invalid)?.to_owned(),
            _ => return Err(ScopeError::Invalid),
        };
        let mut existing = abs.parent().ok_or(ScopeError::Invalid)?.to_path_buf();
        let mut missing = Vec::new();
        let real = loop {
            match std::fs::canonicalize(&existing) {
                Ok(real) => break real,
                Err(_) => match existing.components().next_back() {
                    Some(Component::Normal(c)) => {
                        missing.push(c.to_str().ok_or(ScopeError::Invalid)?.to_owned());
                        existing.pop();
                    }
                    _ => return Err(ScopeError::Invalid),
                },
            }
        };
        let parent = missing.iter().rev().fold(real, |p, c| p.join(c));
        let full = parent.join(&name);
        if self
            .private
            .iter()
            .any(|p| full.starts_with(p) || p.starts_with(&full))
        {
            return Err(ScopeError::OutOfScope);
        }
        let (id, root) = self
            .roots
            .iter()
            .filter(|(_, root)| parent.starts_with(root))
            .max_by_key(|(id, root)| (id == preferred, root.as_os_str().len()))
            .ok_or(ScopeError::OutOfScope)?;
        let parents = parent
            .strip_prefix(root)
            .map_err(|_| ScopeError::OutOfScope)?
            .components()
            .map(|c| match c {
                Component::Normal(c) => c.to_str().map(str::to_owned).ok_or(ScopeError::Invalid),
                _ => Err(ScopeError::Invalid),
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok((id.clone(), parents, name))
    }
}

/// Opens `root/<parents…>` one component at a time without following symlinks.
pub(crate) fn open_chain(
    root: &Path,
    parents: &[String],
    create: bool,
) -> Result<OwnedFd, ScopeError> {
    let mut fd = openat(CWD, root, dir_flags(), Mode::empty()).map_err(opened)?;
    for part in parents {
        fd = match openat(&fd, part.as_str(), dir_flags(), Mode::empty()) {
            Ok(next) => next,
            Err(Errno::NOENT) if create => {
                match mkdirat(&fd, part.as_str(), Mode::from_raw_mode(0o755)) {
                    Ok(()) | Err(Errno::EXIST) => {}
                    Err(e) => return Err(ScopeError::Io(e)),
                }
                openat(&fd, part.as_str(), dir_flags(), Mode::empty()).map_err(opened)?
            }
            Err(e) => return Err(opened(e)),
        };
    }
    Ok(fd)
}
