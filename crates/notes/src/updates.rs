//! Index updates and the committed-document ordering barrier.
use crate::index::{SinkNote, analyze};
use crate::model::NotesChanged;
use crate::service::{NotesService, VAULT};
use crate::vault::{self, hidden, is_note};
use crate::{NotesChangedTopic, R};
use devbox_protocol::{Fail, Internal};
use std::sync::atomic::Ordering;
const CHANGED_PATHS: usize = 256;
const BATCH: usize = 500;

impl NotesService {
    /// Catch up to a snapshot taken before any IO. No Documents namespace or
    /// per-document lock is acquired here, and saves never wait for this consumer.
    /// Rename calls this before planning, independently of background scheduling.
    pub(crate) async fn sync_committed(&self) -> R<()> {
        let _guard = self.s.update.lock().await;
        let cursor = self.s.committed.load(Ordering::SeqCst);
        let changes = self.s.docs.committed_since(cursor);
        if changes.revision == cursor {
            return Ok(());
        }
        match changes.paths {
            Some(paths) => {
                self.refresh_locked(
                    paths
                        .into_iter()
                        .filter(|key| key.root == VAULT)
                        .map(|key| key.path)
                        .collect(),
                )
                .await?
            }
            None => {
                self.rescan_locked(true).await?;
            }
        }
        self.s.committed.store(changes.revision, Ordering::SeqCst);
        Ok(())
    }
    /// Full scan: unchanged hashes are skipped, missing notes removed.
    pub async fn rescan(&self) -> R<()> {
        let _guard = self.s.update.lock().await;
        let revision = self.s.docs.commit_revision();
        if self.rescan_locked(false).await? {
            self.s.committed.store(revision, Ordering::SeqCst);
        }
        Ok(())
    }
    async fn rescan_locked(&self, require_complete: bool) -> R<bool> {
        let vault = self.vault().to_path_buf();
        let known = self.s.index.hashes().await.internal("read note index")?;
        let (fresh, removed, bodies, truncated) = tokio::task::spawn_blocking(move || {
            let (files, truncated) = vault::scan(&vault);
            let mut seen = std::collections::HashSet::new();
            let (mut fresh, mut bodies) = (Vec::new(), Vec::new());
            for rel in files {
                seen.insert(rel.clone());
                let Some((hash, text)) = vault::read_note(&vault, &rel) else {
                    continue;
                };
                if known.get(&rel) == Some(&hash) {
                    continue;
                }
                let note = analyze(&rel, hash, text.as_deref());
                bodies.push(text.unwrap_or_default());
                fresh.push(note);
            }
            let removed: Vec<String> = known.into_keys().filter(|p| !seen.contains(p)).collect();
            (fresh, removed, bodies, truncated)
        })
        .await
        .internal("scan note vault")?;
        if require_complete && truncated {
            return Err(Fail::Internal(
                "note index catchup requires a complete vault scan".into(),
            ));
        }
        let mut changed: Vec<String> = fresh.iter().map(|n| n.path.clone()).collect();
        let mut fresh = fresh;
        let mut bodies = bodies;
        while !fresh.is_empty() {
            let rest = fresh.split_off(fresh.len().min(BATCH));
            let rest_bodies = bodies.split_off(bodies.len().min(BATCH));
            let sink = fresh.iter().zip(bodies).map(|(n, body)| SinkNote {
                path: n.path.clone(),
                title: n.title.clone(),
                body,
            });
            self.s.sink.indexed(sink.collect()).await;
            bodies = rest_bodies;
            self.s
                .index
                .upsert(fresh)
                .await
                .internal("write note index")?;
            fresh = rest;
        }
        if !removed.is_empty() {
            self.s.sink.removed(removed.clone()).await;
        }
        let gone = self
            .s
            .index
            .remove(removed)
            .await
            .internal("write note index")?;
        changed.extend(gone);
        self.changed(changed);
        Ok(!truncated)
    }

    /// Sends every note to the sink, unchanged ones too (its index was rebuilt or
    /// missed writes), then the full list so it drops the rest.
    /// Holds the update lock, so no newer refresh is overtaken.
    pub(crate) async fn replay(&self) -> R<()> {
        let _guard = self.s.update.lock().await;
        let vault = self.vault().to_path_buf();
        let root = vault.clone();
        let (files, _) = tokio::task::spawn_blocking(move || vault::scan(&root))
            .await
            .internal("scan note vault")?;
        let mut sent = Vec::with_capacity(files.len());
        for batch in files.chunks(BATCH) {
            let (root, batch) = (vault.clone(), batch.to_vec());
            let notes = tokio::task::spawn_blocking(move || {
                batch
                    .into_iter()
                    .filter_map(|rel| {
                        let (hash, text) = vault::read_note(&root, &rel)?;
                        let title = analyze(&rel, hash, text.as_deref()).title;
                        Some(SinkNote {
                            path: rel,
                            title,
                            body: text.unwrap_or_default(),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .await
            .internal("read note vault")?;
            sent.extend(notes.iter().map(|n| n.path.clone()));
            self.s.sink.indexed(notes).await;
        }
        self.s.sink.retain(sent).await;
        Ok(())
    }

    /// Re-reads the given vault paths (notes, or removed folders).
    pub(crate) async fn refresh(&self, paths: Vec<String>) -> R<()> {
        let _guard = self.s.update.lock().await;
        self.refresh_locked(paths).await
    }
    async fn refresh_locked(&self, paths: Vec<String>) -> R<()> {
        let mut changed = Vec::new();
        for path in paths {
            if hidden(&path) {
                continue;
            }
            let (root, rel) = (self.vault().to_path_buf(), path.clone());
            let read = tokio::task::spawn_blocking(move || vault::read_note(&root, &rel))
                .await
                .internal("read note")?;
            match read {
                Some((hash, text)) if is_note(&path) => {
                    let known = self.s.index.hash_of(path.clone()).await;
                    if known.internal("read note index")?.as_deref() == Some(hash.as_str()) {
                        continue;
                    }
                    let note = analyze(&path, hash, text.as_deref());
                    let sink = SinkNote {
                        path: note.path.clone(),
                        title: note.title.clone(),
                        body: text.unwrap_or_default(),
                    };
                    self.s.sink.indexed(vec![sink]).await;
                    self.s
                        .index
                        .upsert(vec![note])
                        .await
                        .internal("write note index")?;
                    changed.push(path);
                }
                Some(_) => {}
                None => {
                    self.s.sink.removed(vec![path.clone()]).await;
                    let gone = self.s.index.remove(vec![path]).await;
                    changed.extend(gone.internal("write note index")?);
                }
            }
        }
        self.changed(changed);
        Ok(())
    }
    fn changed(&self, mut paths: Vec<String>) {
        if paths.is_empty() {
            return;
        }
        paths.truncate(CHANGED_PATHS);
        let rev = self.s.rev.fetch_add(1, Ordering::SeqCst) + 1;
        self.s.hub.publish::<NotesChangedTopic>(
            rev,
            &NotesChanged {
                rev: rev.to_string(),
                paths,
            },
        );
    }
}
