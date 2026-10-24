//! Bounded polling watches both the worktree gitdir and shared refs directory.
use crate::GitError;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::sync::watch;
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
async fn stamp(dirs: &[PathBuf]) -> Result<String, GitError> {
    let mut pending = Vec::new();
    for dir in dirs {
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
        let meta = match tokio::fs::symlink_metadata(&path).await {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(GitError::Internal),
        };
        if meta.is_symlink() {
            continue;
        }
        if meta.is_dir() {
            let mut entries = tokio::fs::read_dir(&path)
                .await
                .map_err(|_| GitError::Internal)?;
            while let Some(e) = entries.next_entry().await.map_err(|_| GitError::Internal)? {
                pending.push(e.path());
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
pub struct GitWatcher {
    stop: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
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
    let dirs = directories(dir).await?;
    let mut previous = stamp(&dirs).await?;
    let (stop, mut cancel) = watch::channel(false);
    let task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        let mut changed = None;
        loop {
            tokio::select! {_=cancel.changed()=>break,_=interval.tick()=>{
            match stamp(&dirs).await {Ok(now)if now!=previous=>{previous=now;changed=Some(tokio::time::Instant::now());},Err(_)=>{on_change();break},_=>()}
            if changed.is_some_and(|last|last.elapsed()>=Duration::from_millis(300)){changed=None;on_change();}
            }}
        }
    });
    Ok(GitWatcher { stop, task })
}
