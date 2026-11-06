use crate::{R, RuntimeService, service::project_error};
use devbox_projects::{ListProjects, TrustPreviewStatus};
use devbox_protocol::Internal;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{sync::mpsc, task::JoinHandle, time::Instant};
use tokio_util::sync::CancellationToken;

pub(crate) const WATCHER_RETRY: Duration = Duration::from_secs(30);

pub(crate) struct WatchHandle {
    pub(crate) cancel: CancellationToken,
    pub(crate) join: JoinHandle<()>,
}

/// Builds the watcher. Creation can fail when the inotify instance limit is exhausted;
/// the error is a path-free reason, because notify's own text can contain paths.
pub(crate) type Make =
    fn(&mpsc::Sender<()>, &Arc<Mutex<BTreeSet<PathBuf>>>) -> Result<RecommendedWatcher, String>;

/// Path-free description of a notify failure, safe to log.
fn reason(error: &notify::Error) -> String {
    match &error.kind {
        notify::ErrorKind::MaxFilesWatch => "watch limit".into(),
        notify::ErrorKind::Io(source) => format!("{:?}", source.kind()),
        _ => "other".into(),
    }
}

pub(crate) fn create_watcher(
    send: &mpsc::Sender<()>,
    interesting: &Arc<Mutex<BTreeSet<PathBuf>>>,
) -> Result<RecommendedWatcher, String> {
    let send = send.clone();
    let event_paths = interesting.clone();
    notify::recommended_watcher(move |event: notify::Result<Event>| match event {
        Ok(event)
            if !event.kind.is_access()
                && (event.paths.is_empty()
                    || event.paths.iter().any(|path| {
                        event_paths
                            .lock()
                            .expect("config watch paths")
                            .contains(path)
                    })) =>
        {
            let _ = send.try_send(());
        }
        Err(_) => {
            tracing::warn!("execution config watch error");
            let _ = send.try_send(());
        }
        _ => (),
    })
    .map_err(|error| reason(&error))
}

/// Watch bookkeeping owned by the watch task.
struct Watching {
    watcher: Option<RecommendedWatcher>,
    watched: BTreeSet<PathBuf>,
    /// Directories whose failure was already warned about, so repeats stay quiet.
    failed: BTreeSet<PathBuf>,
}

pub(crate) fn fingerprint(digest: &str, trust: &TrustPreviewStatus) -> String {
    let status = match trust {
        TrustPreviewStatus::NotRequired => "none",
        TrustPreviewStatus::ApprovalRequired => "required",
        TrustPreviewStatus::Approved => "approved",
    };
    format!("{digest}:{status}")
}

impl RuntimeService {
    pub async fn initialize(self: &Arc<Self>) -> R<()> {
        self.initialize_with(create_watcher, WATCHER_RETRY).await
    }

    /// `make` and `retry` are injectable so tests can fail watcher creation briefly.
    pub(crate) async fn initialize_with(self: &Arc<Self>, make: Make, retry: Duration) -> R<()> {
        let mut slot = self.watcher.lock().await;
        if slot.is_some() {
            return Ok(());
        }
        self.initialize_runs().await?;
        // Any queued notification schedules a complete digest comparison, so one slot
        // coalesces bursts without retaining unbounded file event paths.
        let (send, mut receive) = mpsc::channel(1);
        let interesting = Arc::new(Mutex::new(BTreeSet::new()));
        let mut warned_creation = false;
        let watcher = match make(&send, &interesting) {
            Ok(watcher) => Some(watcher),
            Err(reason) => {
                warned_creation = true;
                tracing::warn!(reason, "start execution config watcher failed; retrying");
                None
            }
        };
        let mut state = Watching {
            watcher,
            watched: BTreeSet::new(),
            failed: BTreeSet::new(),
        };
        let mut project_events = self.hub.subscribe("projects.changed");
        let incomplete = self.refresh(&mut state, &interesting, false).await?;
        let cancel = CancellationToken::new();
        let stopped = cancel.clone();
        let service = self.clone();
        let join = tokio::spawn(async move {
            let mut deadline = None;
            // While watching is missing or incomplete, one timer retries it at most
            // once per `retry`; it also re-reads definitions as a polling fallback.
            let mut rearm = incomplete.then(|| Instant::now() + retry);
            loop {
                let wait = deadline.unwrap_or_else(|| Instant::now() + Duration::from_secs(86400));
                let rearm_at = rearm.unwrap_or(wait);
                tokio::select! {
                    _ = stopped.cancelled() => break,
                    _ = tokio::time::sleep_until(rearm_at), if rearm.is_some() => {
                        rearm = None;
                        if state.watcher.is_none() {
                            match make(&send, &interesting) {
                                Ok(watcher) => {
                                    if warned_creation {
                                        tracing::info!("execution config watcher created");
                                    }
                                    state.watcher = Some(watcher);
                                    deadline = Some(Instant::now());
                                }
                                Err(reason) => {
                                    tracing::debug!(reason, "execution config watcher still unavailable");
                                    rearm = Some(Instant::now() + retry);
                                }
                            }
                        } else {
                            deadline = Some(Instant::now());
                        }
                    },
                    event = receive.recv() => {
                        if event.is_none() { break; }
                        deadline = Some(Instant::now() + Duration::from_millis(300));
                    },
                    event = project_events.recv() => {
                        if event.is_none() { break; }
                        deadline = Some(Instant::now() + Duration::from_millis(300));
                    },
                    _ = tokio::time::sleep_until(wait), if deadline.is_some() => {
                        deadline = None;
                        rearm = match service.refresh(&mut state, &interesting, true).await {
                            Ok(true) => Some(Instant::now() + retry),
                            Ok(false) => None,
                            Err(_) => {
                                tracing::warn!("refresh execution definitions failed");
                                Some(Instant::now() + retry)
                            }
                        };
                    },
                }
            }
        });
        *slot = Some(WatchHandle { cancel, join });
        if self.execution.is_some() {
            *self.scheduler.lock().await =
                Some(crate::scheduler::Scheduler::launch(self.clone()).await?);
        }
        Ok(())
    }

    /// End dependency/readiness coordination before the daemon drains accepted mutations.
    /// Unit creation and persisted reservations finish normally; existing units survive.
    pub fn begin_shutdown(&self) {
        self.stopping.cancel();
    }

    pub async fn shutdown(&self) {
        self.begin_shutdown();
        self.copy_tasks.close();
        self.copy_tasks.wait().await;
        if let Some(handle) = self.scheduler.lock().await.take() {
            handle.cancel.cancel();
            let _ = handle.join.await;
        }
        if let Some(handle) = self.run_watcher.lock().await.take() {
            handle.cancel.cancel();
            let _ = handle.join.await;
        }
        let handle = self.watcher.lock().await.take();
        if let Some(handle) = handle {
            handle.cancel.cancel();
            let _ = handle.join.await;
        }
    }

    async fn refresh(
        &self,
        state: &mut Watching,
        interesting: &Mutex<BTreeSet<PathBuf>>,
        publish: bool,
    ) -> R<bool> {
        let projects = self
            .projects
            .list(ListProjects {})
            .await
            .map_err(project_error)?
            .items;
        let mut wanted: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
        let mut targets = BTreeSet::new();
        for project in &projects {
            let root = PathBuf::from(&project.path);
            targets.extend([
                root.clone(),
                root.join(".devbox"),
                root.join(".devbox/devbox.toml"),
            ]);
            // Parent catches project recreation, root catches .devbox creation/removal,
            // .devbox catches direct writes and editors' atomic file replacements.
            for directory in [
                root.parent().map(PathBuf::from),
                Some(root.clone()),
                Some(root.join(".devbox")),
            ]
            .into_iter()
            .flatten()
            {
                if directory.is_dir() {
                    wanted
                        .entry(directory)
                        .or_default()
                        .push(project.id.clone());
                }
            }
        }
        *interesting.lock().expect("config watch paths") = targets;
        let Watching {
            watcher,
            watched,
            failed,
        } = state;
        let stale: Vec<_> = watched
            .iter()
            .filter(|path| !wanted.contains_key(*path))
            .cloned()
            .collect();
        for path in stale {
            if let Some(watcher) = watcher.as_mut() {
                let _ = watcher.unwatch(&path);
            }
            watched.remove(&path);
        }
        failed.retain(|path| wanted.contains_key(path));
        // Watching is best effort: a failed directory stays out of `watched` and is
        // attempted again by the next refresh, while definitions are read as usual.
        let mut incomplete = watcher.is_none();
        for (path, project_ids) in &wanted {
            let Some(watcher) = watcher.as_mut() else {
                watched.clear();
                break;
            };
            // A directory replaced at the same path has a new inode; the previous
            // watch can already have been removed by the OS. Attach to its current owner.
            if watched.remove(path) {
                let _ = watcher.unwatch(path);
            }
            match watcher.watch(path, RecursiveMode::NonRecursive) {
                Ok(()) => {
                    if failed.remove(path) {
                        tracing::info!("execution config directory is now watched");
                    }
                    watched.insert(path.clone());
                }
                Err(error) => {
                    incomplete = true;
                    let reason = reason(&error);
                    let project_id = project_ids.first().map(String::as_str);
                    let projects = project_ids.len();
                    if failed.insert(path.clone()) {
                        tracing::warn!(
                            project_id,
                            projects,
                            reason,
                            "watch execution config directory failed"
                        );
                    } else {
                        tracing::debug!(
                            project_id,
                            projects,
                            reason,
                            "watch execution config directory still failing"
                        );
                    }
                }
            }
        }
        let ids: BTreeSet<_> = projects.iter().map(|p| p.id.clone()).collect();
        self.observation
            .lock()
            .await
            .retain(|id, _| ids.contains(id));
        for project in projects {
            let _guard = self.project_lock(&project.id).await;
            let fingerprint = match self.projects.run_config_status(&project.id).await {
                Ok(state) => fingerprint(&state.digest, &state.trust),
                Err(error) => format!("invalid:{error:?}"),
            };
            let previous = self
                .observation
                .lock()
                .await
                .insert(project.id.clone(), fingerprint.clone());
            // Only initialize establishes a silent baseline. A project discovered
            // after startup may already have been listed before its watch attached.
            if publish && previous.is_none_or(|prior| prior != fingerprint) {
                let (_, rev) = self
                    .db
                    .write(|_| Ok(()))
                    .await
                    .internal("record external definition change")?;
                self.changed(&project.id, rev);
            }
        }
        Ok(incomplete)
    }
}
