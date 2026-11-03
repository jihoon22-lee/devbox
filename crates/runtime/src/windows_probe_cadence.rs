//! How often reconcile probes Windows for a Windows copy reader's attempt (M-3).
use crate::windows_copy_store::ReaderState;
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

/// A held attempt is a committed attempt with a job holder whose run has ended while its job has
/// not emptied: no receipt yet, for example because build servers the run left behind still run
/// in the job. Its receipt is still read on every pass, and that is what releases it: a live
/// holder writes the receipt within 500 ms of the job emptying. Windows itself (the job
/// observation, holder liveness) is probed for it at once when its run ends, then at most once
/// per this interval. A release that needs a probe (an empty job without a receipt, or a holder
/// that died) therefore comes up to one interval and one 10 s pass late, about 70 s, when the
/// probe succeeds. The interval runs from each probe's start, whatever it found: an
/// `Unavailable` or timed-out probe costs another full interval and releases nothing
/// (fail-closed). A live run's attempt and an rc.1 row (no holder, no receipt) keep the 10 s
/// pass cadence.
pub(crate) const HELD_PROBE_INTERVAL: Duration = Duration::from_secs(60);

/// Whether an attempt's run is still going, as the probe cadence sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttemptRun {
    /// Its reader is not sealed and it is the reader's newest attempt: probed on every pass.
    Live,
    /// Its reader is sealed (the run ended), or a newer attempt replaced it (its service
    /// restarted): probed at once, then once per [`HELD_PROBE_INTERVAL`].
    Ended,
}
impl AttemptRun {
    /// `newest` is the nonce of the reader's last started attempt.
    pub(crate) fn of(reader: ReaderState, newest: Option<&str>, nonce: &str) -> Self {
        if reader == ReaderState::Sealed || newest != Some(nonce) {
            Self::Ended
        } else {
            Self::Live
        }
    }
}

/// When reconcile last probed each holder attempt, by nonce, and whether its run had ended by
/// then, in this daemon process only. A restart forgets it, so the first pass after a restart
/// probes every attempt at once.
#[derive(Default)]
pub(crate) struct ProbeCadence(Mutex<HashMap<String, (Instant, AttemptRun)>>);
impl ProbeCadence {
    /// Whether to probe this attempt now: on every claim while its run is live, on the first
    /// claim after its run ended (the job may have emptied with it), and after that only once
    /// [`HELD_PROBE_INTERVAL`] has passed since its last probe. A yes is recorded before the
    /// probe is awaited, so concurrent passes (the poll, a new run's reservation, a stop)
    /// probe a held attempt once.
    pub(crate) fn claim(&self, nonce: &str, run: AttemptRun) -> bool {
        let now = Instant::now();
        let mut last = self.0.lock().expect("Windows probe cadence");
        let waiting = |(at, then): &(Instant, AttemptRun)| {
            *then == AttemptRun::Ended && now.duration_since(*at) < HELD_PROBE_INTERVAL
        };
        if run == AttemptRun::Ended && last.get(nonce).is_some_and(waiting) {
            return false;
        }
        last.insert(nonce.to_owned(), (now, run));
        true
    }
    /// The attempt closed: its probe history goes with it.
    pub(crate) fn forget(&self, nonce: &str) {
        self.0.lock().expect("Windows probe cadence").remove(nonce);
    }
    /// Tests move every recorded probe back in time instead of waiting.
    #[cfg(test)]
    pub(crate) fn backdate(&self, by: Duration) {
        for (at, _) in self.0.lock().expect("Windows probe cadence").values_mut() {
            *at = at
                .checked_sub(by)
                .expect("a monotonic clock older than one window");
        }
    }
}
