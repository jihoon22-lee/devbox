//! Owned temporary roots for unit tests. Nothing outside the temp dir is touched.
use crate::*;
use devbox_core::Hub;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio_util::sync::CancellationToken;

pub type T = Result<(), Box<dyn std::error::Error>>;

pub struct Fx {
    pub _tmp: tempfile::TempDir,
    pub vault: PathBuf,
    pub project: PathBuf,
    pub data: PathBuf,
    pub outside: PathBuf,
    pub hub: Arc<Hub>,
    pub now: Arc<AtomicU64>,
    pub svc: DocumentsService,
    tune: Arc<dyn Fn(&mut DocumentsConfig) + Send + Sync>,
}

impl Fx {
    pub fn new() -> Self {
        Self::with(|_| {})
    }
    pub fn with(tune: impl Fn(&mut DocumentsConfig) + Send + Sync + 'static) -> Self {
        let tmp = tempfile::tempdir().expect("owned fixture directory");
        let base = tmp
            .path()
            .canonicalize()
            .expect("canonical fixture directory");
        let (vault, project, data, outside) = (
            base.join("vault"),
            base.join("project"),
            base.join("data"),
            base.join("outside"),
        );
        for dir in [&vault, &project, &data, &outside] {
            std::fs::create_dir_all(dir).expect("create fixture root");
        }
        let hub = Arc::new(Hub::new());
        let now = Arc::new(AtomicU64::new(1_000));
        let tune: Arc<dyn Fn(&mut DocumentsConfig) + Send + Sync> = Arc::new(tune);
        let svc = build(&vault, &project, &data, &hub, &now, &tune);
        Self {
            _tmp: tmp,
            vault,
            project,
            data,
            outside,
            hub,
            now,
            svc,
            tune,
        }
    }
    /// Recreates the service over the same folders, like a daemon restart.
    pub fn restart(&mut self) {
        self.svc = build(
            &self.vault,
            &self.project,
            &self.data,
            &self.hub,
            &self.now,
            &self.tune,
        );
    }
    pub fn advance(&self, ms: u64) {
        self.now.fetch_add(ms, Ordering::SeqCst);
    }
    pub fn write(&self, rel: &str, text: &str) {
        let path = self.vault.join(rel);
        std::fs::create_dir_all(path.parent().expect("fixture parent")).expect("fixture dir");
        std::fs::write(path, text).expect("write fixture note");
    }
    pub fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.vault.join(rel)).expect("read fixture note")
    }
    pub fn journals(&self) -> Vec<PathBuf> {
        files(&self.data.join("journals"))
    }
    pub fn ledger(&self) -> Vec<PathBuf> {
        files(&self.data.join("documents/tmp"))
    }
}

fn build(
    vault: &Path,
    project: &Path,
    data: &Path,
    hub: &Arc<Hub>,
    now: &Arc<AtomicU64>,
    tune: &Arc<dyn Fn(&mut DocumentsConfig) + Send + Sync>,
) -> DocumentsService {
    let roots = vec![
        Root {
            id: "vault".into(),
            path: vault.into(),
        },
        Root {
            id: "project:p".into(),
            path: project.into(),
        },
    ];
    let roots: Roots = Arc::new(move || {
        let roots = roots.clone();
        Box::pin(async move { roots })
    });
    let mut config = DocumentsConfig::new(data.into(), roots, hub.clone());
    config.private = vec![data.into()];
    let clock = now.clone();
    config.clock = Arc::new(move || clock.load(Ordering::SeqCst));
    tune(&mut config);
    DocumentsService::new(config)
}

pub fn files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut out: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    out.sort();
    out
}
pub fn key(path: &str) -> DocKey {
    DocKey {
        root: "vault".into(),
        path: path.into(),
    }
}
pub fn note() -> DocPolicy {
    DocPolicy {
        kind: DocKind::Note,
    }
}
pub fn claim(path: &str, window: &str) -> ClaimDocument {
    ClaimDocument {
        key: key(path),
        policy: note(),
        window_id: window.into(),
    }
}
pub fn save(c: &Claimed, base: Option<DiskRev>, text: &str) -> SaveDocument {
    SaveDocument {
        lease_id: c.lease_id.clone().expect("fixture claim holds a lease"),
        seq: 1,
        base_rev: base,
        expect: if c.rev.is_some() {
            Expect::Present
        } else {
            Expect::Absent
        },
        text: text.into(),
    }
}
/// Claims with an owner that stays alive for the test.
pub async fn claimed(svc: &DocumentsService, path: &str, window: &str) -> Claimed {
    claim_as(svc, CancellationToken::new(), path, window).await
}
pub async fn claim_as(
    svc: &DocumentsService,
    owner: CancellationToken,
    path: &str,
    window: &str,
) -> Claimed {
    svc.claim(owner, claim(path, window))
        .await
        .map_err(|e| format!("{e:?}"))
        .expect("claim fixture document")
        .expect("fixture owner is live")
}
pub fn fixture_with_note(path: &str, text: &str) -> (Fx, PathBuf) {
    let fx = Fx::new();
    fx.write(path, text);
    let root = fx.vault.clone();
    (fx, root)
}
pub fn domain<T: std::fmt::Debug>(r: R<T>) -> DocumentsError {
    match r {
        Err(devbox_protocol::Fail::Domain(e)) => e,
        other => panic!("expected a domain error, got {other:?}"),
    }
}
