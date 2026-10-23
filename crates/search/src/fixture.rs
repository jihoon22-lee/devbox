//! Owned temporary `index.db`, `devbox.db` and project folders for unit tests.
use crate::*;
use devbox_core::{Db, Hub, IndexDb};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub type T = Result<(), Box<dyn std::error::Error>>;

pub struct Sx {
    pub _tmp: tempfile::TempDir,
    pub base: PathBuf,
    pub hub: Arc<Hub>,
    pub index: IndexDb,
    pub db: Db,
    pub projects: Arc<Mutex<Vec<ProjectRoot>>>,
    pub svc: SearchService,
}

pub fn fixture() -> Sx {
    fixture_with(
        SearchSettings::default(),
        files::GitConfig::default(),
        |_| Vec::new(),
    )
}

/// `private` gets the fixture base folder.
pub fn fixture_with(
    settings: SearchSettings,
    git: files::GitConfig,
    private: impl FnOnce(&Path) -> Vec<PathBuf>,
) -> Sx {
    let tmp = tempfile::tempdir().expect("owned fixture directory");
    let base = tmp.path().canonicalize().expect("canonical fixture");
    let index = IndexDb::open(&base.join("index.db")).expect("fixture index");
    let db = Db::open(
        &base.join("devbox.db"),
        &base.join("backups"),
        "1.0.0",
        MIGRATIONS,
    )
    .expect("fixture db");
    let hub = Arc::new(Hub::new());
    let projects: Arc<Mutex<Vec<ProjectRoot>>> = Arc::default();
    let list = projects.clone();
    let roots: ProjectRoots = Arc::new(move || {
        let list = list.lock().expect("fixture projects").clone();
        Box::pin(async move { list })
    });
    let mut config = SearchConfig::new(settings, index.clone(), db.clone(), hub.clone(), roots);
    config.git = git;
    config.private = private(&base);
    config.replay_marker = Some(base.join("search-reindex"));
    let svc = SearchService::new(config).expect("search service");
    Sx {
        _tmp: tmp,
        base,
        hub,
        index,
        db,
        projects,
        svc,
    }
}

impl Sx {
    /// A new service over the same databases and marker, like a daemon restart.
    pub fn reopen(&mut self) {
        let list = self.projects.clone();
        let roots: ProjectRoots = Arc::new(move || {
            let list = list.lock().expect("fixture projects").clone();
            Box::pin(async move { list })
        });
        let mut config = SearchConfig::new(
            SearchSettings::default(),
            self.index.clone(),
            self.db.clone(),
            self.hub.clone(),
            roots,
        );
        config.replay_marker = Some(self.base.join("search-reindex"));
        self.svc = SearchService::new(config).expect("search service");
    }
    pub async fn index_note(&self, path: &str, title: &str, body: &str) -> T {
        self.svc
            .index_notes(vec![IndexedNote {
                path: path.into(),
                title: title.into(),
                body: body.into(),
            }])
            .await;
        Ok(())
    }
    pub async fn notes(&self, text: &str, limit: u32) -> Result<NoteHits, String> {
        self.svc
            .notes(NotesQuery {
                text: text.into(),
                limit,
            })
            .await
            .map_err(|e| format!("{e:?}"))
    }
    /// Creates `<base>/<id>` and registers it as project `id`.
    pub fn project(&self, id: &str) -> PathBuf {
        let path = self.base.join(id);
        std::fs::create_dir_all(&path).expect("fixture project");
        self.projects
            .lock()
            .expect("fixture projects")
            .push(ProjectRoot {
                id: id.into(),
                path: path.clone(),
            });
        path
    }
    /// Every chunk of one query, in order.
    pub async fn run(&self, q: SearchQuery) -> Result<Vec<SearchChunk>, SearchError> {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1024);
        let result = self
            .svc
            .run(q, tokio_util::sync::CancellationToken::new(), tx)
            .await;
        let mut out = Vec::new();
        while let Some(chunk) = rx.recv().await {
            out.push(chunk);
        }
        result.map_err(fail)?;
        Ok(out)
    }
}

pub fn write(path: &Path, text: impl AsRef<[u8]>) {
    std::fs::create_dir_all(path.parent().expect("fixture parent")).expect("fixture dir");
    std::fs::write(path, text).expect("write fixture file");
}

pub fn q(text: &str, scopes: &[Scope]) -> SearchQuery {
    SearchQuery {
        text: text.into(),
        scopes: scopes.to_vec(),
        project_ids: None,
        regex: false,
        case_sensitive: false,
        limit: 100,
    }
}

/// `snippet[from..to]` in UTF-16 units.
pub fn hl(snippet: &str, (from, to): (u32, u32)) -> String {
    let units: Vec<u16> = snippet.encode_utf16().collect();
    String::from_utf16(&units[from as usize..(to as usize).min(units.len())])
        .expect("range on char boundaries")
}

pub fn fail(e: devbox_protocol::Fail<SearchError>) -> SearchError {
    match e {
        devbox_protocol::Fail::Domain(e) => e,
        devbox_protocol::Fail::Internal(e) => panic!("internal error: {e}"),
    }
}

/// `(root, path)` of every hit in `chunks` of `scope`, sorted.
pub fn found(chunks: &[SearchChunk], scope: Scope) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = chunks
        .iter()
        .filter(|c| c.scope == scope)
        .flat_map(|c| c.items.iter().map(|h| (h.root.clone(), h.path.clone())))
        .collect();
    out.sort();
    out
}

/// An executable shell script at `path`.
pub fn script(path: &Path, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    write(path, format!("#!/bin/sh\n{body}\n"));
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod script");
    path.into()
}

pub fn git(dir: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.email=t@example.invalid", "-c", "user.name=t"])
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("run fixture git");
    assert!(status.success(), "fixture git {args:?}");
}
