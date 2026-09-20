use super::model::{LogRecord, MAX_RECORDS, MAX_SOURCE_BYTES};
use std::collections::VecDeque;

/// A process-memory ring. It never writes raw log lines to disk and reports
/// evictions so the UI can explain backpressure instead of silently claiming
/// to have retained an unbounded history.
#[derive(Debug, Clone)]
pub struct RingBuffer {
    records: VecDeque<LogRecord>,
    bytes: usize,
    line_limit: usize,
    byte_limit: usize,
    dropped_records: usize,
    dropped_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BufferPush {
    pub accepted: bool,
    pub evicted_records: usize,
    pub evicted_bytes: usize,
    pub dropped_records: usize,
    pub dropped_bytes: usize,
}

impl Default for RingBuffer {
    fn default() -> Self {
        Self::new(MAX_RECORDS, MAX_SOURCE_BYTES)
    }
}

impl RingBuffer {
    pub fn new(line_limit: usize, byte_limit: usize) -> Self {
        Self {
            records: VecDeque::new(),
            bytes: 0,
            line_limit: line_limit.max(1),
            byte_limit: byte_limit.max(1),
            dropped_records: 0,
            dropped_bytes: 0,
        }
    }

    pub fn push(&mut self, record: LogRecord) -> BufferPush {
        let record_bytes = record.estimated_bytes();
        if record_bytes > self.byte_limit {
            self.dropped_records = self.dropped_records.saturating_add(1);
            self.dropped_bytes = self.dropped_bytes.saturating_add(record_bytes);
            return BufferPush {
                accepted: false,
                evicted_records: 0,
                evicted_bytes: 0,
                dropped_records: self.dropped_records,
                dropped_bytes: self.dropped_bytes,
            };
        }

        self.bytes = self.bytes.saturating_add(record_bytes);
        self.records.push_back(record);
        let mut evicted_records: usize = 0;
        let mut evicted_bytes: usize = 0;
        while self.records.len() > self.line_limit || self.bytes > self.byte_limit {
            let Some(evicted) = self.records.pop_front() else {
                break;
            };
            let bytes = evicted.estimated_bytes();
            self.bytes = self.bytes.saturating_sub(bytes);
            self.dropped_records = self.dropped_records.saturating_add(1);
            self.dropped_bytes = self.dropped_bytes.saturating_add(bytes);
            evicted_records = evicted_records.saturating_add(1);
            evicted_bytes = evicted_bytes.saturating_add(bytes);
        }
        BufferPush {
            accepted: true,
            evicted_records,
            evicted_bytes,
            dropped_records: self.dropped_records,
            dropped_bytes: self.dropped_bytes,
        }
    }

    pub fn extend<I>(&mut self, records: I) -> BufferPush
    where
        I: IntoIterator<Item = LogRecord>,
    {
        let mut summary = BufferPush {
            accepted: false,
            evicted_records: 0,
            evicted_bytes: 0,
            dropped_records: self.dropped_records,
            dropped_bytes: self.dropped_bytes,
        };
        for record in records {
            let result = self.push(record);
            summary.accepted |= result.accepted;
            summary.evicted_records = summary
                .evicted_records
                .saturating_add(result.evicted_records);
            summary.evicted_bytes = summary.evicted_bytes.saturating_add(result.evicted_bytes);
            summary.dropped_records = result.dropped_records;
            summary.dropped_bytes = result.dropped_bytes;
        }
        summary
    }

    pub fn snapshot(&self) -> Vec<LogRecord> {
        self.records.iter().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn dropped_records(&self) -> usize {
        self.dropped_records
    }

    pub fn dropped_bytes(&self) -> usize {
        self.dropped_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{parse_line, LogFormat};

    #[test]
    fn evicts_oldest_record_at_line_limit() {
        let mut buffer = RingBuffer::new(2, 10_000);
        buffer.push(parse_line("one", "a", 0));
        buffer.push(parse_line("two", "a", 1));
        let result = buffer.push(parse_line("three", "a", 2));
        assert_eq!(buffer.len(), 2);
        assert_eq!(buffer.snapshot()[0].message, "two");
        assert_eq!(result.evicted_records, 1);
        assert_eq!(buffer.dropped_records(), 1);
    }

    #[test]
    fn evicts_by_bytes_and_rejects_oversized_record() {
        let mut buffer = RingBuffer::new(10, 180);
        let first = parse_line("first", "a", 0);
        buffer.push(first);
        let second = parse_line("second", "a", 1);
        buffer.push(second);
        assert!(buffer.bytes() <= 180);
        let mut huge = parse_line("x", "a", 2);
        huge.message = "x".repeat(200);
        let result = buffer.push(huge);
        assert!(!result.accepted);
        assert_eq!(
            result.dropped_records,
            1 + buffer.dropped_records().saturating_sub(1)
        );
    }

    #[test]
    fn extend_reports_backpressure_without_persisting_raw_data() {
        let mut buffer = RingBuffer::new(1, 10_000);
        let result = buffer.extend([parse_line("a", "a", 0), parse_line("b", "a", 1)]);
        assert!(result.accepted);
        assert_eq!(buffer.snapshot()[0].format, LogFormat::Plain);
        assert_eq!(buffer.dropped_records(), 1);
    }
}
