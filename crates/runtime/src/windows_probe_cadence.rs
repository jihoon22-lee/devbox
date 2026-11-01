//! How often reconcile probes Windows for a Windows copy reader's attempt (M-3).
use crate::windows_copy_store::ReaderState;
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

/// A held attempt is a committed attempt with a job holder whose run has ended (its reader is
/// sealed) while its job has not emptied: no receipt yet, for example because build servers the
/// run left behind still run in the job. Its receipt is still read on every pass, and that is
/// what releases it: a live holder writes the receipt within 500 ms of the job emptying. Windows
/// itself (the job observation, holder liveness) is probed for it at most once per this
/// interval, so only a release that needs a probe (an empty job without a receipt, or a holder
/// that died) comes up to one interval late. A live run's attempt (its reader is not sealed) and
/// an rc.1 row (no holder, no receipt) keep the 10 s pass cadence.
pub(crate) const HELD_PROBE_INTERVAL: Duration = Duration::from_secs(60);

/// When reconcile last probed each holder attempt, by nonce, in this daemon process only. A
/// restart forgets it, so the first pass after a restart probes every attempt at once.
#[derive(Default)]
pub(crate) struct ProbeCadence(Mutex<HashMap<String, Instant>>);
impl ProbeCadence {
    /// Whether to probe this attempt of a `reader` in this state now: always while its run is
    /// live, and once its reader is sealed (held) only when [`HELD_PROBE_INTERVAL`] has passed
    /// since its last probe, which may predate the hold. A yes is recorded before the probe is
    /// awaited, so concurrent passes (the poll, a new run's reservation, a stop) probe a held
    /// attempt once.
    pub(crate) fn claim(&self, nonce: &str, reader: ReaderState) -> bool {
        let now = Instant::now();
        let mut last = self.0.lock().expect("Windows probe cadence");
        if reader == ReaderState::Sealed
            && last
                .get(nonce)
                .is_some_and(|at| now.duration_since(*at) < HELD_PROBE_INTERVAL)
        {
            return false;
        }
        last.insert(nonce.to_owned(), now);
        true
    }
    /// The attempt closed: its probe history goes with it.
    pub(crate) fn forget(&self, nonce: &str) {
        self.0.lock().expect("Windows probe cadence").remove(nonce);
    }
    /// Tests move every recorded probe back in time instead of waiting.
    #[cfg(test)]
    pub(crate) fn backdate(&self, by: Duration) {
        for at in self.0.lock().expect("Windows probe cadence").values_mut() {
            *at = at
                .checked_sub(by)
                .expect("a monotonic clock older than one window");
        }
    }
}
