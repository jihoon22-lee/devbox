//! The app's offline queue of activity observations (platform independent).
//!
//! The daemon contract (`activity.ingest`, Task 8): `seq` numbers a run's
//! observations from 1 and never restarts within the run, and `dropped` is the
//! run's cumulative count of observations this buffer threw away. Resending a
//! batch repeats the same value; the daemon adds only the increase. So neither
//! counter is reset by `ack` or `clear`; a new run is a new buffer.
use devbox_activity::model::event::Observation;
use serde::Serialize;
use std::collections::VecDeque;

/// Observations kept while the daemon is unreachable (01 §6.1).
pub const CAPACITY: usize = 1000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Batch {
    pub events: Vec<Observation>,
    /// Cumulative for the run, unchanged by `ack`.
    pub dropped: u32,
    /// `seq` of the last event; `0` when nothing is waiting.
    pub upto: u64,
}

#[derive(Debug)]
pub struct OfflineBuffer {
    queue: VecDeque<Observation>,
    next_seq: u64,
    dropped: u32,
}

impl Default for OfflineBuffer {
    fn default() -> Self {
        Self {
            queue: VecDeque::new(),
            next_seq: 1,
            dropped: 0,
        }
    }
}

impl OfflineBuffer {
    /// Numbers the observation and queues it; a full queue drops its oldest.
    pub fn push(&mut self, mut observation: Observation) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        observation.seq = seq;
        if self.queue.len() >= CAPACITY {
            self.queue.pop_front();
            self.dropped = self.dropped.saturating_add(1);
        }
        self.queue.push_back(observation);
        seq
    }

    /// The oldest `max` observations, left queued until `ack`.
    pub fn drain(&self, max: usize) -> Batch {
        let events: Vec<Observation> = self.queue.iter().take(max).cloned().collect();
        Batch {
            upto: events.last().map_or(0, |e| e.seq),
            events,
            dropped: self.dropped,
        }
    }

    /// The daemon accepted everything up to `upto`.
    pub fn ack(&mut self, upto: u64) {
        while self.queue.front().is_some_and(|e| e.seq <= upto) {
            self.queue.pop_front();
        }
    }

    /// Consent withdrawn: forget what is waiting (not counted as dropped).
    pub fn clear(&mut self) {
        self.queue.clear();
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use devbox_activity::model::event::ObservationKind;

    fn obs(at_ms: i64) -> Observation {
        Observation {
            seq: 0,
            at_ms,
            kind: ObservationKind::Foreground,
            app: Some("code.exe".into()),
            title: None,
            last_input_ms: at_ms,
        }
    }
    fn seqs(batch: &Batch) -> Vec<u64> {
        batch.events.iter().map(|e| e.seq).collect()
    }

    #[test]
    fn offline_buffer_caps_at_1000_and_counts_drops() {
        let mut buffer = OfflineBuffer::default();
        assert_eq!(buffer.drain(10).upto, 0, "nothing waiting");
        for at in 0..1003 {
            buffer.push(obs(at));
        }
        assert_eq!(buffer.len(), 1000);
        let all = buffer.drain(1000);
        // The oldest three went; numbering starts at 1 and has no gaps.
        assert_eq!(seqs(&all), (4..=1003).collect::<Vec<_>>());
        assert_eq!(all.events[0].at_ms, 3);
        assert_eq!((all.dropped, all.upto), (3, 1003));
        assert_eq!(
            buffer.drain(1000),
            all,
            "drain leaves the queue for a resend"
        );

        let first = buffer.drain(10);
        assert_eq!((seqs(&first)[0], first.upto, first.dropped), (4, 13, 3));
        buffer.ack(first.upto);
        assert_eq!(buffer.len(), 990);
        let rest = buffer.drain(1000);
        assert_eq!(
            (rest.events[0].seq, rest.dropped),
            (14, 3),
            "ack keeps the run's total"
        );
        buffer.ack(5); // already acknowledged
        assert_eq!(buffer.len(), 990);

        for at in 0..15 {
            buffer.push(obs(10_000 + at));
        }
        let full = buffer.drain(1000);
        assert_eq!(
            (full.events.len(), full.dropped),
            (1000, 8),
            "drops accumulate per run"
        );
        assert_eq!((full.events[0].seq, full.upto), (19, 1018));

        buffer.ack(u64::MAX);
        assert!(buffer.is_empty());
        assert_eq!(buffer.drain(1000).dropped, 8);
    }

    #[test]
    fn clear_forgets_the_queue_but_not_the_run_counters() {
        let mut buffer = OfflineBuffer::default();
        for at in 0..1001 {
            buffer.push(obs(at));
        }
        buffer.clear();
        assert!(buffer.is_empty());
        // The daemon skips `seq <= upto` of the run, so numbering must continue.
        assert_eq!(buffer.push(obs(5)), 1002);
        let batch = buffer.drain(1000);
        assert_eq!(
            (seqs(&batch), batch.dropped, batch.upto),
            (vec![1002], 1, 1002)
        );
    }
}
