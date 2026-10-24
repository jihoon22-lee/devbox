//! `ActivityService`: ingest → sessionizer → privacy rules (again, server side) →
//! project attribution → batched writer, plus rules, status, summaries, exports.
use crate::model::ClosedSession;
use crate::model::attribution::{ProjectMatch, attribute_title};
use crate::model::event::{Observation, ObservationKind};
use crate::model::privacy::CompiledRules;
use crate::model::sessionizer::Sessionizer;
use crate::rules::{ActivitySettings, WriteError};
use crate::store::{DbSink, Session, SessionSink};
use crate::writer::Writer;
use crate::*;
use devbox_core::{Db, Hub};
use devbox_protocol::router::BoxFuture;
use devbox_protocol::{Fail, Internal};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

/// Registered projects as attribution candidates, read per batch.
pub type ProjectNames = Arc<dyn Fn() -> BoxFuture<Vec<ProjectMatch>> + Send + Sync>;

pub struct ActivityConfig {
    pub db: Db,
    pub hub: Arc<Hub>,
    pub settings: ActivitySettings,
    /// Holds `config.toml` (`[activity]`) and `.config.toml.lock`.
    pub config_dir: PathBuf,
    /// `<data>/exports`.
    pub exports_dir: PathBuf,
    pub projects: ProjectNames,
    /// Tests inject failures; `None` is `devbox.db`.
    pub sink: Option<Arc<dyn SessionSink>>,
}
impl ActivityConfig {
    pub fn new(
        db: Db,
        hub: Arc<Hub>,
        settings: ActivitySettings,
        config_dir: PathBuf,
        exports_dir: PathBuf,
        projects: ProjectNames,
    ) -> Self {
        Self {
            db,
            hub,
            settings,
            config_dir,
            exports_dir,
            projects,
            sink: None,
        }
    }
}

struct Rules {
    settings: ActivitySettings,
    compiled: Arc<CompiledRules>,
}

/// One app run's sessionizer. `opened` mirrors which observation opened each
/// session not yet emitted (the ported sessionizer does not expose it), so the
/// session id is the same when the same batch is replayed.
struct Run {
    id: Option<String>,
    upto: u64,
    sessionizer: Sessionizer,
    /// `(app, title, end)` of the open session, as the sessionizer keeps it.
    current: Option<(String, String, i64)>,
    opened: Vec<(String, String, i64, u64)>,
    /// Latest `lastInputMs` seen, where a run that stops without idle or pause ends.
    last_input: i64,
    /// The run's cumulative offline-drop count already added to the status.
    dropped: u32,
}
impl Run {
    fn new(id: Option<String>) -> Self {
        Self {
            id,
            upto: 0,
            sessionizer: Sessionizer::new(),
            current: None,
            opened: Vec::new(),
            last_input: i64::MIN,
            dropped: 0,
        }
    }
    fn observe(
        &mut self,
        seq: u64,
        app: String,
        title: String,
        at: i64,
        last: i64,
    ) -> Vec<ClosedSession> {
        match &mut self.current {
            Some((a, t, end)) if *a == app && *t == title => *end = (*end).max(at),
            Some((_, _, end)) => {
                let start = at.max(*end);
                self.opened.push((app.clone(), title.clone(), start, seq));
                self.current = Some((app.clone(), title.clone(), start));
            }
            None => {
                self.opened.push((app.clone(), title.clone(), at, seq));
                self.current = Some((app.clone(), title.clone(), at));
            }
        }
        self.sessionizer.observe_with_input(app, title, at, last)
    }
    /// Ids for what the sessionizer emitted; `drained`: it holds nothing now.
    fn identify(
        &mut self,
        closed: Vec<ClosedSession>,
        drained: bool,
    ) -> Vec<(String, ClosedSession)> {
        let out = closed
            .into_iter()
            .map(|s| (self.session_id(&s), s))
            .collect();
        if drained {
            self.current = None;
            self.opened.clear();
        }
        out
    }
    fn session_id(&mut self, s: &ClosedSession) -> String {
        let at = self
            .opened
            .iter()
            .position(|(a, t, start, _)| *a == s.app && *t == s.title && *start == s.start_ts);
        match (&self.id, at) {
            (Some(run), Some(at)) => {
                let (_, _, start, seq) = self.opened.remove(at);
                let digest = Sha256::digest(format!("{run}\0{seq}\0{start}"));
                digest
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()[..32]
                    .to_owned()
            }
            _ => uuid::Uuid::new_v4().simple().to_string(),
        }
    }
}

pub struct ActivityService {
    db: Db,
    hub: Arc<Hub>,
    writer: Arc<Writer>,
    rules: RwLock<Rules>,
    rules_rev: AtomicU64,
    rules_write: tokio::sync::Mutex<()>,
    runs: Mutex<Option<Run>>,
    config_dir: PathBuf,
    exports_dir: PathBuf,
    projects: ProjectNames,
}

fn cut(mut text: String, max: usize) -> String {
    if text.len() > max {
        let mut end = max;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    text
}

fn valid_run_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_RUN_ID_BYTES
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl ActivityService {
    pub fn new(config: ActivityConfig) -> Self {
        let sink = config
            .sink
            .unwrap_or_else(|| Arc::new(DbSink(config.db.clone())));
        let compiled = Arc::new(config.settings.compiled());
        Self {
            writer: Arc::new(Writer::new(sink, config.hub.clone())),
            db: config.db,
            hub: config.hub,
            rules: RwLock::new(Rules {
                settings: config.settings,
                compiled,
            }),
            rules_rev: AtomicU64::new(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0),
            ),
            rules_write: tokio::sync::Mutex::new(()),
            runs: Mutex::new(None),
            config_dir: config.config_dir,
            exports_dir: config.exports_dir,
            projects: config.projects,
        }
    }

    /// The writer's 1 s loop; call inside the daemon runtime.
    pub fn start(&self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(self.writer.clone().run())
    }

    /// Writes already closed sessions without splitting the live foreground run.
    pub async fn flush(&self) {
        self.writer.flush().await;
    }

    /// Finalizes the run after ingress has drained. Never counts the idle tail.
    /// Once taken from the sessionizer, every closed interval enters the writer
    /// before the next await. Failed writes remain queued for a later retry.
    /// This is graceful finalization, not durability across abrupt termination.
    pub async fn shutdown(&self) {
        let projects = (self.projects)().await;
        {
            let mut runs = self.runs.lock().expect("activity runs");
            if let Some(mut run) = runs.take() {
                let ended = run.sessionizer.close_at_last_input(run.last_input);
                let closed = run.identify(ended, true);
                self.writer.push(self.finish(closed, &projects));
            }
        }
        self.writer.flush().await;
    }

    /// Sessions closed by this batch are queued before returning, with no await
    /// in between, so a cancelled request cannot lose them.
    pub async fn ingest(&self, p: Ingest) -> R<Ingested> {
        if p.events.len() > MAX_EVENTS || p.batch.as_ref().is_some_and(|b| !valid_run_id(&b.run_id))
        {
            return Err(ActivityError::InvalidInput.into());
        }
        let projects = (self.projects)().await;
        let mut runs = self.runs.lock().expect("activity runs");
        let id = p.batch.as_ref().map(|b| b.run_id.clone());
        let mut closed = Vec::new();
        // A new app run: the previous one ended with the app; keep only its input.
        if runs.as_ref().is_none_or(|run| run.id != id) {
            if let Some(mut old) = runs.take() {
                let ended = old.sessionizer.close_at_last_input(old.last_input);
                closed.extend(old.identify(ended, true));
            }
            *runs = Some(Run::new(id));
        }
        let run = runs.as_mut().expect("current run");
        let replayed = p.batch.as_ref().is_some_and(|b| b.upto <= run.upto);
        let mut accepted = 0u32;
        if !replayed {
            // With a batch, `dropped` is the run's running total: add the increase.
            let added = match p.batch {
                Some(_) => p.dropped.saturating_sub(run.dropped),
                None => p.dropped,
            };
            run.dropped = run.dropped.max(p.dropped);
            self.writer.add_dropped(u64::from(added));
            for event in p.events {
                if p.batch.is_some() && event.seq <= run.upto {
                    continue;
                }
                accepted += 1;
                run.last_input = run.last_input.max(event.last_input_ms);
                let drained = matches!(event.kind, ObservationKind::Idle | ObservationKind::Pause);
                let emitted = apply(run, event);
                closed.extend(run.identify(emitted, drained));
            }
            if let Some(batch) = &p.batch {
                run.upto = batch.upto;
            }
        }
        let sessions = self.finish(closed, &projects);
        self.writer.push(sessions);
        Ok(Ingested { accepted })
    }

    /// Rules (always the daemon's), then attribution on the stored title.
    fn finish(
        &self,
        closed: Vec<(String, ClosedSession)>,
        projects: &[ProjectMatch],
    ) -> Vec<Session> {
        let rules = self.rules.read().expect("activity rules").compiled.clone();
        closed
            .into_iter()
            .filter(|(_, s)| s.end_ts > s.start_ts)
            .filter_map(|(id, s)| {
                // Rules see the whole app name and title; only the result is cut.
                let (app, title) = rules.apply(&s.app, &s.title)?;
                let (app, title) = (cut(app, MAX_APP_BYTES), cut(title, MAX_TITLE_BYTES));
                let project_id = (!title.is_empty())
                    .then(|| attribute_title(&title, projects).map(|p| p.project_id.clone()))
                    .flatten();
                Some(Session {
                    id,
                    app,
                    title,
                    project_id,
                    start_ms: s.start_ts,
                    end_ms: s.end_ts,
                })
            })
            .collect()
    }

    pub async fn status(&self, _p: NoParams) -> R<ActivityStatus> {
        Ok(self.writer.status())
    }

    pub async fn rules(&self, _p: NoParams) -> R<RulesView> {
        let rules = self.rules.read().expect("activity rules");
        Ok(RulesView {
            rev: self.rules_rev.load(Ordering::Acquire).to_string(),
            rules: rules.settings.rules.clone(),
            idle_threshold_ms: rules.settings.idle_threshold_ms,
        })
    }

    pub async fn set_rules(&self, p: SetRules) -> R<RulesRev> {
        if !crate::rules::valid_idle(p.idle_threshold_ms) {
            return Err(ActivityError::InvalidInput.into());
        }
        let compiled = CompiledRules::compile(&p.rules)
            .map_err(|problems| ActivityError::InvalidRules { problems })?;
        let _one = self.rules_write.lock().await;
        let (dir, rules, idle) = (
            self.config_dir.clone(),
            p.rules.clone(),
            p.idle_threshold_ms,
        );
        match tokio::task::spawn_blocking(move || crate::rules::write(&dir, &rules, idle))
            .await
            .internal("write activity rules")?
        {
            Ok(()) => {}
            Err(WriteError::Unavailable) => return Err(ActivityError::ConfigUnavailable.into()),
            Err(WriteError::Io(e)) => {
                return Err(Fail::Internal(format!("write activity rules: {e}")));
            }
        }
        *self.rules.write().expect("activity rules") = Rules {
            settings: ActivitySettings {
                rules: p.rules,
                idle_threshold_ms: p.idle_threshold_ms,
                readable: true,
            },
            compiled: Arc::new(compiled),
        };
        let rev = self.rules_rev.fetch_add(1, Ordering::AcqRel) + 1;
        self.hub.publish::<ActivityChangedTopic>(
            rev,
            &ActivityChanged {
                rev: rev.to_string(),
            },
        );
        Ok(RulesRev {
            rev: rev.to_string(),
        })
    }

    pub async fn day(&self, p: DayQuery) -> R<DayActivity> {
        crate::export::day(&self.db, p).await
    }
    pub async fn range(&self, p: RangeQuery) -> R<RangeActivity> {
        crate::export::range(&self.db, p).await
    }
    pub async fn export(&self, p: ExportQuery) -> R<Exported> {
        crate::export::export(&self.db, &self.exports_dir, p).await
    }
}

/// One observation through the ported sessionizer (Task 8 mapping).
fn apply(run: &mut Run, event: Observation) -> Vec<ClosedSession> {
    let last = event.last_input_ms;
    match event.kind {
        ObservationKind::Foreground => {
            let app = event.app.unwrap_or_else(|| "unknown".into());
            let title = event.title.unwrap_or_default();
            run.observe(event.seq, app, title, event.at_ms, last)
        }
        ObservationKind::Input => {
            let open = run.current.as_ref().map(|(a, t, _)| (a.clone(), t.clone()));
            let named = event.app.map(|app| (app, event.title.unwrap_or_default()));
            // The open session first: a heartbeat never splits it.
            match open.or(named) {
                Some((app, title)) => run.observe(event.seq, app, title, event.at_ms, last),
                None => Vec::new(),
            }
        }
        ObservationKind::Idle => run.sessionizer.close_at_last_input(last),
        ObservationKind::Pause => run.sessionizer.finish_all(event.at_ms),
    }
}
