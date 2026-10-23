//! Owned temporary `devbox.db`, config folder and a sink that fails on demand.
use crate::model::attribution::ProjectMatch;
use crate::model::event::{Observation, ObservationKind};
use crate::store::DbSink;
use crate::*;
use devbox_core::{Db, Hub};
use devbox_protocol::router::BoxFuture;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

pub type T = Result<(), Box<dyn std::error::Error>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Store,
    /// Nothing stored, `Database` error.
    Fail,
    /// The batch is committed, then the call reports `Database` (lost reply).
    CommitThenFail,
}

/// `DbSink` behind a switch; counts calls.
pub struct FlakySink {
    pub inner: DbSink,
    pub mode: Mutex<Mode>,
    pub calls: AtomicUsize,
}
impl SessionSink for FlakySink {
    fn insert(&self, batch: Vec<Session>) -> BoxFuture<Result<u64, SinkError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mode = *self.mode.lock().expect("fixture mode");
        let stored = self.inner.insert(batch);
        Box::pin(async move {
            match mode {
                Mode::Store => stored.await,
                Mode::Fail => Err(SinkError::Database("injected".into())),
                Mode::CommitThenFail => {
                    stored.await?;
                    Err(SinkError::Database("injected after commit".into()))
                }
            }
        })
    }
}

pub struct Ax {
    pub _tmp: tempfile::TempDir,
    pub config_dir: PathBuf,
    pub exports_dir: PathBuf,
    pub db: Db,
    pub hub: Arc<Hub>,
    pub sink: Arc<FlakySink>,
    pub projects: Arc<Mutex<Vec<ProjectMatch>>>,
    pub svc: Arc<ActivityService>,
    seq: AtomicU64,
}

pub fn fixture() -> Ax {
    fixture_with("")
}

/// `activity` is the `[activity]` table text in `config.toml`.
pub fn fixture_with(activity: &str) -> Ax {
    let tmp = tempfile::tempdir().expect("owned fixture directory");
    let base = tmp.path().canonicalize().expect("canonical fixture");
    let config_dir = base.join("config");
    std::fs::create_dir_all(&config_dir).expect("fixture config dir");
    let db = Db::open(
        &base.join("devbox.db"),
        &base.join("backups"),
        "1.0.0",
        MIGRATIONS,
    )
    .expect("fixture db");
    let hub = Arc::new(Hub::new());
    let sink = Arc::new(FlakySink {
        inner: DbSink(db.clone()),
        mode: Mutex::new(Mode::Store),
        calls: AtomicUsize::new(0),
    });
    let projects: Arc<Mutex<Vec<ProjectMatch>>> = Arc::default();
    let svc = service(&db, &hub, &sink, &projects, &config_dir, &base, activity);
    Ax {
        _tmp: tmp,
        exports_dir: base.join("exports"),
        config_dir,
        db,
        hub,
        sink,
        projects,
        svc,
        seq: AtomicU64::new(0),
    }
}

fn service(
    db: &Db,
    hub: &Arc<Hub>,
    sink: &Arc<FlakySink>,
    projects: &Arc<Mutex<Vec<ProjectMatch>>>,
    config_dir: &std::path::Path,
    base: &std::path::Path,
    activity: &str,
) -> Arc<ActivityService> {
    let table: toml::Table = activity.parse().expect("fixture [activity] toml");
    let list = projects.clone();
    let names: ProjectNames = Arc::new(move || {
        let list = list.lock().expect("fixture projects").clone();
        Box::pin(async move { list })
    });
    let mut config = ActivityConfig::new(
        db.clone(),
        hub.clone(),
        ActivitySettings::from_table(&table),
        config_dir.to_path_buf(),
        base.join("exports"),
        names,
    );
    config.sink = Some(sink.clone());
    Arc::new(ActivityService::new(config))
}

impl Ax {
    /// A new service on the same database and config (a daemon restart).
    pub fn restart(&mut self) {
        let base = self
            .config_dir
            .parent()
            .expect("fixture base")
            .to_path_buf();
        self.svc = service(
            &self.db,
            &self.hub,
            &self.sink,
            &self.projects,
            &self.config_dir,
            &base,
            "",
        );
    }
    pub fn set_mode(&self, mode: Mode) {
        *self.sink.mode.lock().expect("fixture mode") = mode;
    }
    fn obs(
        &self,
        kind: ObservationKind,
        app: Option<&str>,
        title: Option<&str>,
        at: i64,
        last: i64,
    ) -> Observation {
        Observation {
            seq: self.seq.fetch_add(1, Ordering::SeqCst) + 1,
            at_ms: at,
            kind,
            app: app.map(Into::into),
            title: title.map(Into::into),
            last_input_ms: last,
        }
    }
    pub fn fg(&self, app: &str, title: &str, at: i64, last: i64) -> Observation {
        self.obs(
            ObservationKind::Foreground,
            Some(app),
            Some(title),
            at,
            last,
        )
    }
    /// An input heartbeat as the collector sends it: the app, no title.
    pub fn input(&self, app: &str, at: i64) -> Observation {
        self.obs(ObservationKind::Input, Some(app), None, at, at)
    }
    pub fn idle(&self, at: i64, last: i64) -> Observation {
        self.obs(ObservationKind::Idle, None, None, at, last)
    }
    pub fn pause(&self, at: i64) -> Observation {
        self.obs(ObservationKind::Pause, None, None, at, at)
    }
    /// One batch of run `run-1` whose `upto` is its last `seq`.
    pub async fn send(&self, events: Vec<Observation>) -> R<Ingested> {
        let upto = events.iter().map(|e| e.seq).max().unwrap_or(0);
        self.svc
            .ingest(Ingest {
                batch: Some(IngestBatch {
                    run_id: "run-1".into(),
                    upto,
                }),
                events,
                dropped: 0,
            })
            .await
    }
    /// A batch of `run` whose `upto` is its last `seq`; `dropped` is the run's
    /// cumulative offline-drop count.
    pub async fn send_as(&self, run: &str, events: Vec<Observation>, dropped: u32) -> R<Ingested> {
        let upto = events.iter().map(|e| e.seq).max().unwrap_or(0);
        self.svc
            .ingest(Ingest {
                batch: Some(IngestBatch {
                    run_id: run.into(),
                    upto,
                }),
                events,
                dropped,
            })
            .await
    }
    pub async fn ingest(&self, event: Observation) -> R<Ingested> {
        self.send(vec![event]).await
    }
    pub async fn flush(&self) {
        self.svc.flush().await;
    }
    /// Every stored row, oldest first.
    pub async fn sessions(&self) -> Vec<Session> {
        store::overlapping(&self.db, i64::MIN, i64::MAX)
            .await
            .expect("read fixture sessions")
    }
    pub fn config(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }
}

pub fn session(id: &str, start_ms: i64) -> Session {
    Session {
        id: id.into(),
        app: "app.exe".into(),
        title: String::new(),
        project_id: None,
        start_ms,
        end_ms: start_ms + 1,
    }
}
