//! 컬렉션 폴더 감시. 루트를 처음 목록·읽기·쓰기할 때 그 컬렉션 폴더에만 재귀로 붙이고(프로젝트 전체가 아님),
//! 300ms 동안 조용해지면 `api.collections_changed`를 한 번 낸다.
use super::files::TEMP_PREFIX;
use crate::{ApiCollectionsChanged, CollectionRoot, CollectionsChange};
use devbox_core::Hub;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

const DEBOUNCE: Duration = Duration::from_millis(300);
/// 계속 편집돼도 이 시간 안에는 한 번 낸다.
const MAX_WAIT: Duration = Duration::from_secs(1);
/// 동시에 감시하는 루트 상한(inotify 자원 보호).
const MAX_ROOTS: usize = 64;

struct Entry {
    dir: PathBuf,
    /// 폴더 정체(dev, inode). `git checkout`으로 지웠다 다시 만든 폴더는 같은 경로라도 다시 붙인다.
    id: Option<(u64, u64)>,
    /// 삭제된 폴더의 inode가 새 폴더에 재사용되지 않도록 등록 수명 동안 고정한다.
    #[cfg(unix)]
    directory: std::fs::File,
    _watcher: RecommendedWatcher,
}

impl Entry {
    fn is_current(&self, dir: &std::path::Path, id: Option<(u64, u64)>) -> bool {
        let same = self.dir == dir && id.is_some() && self.id == id;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            // inotify의 삭제 이벤트를 아직 처리하지 않았어도 unlink는 즉시 보인다.
            same && self.directory.metadata().is_ok_and(|m| m.nlink() != 0)
        }
        #[cfg(not(unix))]
        same
    }
}

#[cfg(unix)]
fn identity(directory: &std::fs::File) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    directory.metadata().ok().map(|m| (m.dev(), m.ino()))
}

pub(crate) struct Watches {
    hub: Arc<Hub>,
    rev: Arc<AtomicU64>,
    roots: Mutex<HashMap<String, Entry>>,
}

fn interesting(event: &Event) -> bool {
    !event.kind.is_access()
        && (event.paths.is_empty()
            || event.paths.iter().any(|path| {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .is_none_or(|n| !n.starts_with(TEMP_PREFIX))
            }))
}

impl Watches {
    pub(crate) fn new(hub: Arc<Hub>) -> Self {
        Self {
            hub,
            rev: Arc::new(AtomicU64::new(0)),
            roots: Mutex::new(HashMap::new()),
        }
    }

    /// 감시가 없거나 다른 폴더를 보고 있으면 새로 붙인다. 실패하면 감시 없이 계속한다(목록은 화면이 다시 읽음).
    pub(crate) fn ensure(&self, key: String, root: CollectionRoot, dir: PathBuf) {
        let mut roots = self.roots.lock().expect("collection watches");
        #[cfg(unix)]
        let directory = {
            use nix::{
                fcntl::{OFlag, open},
                sys::stat::Mode,
            };
            match open(
                &dir,
                OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
                Mode::empty(),
            ) {
                Ok(fd) => std::fs::File::from(fd),
                Err(_) => {
                    tracing::warn!("open collection watch folder failed");
                    return;
                }
            }
        };
        #[cfg(unix)]
        let id = identity(&directory);
        #[cfg(not(unix))]
        let id = None;
        if roots.get(&key).is_some_and(|e| e.is_current(&dir, id)) {
            return;
        }
        if !roots.contains_key(&key) && roots.len() >= MAX_ROOTS {
            tracing::warn!("collection watch limit reached");
            return;
        }
        let (send, receive) = mpsc::channel(1);
        let watcher =
            notify::recommended_watcher(move |event: notify::Result<Event>| match event {
                Ok(event) if interesting(&event) => {
                    let _ = send.try_send(());
                }
                Ok(_) => (),
                Err(_) => {
                    let _ = send.try_send(());
                }
            });
        let Ok(mut watcher) = watcher else {
            tracing::warn!("start collection watcher failed");
            return;
        };
        if watcher.watch(&dir, RecursiveMode::Recursive).is_err() {
            tracing::warn!("watch collection folder failed");
            return;
        }
        tokio::spawn(debounce(receive, self.hub.clone(), self.rev.clone(), root));
        roots.insert(
            key,
            Entry {
                dir,
                id,
                #[cfg(unix)]
                directory,
                _watcher: watcher,
            },
        );
    }
}

/// 감시자가 사라지면(송신 쪽 drop) 끝난다.
async fn debounce(
    mut receive: mpsc::Receiver<()>,
    hub: Arc<Hub>,
    rev: Arc<AtomicU64>,
    root: CollectionRoot,
) {
    while receive.recv().await.is_some() {
        let deadline = tokio::time::Instant::now() + MAX_WAIT;
        loop {
            let quiet = (tokio::time::Instant::now() + DEBOUNCE).min(deadline);
            match tokio::time::timeout_at(quiet, receive.recv()).await {
                Ok(Some(())) if tokio::time::Instant::now() >= deadline => break,
                Ok(Some(())) => continue,
                Ok(None) => return,
                Err(_) => break,
            }
        }
        let next = rev.fetch_add(1, Ordering::AcqRel) + 1;
        hub.publish::<ApiCollectionsChanged>(
            next,
            &CollectionsChange {
                root: root.clone(),
                rev: next.to_string(),
            },
        );
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn deleted_watch_is_not_reused_when_the_path_reports_the_same_identity() {
        let fixture = tempfile::tempdir().expect("owned directory");
        let dir = fixture.path().join("api");
        std::fs::create_dir(&dir).expect("create collection");
        let watches = Watches::new(Arc::new(Hub::new()));
        watches.ensure("owned".into(), CollectionRoot::Personal, dir.clone());
        let roots = watches.roots.lock().expect("owned watch map");
        let entry = roots.get("owned").expect("registered watch");
        let reported_id = entry.id;
        assert!(entry.is_current(&dir, reported_id));
        std::fs::write(dir.join("ordinary.request.json"), "{}").expect("ordinary child edit");
        assert!(
            entry.is_current(&dir, reported_id),
            "child edits must keep the watch"
        );
        std::fs::remove_dir_all(&dir).expect("delete watched directory");
        std::fs::create_dir(&dir).expect("replace same path");
        // Inject the metadata boundary seen when a filesystem reuses dev/inode.
        // Do not rely on this machine's allocator reproducing that rare timing.
        assert!(
            !entry.is_current(&dir, reported_id),
            "a deleted registration is stale even when the path identity collides"
        );
    }
}
