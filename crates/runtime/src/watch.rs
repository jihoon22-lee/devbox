use crate::{R, RuntimeService, service::project_error};
use devbox_projects::{ListProjects, TrustPreviewStatus};
use devbox_protocol::Internal;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{sync::mpsc, task::JoinHandle, time::Instant};
use tokio_util::sync::CancellationToken;

pub(crate) struct WatchHandle {
    pub(crate) cancel: CancellationToken,
    pub(crate) join: JoinHandle<()>,
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
        let mut slot = self.watcher.lock().await;
        if slot.is_some() {
            return Ok(());
        }
        self.initialize_runs().await?;
        // Any queued notification schedules a complete digest comparison, so one slot
        // coalesces bursts without retaining unbounded file event paths.
        let (send, mut receive) = mpsc::channel(1);
        let interesting = Arc::new(Mutex::new(BTreeSet::new()));
        let event_paths = interesting.clone();
        let mut watcher =
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
            .internal("start execution config watcher")?;
        let mut watched = BTreeSet::new();
        let mut project_events = self.hub.subscribe("projects.changed");
        self.refresh(&mut watcher, &mut watched, &interesting, false)
            .await?;
        let cancel = CancellationToken::new();
        let stopped = cancel.clone();
        let service = self.clone();
        let join = tokio::spawn(async move {
            let mut deadline = None;
            loop {
                let wait = deadline.unwrap_or_else(|| Instant::now() + Duration::from_secs(86400));
                tokio::select! {
                    _ = stopped.cancelled() => break,
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
                        if service.refresh(&mut watcher, &mut watched, &interesting, true).await.is_err() {
                            tracing::warn!("refresh execution definitions failed");
                        }
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

    pub async fn shutdown(&self) {
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
        watcher: &mut RecommendedWatcher,
        watched: &mut BTreeSet<PathBuf>,
        interesting: &Mutex<BTreeSet<PathBuf>>,
        publish: bool,
    ) -> R<()> {
        let projects = self
            .projects
            .list(ListProjects {})
            .await
            .map_err(project_error)?
            .items;
        let mut wanted = BTreeSet::new();
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
                    wanted.insert(directory);
                }
            }
        }
        *interesting.lock().expect("config watch paths") = targets;
        for path in watched.difference(&wanted).cloned().collect::<Vec<_>>() {
            let _ = watcher.unwatch(&path);
            watched.remove(&path);
        }
        for path in wanted {
            // A directory replaced at the same path has a new inode; the previous
            // watch can already have been removed by the OS. Attach to its current owner.
            if watched.remove(&path) {
                let _ = watcher.unwatch(&path);
            }
            watcher
                .watch(&path, RecursiveMode::NonRecursive)
                .internal("watch execution config directory")?;
            watched.insert(path);
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
        Ok(())
    }
}
