//! Small process-local operation controls used by Workbench commands.
//!
//! Tauri can drop an invocation future when a window navigates or closes, but
//! that does not by itself stop a child process or a blocking file/git task.
//! This module gives command layers an explicit cancellation bit, a monotonic
//! deadline, and a single-flight slot whose drop cleanup is generation-safe.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(25);
const MAX_PENDING_OPERATIONS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationError {
    Cancelled,
    TimedOut,
}

impl OperationError {
    pub fn message(self) -> &'static str {
        match self {
            Self::Cancelled => "Workspace 작업이 취소되었습니다",
            Self::TimedOut => "Workspace 작업 시간이 초과되었습니다",
        }
    }
}

/// A cloneable cancellation signal shared by async tasks and blocking
/// workers. The bit is intentionally sticky so a cancellation racing with a
/// worker's next poll cannot be lost.
#[derive(Clone, Debug)]
pub struct OperationToken {
    cancelled: Arc<AtomicBool>,
}

impl OperationToken {
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// Share the same cancellation bit with a blocking native worker. The
    /// returned `Arc` contains no operation data and remains safe to retain
    /// until that worker has joined.
    pub fn cancellation_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancelled)
    }

    pub fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.cancelled, &other.cancelled)
    }

    pub fn check(&self, deadline: Instant) -> Result<(), OperationError> {
        if self.is_cancelled() {
            Err(OperationError::Cancelled)
        } else if Instant::now() >= deadline {
            Err(OperationError::TimedOut)
        } else {
            Ok(())
        }
    }
}

impl Default for OperationToken {
    fn default() -> Self {
        Self::new()
    }
}

/// A fixed monotonic budget shared by every native step in one operation.
#[derive(Debug, Clone, Copy)]
pub struct OperationBudget {
    deadline: Instant,
}

impl OperationBudget {
    pub fn from_now(timeout: Duration) -> Self {
        let now = Instant::now();
        Self {
            // A duration supplied by a future caller must never turn an
            // overflow into an effectively unbounded operation. Fail closed
            // at `now`; current fixed budgets are far below this boundary.
            deadline: now.checked_add(timeout).unwrap_or(now),
        }
    }

    pub fn remaining(self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }

    pub fn check(self, token: &OperationToken) -> Result<(), OperationError> {
        token.check(self.deadline)
    }
}

/// A process-local single-flight coordinator. Command layers cancel an older
/// claim, wait for its worker to drop the slot, and then use `claim_reject` to
/// admit exactly one native operation.
pub struct SingleFlight {
    state: Mutex<FlightState>,
}

struct ActiveOperation {
    key: String,
    token: OperationToken,
    workers: usize,
    claim_dropped: bool,
}

struct FlightState {
    active: Option<ActiveOperation>,
    pending: std::collections::HashMap<String, OperationToken>,
}

pub struct OperationClaim {
    flight: Arc<SingleFlight>,
    key: String,
    token: OperationToken,
}

/// Holds the single-flight slot for a blocking worker after its parent
/// invocation has been dropped. The worker must release this guard only after
/// its native child/file operation has stopped, preventing a detached worker
/// from overlapping the next request.
pub struct OperationWorkerGuard {
    flight: Arc<SingleFlight>,
    key: String,
    token: OperationToken,
    registered: bool,
}

/// A request which has an exact cancellation key but is waiting for the
/// previous active operation to finish. Keeping this ticket alive closes the
/// small race where the frontend cancels a newer request before it owns the
/// active native slot.
pub struct PendingOperation {
    flight: Arc<SingleFlight>,
    key: String,
    token: OperationToken,
    registered: bool,
}

impl OperationClaim {
    pub fn token(&self) -> OperationToken {
        self.token.clone()
    }

    pub fn worker_guard(&self) -> Result<OperationWorkerGuard, &'static str> {
        self.flight.register_worker(&self.key, &self.token)
    }
}

impl Drop for OperationClaim {
    fn drop(&mut self) {
        // Dropping a Tauri invocation is itself cancellation. The worker may
        // still hold a clone of this token after the claim guard is gone, so
        // it can kill its native child or stop its next bounded read instead
        // of continuing detached work silently.
        self.token.cancel();
        if let Ok(mut state) = self.flight.state.lock() {
            if state
                .active
                .as_ref()
                .is_some_and(|current| current.key == self.key && current.token.same(&self.token))
            {
                let clear = if let Some(current) = state.active.as_mut() {
                    current.claim_dropped = true;
                    current.workers == 0
                } else {
                    false
                };
                if clear {
                    state.active = None;
                }
            }
        }
    }
}

impl Drop for OperationWorkerGuard {
    fn drop(&mut self) {
        if !self.registered {
            return;
        }
        if let Ok(mut state) = self.flight.state.lock() {
            let clear = if let Some(current) = state
                .active
                .as_mut()
                .filter(|current| current.key == self.key && current.token.same(&self.token))
            {
                current.workers = current.workers.saturating_sub(1);
                current.workers == 0 && current.claim_dropped
            } else {
                false
            };
            if clear {
                state.active = None;
            }
        }
    }
}

impl PendingOperation {
    pub fn token(&self) -> OperationToken {
        self.token.clone()
    }

    /// Promote a pending ticket only after the active slot has become idle.
    /// The caller should check its budget before calling this method; a
    /// cancellation which races that check is still handled here.
    pub fn claim(mut self) -> Result<OperationClaim, &'static str> {
        let mut state = self
            .flight
            .state
            .lock()
            .map_err(|_| "작업 상태를 확인할 수 없습니다")?;
        if self.token.is_cancelled() {
            if state
                .pending
                .get(&self.key)
                .is_some_and(|pending| pending.same(&self.token))
            {
                state.pending.remove(&self.key);
            }
            self.registered = false;
            return Err(OperationError::Cancelled.message());
        }
        if state.active.is_some() {
            return Err("다른 작업이 이미 진행 중입니다");
        }
        let owned_pending = state
            .pending
            .get(&self.key)
            .is_some_and(|pending| pending.same(&self.token));
        if !owned_pending {
            self.registered = false;
            return Err(OperationError::Cancelled.message());
        }
        state.pending.remove(&self.key);
        state.active = Some(ActiveOperation {
            key: self.key.clone(),
            token: self.token.clone(),
            workers: 0,
            claim_dropped: false,
        });
        self.registered = false;
        Ok(OperationClaim {
            flight: Arc::clone(&self.flight),
            key: self.key.clone(),
            token: self.token.clone(),
        })
    }
}

impl Drop for PendingOperation {
    fn drop(&mut self) {
        if !self.registered {
            return;
        }
        self.token.cancel();
        if let Ok(mut state) = self.flight.state.lock() {
            if state
                .pending
                .get(&self.key)
                .is_some_and(|token| token.same(&self.token))
            {
                state.pending.remove(&self.key);
            }
        }
    }
}

impl SingleFlight {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(FlightState {
                active: None,
                pending: std::collections::HashMap::new(),
            }),
        })
    }

    /// Register an exact-key request before waiting for the active operation
    /// to finish. Its ticket owns cancellation until it is promoted or
    /// dropped, so navigation/window teardown cannot leave a queued native
    /// request running after the caller has gone away.
    pub fn prepare(
        self: &Arc<Self>,
        key: impl Into<String>,
    ) -> Result<PendingOperation, &'static str> {
        let key = key.into();
        let mut state = self
            .state
            .lock()
            .map_err(|_| "작업 상태를 확인할 수 없습니다")?;
        if state
            .active
            .as_ref()
            .is_some_and(|active| active.key == key && !active.token.is_cancelled())
        {
            return Err("다른 작업이 이미 진행 중입니다");
        }
        if state
            .pending
            .get(&key)
            .is_some_and(|pending| !pending.is_cancelled())
        {
            return Err("다른 작업이 이미 진행 중입니다");
        }
        // A previous same-key request may have been cancelled while it was
        // still queued. Its ticket will also notice the identity mismatch on
        // drop, so it is safe to replace the stale map entry here.
        if state
            .pending
            .get(&key)
            .is_some_and(|pending| pending.is_cancelled())
        {
            state.pending.remove(&key);
        }
        if state.pending.len() >= MAX_PENDING_OPERATIONS {
            return Err("대기 중인 작업이 너무 많습니다");
        }
        let token = OperationToken::new();
        state.pending.insert(key.clone(), token.clone());
        Ok(PendingOperation {
            flight: Arc::clone(self),
            key,
            token,
            registered: true,
        })
    }

    fn register_worker(
        self: &Arc<Self>,
        key: &str,
        token: &OperationToken,
    ) -> Result<OperationWorkerGuard, &'static str> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "작업 상태를 확인할 수 없습니다")?;
        let Some(active) = state
            .active
            .as_mut()
            .filter(|active| active.key == key && active.token.same(token))
        else {
            return Err("작업 상태가 이미 종료되었습니다");
        };
        active.workers = active
            .workers
            .checked_add(1)
            .ok_or("작업 worker 수가 올바르지 않습니다")?;
        Ok(OperationWorkerGuard {
            flight: Arc::clone(self),
            key: key.to_string(),
            token: token.clone(),
            registered: true,
        })
    }

    pub fn claim_reject(
        self: &Arc<Self>,
        key: impl Into<String>,
    ) -> Result<OperationClaim, &'static str> {
        self.claim_reject_with_token(key, OperationToken::new())
    }

    /// Claim a slot with a caller-owned token. Start Workspace uses this to
    /// reserve the health/Git native lane for its whole transition while
    /// still letting an explicit newer health request cancel the transition's
    /// shared token.
    pub fn claim_reject_with_token(
        self: &Arc<Self>,
        key: impl Into<String>,
        token: OperationToken,
    ) -> Result<OperationClaim, &'static str> {
        let key = key.into();
        let mut state = self
            .state
            .lock()
            .map_err(|_| "작업 상태를 확인할 수 없습니다")?;
        if state.active.is_some() {
            return Err("다른 작업이 이미 진행 중입니다");
        }
        state.active = Some(ActiveOperation {
            key: key.clone(),
            token: token.clone(),
            workers: 0,
            claim_dropped: false,
        });
        Ok(OperationClaim {
            flight: Arc::clone(self),
            key,
            token,
        })
    }

    #[cfg(test)]
    fn claim_latest(
        self: &Arc<Self>,
        key: impl Into<String>,
    ) -> Result<OperationClaim, &'static str> {
        let key = key.into();
        let mut state = self
            .state
            .lock()
            .map_err(|_| "작업 상태를 확인할 수 없습니다")?;
        if let Some(previous) = state.active.as_ref() {
            previous.token.cancel();
        }
        let token = OperationToken::new();
        state.active = Some(ActiveOperation {
            key: key.clone(),
            token: token.clone(),
            workers: 0,
            claim_dropped: false,
        });
        Ok(OperationClaim {
            flight: Arc::clone(self),
            key,
            token,
        })
    }

    pub fn cancel(&self, key: &str) -> Result<bool, &'static str> {
        let state = self
            .state
            .lock()
            .map_err(|_| "작업 상태를 확인할 수 없습니다")?;
        let mut cancelled = false;
        if let Some(current) = state.active.as_ref().filter(|current| current.key == key) {
            current.token.cancel();
            cancelled = true;
        }
        if let Some(current) = state.pending.get(key) {
            current.cancel();
            cancelled = true;
        }
        Ok(cancelled)
    }

    pub fn cancel_active(&self) -> Result<bool, &'static str> {
        let state = self
            .state
            .lock()
            .map_err(|_| "작업 상태를 확인할 수 없습니다")?;
        let mut cancelled = false;
        if let Some(current) = state.active.as_ref() {
            current.token.cancel();
            cancelled = true;
        }
        for current in state.pending.values() {
            current.cancel();
            cancelled = true;
        }
        Ok(cancelled)
    }

    /// Wait until a previously cancelled claim has dropped its slot. A new
    /// native operation must not merely replace the pointer while the old
    /// worker is still unwinding; doing so would turn single-flight into a
    /// best-effort UI convention instead of a real concurrency bound.
    pub async fn wait_until_idle(
        &self,
        token: OperationToken,
        budget: OperationBudget,
    ) -> Result<(), String> {
        loop {
            budget
                .check(&token)
                .map_err(|error| error.message().to_string())?;
            let idle = self
                .state
                .lock()
                .map_err(|_| "작업 상태를 확인할 수 없습니다".to_string())?
                .active
                .is_none();
            if idle {
                return Ok(());
            }
            tokio::time::sleep(budget.remaining().min(POLL_INTERVAL)).await;
        }
    }
}

/// Await until a token is cancelled or its budget expires. Polling is bounded
/// so cancellation does not rely on a notification that could race with a
/// check performed by a native child worker.
pub async fn wait_for_change(token: OperationToken, budget: OperationBudget) -> OperationError {
    loop {
        if let Err(error) = budget.check(&token) {
            return error;
        }
        tokio::time::sleep(budget.remaining().min(POLL_INTERVAL)).await;
    }
}

pub const fn poll_interval() -> Duration {
    POLL_INTERVAL
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn cancellation_is_sticky_and_claim_drop_cannot_clear_a_newer_generation() {
        let flight = SingleFlight::new();
        let first = flight.claim_reject("p-1").unwrap();
        let first_token = first.token();
        flight.cancel("p-1").unwrap();
        assert!(first_token.is_cancelled());
        drop(first);

        let second = flight.claim_reject("p-2").unwrap();
        let second_token = second.token();
        assert!(!second_token.is_cancelled());
        drop(second);
    }

    #[test]
    fn latest_claim_cancels_only_the_previous_claim() {
        let flight = SingleFlight::new();
        let first = flight.claim_latest("p-1").unwrap();
        let first_token = first.token();
        let second = flight.claim_latest("p-2").unwrap();
        assert!(first_token.is_cancelled());
        assert!(!second.token().is_cancelled());
        drop(first);
        assert!(flight.claim_reject("p-3").is_err());
        drop(second);
        assert!(flight.claim_reject("p-3").is_ok());
    }

    #[test]
    fn pending_ticket_can_be_cancelled_by_its_exact_key() {
        let flight = SingleFlight::new();
        let active = flight.claim_reject("active").unwrap();
        let pending = flight.prepare("queued").unwrap();
        let token = pending.token();
        assert!(flight.cancel("queued").unwrap());
        assert!(token.is_cancelled());
        drop(pending);
        drop(active);
        assert!(flight.claim_reject("next").is_ok());
    }

    #[test]
    fn cancelled_same_key_pending_entry_can_be_replaced() {
        let flight = SingleFlight::new();
        let pending = flight.prepare("same").unwrap();
        assert!(flight.cancel("same").unwrap());
        let replacement = flight.prepare("same").unwrap();
        assert!(!replacement.token().is_cancelled());
        drop(pending);
        drop(replacement);
    }

    #[test]
    fn cancelled_old_ticket_cannot_remove_a_same_key_replacement() {
        let flight = SingleFlight::new();
        let pending = flight.prepare("same").unwrap();
        assert!(flight.cancel("same").unwrap());
        let replacement = flight.prepare("same").unwrap();
        assert!(pending.claim().is_err());
        assert!(replacement.claim().is_ok());
    }

    #[test]
    fn worker_lease_keeps_slot_after_claim_drop_until_worker_finishes() {
        let flight = SingleFlight::new();
        let claim = flight.claim_reject("running").unwrap();
        let worker = claim.worker_guard().unwrap();
        drop(claim);
        assert!(flight.claim_reject("next").is_err());
        drop(worker);
        assert!(flight.claim_reject("next").is_ok());
    }

    #[test]
    fn deadline_is_monotonic_and_finite() {
        let budget = OperationBudget::from_now(Duration::from_millis(20));
        let token = OperationToken::new();
        assert!(budget.check(&token).is_ok());
        thread::sleep(Duration::from_millis(25));
        assert_eq!(budget.check(&token), Err(OperationError::TimedOut));
    }

    #[test]
    fn deadline_add_overflow_fails_closed() {
        let budget = OperationBudget::from_now(Duration::MAX);
        assert!(budget.remaining() < Duration::from_secs(1));
    }
}
