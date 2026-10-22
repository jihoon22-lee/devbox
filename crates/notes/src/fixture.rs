//! Owned temporary vault, data folder and `index.db` for unit tests.
use crate::*;
use devbox_core::{Hub, IndexDb};
use devbox_documents::{DocumentsConfig, DocumentsService, Root, Roots};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

pub type T = Result<(), Box<dyn std::error::Error>>;

pub struct Nx {
    pub _tmp: tempfile::TempDir,
    pub home: PathBuf,
    pub data: PathBuf,
    pub hub: Arc<Hub>,
    pub now: Arc<AtomicU64>,
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
        let (docs, svc) = build(&home, &data, &hub, &now);
        Self {
            _tmp: tmp,
            home,
            data,
            hub,
            now,
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
        (self.docs, self.svc) = build(&self.home, &self.data, &self.hub, &self.now);
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
    now: &Arc<AtomicU64>,
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
    let mut config = NotesConfig::new(settings, docs.clone(), index, hub.clone(), pc);
    let clock = now.clone();
    config.clock = Arc::new(move || clock.load(Ordering::SeqCst));
    config.today = Arc::new(|| ("2026-10-22".into(), "09:30".into()));
    (docs, NotesService::new(config).expect("notes service"))
}

pub fn domain<X: std::fmt::Debug>(r: R<X>) -> NotesError {
    match r {
        Err(devbox_protocol::Fail::Domain(e)) => e,
        other => panic!("expected a domain error, got {other:?}"),
    }
}
