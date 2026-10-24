//! `SearchService`: owns the note full-text index and its memory rev, answers the
//! RPCs and runs `search.query` streams (notes → files → code).
use crate::code::{CodeRoot, CodeSearch};
use crate::files::{FileNames, GitConfig};
use crate::fts::{IndexedNote, Mode, NoteIndex, fts_query};
use crate::model::*;
use crate::settings::SearchSettings;
use crate::{R, SearchChangedTopic, compile};
use devbox_core::{CoreError, Db, Hub, IndexDb};
use devbox_protocol::router::BoxFuture;
use devbox_protocol::{Ctx, Done, Internal, StreamOpened};
use grep_regex::RegexMatcher;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// A registered project (`id` without the `project:` prefix).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRoot {
    pub id: String,
    pub path: PathBuf,
}
/// Registered projects, read per query.
pub type ProjectRoots = Arc<dyn Fn() -> BoxFuture<Vec<ProjectRoot>> + Send + Sync>;

pub struct SearchConfig {
    pub settings: SearchSettings,
    pub index: IndexDb,
    pub db: Db,
    pub hub: Arc<Hub>,
    pub projects: ProjectRoots,
    /// Daemon-owned folders never read by code or file-name search.
    pub private: Vec<PathBuf>,
    pub git: GitConfig,
    /// Present while the index may lack writes that failed: the next start feeds
    /// every note again (`<data>/search-reindex`).
    pub replay_marker: Option<PathBuf>,
}
impl SearchConfig {
    pub fn new(
        settings: SearchSettings,
        index: IndexDb,
        db: Db,
        hub: Arc<Hub>,
        projects: ProjectRoots,
    ) -> Self {
        Self {
            settings,
            index,
            db,
            hub,
            projects,
            private: Vec::new(),
            git: GitConfig::default(),
            replay_marker: None,
        }
    }
}

pub(crate) struct Inner {
    pub settings: SearchSettings,
    pub notes: NoteIndex,
    pub rebuilt: bool,
    pub db: Db,
    pub hub: Arc<Hub>,
    pub projects: ProjectRoots,
    pub private: Vec<PathBuf>,
    pub files: FileNames,
    pub rev: AtomicU64,
    pub visited: Arc<AtomicU64>,
    pub replay_marker: Option<PathBuf>,
    /// Index writes that failed since this start.
    pub failures: AtomicU64,
    /// Tests: make every index write fail.
    #[cfg(test)]
    pub fail_writes: std::sync::atomic::AtomicBool,
}

#[derive(Clone)]
pub struct SearchService {
    pub(crate) s: Arc<Inner>,
}

/// Checks a query before any work: text 1..=1024 bytes without NUL, `limit`
/// 1..=500, one or more scopes, at most 256 project ids of at most 128 bytes.
pub(crate) fn validate(q: &SearchQuery) -> Result<(), SearchError> {
    let ids_ok = q.project_ids.as_ref().is_none_or(|ids| {
        ids.len() <= MAX_PROJECT_IDS && ids.iter().all(|id| !id.is_empty() && id.len() <= 128)
    });
    if !text_ok(&q.text)
        || !(1..=MAX_QUERY_LIMIT).contains(&q.limit)
        || q.scopes.is_empty()
        || !ids_ok
    {
        return Err(SearchError::InvalidInput);
    }
    Ok(())
}
fn text_ok(text: &str) -> bool {
    !text.trim().is_empty() && text.len() <= MAX_TEXT_BYTES && !text.contains('\0')
}

/// A checked query, ready to run.
struct Prepared {
    scopes: Vec<Scope>,
    text: String,
    regex: bool,
    case_sensitive: bool,
    limit: usize,
    matcher: RegexMatcher,
    roots: Vec<ProjectRoot>,
}

/// Sends chunks as soon as they exist (a held-back chunk would reach the screen,
/// and a closed stream would stop the walk, only after the next one). A scope
/// known at once (notes, files) marks its last chunk `truncated`; the code scope,
/// found while walking, ends with an empty chunk carrying `truncated`. The
/// stream's last chunk has `done`.
struct Chunks {
    out: mpsc::Sender<SearchChunk>,
    rev: String,
}
impl Chunks {
    async fn send(&self, scope: Scope, items: Vec<SearchHit>, truncated: bool, done: bool) -> bool {
        let chunk = SearchChunk {
            scope,
            rev: self.rev.clone(),
            items,
            done,
            truncated,
        };
        self.out.send(chunk).await.is_ok()
    }
    /// All of a scope's hits; at least one (possibly empty) chunk.
    async fn whole(
        &self,
        scope: Scope,
        items: Vec<SearchHit>,
        truncated: bool,
        last: bool,
    ) -> bool {
        let mut items = items.into_iter().peekable();
        loop {
            let chunk: Vec<SearchHit> = items.by_ref().take(CHUNK).collect();
            let end = items.peek().is_none();
            if !self.send(scope, chunk, truncated && end, last && end).await {
                return false;
            }
            if end {
                return true;
            }
        }
    }
}

impl SearchService {
    /// Synchronous: prepares the `search` index domain once.
    pub fn new(c: SearchConfig) -> Result<Self, CoreError> {
        let (notes, rebuilt) = NoteIndex::open(c.index, crate::fts::SCHEMA_VERSION)?;
        // A write failed before the last exit: feed every note again.
        let rebuilt = rebuilt || c.replay_marker.as_ref().is_some_and(|m| m.exists());
        Ok(Self {
            s: Arc::new(Inner {
                settings: c.settings,
                notes,
                rebuilt,
                db: c.db,
                hub: c.hub,
                projects: c.projects,
                private: c.private,
                files: FileNames::new(c.git),
                rev: AtomicU64::new(0),
                visited: Arc::default(),
                replay_marker: c.replay_marker,
                failures: AtomicU64::new(0),
                #[cfg(test)]
                fail_writes: Default::default(),
            }),
        })
    }
    /// The domain was (re)created at start, or a write failed before the last
    /// exit: every note must be fed again.
    pub fn rebuilt(&self) -> bool {
        self.s.rebuilt
    }
    pub fn rev(&self) -> u64 {
        self.s.rev.load(Ordering::SeqCst)
    }
    /// Files opened by code searches so far (tests).
    pub fn code_files_visited(&self) -> u64 {
        self.s.visited.load(Ordering::SeqCst)
    }
    fn changed(&self) {
        let rev = self.s.rev.fetch_add(1, Ordering::SeqCst) + 1;
        self.s.hub.publish::<SearchChangedTopic>(
            rev,
            &SearchChanged {
                rev: rev.to_string(),
            },
        );
    }

    /// Notes the notes index (re)read. Same content changes nothing (R8).
    pub async fn index_notes(&self, notes: Vec<IndexedNote>) {
        if let Some(true) = self.write_ok(self.s.notes.upsert(notes)).await {
            self.changed();
        }
    }
    /// Notes or folders the notes index dropped.
    pub async fn remove_notes(&self, paths: Vec<String>) {
        if let Some(true) = self.write_ok(self.s.notes.remove(paths)).await {
            self.changed();
        }
    }

    /// End of a full feed: `paths` are every note; others leave the index. With
    /// no write failure since this start, the replay marker is cleared.
    pub async fn retain(&self, paths: Vec<String>) {
        if self.write_ok(self.s.notes.retain(paths)).await == Some(true) {
            self.changed();
        }
        if self.s.failures.load(Ordering::SeqCst) == 0
            && let Some(marker) = &self.s.replay_marker
            && std::fs::remove_file(marker).is_err()
            && marker.exists()
        {
            tracing::warn!("search replay marker was not removed");
        }
    }

    /// The write's result, or `None` after recording the failure: the marker
    /// makes the next start feed every note again (same content is skipped by hash).
    async fn write_ok(
        &self,
        write: impl std::future::Future<Output = Result<bool, CoreError>>,
    ) -> Option<bool> {
        #[cfg(test)]
        if self.s.fail_writes.load(Ordering::SeqCst) {
            drop(write);
            return self.failed();
        }
        match write.await {
            Ok(changed) => Some(changed),
            Err(_) => self.failed(),
        }
    }
    fn failed(&self) -> Option<bool> {
        tracing::warn!("search note index write failed");
        self.s.failures.fetch_add(1, Ordering::SeqCst);
        if let Some(marker) = &self.s.replay_marker
            && std::fs::write(marker, b"").is_err()
        {
            tracing::warn!("search replay marker was not written");
        }
        None
    }

    /// FTS for plain text; a case-sensitive, regex or punctuation-only query scans stored text.
    fn mode(text: &str, matcher: &RegexMatcher, regex: bool, case_sensitive: bool) -> R<Mode> {
        if !regex
            && !case_sensitive
            && let Some(q) = fts_query(text)
        {
            let marks = compile(&q.highlight, true, false).internal("compile note highlight")?;
            return Ok(Mode::Fts(q.expr, marks));
        }
        Ok(Mode::Scan(matcher.clone()))
    }

    pub async fn notes(&self, p: NotesQuery) -> R<NoteHits> {
        if !text_ok(&p.text) || !(1..=MAX_NOTES_LIMIT).contains(&p.limit) {
            return Err(SearchError::InvalidInput.into());
        }
        let rev = self.rev().to_string();
        let matcher = compile(&p.text, false, false)?;
        let mode = Self::mode(&p.text, &matcher, false, false)?;
        let (items, truncated) = self
            .s
            .notes
            .search(mode, p.limit as usize, CancellationToken::new())
            .await
            .internal("read search index")?;
        Ok(NoteHits {
            rev,
            items,
            truncated,
        })
    }

    async fn prepare(&self, p: SearchQuery) -> R<Prepared> {
        validate(&p)?;
        let matcher = compile(&p.text, p.regex, p.case_sensitive)?;
        let all = (self.s.projects)().await;
        let roots = match &p.project_ids {
            None => all,
            Some(ids) => {
                let mut roots = Vec::new();
                for id in ids {
                    let root = all
                        .iter()
                        .find(|r| &r.id == id)
                        .ok_or(SearchError::ProjectNotFound)?;
                    roots.push(root.clone());
                }
                roots
            }
        };
        let mut scopes = p.scopes;
        scopes.sort();
        scopes.dedup();
        Ok(Prepared {
            scopes,
            text: p.text,
            regex: p.regex,
            case_sensitive: p.case_sensitive,
            limit: p.limit as usize,
            matcher,
            roots,
        })
    }

    /// Opens the stream and answers at once; chunks are written in the background
    /// as JSON lines until done or the stream closes (which stops the code walk).
    pub async fn query(&self, ctx: Ctx, p: SearchQuery) -> R<StreamOpened> {
        let prepared = self.prepare(p).await?;
        let ends = ctx
            .open_stream()
            .map_err(|_| SearchError::StreamUnsupported)?;
        let id = ends.id;
        let (tx, mut rx) = mpsc::channel(4);
        let svc = self.clone();
        // A client close cancels the walk; so does a failed write (a child token,
        // so the stream's own token stays live for `finish`).
        let cancel = ends.tx.closed().child_token();
        tokio::spawn(async move {
            let run = tokio::spawn({
                let cancel = cancel.clone();
                async move { svc.execute(prepared, cancel, tx).await }
            });
            while let Some(chunk) = rx.recv().await {
                let mut line = match serde_json::to_vec(&chunk) {
                    Ok(line) => line,
                    Err(_) => break,
                };
                line.push(b'\n');
                if ends.tx.write(&line).await.is_err() {
                    break;
                }
            }
            cancel.cancel();
            drop(rx);
            let _ = run.await;
            ends.tx.finish().await;
        });
        Ok(StreamOpened { stream_id: id })
    }

    /// Validates `p` and sends its chunks to `out` until done or `cancel`.
    pub async fn run(
        &self,
        p: SearchQuery,
        cancel: CancellationToken,
        out: mpsc::Sender<SearchChunk>,
    ) -> R<()> {
        let prepared = self.prepare(p).await?;
        self.execute(prepared, cancel, out).await;
        Ok(())
    }

    async fn execute(
        &self,
        p: Prepared,
        cancel: CancellationToken,
        out: mpsc::Sender<SearchChunk>,
    ) {
        let chunks = Chunks {
            out,
            rev: self.rev().to_string(),
        };
        for (i, scope) in p.scopes.iter().enumerate() {
            if cancel.is_cancelled() {
                return;
            }
            let last = i + 1 == p.scopes.len();
            let sent = match scope {
                Scope::Notes => self.notes_scope(&p, &cancel, &chunks, last).await,
                Scope::Files => self.files_scope(&p, &cancel, &chunks, last).await,
                Scope::Code => self.code_scope(&p, &cancel, &chunks, last).await,
            };
            if !sent {
                return;
            }
        }
    }

    async fn notes_scope(
        &self,
        p: &Prepared,
        cancel: &CancellationToken,
        chunks: &Chunks,
        last: bool,
    ) -> bool {
        let Ok(mode) = Self::mode(&p.text, &p.matcher, p.regex, p.case_sensitive) else {
            return chunks.whole(Scope::Notes, Vec::new(), false, last).await;
        };
        let (hits, truncated) = match self.s.notes.search(mode, p.limit, cancel.clone()).await {
            Ok(found) => found,
            Err(_) => {
                tracing::warn!("search note index read failed");
                (Vec::new(), false)
            }
        };
        let items = hits
            .into_iter()
            .map(|h| SearchHit {
                root: "vault".into(),
                path: h.path,
                title: Some(h.title),
                line: None,
                snippet: h.snippet,
                ranges_utf16: h.ranges_utf16,
            })
            .collect();
        chunks.whole(Scope::Notes, items, truncated, last).await
    }

    async fn files_scope(
        &self,
        p: &Prepared,
        cancel: &CancellationToken,
        chunks: &Chunks,
        last: bool,
    ) -> bool {
        let mut candidates: Vec<(String, String)> = match self.s.notes.paths().await {
            Ok(paths) => paths
                .into_iter()
                .map(|path| ("vault".to_string(), path))
                .collect(),
            Err(_) => Vec::new(),
        };
        let mut truncated = false;
        for root in &p.roots {
            // Each listing may run `git` for up to its timeout; a closed stream stops here.
            if cancel.is_cancelled() {
                return false;
            }
            let listing = self
                .s
                .files
                .list(&root.path, &self.s.settings.exclude, &self.s.private)
                .await;
            truncated |= listing.truncated;
            let id = format!("project:{}", root.id);
            candidates.extend(listing.names.iter().map(|n| (id.clone(), n.clone())));
        }
        // Scoring 100,000 names takes a while: off the async threads, and cancellable.
        let (text, matcher, cancel) = (
            p.text.clone(),
            p.regex.then(|| p.matcher.clone()),
            cancel.clone(),
        );
        let (case_sensitive, limit) = (p.case_sensitive, p.limit);
        let ranked = tokio::task::spawn_blocking(move || {
            let ranked = crate::files::rank(
                &text,
                matcher.as_ref(),
                candidates,
                case_sensitive,
                limit,
                &cancel,
            );
            (!cancel.is_cancelled()).then_some(ranked)
        })
        .await;
        let Ok(Some((items, cut))) = ranked else {
            return false;
        };
        chunks
            .whole(Scope::Files, items, truncated || cut, last)
            .await
    }

    async fn code_scope(
        &self,
        p: &Prepared,
        cancel: &CancellationToken,
        chunks: &Chunks,
        last: bool,
    ) -> bool {
        let roots = p
            .roots
            .iter()
            .map(|r| CodeRoot {
                root: format!("project:{}", r.id),
                path: r.path.clone(),
            })
            .collect();
        let (mut rx, walk) = self.walk(roots, p.matcher.clone(), p.limit, cancel.clone());
        while let Some(items) = rx.recv().await {
            if !chunks.send(Scope::Code, items, false, false).await {
                cancel.cancel();
                return false;
            }
        }
        let truncated = walk.await.unwrap_or(false);
        !cancel.is_cancelled() && chunks.send(Scope::Code, Vec::new(), truncated, last).await
    }

    /// Runs the blocking walk; chunks arrive on the receiver, the handle gives `truncated`.
    fn walk(
        &self,
        roots: Vec<CodeRoot>,
        matcher: RegexMatcher,
        limit: usize,
        cancel: CancellationToken,
    ) -> (
        mpsc::Receiver<Vec<SearchHit>>,
        tokio::task::JoinHandle<bool>,
    ) {
        let (tx, rx) = mpsc::channel(2);
        let (exclude, private) = (self.s.settings.exclude.clone(), self.s.private.clone());
        let visited = self.s.visited.clone();
        let walk = tokio::task::spawn_blocking(move || {
            let s = CodeSearch {
                matcher: &matcher,
                exclude: &exclude,
                private: &private,
                limit,
            };
            crate::code::search(&roots, &s, &cancel, &visited, &mut |chunk| {
                tx.blocking_send(chunk).is_ok()
            })
        });
        (rx, walk)
    }

    /// Code search for other callers (the S4 editor's project search): chunks of
    /// hits from `roots` until done or `cancel`.
    pub async fn code(
        &self,
        roots: Vec<CodeRoot>,
        text: String,
        regex: bool,
        case_sensitive: bool,
        limit: usize,
        cancel: CancellationToken,
    ) -> R<mpsc::Receiver<Vec<SearchHit>>> {
        if !text_ok(&text) || !(1..=MAX_QUERY_LIMIT as usize).contains(&limit) {
            return Err(SearchError::InvalidInput.into());
        }
        let matcher = compile(&text, regex, case_sensitive)?;
        Ok(self.walk(roots, matcher, limit, cancel).0)
    }

    pub async fn saved(&self, _: NoParams) -> R<SavedSearches> {
        crate::saved::list(&self.s.db).await
    }
    pub async fn save(&self, p: SaveSearch) -> R<SavedRef> {
        crate::saved::save(&self.s.db, p, now_ms()).await
    }
    pub async fn unsave(&self, p: SavedId) -> R<Done> {
        crate::saved::unsave(&self.s.db, p).await
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
