//! A PTY has one reader and a bounded replay log independent of renderer lifetime.
//! Product consumers pull ordered frames; output never enters a global event queue.
use serde::Serialize;
use std::collections::VecDeque;

pub const MAX_REPLAY_BYTES: usize = 512 * 1024;
pub const MAX_REPLAY_FRAMES: usize = 512;
pub const MAX_BATCH_BYTES: usize = 64 * 1024;
pub const MAX_FRAME_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Frame {
    pub sequence: u64,
    pub data: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputBatch {
    pub frames: Vec<Frame>,
    pub cursor: u64,
    /// The consumer must visibly mark a gap and reset incomplete parser state.
    pub truncated: bool,
    pub closed: bool,
    pub more: bool,
}

#[derive(Default)]
pub struct OutputBuffer {
    frames: VecDeque<Frame>,
    bytes: usize,
    sequence: u64,
    closed: bool,
}

impl OutputBuffer {
    pub fn append(&mut self, text: &str) {
        if self.closed {
            return;
        }
        let mut rest = text;
        while !rest.is_empty() {
            let mut end = rest.len().min(MAX_FRAME_BYTES);
            while !rest.is_char_boundary(end) {
                end -= 1;
            }
            let data = rest[..end].to_owned();
            rest = &rest[end..];
            self.sequence += 1;
            self.bytes += data.len();
            self.frames.push_back(Frame {
                sequence: self.sequence,
                data,
            });
            while self.bytes > MAX_REPLAY_BYTES || self.frames.len() > MAX_REPLAY_FRAMES {
                if let Some(frame) = self.frames.pop_front() {
                    self.bytes -= frame.data.len();
                }
            }
        }
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    pub fn close(&mut self) {
        self.closed = true;
    }

    pub fn read(&self, after: u64) -> Result<OutputBatch, &'static str> {
        if after > self.sequence {
            return Err("terminal_cursor_invalid");
        }
        let first = self
            .frames
            .front()
            .map_or(self.sequence + 1, |frame| frame.sequence);
        let truncated = after < first.saturating_sub(1);
        let mut cursor = after.max(first.saturating_sub(1));
        let mut bytes = 0;
        let mut frames = Vec::new();
        for frame in &self.frames {
            if frame.sequence <= after {
                continue;
            }
            if bytes + frame.data.len() > MAX_BATCH_BYTES {
                break;
            }
            bytes += frame.data.len();
            cursor = frame.sequence;
            frames.push(frame.clone());
        }
        let more = cursor < self.sequence;
        Ok(OutputBatch {
            frames,
            cursor,
            truncated,
            closed: self.closed && !more,
            more,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_renderer_observes_gap_and_bounded_utf8_batches_before_close() {
        let mut output = OutputBuffer::default();
        output.append(&"한글😀".repeat(MAX_REPLAY_BYTES));
        output.close();
        assert!(output.bytes <= MAX_REPLAY_BYTES);
        let first = output.read(0).unwrap();
        assert!(first.truncated);
        assert!(first.more);
        assert!(!first.closed);
        let mut cursor = first.cursor;
        loop {
            let batch = output.read(cursor).unwrap();
            assert!(!batch.truncated);
            assert!(
                batch
                    .frames
                    .iter()
                    .map(|frame| frame.data.len())
                    .sum::<usize>()
                    <= MAX_BATCH_BYTES
            );
            for pair in batch.frames.windows(2) {
                assert_eq!(pair[0].sequence + 1, pair[1].sequence);
            }
            cursor = batch.cursor;
            if batch.closed {
                break;
            }
        }
        assert_eq!(cursor, output.sequence);
        assert!(output.read(cursor + 1).is_err());
    }

    #[test]
    fn reload_has_independent_cursor_and_old_ack_cannot_consume_output() {
        let mut output = OutputBuffer::default();
        output.append("first");
        let first = output.read(0).unwrap();
        output.append("second");
        assert_eq!(output.read(first.cursor).unwrap().frames[0].data, "second");
        assert_eq!(output.read(0).unwrap().frames.len(), 2);
        output.close();
        output.append("ignored");
        assert_eq!(output.read(0).unwrap().frames.len(), 2);
    }

    #[test]
    fn tiny_frames_are_bounded_as_well_as_bytes() {
        let mut output = OutputBuffer::default();
        for _ in 0..(MAX_REPLAY_FRAMES + 10) {
            output.append("x");
        }
        assert_eq!(output.frames.len(), MAX_REPLAY_FRAMES);
        assert!(output.read(0).unwrap().truncated);
    }
}
