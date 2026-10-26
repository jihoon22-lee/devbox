//! Native attempt lifetime is independent of transport completion and task ownership.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Observation {
    Active,
    Unknown,
    Complete,
}

pub fn observe(direct_exited: Option<bool>, job_active: Option<u32>) -> Observation {
    match (direct_exited, job_active) {
        (Some(true), Some(0)) => Observation::Complete,
        (Some(_), Some(_)) => Observation::Active,
        _ => Observation::Unknown,
    }
}

#[derive(Default)]
pub struct Ledger {
    entries: std::collections::BTreeMap<u64, Observation>,
    next: u64,
    epoch: u64,
}
impl Ledger {
    pub fn reserve(&mut self, sealed: bool) -> Option<u64> {
        // Unknown lifetimes cannot accumulate unbounded handles or background readers.
        if sealed || self.entries.len() >= 64 {
            return None;
        }
        self.next = self.next.checked_add(1)?;
        self.entries.insert(self.next, Observation::Unknown);
        self.epoch = self.epoch.wrapping_add(1);
        Some(self.next)
    }
    pub fn observe(&mut self, id: u64, observation: Observation) {
        let Some(previous) = self.entries.get(&id) else {
            return;
        };
        if *previous == observation {
            return;
        }
        if observation == Observation::Complete {
            self.entries.remove(&id);
        } else {
            self.entries.insert(id, observation);
        }
        self.epoch = self.epoch.wrapping_add(1);
    }
    /// Only the adapter's explicit no-spawn branch may use this shortcut.
    pub fn not_spawned(&mut self, id: u64) {
        self.observe(id, Observation::Complete);
    }
    pub fn snapshot(&self) -> (Option<u64>, u64) {
        let active = if self
            .entries
            .values()
            .any(|state| *state == Observation::Unknown)
        {
            None
        } else {
            Some(self.entries.len() as u64)
        };
        (active, self.epoch)
    }
}

#[cfg(test)]
#[path = "native_activity_tests.rs"]
mod tests;
