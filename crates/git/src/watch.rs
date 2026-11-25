//! Bounded polling watches both the worktree gitdir and shared refs directory.
//!
//! Every `POLL` (500 ms) one blocking walk stamps `HEAD`, `index`, `packed-refs` and `refs/**` of
//! the gitdir and the common dir. A changed stamp is reported once it has been still for `QUIET`
//! (300 ms), so `on_change` follows a change made outside the daemon by at most about 0.8 s.
//!
//! A watcher never stops silently. When its checkout is gone, its stamp keeps failing or no call
//! has used it for `IDLE`, it marks itself stopped and calls `on_change` one last time: whoever
//! shows that checkout re-reads it, and that read starts a new watcher.
use crate::GitError;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{sync::watch, time::Instant};
const POLL: Duration = Duration::from_millis(500);
const QUIET: Duration = Duration::from_millis(300);
/// A watcher no `git.*` call has used for this long stops. Correctness does not depend on the
/// value, because the stop is reported like a change (see the module text); it only sets how long
/// an unused watcher lingers and how often a view that never polls is asked to re-read. The app
/// reads `git.status` of the current project every 15 s while its window is visible
/// (`app/src/features/source/GitStatus.tsx`), so at 5 minutes (20 missed polls) a visible window
/// never loses that watcher. A view that stays mounted without polling re-reads on the last
/// report and so starts a new watcher every 5 minutes. With no such view (the window hidden on
/// another page, the app closed, another project chosen) the watcher is gone until the next call.
const IDLE: Duration = Duration::from_secs(300);
/// Consecutive failed stamps (a path changing type mid-walk, a permission error) before stopping.
const MAX_FAILURES: u32 = 3;
async fn directories(dir: &Path) -> Result<Vec<PathBuf>, GitError> {
    let mut dirs = Vec::new();
    for option in ["--absolute-git-dir", "--git-common-dir"] {
        let out = crate::run::git(dir, &["rev-parse", "--path-format=absolute", option]).await?;
        let path = PathBuf::from(out.trim_end_matches('\n'));
        if !dirs.contains(&path) {
            dirs.push(path);
        }
    }
    Ok(dirs)
}
/// The checkout and its Git directories.
struct Watched {
    checkout: PathBuf,
    dirs: Vec<PathBuf>,
}
fn missing(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::NotFound
}
/// `NotRepo` when the checkout or one of its Git directories is gone.
fn stamp(watched: &Watched) -> Result<String, GitError> {
    let mut pending = Vec::new();
    for dir in std::iter::once(&watched.checkout).chain(&watched.dirs) {
        if std::fs::symlink_metadata(dir).is_err_and(|e| missing(&e)) {
            return Err(GitError::NotRepo);
        }
    }
    for dir in &watched.dirs {
        for name in ["HEAD", "index", "packed-refs", "refs"] {
            pending.push(dir.join(name));
        }
    }
    let mut records = Vec::new();
    let mut count = 0;
    while let Some(path) = pending.pop() {
        count += 1;
        if count > 100_000 {
            return Err(GitError::LimitExceeded);
        }
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if missing(&e) => continue,
            Err(_) => return Err(GitError::Internal),
        };
        if meta.is_symlink() {
            continue;
        }
        if meta.is_dir() {
            let entries = match std::fs::read_dir(&path) {
                Ok(entries) => entries,
                // Git removed the emptied ref folder after the lstat above.
                Err(e) if missing(&e) => continue,
                Err(_) => return Err(GitError::Internal),
            };
            for entry in entries {
                pending.push(entry.map_err(|_| GitError::Internal)?.path());
            }
        } else {
            records.push(format!(
                "{}:{}:{:?}",
                path.display(),
                meta.len(),
                meta.modified().map_err(|_| GitError::Internal)?
            ));
        }
    }
    records.sort();
    Ok(crate::hunks::digest(records.join("\0").as_bytes()))
}
/// One stamp is one hop to the blocking pool, not one per file.
async fn stamp_blocking(watched: &Arc<Watched>) -> Result<String, GitError> {
    let watched = watched.clone();
    tokio::task::spawn_blocking(move || stamp(&watched))
        .await
        .map_err(|_| GitError::Internal)?
}
struct Use {
    at: Instant,
    stopped: bool,
}
pub struct GitWatcher {
    stop: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
    used: Arc<Mutex<Use>>,
}
impl GitWatcher {
    /// Records a use by a call. `false` when the watcher has stopped and must be replaced: under
    /// one lock with the stop, so a call never counts on a watcher that is stopping.
    pub fn touch(&self) -> bool {
        let mut used = self.used.lock().expect("watcher use");
        if !used.stopped {
            used.at = Instant::now();
        }
        !used.stopped
    }
    pub fn stopped(&self) -> bool {
        self.used.lock().expect("watcher use").stopped
    }
    pub fn last_used(&self) -> Instant {
        self.used.lock().expect("watcher use").at
    }
}
impl Drop for GitWatcher {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
        self.task.abort();
    }
}
pub async fn watch<F>(dir: &Path, on_change: F) -> Result<GitWatcher, GitError>
where
    F: Fn() + Send + 'static,
{
    let watched = Arc::new(Watched {
        checkout: dir.into(),
        dirs: directories(dir).await?,
    });
    let mut previous = stamp_blocking(&watched).await?;
    let (stop, mut cancel) = watch::channel(false);
    let used = Arc::new(Mutex::new(Use {
        at: Instant::now(),
        stopped: false,
    }));
    let state = used.clone();
    let task = tokio::spawn(async move {
        let mut unreported = false;
        let mut failures = 0;
        loop {
            let wait = if unreported { QUIET } else { POLL };
            tokio::select! {
                _ = cancel.changed() => return,
                _ = tokio::time::sleep(wait) => (),
            }
            {
                let mut state = state.lock().expect("watcher use");
                state.stopped = state.at.elapsed() >= IDLE;
                if state.stopped {
                    break;
                }
            }
            match stamp_blocking(&watched).await {
                Ok(now) if now != previous => {
                    (previous, unreported, failures) = (now, true, 0);
                }
                Ok(_) => {
                    failures = 0;
                    if std::mem::take(&mut unreported) {
                        on_change();
                    }
                }
                Err(GitError::NotRepo) => break,
                Err(_) => {
                    failures += 1;
                    if failures == MAX_FAILURES {
                        break;
                    }
                }
            }
        }
        state.lock().expect("watcher use").stopped = true;
        on_change();
    });
    Ok(GitWatcher { stop, task, used })
}
