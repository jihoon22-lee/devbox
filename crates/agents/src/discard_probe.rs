//! Test observation of each file whose bytes a discard hash reads, and caps on its Git
//! listings in place of the real one.
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
type Watch = Arc<dyn Fn(&Path) + Send + Sync>;
/// Keyed by the hashed directory, so each test sees only its own worktree.
static WATCHES: Mutex<Vec<(PathBuf, Watch)>> = Mutex::new(Vec::new());
/// Watching stops when this is dropped.
pub(crate) struct Guard(PathBuf);
/// Calls `watch` with each file, relative to `root`, just before its bytes are read.
pub(crate) fn watch(root: &Path, watch: impl Fn(&Path) + Send + Sync + 'static) -> Guard {
    let mut watches = WATCHES.lock().expect("discard probe");
    watches.push((root.to_owned(), Arc::new(watch)));
    Guard(root.to_owned())
}
pub(super) fn read(root: &Path, relative: &Path) {
    let watches: Vec<Watch> = WATCHES
        .lock()
        .expect("discard probe")
        .iter()
        .filter(|(watched, _)| watched == root)
        .map(|(_, watch)| watch.clone())
        .collect();
    for watch in watches {
        watch(relative);
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        if let Ok(mut watches) = WATCHES.lock() {
            watches.retain(|(watched, _)| watched != &self.0);
        }
    }
}
/// Listing caps in place of the real one, by worktree.
static CAPS: Mutex<Vec<(PathBuf, usize)>> = Mutex::new(Vec::new());
/// The cap applies until this is dropped.
pub(crate) struct Cap(PathBuf);
/// Caps each Git listing of `root` at `bytes`.
pub(crate) fn cap_listings(root: &Path, bytes: usize) -> Cap {
    let mut caps = CAPS.lock().expect("discard probe");
    caps.push((root.to_owned(), bytes));
    Cap(root.to_owned())
}
pub(super) fn listing_cap(root: &Path) -> Option<usize> {
    let caps = CAPS.lock().expect("discard probe");
    caps.iter()
        .find(|(capped, _)| capped == root)
        .map(|(_, bytes)| *bytes)
}
impl Drop for Cap {
    fn drop(&mut self) {
        if let Ok(mut caps) = CAPS.lock() {
            caps.retain(|(capped, _)| capped != &self.0);
        }
    }
}
