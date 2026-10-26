use super::{
    Action, Disabled, Phase, R, Snapshot, UpdateError,
    http::Http,
    release::{self, Candidate},
    storage::{ReadyProof, Root},
};
use crate::core::channel::AppChannel;
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
enum Work {
    Check(u64, CancellationToken),
    Download(u64, Candidate, CancellationToken),
}

struct Inner {
    snapshot: Snapshot,
    candidate: Option<Candidate>,
    ready: Option<Arc<ReadyProof>>,
    ticket: Option<install::Ticket>,
    committed: bool,
    recovery_pending: bool,
    cancel: Option<CancellationToken>,
    last_emit: Instant,
}
pub struct Service {
    inner: Mutex<Inner>,
    root: Option<PathBuf>,
    channel: AppChannel,
    compare_version: String,
    publish: Arc<dyn Fn(Snapshot) + Send + Sync>,
    pending: watch::Sender<Option<Work>>,
    receiver: Mutex<Option<watch::Receiver<Option<Work>>>>,
    active: AtomicU64,
    #[cfg(any(test, devbox_update_fixture))]
    fixture: Option<super::fixture::Fixture>,
}
impl Service {
    pub fn new(
        channel: AppChannel,
        version: &str,
        root: Option<PathBuf>,
        publish: impl Fn(Snapshot) + Send + Sync + 'static,
    ) -> Arc<Self> {
        let disabled = if channel != AppChannel::Prod {
            Some(Disabled::Channel)
        } else if release::stable(version).is_none() {
            Some(Disabled::DevelopmentVersion)
        } else {
            None
        };
        let (pending, receiver) = watch::channel(None);
        Arc::new(Self {
            inner: Mutex::new(Inner {
                snapshot: Snapshot {
                    enabled: disabled.is_none(),
                    disabled_reason: disabled,
                    phase: Phase::Idle,
                    generation: 0,
                    revision: 0,
                    current_version: version.into(),
                    available_version: None,
                    received_bytes: 0,
                    total_bytes: None,
                    error: None,
                    next_action: if disabled.is_none() {
                        Action::Check
                    } else {
                        Action::None
                    },
                },
                candidate: None,
                ready: None,
                ticket: None,
                committed: false,
                recovery_pending: false,
                cancel: None,
                last_emit: Instant::now(),
            }),
            root,
            channel,
            compare_version: version.into(),
            publish: Arc::new(publish),
            pending,
            receiver: Mutex::new(Some(receiver)),
            active: AtomicU64::new(0),
            #[cfg(any(test, devbox_update_fixture))]
            fixture: None,
        })
    }
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn snapshot(&self) -> Snapshot {
        self.lock().snapshot.clone()
    }
    /// Startup-only: remove `download-*` leaked by a previous process exit. The caller must be the
    /// single main instance and run this before any check/download work starts.
    pub fn sweep_stale_downloads(&self) -> R<usize> {
        match &self.root {
            Some(root) => super::storage::sweep_downloads(root, self.channel),
            None => Ok(0),
        }
    }
    fn storage(&self) -> R<Root> {
        Root::open(
            self.root.as_deref().ok_or(UpdateError::Storage)?,
            self.channel,
        )
    }
    fn emit(&self, value: Snapshot) {
        (self.publish)(value);
    }
    pub fn check(self: &Arc<Self>, automatic: bool) -> R<Snapshot> {
        let snapshot = {
            let mut state = self.lock();
            if !state.snapshot.enabled
                || matches!(
                    state.snapshot.phase,
                    Phase::Checking | Phase::Downloading | Phase::Ready | Phase::Installing
                )
            {
                return Ok(state.snapshot.clone());
            }
            if automatic && state.recovery_pending {
                return Ok(state.snapshot.clone());
            }
            if !automatic {
                state.recovery_pending = false;
            }
            if automatic {
                let due = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|_| UpdateError::Storage)
                    .and_then(|now| self.storage()?.automatic_due(now.as_secs()));
                match due {
                    Ok(false) => return Ok(state.snapshot.clone()),
                    Ok(true) => {}
                    Err(error) => {
                        state.snapshot.generation += 1;
                        state.snapshot.revision += 1;
                        state.snapshot.phase = Phase::Failed;
                        state.snapshot.error = Some(error);
                        state.snapshot.next_action = Action::Check;
                        state.snapshot.available_version = None;
                        state.snapshot.total_bytes = None;
                        state.candidate = None;
                        let failed = state.snapshot.clone();
                        drop(state);
                        self.emit(failed.clone());
                        return Ok(failed);
                    }
                }
            }
            state.snapshot.generation += 1;
            state.snapshot.revision += 1;
            state.snapshot.phase = Phase::Checking;
            state.snapshot.error = None;
            state.snapshot.next_action = Action::Cancel;
            state.snapshot.available_version = None;
            state.snapshot.received_bytes = 0;
            state.snapshot.total_bytes = None;
            state.candidate = None;
            state.ready = None;
            let token = CancellationToken::new();
            state.cancel = Some(token.clone());
            self.pending
                .send_replace(Some(Work::Check(state.snapshot.generation, token)));
            state.snapshot.clone()
        };
        self.emit(snapshot.clone());
        Ok(snapshot)
    }
    /// Exactly one worker for the app lifetime; latest pending work replaces older queued work.
    /// The caller supplies its runtime. No command spawns a task or a second writer.
    pub async fn run(self: Arc<Self>) {
        let Some(mut receiver) = self
            .receiver
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        else {
            return;
        };
        while receiver.changed().await.is_ok() {
            let Some(work) = receiver.borrow_and_update().clone() else {
                continue;
            };
            let (generation, token) = match &work {
                Work::Check(g, t) | Work::Download(g, _, t) => (*g, t),
            };
            if token.is_cancelled() || self.snapshot().generation != generation {
                continue;
            }
            self.active.store(generation, Ordering::SeqCst);
            match work {
                Work::Check(g, token) => self.check_work(g, token).await,
                Work::Download(g, candidate, token) => {
                    self.download_work(g, candidate, token).await
                }
            }
            // The previous request, writer and owned temporary resources are now dropped.
            self.active.store(0, Ordering::SeqCst);
        }
    }
    async fn check_work(&self, generation: u64, token: CancellationToken) {
        let result = self
            .fetch_metadata(&token)
            .await
            .and_then(|bytes| release::select(&bytes, &self.compare_version));
        let value = {
            let mut state = self.lock();
            if state.snapshot.generation != generation || state.snapshot.phase != Phase::Checking {
                return;
            }
            state.cancel = None;
            state.snapshot.revision += 1;
            match result {
                Ok(candidate) => {
                    state.snapshot.phase = if candidate.is_some() {
                        Phase::Available
                    } else {
                        Phase::Idle
                    };
                    state.snapshot.available_version =
                        candidate.as_ref().map(|c| c.version.clone());
                    state.snapshot.total_bytes = candidate.as_ref().map(|c| c.size);
                    state.snapshot.next_action = if candidate.is_some() {
                        Action::Download
                    } else {
                        Action::Check
                    };
                    state.candidate = candidate;
                }
                Err(error) => {
                    state.snapshot.phase = Phase::Failed;
                    state.snapshot.error = Some(error);
                    state.snapshot.next_action = Action::Check;
                }
            }
            state.snapshot.clone()
        };
        self.emit(value);
    }
    async fn fetch_metadata(&self, token: &CancellationToken) -> R<Vec<u8>> {
        #[cfg(any(test, devbox_update_fixture))]
        if let Some(fixture) = &self.fixture {
            return fixture.metadata(token).await;
        }
        Http::new()?.metadata(token).await
    }
    pub fn download(self: &Arc<Self>, generation: u64) -> R<Snapshot> {
        let snapshot = {
            let mut state = self.lock();
            if !state.snapshot.enabled {
                return Ok(state.snapshot.clone());
            }
            if generation != state.snapshot.generation {
                return Err(UpdateError::StaleOperation);
            }
            if matches!(
                state.snapshot.phase,
                Phase::Downloading | Phase::Ready | Phase::Installing
            ) {
                return Ok(state.snapshot.clone());
            }
            let candidate = state.candidate.clone().ok_or(UpdateError::StaleOperation)?;
            state.snapshot.generation += 1;
            state.snapshot.revision += 1;
            state.snapshot.phase = Phase::Downloading;
            state.snapshot.received_bytes = 0;
            state.snapshot.error = None;
            state.snapshot.next_action = Action::Cancel;
            let token = CancellationToken::new();
            state.cancel = Some(token.clone());
            self.pending.send_replace(Some(Work::Download(
                state.snapshot.generation,
                candidate,
                token,
            )));
            state.snapshot.clone()
        };
        self.emit(snapshot.clone());
        Ok(snapshot)
    }
    async fn download_work(&self, generation: u64, candidate: Candidate, token: CancellationToken) {
        let result = self.transfer(&candidate, &token, generation).await;
        let value = {
            let mut state = self.lock();
            if state.snapshot.generation != generation || state.snapshot.phase != Phase::Downloading
            {
                return;
            }
            state.cancel = None;
            state.snapshot.revision += 1;
            match result {
                Ok(proof) => {
                    state.ready = Some(Arc::new(proof));
                    state.snapshot.phase = Phase::Ready;
                    state.snapshot.received_bytes = candidate.size;
                    state.snapshot.next_action = Action::Install;
                }
                Err(error) => {
                    state.snapshot.phase = Phase::Failed;
                    state.snapshot.error = Some(error);
                    state.snapshot.next_action = Action::Download;
                }
            }
            state.snapshot.clone()
        };
        self.emit(value);
    }
    async fn transfer(
        &self,
        candidate: &Candidate,
        token: &CancellationToken,
        generation: u64,
    ) -> R<ReadyProof> {
        if token.is_cancelled() {
            return Err(UpdateError::Cancelled);
        }
        let pending = self.storage()?.pending()?;
        #[cfg(any(test, devbox_update_fixture))]
        if let Some(fixture) = &self.fixture {
            return fixture
                .download(candidate, pending, token, |n| self.progress(generation, n))
                .await;
        }
        Http::new()?
            .download(candidate, pending, token, |n| self.progress(generation, n))
            .await
    }
    fn progress(&self, generation: u64, bytes: u64) {
        let value = {
            let mut state = self.lock();
            if state.snapshot.generation != generation
                || state.snapshot.phase != Phase::Downloading
                || bytes < state.snapshot.received_bytes
            {
                return;
            }
            state.snapshot.received_bytes = bytes;
            state.snapshot.revision += 1;
            if state.last_emit.elapsed() < Duration::from_millis(100) {
                return;
            }
            state.last_emit = Instant::now();
            state.snapshot.clone()
        };
        self.emit(value);
    }
    pub fn cancel(&self, generation: u64) -> R<Snapshot> {
        let value = {
            let mut state = self.lock();
            if generation != state.snapshot.generation {
                return Err(UpdateError::StaleOperation);
            }
            if state.snapshot.phase == Phase::Installing {
                if state.committed {
                    return Err(UpdateError::StaleOperation);
                }
                if let Some(token) = &state.cancel {
                    token.cancel();
                }
                state.snapshot.error = Some(UpdateError::Cancelled);
                state.snapshot.next_action = Action::None;
                state.snapshot.revision += 1;
                let snapshot = state.snapshot.clone();
                drop(state);
                self.emit(snapshot.clone());
                return Ok(snapshot);
            }
            if !matches!(state.snapshot.phase, Phase::Checking | Phase::Downloading) {
                return Ok(state.snapshot.clone());
            }
            if let Some(token) = state.cancel.take() {
                token.cancel();
            }
            self.pending.send_replace(None);
            state.snapshot.generation += 1;
            state.snapshot.revision += 1;
            state.snapshot.phase = if state.candidate.is_some() {
                Phase::Available
            } else {
                Phase::Idle
            };
            state.snapshot.received_bytes = 0;
            state.snapshot.error = None;
            state.snapshot.next_action = if state.candidate.is_some() {
                Action::Download
            } else {
                Action::Check
            };
            state.snapshot.clone()
        };
        self.emit(value.clone());
        Ok(value)
    }
    #[cfg(any(test, devbox_update_fixture))]
    pub fn fixture(
        channel: AppChannel,
        root: Option<PathBuf>,
        fixture: super::fixture::Fixture,
        publish: impl Fn(Snapshot) + Send + Sync + 'static,
    ) -> R<Arc<Self>> {
        if channel != AppChannel::Accept {
            return Err(UpdateError::InvalidRelease);
        }
        let mut service = Self::new(channel, env!("CARGO_PKG_VERSION"), root, publish);
        let owned = Arc::get_mut(&mut service).ok_or(UpdateError::Storage)?;
        let state = owned.inner.get_mut().map_err(|_| UpdateError::Storage)?;
        state.snapshot.enabled = true;
        state.snapshot.disabled_reason = None;
        state.snapshot.next_action = Action::Check;
        owned.compare_version = fixture.current_version.clone();
        owned.fixture = Some(fixture);
        Ok(service)
    }
}
#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;

#[path = "install.rs"]
pub mod install;
