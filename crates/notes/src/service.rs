//! `NotesService`: owns the note index, keeps it current from the vault scan and
//! `documents.changed`, and answers the read-side RPCs. Writes are in `ops.rs`,
//! tree paging in `tree.rs`.
use crate::index::{NoteIndex, NoteSink, SinkNote, analyze};
use crate::links::{markdown_note_target, normalize_link_key, path_key, pick};
use crate::model::*;
use crate::settings::NotesSettings;
use crate::vault::{self, hidden, is_note};
use crate::{NotesChangedTopic, R};
use devbox_core::{CoreError, Hub, HubMessage, IndexDb};
use devbox_documents::{DocKey, DocumentChanged, DocumentsError, DocumentsService};
use devbox_protocol::router::BoxFuture;
use devbox_protocol::{Fail, Internal, Topic};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Milliseconds from an arbitrary start (tests inject one).
pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;
/// Local `(YYYY-MM-DD, HH:MM)` now (tests inject one).
pub type Today = Arc<dyn Fn() -> (String, String) + Send + Sync>;
/// This PC's display name for `{{pc}}`.
pub type PcName = Arc<dyn Fn() -> BoxFuture<String> + Send + Sync>;
pub(crate) const VAULT: &str = "vault";
/// List responses: at most 256 rows (S1 rule).
pub(crate) const MAX_ROWS: usize = 256;
const CHANGED_PATHS: usize = 256;
const BATCH: usize = 500;

pub struct NotesConfig {
    pub settings: NotesSettings,
    pub documents: Arc<DocumentsService>,
    pub index: IndexDb,
    pub hub: Arc<Hub>,
    pub clock: Clock,
    pub today: Today,
    pub pc: PcName,
    pub sink: Arc<dyn NoteSink>,
    /// After the first scan, send every note to `sink` (its index was rebuilt).
    pub replay_sink: bool,
    /// `<data>/notes-rename`: rename progress journals.
    pub rename_dir: PathBuf,
    /// Test instances only (see [`crate::StopAt`]).
    pub stop: Option<crate::StopAt>,
}
impl NotesConfig {
    pub fn new(
        settings: NotesSettings,
        documents: Arc<DocumentsService>,
        index: IndexDb,
        hub: Arc<Hub>,
        pc: PcName,
        rename_dir: PathBuf,
    ) -> Self {
        let start = std::time::Instant::now();
        Self {
            settings,
            documents,
            index,
            hub,
            clock: Arc::new(move || start.elapsed().as_millis() as u64),
            today: Arc::new(|| {
                let now = chrono::Local::now();
                (
                    now.format("%Y-%m-%d").to_string(),
                    now.format("%H:%M").to_string(),
                )
            }),
            pc,
            sink: Arc::new(crate::NoSink),
            replay_sink: false,
            rename_dir,
            stop: None,
        }
    }
}

pub(crate) struct Inner {
    pub settings: NotesSettings,
    pub docs: Arc<DocumentsService>,
    pub index: NoteIndex,
    pub hub: Arc<Hub>,
    pub clock: Clock,
    pub today: Today,
    pub pc: PcName,
    pub sink: Arc<dyn NoteSink>,
    pub replay_sink: bool,
    pub rev: AtomicU64,
    pub tree: Mutex<crate::tree::Snapshots>,
    /// Serializes index updates (scan, watch events, own writes).
    pub update: tokio::sync::Mutex<()>,
    pub rename_dir: PathBuf,
    pub stop: Option<crate::StopAt>,
    pub renames: crate::rename::Renames,
    /// Runs after an append or summary read the note, before writing (tests edit there).
    #[cfg(test)]
    pub after_read: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

#[derive(Clone)]
pub struct NotesService {
    pub(crate) s: Arc<Inner>,
}

pub(crate) fn docs_fail(e: Fail<DocumentsError>) -> Fail<NotesError> {
    match e {
        Fail::Domain(d) => match d {
            DocumentsError::OutOfScope => NotesError::OutOfVault.into(),
            DocumentsError::NotFound => NotesError::NotFound.into(),
            DocumentsError::Exists | DocumentsError::Conflict { .. } => NotesError::Exists.into(),
            DocumentsError::TooLarge { limit } => NotesError::TooLarge { limit }.into(),
            DocumentsError::InvalidInput => NotesError::InvalidInput.into(),
            DocumentsError::LeaseLost { .. } => NotesError::Busy.into(),
            d => Fail::Internal(format!("documents: {d}")),
        },
        Fail::Internal(e) => Fail::Internal(e),
    }
}
pub(crate) fn invalid<T>() -> R<T> {
    Err(NotesError::InvalidInput.into())
}

impl NotesService {
    /// Synchronous: prepares the `notes` index domain once.
    pub fn new(c: NotesConfig) -> Result<Self, CoreError> {
        Ok(Self {
            s: Arc::new(Inner {
                index: NoteIndex::open(c.index)?,
                settings: c.settings,
                docs: c.documents,
                hub: c.hub,
                clock: c.clock,
                today: c.today,
                pc: c.pc,
                sink: c.sink,
                replay_sink: c.replay_sink,
                rev: AtomicU64::new(0),
                tree: Mutex::default(),
                update: tokio::sync::Mutex::new(()),
                rename_dir: c.rename_dir,
                stop: c.stop,
                renames: Default::default(),
                #[cfg(test)]
                after_read: Mutex::default(),
            }),
        })
    }
    /// Read through documents' guarded root/file descriptor path, never a bare filesystem read.
    pub async fn read(&self, p: NotePath) -> R<NoteContent> {
        if !vault::valid_note_path(&p.path) {
            return Err(NotesError::OutOfVault.into());
        }
        let disk = self
            .s
            .docs
            .read_disk(&self.key(&p.path), devbox_documents::DocKind::Note)
            .await
            .map_err(docs_fail)?;
        let rev = disk.rev.ok_or(NotesError::NotFound)?;
        if disk
            .read_only
            .as_ref()
            .is_some_and(|r| r.reason == devbox_documents::ReadOnlyReason::TooLarge)
        {
            return Err(NotesError::TooLarge {
                limit: devbox_documents::NOTE_LIMIT,
            }
            .into());
        }
        let text = disk.text.ok_or(NotesError::InvalidInput)?;
        Ok(NoteContent {
            path: p.path,
            text,
            rev,
        })
    }

    pub fn vault(&self) -> &Path {
        &self.s.settings.vault
    }
    pub fn settings(&self) -> &NotesSettings {
        &self.s.settings
    }
    pub(crate) fn key(&self, path: &str) -> DocKey {
        DocKey {
            root: VAULT.into(),
            path: path.into(),
        }
    }
    pub(crate) fn rev(&self) -> u64 {
        self.s.rev.load(Ordering::SeqCst)
    }

    /// Creates a missing vault (before `documents` starts watching it), subscribes to
    /// document changes, then scans. A vault created here gets `.gitignore` = `.trash/`;
    /// an existing vault's `.gitignore` is never read or changed. The handle ends
    /// after the first scan; index updates keep running.
    pub fn start(&self) -> tokio::task::JoinHandle<()> {
        let created = match vault::ensure(self.vault()) {
            Ok(created) => created,
            Err(_) => {
                tracing::warn!("note vault folder could not be created");
                false
            }
        };
        let mut changes = self
            .s
            .hub
            .subscribe(devbox_documents::DocumentsChanged::NAME);
        let weak = Arc::downgrade(&self.s);
        let svc = self.clone();
        tokio::spawn(async move {
            if created {
                let key = svc.key(".gitignore");
                let write = svc.s.docs.write_disk(
                    &key,
                    None,
                    devbox_documents::Expect::Absent,
                    ".trash/\n",
                );
                if write.await.is_err() {
                    tracing::warn!("new vault .gitignore was not written");
                }
            }
            if svc.rescan().await.is_err() {
                tracing::warn!("note index scan failed");
            }
            if svc.s.replay_sink && svc.replay().await.is_err() {
                tracing::warn!("notes were not replayed to search");
            }
            // Renames cut short by an earlier exit continue in the background.
            let resume = svc.clone();
            tokio::spawn(async move {
                if resume.resume().await.is_err() {
                    tracing::warn!("note rename journals were not resumed");
                }
            });
            drop(svc);
            tokio::spawn(async move {
                while let Some(message) = changes.recv().await {
                    let Some(s) = weak.upgrade() else { break };
                    let svc = NotesService { s };
                    let result = match message {
                        HubMessage::Resync => svc.rescan().await,
                        HubMessage::Event { payload, .. } => {
                            match serde_json::from_str::<DocumentChanged>(&payload) {
                                Ok(c) if c.key.root == VAULT => svc.refresh(vec![c.key.path]).await,
                                _ => Ok(()),
                            }
                        }
                    };
                    if result.is_err() {
                        tracing::warn!("note index update failed");
                    }
                }
            });
        })
    }

    /// Full scan: unchanged hashes are skipped, missing notes removed.
    pub async fn rescan(&self) -> R<()> {
        let _guard = self.s.update.lock().await;
        let vault = self.vault().to_path_buf();
        let known = self.s.index.hashes().await.internal("read note index")?;
        let (fresh, removed, bodies) = tokio::task::spawn_blocking(move || {
            let (files, _) = vault::scan(&vault);
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
            (fresh, removed, bodies)
        })
        .await
        .internal("scan note vault")?;
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
        Ok(())
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

    pub async fn resolve(&self, p: ResolveLink) -> R<ResolvedLink> {
        let from = match p.from.as_deref() {
            Some(from) if !vault::valid_note_path(from) => return invalid(),
            from => from,
        };
        if p.target.len() > 4096 || p.target.contains('\0') {
            return invalid();
        }
        let none = ResolvedLink {
            paths: Vec::new(),
            ambiguous: false,
        };
        let (key, exact) = match p.kind {
            LinkKind::Wiki => {
                let target = p.target.split('|').next().unwrap_or("");
                let target = target.split('#').next().unwrap_or("");
                (normalize_link_key(target), false)
            }
            LinkKind::Md => (
                markdown_note_target(from, &p.target).and_then(|(path, _)| path_key(&path)),
                true,
            ),
        };
        let Some(key) = key else { return Ok(none) };
        let r = self.pick(&key, from, exact, &mut HashMap::new()).await?;
        Ok(ResolvedLink {
            paths: r.paths,
            ambiguous: r.ambiguous,
        })
    }
    async fn pick(
        &self,
        key: &str,
        from: Option<&str>,
        exact: bool,
        cache: &mut HashMap<String, Vec<String>>,
    ) -> R<crate::links::Resolution> {
        let candidates = match cache.get(key) {
            Some(c) => c.clone(),
            None => {
                let c = self.s.index.candidates(key.into()).await;
                let c = c.internal("read note index")?;
                cache.insert(key.into(), c.clone());
                c
            }
        };
        let candidates = if exact {
            candidates
                .into_iter()
                .filter(|c| path_key(c).as_deref() == Some(key))
                .collect()
        } else {
            candidates
        };
        Ok(pick(key, from, candidates))
    }

    pub async fn backlinks(&self, p: NotePath) -> R<Backlinks> {
        if !vault::valid_note_path(&p.path) {
            return invalid();
        }
        let rev = self.rev().to_string();
        let hits = self.s.index.links_to(p.path.clone()).await;
        let mut cache = HashMap::new();
        let (mut items, mut truncated) = (Vec::new(), false);
        for ((src, kind, key, line, from, to), title) in hits.internal("read note index")? {
            let r = self
                .pick(&key, Some(&src), kind == "md", &mut cache)
                .await?;
            if r.ambiguous || r.paths != [p.path.as_str()] {
                continue;
            }
            if items.len() == MAX_ROWS {
                truncated = true;
                break;
            }
            items.push(Backlink {
                src,
                title,
                line,
                from_utf16: from,
                to_utf16: to,
            });
        }
        Ok(Backlinks {
            rev,
            items,
            truncated,
        })
    }
    pub async fn tags(&self, _: NoParams) -> R<Tags> {
        let rev = self.rev().to_string();
        let mut all = self.s.index.tags().await.internal("read note index")?;
        let truncated = all.len() > MAX_ROWS;
        all.truncate(MAX_ROWS);
        Ok(Tags {
            rev,
            items: all
                .into_iter()
                .map(|(tag, count)| TagCount { tag, count })
                .collect(),
            truncated,
        })
    }
    pub async fn templates(&self, _: NoParams) -> R<Templates> {
        let prefix = format!("{}/", self.s.settings.templates_folder);
        let list = self.s.index.list().await.internal("read note index")?;
        let items = list
            .into_iter()
            .filter_map(|(path, _)| {
                let name = vault::file_stem(path.strip_prefix(&prefix)?);
                (!path[prefix.len()..].contains('/')).then_some(TemplateItem { name, path })
            })
            .take(MAX_ROWS)
            .collect();
        Ok(Templates { items })
    }
    /// Index title of a note (tests and later tasks).
    pub async fn title(&self, path: &str) -> R<Option<String>> {
        self.s
            .index
            .title(path.into())
            .await
            .internal("read note index")
    }
}
