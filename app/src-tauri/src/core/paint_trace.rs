//! SC2 acceptance timing on the native monotonic clock only; renderer clocks are never subtracted.
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

pub const PENDING_LIMIT: usize = 16;

#[derive(Default)]
pub struct PaletteTraces {
    last: u64,
    pending: VecDeque<(u64, Instant)>,
}
#[derive(Debug, PartialEq, Eq)]
pub enum TraceEnd {
    Elapsed(Duration),
    /// Allocated earlier but already acknowledged or dropped to keep the store bounded.
    Stale,
    Unknown,
}
impl PaletteTraces {
    /// Returns the new trace ID and the oldest pending ID dropped to stay within the limit.
    pub fn allocate(&mut self, now: Instant) -> (u64, Option<u64>) {
        self.last = self.last.saturating_add(1);
        let dropped = if self.pending.len() == PENDING_LIMIT {
            self.pending.pop_front().map(|(id, _)| id)
        } else {
            None
        };
        self.pending.push_back((self.last, now));
        (self.last, dropped)
    }
    pub fn finish(&mut self, id: u64, now: Instant) -> TraceEnd {
        match self.pending.iter().position(|(pending, _)| *pending == id) {
            Some(index) => {
                let (_, started) = self.pending.remove(index).expect("pending trace");
                TraceEnd::Elapsed(now.saturating_duration_since(started))
            }
            None if (1..=self.last).contains(&id) => TraceEnd::Stale,
            None => TraceEnd::Unknown,
        }
    }
}
pub struct FirstData {
    started: Instant,
    reported: bool,
}
impl FirstData {
    pub fn new(started: Instant) -> Self {
        Self {
            started,
            reported: false,
        }
    }
    /// Only the first report in the process carries an elapsed time.
    pub fn finish(&mut self, now: Instant) -> Option<Duration> {
        (!std::mem::replace(&mut self.reported, true))
            .then(|| now.saturating_duration_since(self.started))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn palette_trace_is_acknowledged_once_and_unknown_ids_carry_no_elapsed_time() {
        let start = Instant::now();
        let mut traces = PaletteTraces::default();
        let (first, dropped) = traces.allocate(start);
        assert_eq!((first, dropped), (1, None));
        let (second, _) = traces.allocate(start + Duration::from_millis(5));
        assert_eq!(second, 2);
        assert_eq!(
            traces.finish(second, start + Duration::from_millis(45)),
            TraceEnd::Elapsed(Duration::from_millis(40))
        );
        assert_eq!(traces.finish(second, start), TraceEnd::Stale);
        assert_eq!(traces.finish(0, start), TraceEnd::Unknown);
        assert_eq!(traces.finish(99, start), TraceEnd::Unknown);
        assert_eq!(
            traces.finish(first, start + Duration::from_millis(7)),
            TraceEnd::Elapsed(Duration::from_millis(7))
        );
    }
    #[test]
    fn overflow_drops_and_expires_the_oldest_pending_trace() {
        let start = Instant::now();
        let mut traces = PaletteTraces::default();
        for expected in 1..=PENDING_LIMIT as u64 {
            assert_eq!(traces.allocate(start), (expected, None));
        }
        let (next, dropped) = traces.allocate(start);
        assert_eq!((next, dropped), (PENDING_LIMIT as u64 + 1, Some(1)));
        assert_eq!(traces.finish(1, start), TraceEnd::Stale);
        assert_eq!(
            traces.finish(2, start + Duration::from_millis(3)),
            TraceEnd::Elapsed(Duration::from_millis(3))
        );
        assert_eq!(traces.pending.len(), PENDING_LIMIT - 1);
    }
    #[test]
    fn first_data_paint_is_reported_once_per_process() {
        let start = Instant::now();
        let mut first = FirstData::new(start);
        assert_eq!(
            first.finish(start + Duration::from_millis(1200)),
            Some(Duration::from_millis(1200))
        );
        assert_eq!(first.finish(start + Duration::from_millis(1500)), None);
    }
}
