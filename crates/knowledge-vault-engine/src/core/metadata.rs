//! Bounded scan ownership, independent of the SQLite mutex.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, Weak,
};

#[derive(Default)]
pub struct ScanControl(Mutex<Vec<Weak<AtomicBool>>>);

pub struct ScanTicket(Arc<AtomicBool>);
impl ScanTicket {
    pub fn cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}
impl ScanControl {
    /// A newer accepted scan cancels the old one cooperatively. At most two
    /// workers can be outstanding, including a cancelled worker stuck in OS I/O.
    pub fn begin(&self) -> Result<ScanTicket, String> {
        let mut active = self.0.lock().map_err(|_| "metadata_busy")?;
        active.retain(|token| token.strong_count() > 0);
        if active.len() >= 2 {
            return Err("metadata_busy".into());
        }
        for token in active.iter().filter_map(Weak::upgrade) {
            token.store(true, Ordering::Release);
        }
        let token = Arc::new(AtomicBool::new(false));
        active.push(Arc::downgrade(&token));
        Ok(ScanTicket(token))
    }
    /// Called while changing the configured root under the DB mutex, including
    /// A → B → A. Path equality alone cannot reauthorize an earlier scan.
    pub fn invalidate(&self) {
        if let Ok(active) = self.0.lock() {
            for token in active.iter().filter_map(Weak::upgrade) {
                token.store(true, Ordering::Release);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn supersession_root_changes_and_stalled_workers_are_bounded() {
        let control = ScanControl::default();
        let first = control.begin().unwrap();
        let second = control.begin().unwrap();
        assert!(first.cancelled());
        assert!(!second.cancelled());
        assert!(control.begin().is_err());
        assert!(!second.cancelled());
        control.invalidate();
        assert!(second.cancelled());
        drop(first);
        let third = control.begin().unwrap();
        assert!(!third.cancelled());
        drop(second);
        drop(third);
        assert!(control.begin().is_ok());
    }
}
