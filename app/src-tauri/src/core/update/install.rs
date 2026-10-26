//! Confirmation capabilities bind the verified download to both admission epochs.
use super::*;
use serde::Serialize;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InstallBlock {
    NativeActive,
    NativeUnknown,
    NotReady,
    Busy,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallSummary {
    pub generation: u64,
    pub confirmation_id: Option<u64>,
    pub version: String,
    pub native_active: Option<u64>,
    pub blocked: Option<InstallBlock>,
}
pub(super) struct Ticket {
    id: u64,
    native_epoch: u64,
    window_epoch: u64,
}
pub struct CommitFence<'a> {
    pub generation: u64,
    pub round: u64,
    pub labels: &'a [String],
    pub native: (Option<u64>, u64),
    pub expected_native_epoch: u64,
}
pub struct InstallWork {
    pub generation: u64,
    pub token: CancellationToken,
    pub proof: Arc<ReadyProof>,
    pub version: String,
    pub fixture_install: bool,
}
impl Service {
    fn fixture_active(&self) -> bool {
        #[cfg(any(test, devbox_update_fixture))]
        {
            self.fixture.is_some() && self.channel == AppChannel::Accept
        }
        #[cfg(not(any(test, devbox_update_fixture)))]
        {
            false
        }
    }

    pub fn install_summary(
        &self,
        generation: u64,
        native: Option<u64>,
        native_epoch: u64,
        window_epoch: u64,
    ) -> InstallSummary {
        let mut state = self.lock();
        let blocked = if state.snapshot.phase == Phase::Installing {
            Some(InstallBlock::Busy)
        } else if generation != state.snapshot.generation
            || state.snapshot.phase != Phase::Ready
            || state.ready.is_none()
        {
            Some(InstallBlock::NotReady)
        } else {
            match native {
                None => Some(InstallBlock::NativeUnknown),
                Some(0) => None,
                Some(_) => Some(InstallBlock::NativeActive),
            }
        };
        state.snapshot.revision += 1;
        let id = state.snapshot.revision;
        state.ticket = blocked.is_none().then_some(Ticket {
            id,
            native_epoch,
            window_epoch,
        });
        InstallSummary {
            generation,
            confirmation_id: blocked.is_none().then_some(id),
            version: state
                .snapshot
                .available_version
                .clone()
                .unwrap_or_else(|| state.snapshot.current_version.clone()),
            native_active: native,
            blocked,
        }
    }
    pub fn begin_install(
        &self,
        generation: u64,
        confirmation: u64,
        native: Option<u64>,
        native_epoch: u64,
        window_epoch: u64,
    ) -> R<InstallWork> {
        let work = {
            let mut state = self.lock();
            if generation != state.snapshot.generation || state.snapshot.phase != Phase::Ready {
                return Err(UpdateError::StaleOperation);
            }
            match native {
                Some(0) => {}
                Some(_) => return Err(UpdateError::NativeActive),
                None => return Err(UpdateError::NativeUnknown),
            }
            let ticket = state.ticket.take().ok_or(UpdateError::StaleOperation)?;
            if ticket.id != confirmation
                || ticket.native_epoch != native_epoch
                || ticket.window_epoch != window_epoch
            {
                return Err(UpdateError::StaleOperation);
            }
            let proof = state.ready.clone().ok_or(UpdateError::StaleOperation)?;
            let version = state
                .snapshot
                .available_version
                .clone()
                .ok_or(UpdateError::StaleOperation)?;
            state.snapshot.generation += 1;
            state.snapshot.revision += 1;
            state.snapshot.phase = Phase::Installing;
            state.snapshot.next_action = Action::Cancel;
            state.snapshot.error = None;
            state.committed = false;
            let token = CancellationToken::new();
            state.cancel = Some(token.clone());
            InstallWork {
                generation: state.snapshot.generation,
                token,
                proof,
                version,
                fixture_install: self.fixture_active(),
            }
        };
        self.emit(self.snapshot());
        Ok(work)
    }
    /// The platform holds its admission lock across native refresh and this fence.
    pub fn commit_after_quit(
        &self,
        quit: &mut crate::core::quit::Quit,
        fence: CommitFence<'_>,
        commit: impl FnOnce() -> R<()>,
    ) -> R<()> {
        if self.installation_committed(fence.generation) {
            return Err(UpdateError::StaleOperation);
        }
        let result = if fence.native.0 != Some(0) || fence.native.1 != fence.expected_native_epoch {
            Err(if fence.native.0.is_none() {
                UpdateError::NativeUnknown
            } else {
                UpdateError::NativeActive
            })
        } else if !quit.seal(fence.round, fence.labels) {
            Err(UpdateError::QuitRejected)
        } else {
            self.commit_install(fence.generation, commit)
        };
        if result.is_err() {
            quit.revoke(fence.round);
        }
        result
    }
    pub fn installation_committed(&self, generation: u64) -> bool {
        let state = self.lock();
        state.snapshot.generation == generation && state.committed
    }
    /// Call while holding the lifecycle admission lock. This lock also fences cancel.
    pub fn commit_install(&self, generation: u64, commit: impl FnOnce() -> R<()>) -> R<()> {
        let snapshot = {
            let mut state = self.lock();
            if state.snapshot.generation != generation
                || state.snapshot.phase != Phase::Installing
                || state.committed
                || state
                    .cancel
                    .as_ref()
                    .is_none_or(CancellationToken::is_cancelled)
            {
                return Err(UpdateError::Cancelled);
            }
            commit()?;
            // Tauri run() exits the process directly; release the original download before exit.
            // The committed runner owns its independent verified staging copy.
            state.ready = None;
            state.committed = true;
            state.snapshot.revision += 1;
            state.snapshot.error = None;
            state.snapshot.next_action = Action::None;
            state.snapshot.clone()
        };
        self.emit(snapshot);
        Ok(())
    }
    /// Only after actual runner termination is proven may the original proof be retried.
    pub fn install_aborted(&self, generation: u64, error: UpdateError) {
        let snapshot = {
            let mut state = self.lock();
            if state.snapshot.generation != generation || state.committed {
                return;
            }
            state.cancel = None;
            state.snapshot.phase = Phase::Ready;
            state.snapshot.revision += 1;
            state.snapshot.error = Some(error);
            state.snapshot.next_action = Action::Install;
            state.snapshot.clone()
        };
        self.emit(snapshot);
    }
    pub fn install_quarantined(&self, generation: u64) {
        let snapshot = {
            let mut state = self.lock();
            if state.snapshot.generation != generation || state.committed {
                return;
            }
            state.snapshot.error = Some(UpdateError::Runner);
            state.snapshot.next_action = Action::None;
            state.snapshot.revision += 1;
            state.snapshot.clone()
        };
        self.emit(snapshot);
    }
    pub fn recovered_install(&self, error: UpdateError, unknown: bool) {
        let snapshot = {
            let mut state = self.lock();
            state.snapshot.phase = if unknown {
                Phase::Installing
            } else {
                Phase::Failed
            };
            state.committed = unknown;
            state.recovery_pending = true;
            state.snapshot.error = Some(error);
            state.snapshot.next_action = if unknown {
                Action::None
            } else if state.snapshot.enabled {
                Action::Check
            } else {
                Action::None
            };
            state.snapshot.revision += 1;
            state.snapshot.clone()
        };
        self.emit(snapshot);
    }
}
