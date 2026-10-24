//! `notes.tree` paging: 1–256 rows and ≤ 768 KiB JSON per page. The first page
//! stores the listing under its index rev for 60 s (the latest two revs), so a
//! walk that started at one rev ends at that rev even if the index moves on.
use crate::R;
use crate::model::{NoteTree, NotesError, TreeItem, TreeParams};
use crate::service::{MAX_ROWS, NotesService};
use crate::vault::MAX_NOTES;
use devbox_protocol::Internal;
use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;

pub(crate) const PAGE_BYTES: usize = 768 * 1024;
pub(crate) const TTL_MS: u64 = 60_000;
const KEEP: usize = 2;

struct Snapshot {
    rev: u64,
    made_ms: u64,
    items: Arc<Vec<TreeItem>>,
    truncated: bool,
}
#[derive(Default)]
pub(crate) struct Snapshots {
    list: VecDeque<Snapshot>,
}
impl Snapshots {
    fn prune(&mut self, now: u64) {
        self.list
            .retain(|s| now.saturating_sub(s.made_ms) <= TTL_MS);
        while self.list.len() > KEEP {
            self.list.pop_front();
        }
    }
    fn get(&mut self, rev: u64, now: u64) -> Option<(Arc<Vec<TreeItem>>, bool)> {
        self.prune(now);
        self.list
            .iter()
            .find(|s| s.rev == rev)
            .map(|s| (s.items.clone(), s.truncated))
    }
    fn put(&mut self, rev: u64, now: u64, items: Arc<Vec<TreeItem>>, truncated: bool) {
        self.list.retain(|s| s.rev != rev);
        self.list.push_back(Snapshot {
            rev,
            made_ms: now,
            items,
            truncated,
        });
        self.prune(now);
    }
}

/// Notes plus their folders, ordered component by component.
fn items(notes: Vec<(String, String)>) -> (Vec<TreeItem>, bool) {
    let mut dirs = BTreeSet::new();
    for (path, _) in &notes {
        let mut at = 0;
        while let Some(i) = path[at..].find('/') {
            dirs.insert(path[..at + i].to_owned());
            at += i + 1;
        }
    }
    let mut all: Vec<TreeItem> = dirs
        .into_iter()
        .map(|path| TreeItem {
            title: path.rsplit('/').next().unwrap_or(&path).to_owned(),
            path,
            dir: true,
        })
        .chain(notes.into_iter().map(|(path, title)| TreeItem {
            path,
            dir: false,
            title,
        }))
        .collect();
    all.sort_by(|a, b| a.path.split('/').cmp(b.path.split('/')));
    let truncated = all.len() > MAX_NOTES;
    all.truncate(MAX_NOTES);
    (all, truncated)
}

fn parse_cursor(cursor: &str) -> Option<(u64, usize)> {
    let (rev, offset) = cursor.split_once(':')?;
    Some((rev.parse().ok()?, offset.parse().ok()?))
}

impl NotesService {
    pub async fn tree(&self, p: TreeParams) -> R<NoteTree> {
        let now = (self.s.clock)();
        let (rev, offset, items, truncated) = match p.cursor.as_deref() {
            None => {
                let rev = self.rev();
                let cached = self.s.tree.lock().expect("tree").get(rev, now);
                let (items, truncated) = match cached {
                    Some(hit) => hit,
                    None => {
                        let list = self.s.index.list().await.internal("read note index")?;
                        let (items, truncated) = items(list);
                        let items = Arc::new(items);
                        let mut tree = self.s.tree.lock().expect("tree");
                        tree.put(rev, now, items.clone(), truncated);
                        (items, truncated)
                    }
                };
                (rev, 0, items, truncated)
            }
            Some(cursor) => {
                let (rev, offset) = parse_cursor(cursor).ok_or(NotesError::InvalidInput)?;
                let hit = self.s.tree.lock().expect("tree").get(rev, now);
                let (items, truncated) = hit.ok_or(NotesError::StaleCursor)?;
                if offset > items.len() {
                    return Err(NotesError::InvalidInput.into());
                }
                (rev, offset, items, truncated)
            }
        };
        let (mut page, mut bytes) = (Vec::new(), 0);
        for item in items[offset..].iter().take(MAX_ROWS) {
            let size = serde_json::to_string(item).map_or(0, |s| s.len()) + 1;
            if !page.is_empty() && bytes + size > PAGE_BYTES {
                break;
            }
            bytes += size;
            page.push(item.clone());
        }
        let next = offset + page.len();
        let more = next < items.len();
        Ok(NoteTree {
            rev: rev.to_string(),
            items: page,
            next_cursor: more.then(|| format!("{rev}:{next}")),
            truncated: truncated && !more,
        })
    }
}
