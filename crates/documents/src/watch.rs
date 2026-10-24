//! External change observation: the vault (recursive) and the parent folders of
//! claimed documents (non-recursive) through inotify, batched for 300 ms.
//! A change is published only when the content hash differs (R8).
use crate::model::{DiskRev, DocKey, DocumentChanged};
use crate::scope::is_internal;
use crate::sys::{open_dir, read_at};
use devbox_core::Hub;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

const BATCH: Duration = Duration::from_millis(300);

#[derive(Default)]
struct State {
    watcher: Option<RecommendedWatcher>,
    vault: Option<(String, PathBuf)>,
    dirs: HashMap<PathBuf, usize>,
    docs: HashMap<PathBuf, DocKey>,
    paths: HashMap<DocKey, PathBuf>,
    /// Last content hash seen per path (`None` = absent).
    known: HashMap<PathBuf, Option<String>>,
}

pub struct Watch {
    state: Mutex<State>,
    hub: Arc<Hub>,
    rev: Arc<AtomicU64>,
}

impl Watch {
    pub fn new(hub: Arc<Hub>, rev: Arc<AtomicU64>) -> Self {
        Self {
            state: Mutex::default(),
            hub,
            rev,
        }
    }
    /// Creates the inotify watcher and its batching task (needs a tokio runtime).
    pub fn start(self: &Arc<Self>) {
        let mut st = self.state.lock().expect("watch");
        if st.watcher.is_some() {
            return;
        }
        let (tx, mut rx) = mpsc::unbounded_channel::<Vec<PathBuf>>();
        let handler = move |event: notify::Result<notify::Event>| {
            // Our own hash reads raise open/close-nowrite events; only changes matter.
            if let Ok(event) = event
                && !matches!(event.kind, notify::EventKind::Access(_))
            {
                let _ = tx.send(event.paths);
            }
        };
        let config = notify::Config::default().with_follow_symlinks(false);
        match RecommendedWatcher::new(handler, config) {
            Ok(watcher) => st.watcher = Some(watcher),
            Err(_) => {
                tracing::warn!("document watcher unavailable");
                return;
            }
        }
        let watch = Arc::downgrade(self);
        tokio::spawn(async move {
            while let Some(first) = rx.recv().await {
                let mut batch: HashSet<PathBuf> = first.into_iter().collect();
                let deadline = tokio::time::sleep(BATCH);
                tokio::pin!(deadline);
                loop {
                    tokio::select! {
                        _ = &mut deadline => break,
                        more = rx.recv() => match more {
                            Some(paths) => batch.extend(paths),
                            None => break,
                        },
                    }
                }
                let Some(watch) = watch.upgrade() else { break };
                let _ = tokio::task::spawn_blocking(move || watch.check(batch)).await;
            }
        });
    }
    pub fn watch_vault(&self, id: &str, root: &Path) {
        let mut st = self.state.lock().expect("watch");
        let Some(watcher) = st.watcher.as_mut() else {
            return;
        };
        if watcher.watch(root, RecursiveMode::Recursive).is_ok() {
            st.vault = Some((id.into(), root.into()));
        } else {
            tracing::warn!("vault watch unavailable");
        }
    }
    fn in_vault(st: &State, path: &Path) -> bool {
        st.vault
            .as_ref()
            .is_some_and(|(_, root)| path.starts_with(root))
    }
    /// Watches a claimed document's folder and remembers its current hash.
    pub fn add(&self, key: &DocKey, abs: PathBuf, hash: Option<String>) {
        let mut st = self.state.lock().expect("watch");
        st.known.insert(abs.clone(), hash);
        if st.paths.contains_key(key) {
            return;
        }
        let dir = abs.parent().map(Path::to_path_buf).unwrap_or_default();
        let count = st.dirs.entry(dir.clone()).or_default();
        *count += 1;
        let first = *count == 1;
        if first
            && !Self::in_vault(&st, &dir)
            && let Some(watcher) = st.watcher.as_mut()
            && watcher.watch(&dir, RecursiveMode::NonRecursive).is_err()
        {
            tracing::debug!("document folder watch unavailable");
        }
        st.docs.insert(abs.clone(), key.clone());
        st.paths.insert(key.clone(), abs);
    }
    pub fn remove(&self, key: &DocKey) {
        let mut st = self.state.lock().expect("watch");
        let Some(abs) = st.paths.remove(key) else {
            return;
        };
        st.docs.remove(&abs);
        if !Self::in_vault(&st, &abs) {
            st.known.remove(&abs);
        }
        let dir = abs.parent().map(Path::to_path_buf).unwrap_or_default();
        let last = match st.dirs.get_mut(&dir) {
            Some(count) => {
                *count -= 1;
                *count == 0
            }
            None => false,
        };
        if last {
            st.dirs.remove(&dir);
            if !Self::in_vault(&st, &dir)
                && let Some(watcher) = st.watcher.as_mut()
            {
                let _ = watcher.unwatch(&dir);
            }
        }
    }
    #[cfg(test)]
    pub fn watching(&self, key: &DocKey) -> bool {
        self.state.lock().expect("watch").paths.contains_key(key)
    }
    /// Follows a move made by this service; the old path is known to be gone.
    pub fn rekey(&self, old: &DocKey, new: &DocKey, abs: PathBuf) {
        let (old_abs, hash) = {
            let st = self.state.lock().expect("watch");
            let Some(old_abs) = st.paths.get(old).cloned() else {
                return;
            };
            let hash = st.known.get(&old_abs).cloned().flatten();
            (old_abs, hash)
        };
        self.remove(old);
        self.note(&old_abs, None);
        self.add(new, abs, hash);
    }
    /// Records a revision this service wrote itself so its echo is not published.
    pub fn note(&self, abs: &Path, hash: Option<String>) {
        self.state
            .lock()
            .expect("watch")
            .known
            .insert(abs.into(), hash);
    }
    fn key_for(st: &State, path: &Path) -> Option<DocKey> {
        if let Some(key) = st.docs.get(path) {
            return Some(key.clone());
        }
        let (id, root) = st.vault.as_ref()?;
        let rel = path.strip_prefix(root).ok()?.to_str()?;
        (!rel.is_empty()).then(|| DocKey {
            root: id.clone(),
            path: rel.into(),
        })
    }
    fn check(&self, batch: HashSet<PathBuf>) {
        for path in batch {
            let Some(key) = Self::key_for(&self.state.lock().expect("watch"), &path) else {
                continue;
            };
            if is_internal(&key.path) {
                continue;
            }
            let Some(rev) = disk_rev(&path) else { continue };
            let hash = rev.as_ref().map(|r| r.hash.clone());
            {
                let mut st = self.state.lock().expect("watch");
                if st.known.get(&path) == Some(&hash) {
                    continue;
                }
                st.known.insert(path.clone(), hash);
            }
            let rev_no = self.rev.fetch_add(1, Ordering::SeqCst) + 1;
            self.hub
                .publish::<crate::DocumentsChanged>(rev_no, &DocumentChanged { key, rev });
        }
    }
}

/// `Some(None)` = absent, `None` = not a readable regular file (skip).
fn disk_rev(path: &Path) -> Option<Option<DiskRev>> {
    let (parent, name) = (path.parent()?, path.file_name()?.to_str()?);
    let dir = match open_dir(parent) {
        Ok(dir) => dir,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some(None),
        Err(_) => return None,
    };
    match read_at(&dir, name, 0) {
        Ok(file) => Some(file.map(|f| f.rev)),
        Err(_) => None,
    }
}
