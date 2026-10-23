//! Owned temporary vault, data folder and `index.db` for unit tests.
use crate::*;
use devbox_core::{Hub, IndexDb};
use devbox_documents::{DocumentsConfig, DocumentsService, Root, Roots};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

pub type T = Result<(), Box<dyn std::error::Error>>;
/// Local `(date, time)` shared with the services under test.
pub type FixedToday = Arc<std::sync::Mutex<(String, String)>>;

pub struct Nx {
    pub _tmp: tempfile::TempDir,
    pub home: PathBuf,
    pub data: PathBuf,
    pub hub: Arc<Hub>,
    pub now: Arc<AtomicU64>,
    /// Local `(date, time)` the services see.
    pub today: FixedToday,
    pub docs: Arc<DocumentsService>,
    pub svc: NotesService,
}

impl Nx {
    /// Services over `<tmp>/home/notes` (not created) without scanning.
    pub fn empty() -> Self {
        let tmp = tempfile::tempdir().expect("owned fixture directory");
        let base = tmp.path().canonicalize().expect("canonical fixture");
        let (home, data) = (base.join("home"), base.join("data"));
        std::fs::create_dir_all(&home).expect("fixture home");
        std::fs::create_dir_all(&data).expect("fixture data");
        let hub = Arc::new(Hub::new());
        let now = Arc::new(AtomicU64::new(1_000));
        let today = Arc::new(std::sync::Mutex::new(("2026-10-22".into(), "09:30".into())));
        let (docs, svc) = build(&home, &data, &hub, (&now, &today), None);
        Self {
            _tmp: tmp,
            home,
            data,
            hub,
            now,
            today,
            docs,
            svc,
        }
    }
    pub fn vault(&self) -> PathBuf {
        self.home.join("notes")
    }
    pub fn write(&self, rel: &str, text: impl AsRef<[u8]>) {
        let path = self.vault().join(rel);
        std::fs::create_dir_all(path.parent().expect("fixture parent")).expect("fixture dir");
        std::fs::write(path, text).expect("write fixture note");
    }
    pub fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.vault().join(rel)).expect("read fixture note")
    }
    pub fn advance(&self, ms: u64) {
        self.now.fetch_add(ms, Ordering::SeqCst);
    }
    /// Recreates both services over the same folders, like a daemon restart.
    pub fn restart(&mut self) {
        let clocks = (&self.now, &self.today);
        (self.docs, self.svc) = build(&self.home, &self.data, &self.hub, clocks, None);
    }
    /// [`Self::restart`] with a search sink; `replay` as in `NotesConfig::replay_sink`.
    pub fn restart_with(&mut self, sink: Arc<dyn NoteSink>, replay: bool) {
        let clocks = (&self.now, &self.today);
        let sink = Some((sink, replay));
        (self.docs, self.svc) = build(&self.home, &self.data, &self.hub, clocks, sink);
    }
    pub async fn index_title(&self, path: &str) -> Result<String, String> {
        self.svc
            .title(path)
            .await
            .map_err(|e| format!("{e:?}"))?
            .ok_or_else(|| "not indexed".into())
    }
    pub async fn resolve(&self, target: &str) -> Result<ResolvedLink, String> {
        self.resolve_from(target, None).await
    }
    pub async fn resolve_from(
        &self,
        target: &str,
        from: Option<&str>,
    ) -> Result<ResolvedLink, String> {
        self.svc
            .resolve(ResolveLink {
                target: target.into(),
                kind: LinkKind::Wiki,
                from: from.map(Into::into),
            })
            .await
            .map_err(|e| format!("{e:?}"))
    }
}

/// A vault with `notes`, scanned once.
pub async fn fixture(notes: &[(&str, &str)]) -> Nx {
    let n = Nx::empty();
    std::fs::create_dir_all(n.vault()).expect("fixture vault");
    for (path, text) in notes {
        n.write(path, text);
    }
    n.svc.rescan().await.expect("scan fixture vault");
    n
}

fn build(
    home: &Path,
    data: &Path,
    hub: &Arc<Hub>,
    (now, today): (&Arc<AtomicU64>, &FixedToday),
    sink: Option<(Arc<dyn NoteSink>, bool)>,
) -> (Arc<DocumentsService>, NotesService) {
    let settings =
        NotesSettings::from_table(&toml::Table::new(), home).expect("default note settings");
    let vault = settings.vault.clone();
    // A second registered root, like a project the daemon also serves.
    let project = home.join("project");
    std::fs::create_dir_all(&project).expect("fixture project root");
    let roots: Roots = Arc::new(move || {
        let (vault, project) = (vault.clone(), project.clone());
        Box::pin(async move {
            vec![
                Root {
                    id: "vault".into(),
                    path: vault,
                },
                Root {
                    id: "project:p".into(),
                    path: project,
                },
            ]
        })
    });
    let mut config = DocumentsConfig::new(data.into(), roots, hub.clone());
    config.private = vec![data.into()];
    let docs = Arc::new(DocumentsService::new(config));
    let index = IndexDb::open(&data.join("index.db")).expect("fixture index");
    let pc: PcName = Arc::new(|| Box::pin(async { "desk".to_string() }));
    let rename_dir = data.join("notes-rename");
    let mut config = NotesConfig::new(settings, docs.clone(), index, hub.clone(), pc, rename_dir);
    let clock = now.clone();
    config.clock = Arc::new(move || clock.load(Ordering::SeqCst));
    let today = today.clone();
    config.today = Arc::new(move || today.lock().expect("fixture today").clone());
    if let Some((sink, replay)) = sink {
        (config.sink, config.replay_sink) = (sink, replay);
    }
    (docs, NotesService::new(config).expect("notes service"))
}

pub fn domain<X: std::fmt::Debug>(r: R<X>) -> NotesError {
    match r {
        Err(devbox_protocol::Fail::Domain(e)) => e,
        other => panic!("expected a domain error, got {other:?}"),
    }
}

// Rename helpers (Task 5).
pub fn rename(from: &str, to: &str) -> RenameNote {
    RenameNote {
        path: from.into(),
        new_path: to.into(),
        rewrite_links: true,
    }
}
pub fn vault_key(path: &str) -> devbox_documents::DocKey {
    devbox_documents::DocKey {
        root: "vault".into(),
        path: path.into(),
    }
}
impl Nx {
    /// Starts a rename and waits (bounded) until it is no longer running.
    pub async fn renamed(&self, from: &str, to: &str) -> Result<RenameStatus, String> {
        let op = self
            .svc
            .rename(rename(from, to))
            .await
            .map_err(|e| format!("{e:?}"))?;
        self.settled(&op.op_id).await
    }
    pub async fn settled(&self, op: &str) -> Result<RenameStatus, String> {
        tokio::time::timeout(std::time::Duration::from_secs(10), self.svc.wait_rename(op))
            .await
            .map_err(|_| "rename still running after 10 s".to_string())?
            .map_err(|e| format!("{e:?}"))
    }
    /// A daemon restart: new services, first scan, then the rename resume of `start`.
    pub async fn reopen(&mut self) -> Result<(), String> {
        self.restart();
        self.svc.rescan().await.map_err(|e| format!("{e:?}"))?;
        self.svc.resume().await.map_err(|e| format!("{e:?}"))
    }
    pub fn rename_journals(&self) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(self.data.join("notes-rename")) else {
            return Vec::new();
        };
        let mut out: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        out.sort();
        out
    }
    /// `(inode, mtime ns)`: a rewrite replaces the file, so both change.
    pub fn identity(&self, rel: &str) -> (u64, i64) {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::metadata(self.vault().join(rel)).expect("fixture note metadata");
        (m.ino(), m.mtime_nsec() + m.mtime() * 1_000_000_000)
    }
    /// Opens `rel` in window `window`; with `draft`, also journals unsaved text.
    pub async fn open_note(
        &self,
        rel: &str,
        window: &str,
        draft: Option<(u64, &str)>,
    ) -> devbox_documents::Claimed {
        let claimed = self
            .docs
            .claim(
                tokio_util::sync::CancellationToken::new(),
                devbox_documents::ClaimDocument {
                    key: vault_key(rel),
                    policy: devbox_documents::DocPolicy {
                        kind: devbox_documents::DocKind::Note,
                    },
                    window_id: window.into(),
                },
            )
            .await
            .expect("claim fixture note")
            .expect("fixture owner is live");
        if let Some((seq, text)) = draft {
            self.docs
                .journal(devbox_documents::WriteJournal {
                    lease_id: claimed.lease_id.clone().expect("fixture lease"),
                    seq,
                    base_rev: claimed.rev.clone(),
                    text: text.into(),
                })
                .await
                .expect("journal fixture draft");
        }
        claimed
    }
}
pub fn failed(status: &RenameStatus) -> Vec<(&str, RewriteFailure)> {
    status
        .failed
        .iter()
        .map(|f| (f.path.as_str(), f.reason))
        .collect()
}
