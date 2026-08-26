//! Bounded, incremental parser for the Server-Sent Events wire format.
//!
//! The parser deliberately has no network, filesystem, logging, or UI concerns.  It accepts
//! arbitrary byte chunk boundaries, keeps an incomplete UTF-8 code point between calls, and
//! returns only complete events.  All limits are protocol limits rather than allocation hints:
//! callers must treat a limit error as a failed stream and must not continue with partial data.

use std::collections::VecDeque;
use std::fmt;

pub const MAX_DECODED_BYTES: usize = 20 * 1024 * 1024;
pub const MAX_RETAINED_EVENTS: usize = 10_000;
pub const MAX_LINE_BYTES: usize = 64 * 1024;
pub const MAX_FIELD_BYTES: usize = 64 * 1024;
pub const MAX_EVENT_NAME_BYTES: usize = 256;
pub const MAX_EVENT_DATA_BYTES: usize = 1024 * 1024;
pub const MAX_EVENT_ID_BYTES: usize = 256;
pub const MAX_RETRY_MS: u64 = 60_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: String,
    pub data: String,
    pub id: Option<String>,
    pub retry_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    InvalidUtf8,
    LineTooLong,
    FieldTooLong,
    DataTooLong,
    EventNameTooLong,
    EventIdTooLong,
    InvalidEventId,
    InvalidRetry,
    StreamTooLarge,
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never include a line, field, byte sequence, or source value in this display string.
        formatter.write_str(match self {
            Self::InvalidUtf8 => "SSE stream text is invalid",
            Self::LineTooLong => "SSE stream line is too long",
            Self::FieldTooLong => "SSE stream field is too long",
            Self::DataTooLong => "SSE event data is too large",
            Self::EventNameTooLong => "SSE event name is too long",
            Self::EventIdTooLong => "SSE event id is too long",
            Self::InvalidEventId => "SSE event id is invalid",
            Self::InvalidRetry => "SSE retry value is invalid",
            Self::StreamTooLarge => "SSE stream is too large",
        })
    }
}

impl std::error::Error for ParseError {}

/// A bounded event history for callers that need replay/pause support.  The parser itself emits
/// events and does not retain the stream, while this small ring makes the 10,000 event / 20 MiB
/// contract explicit and testable without involving a webview.
#[derive(Debug, Clone, Default)]
pub struct EventBuffer {
    events: VecDeque<SseEvent>,
    bytes: usize,
    evicted: usize,
}

impl EventBuffer {
    pub fn push(&mut self, event: SseEvent) -> usize {
        let event_bytes = event_size(&event);
        self.events.push_back(event);
        self.bytes = self.bytes.saturating_add(event_bytes);
        let mut removed = 0;
        while self.events.len() > MAX_RETAINED_EVENTS || self.bytes > MAX_DECODED_BYTES {
            let Some(oldest) = self.events.pop_front() else {
                self.bytes = 0;
                break;
            };
            self.bytes = self.bytes.saturating_sub(event_size(&oldest));
            removed += 1;
            self.evicted = self.evicted.saturating_add(1);
        }
        removed
    }

    #[allow(dead_code)]
    pub fn events(&self) -> &VecDeque<SseEvent> {
        &self.events
    }

    #[allow(dead_code)]
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn evicted(&self) -> usize {
        self.evicted
    }
}

fn event_size(event: &SseEvent) -> usize {
    event
        .event
        .len()
        .saturating_add(event.data.len())
        .saturating_add(event.id.as_ref().map_or(0, String::len))
        .saturating_add(32)
}

#[derive(Debug, Default)]
pub struct SseParser {
    pending_utf8: Vec<u8>,
    line: String,
    pending_cr: bool,
    stream_started: bool,
    decoded_bytes: usize,
    event_name: String,
    data: String,
    last_event_id: String,
    retry_ms: Option<u64>,
    // Events completed by line terminators are staged here so a single input chunk can return all
    // of them without exposing mutable parser internals.
    ready: Vec<SseEvent>,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    #[allow(dead_code)]
    pub fn decoded_bytes(&self) -> usize {
        self.decoded_bytes
    }

    pub fn retry_ms(&self) -> Option<u64> {
        self.retry_ms
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<SseEvent>, ParseError> {
        self.decoded_bytes = self
            .decoded_bytes
            .checked_add(bytes.len())
            .ok_or(ParseError::StreamTooLarge)?;
        if self.decoded_bytes > MAX_DECODED_BYTES {
            return Err(ParseError::StreamTooLarge);
        }

        self.pending_utf8.extend_from_slice(bytes);
        let valid_len = match std::str::from_utf8(&self.pending_utf8) {
            Ok(text) => {
                let text = text.to_owned();
                self.pending_utf8.clear();
                self.consume_text(&text)?;
                return Ok(self.take_events());
            }
            Err(error) if error.error_len().is_none() => error.valid_up_to(),
            Err(_) => return Err(ParseError::InvalidUtf8),
        };

        let text = String::from_utf8(self.pending_utf8[..valid_len].to_vec())
            .map_err(|_| ParseError::InvalidUtf8)?;
        let remainder = self.pending_utf8[valid_len..].to_vec();
        self.pending_utf8 = remainder;
        self.consume_text(&text)?;
        Ok(self.take_events())
    }

    /// Flush the final unterminated line/event at EOF.  An incomplete UTF-8 sequence is a hard
    /// error; replacing it would make secrets and control values ambiguous.
    pub fn finish(&mut self) -> Result<Vec<SseEvent>, ParseError> {
        if !self.pending_utf8.is_empty() {
            return Err(ParseError::InvalidUtf8);
        }
        if self.pending_cr {
            self.pending_cr = false;
        }
        if !self.line.is_empty() {
            self.finish_line()?
        }
        if let Some(event) = self.dispatch()? {
            self.ready.push(event);
        }
        Ok(self.take_events())
    }

    fn consume_text(&mut self, text: &str) -> Result<(), ParseError> {
        if text.is_empty() {
            return Ok(());
        }
        let mut chars = text.chars();
        if !self.stream_started {
            self.stream_started = true;
            if chars.next() != Some('\u{feff}') {
                // The first character was not a BOM.  Process it below by rebuilding the small
                // iterator; this avoids retaining a potentially large first chunk.
                self.consume_chars(text.chars())?;
                return Ok(());
            }
        }
        self.consume_chars(chars)
    }

    fn consume_chars<I>(&mut self, chars: I) -> Result<(), ParseError>
    where
        I: Iterator<Item = char>,
    {
        for character in chars {
            if self.pending_cr {
                self.pending_cr = false;
                if character == '\n' {
                    continue;
                }
            }
            match character {
                '\r' => {
                    self.finish_line()?;
                    self.pending_cr = true;
                }
                '\n' => self.finish_line()?,
                _ => {
                    self.line.push(character);
                    if self.line.len() > MAX_LINE_BYTES {
                        return Err(ParseError::LineTooLong);
                    }
                }
            }
        }
        Ok(())
    }

    fn take_events(&mut self) -> Vec<SseEvent> {
        // `feed` uses a small staging queue to let a chunk return more than one complete event.
        // The queue is represented by `ready` in the parser extension below.
        std::mem::take(&mut self.ready)
    }

    fn finish_line(&mut self) -> Result<(), ParseError> {
        let line = std::mem::take(&mut self.line);
        if line.is_empty() {
            let event = self.dispatch()?;
            if let Some(event) = event {
                self.ready.push(event);
            }
            return Ok(());
        }
        if line.starts_with(':') {
            return Ok(());
        }
        let (field, mut value) = line
            .split_once(':')
            .map_or((line.as_str(), ""), |(field, value)| (field, value));
        if field.is_empty() || field.len() > MAX_FIELD_BYTES {
            return Err(ParseError::FieldTooLong);
        }
        if value.starts_with(' ') {
            value = &value[1..];
        }
        if value.len() > MAX_FIELD_BYTES {
            return Err(ParseError::FieldTooLong);
        }
        match field {
            "event" => {
                if value.len() > MAX_EVENT_NAME_BYTES {
                    return Err(ParseError::EventNameTooLong);
                }
                if value.chars().any(|character| character == '\0') {
                    return Err(ParseError::InvalidEventId);
                }
                self.event_name.clear();
                self.event_name.push_str(value);
            }
            "data" => {
                let next = self
                    .data
                    .len()
                    .checked_add(value.len())
                    .and_then(|length| length.checked_add(1))
                    .ok_or(ParseError::DataTooLong)?;
                if next > MAX_EVENT_DATA_BYTES {
                    return Err(ParseError::DataTooLong);
                }
                self.data.push_str(value);
                self.data.push('\n');
            }
            "id" => {
                if value.len() > MAX_EVENT_ID_BYTES {
                    return Err(ParseError::EventIdTooLong);
                }
                if value.chars().any(|character| character == '\0') {
                    return Err(ParseError::InvalidEventId);
                }
                self.last_event_id.clear();
                self.last_event_id.push_str(value);
            }
            "retry" => {
                self.retry_ms = Some(parse_retry(value)?);
            }
            // SSE ignores extension fields.  We still apply the field/line bounds above so an
            // untrusted extension cannot consume unbounded memory.
            _ => {}
        }
        Ok(())
    }

    fn dispatch(&mut self) -> Result<Option<SseEvent>, ParseError> {
        if self.data.is_empty() {
            self.event_name.clear();
            return Ok(None);
        }
        if self.data.ends_with('\n') {
            self.data.pop();
        }
        let event = SseEvent {
            event: if self.event_name.is_empty() {
                "message".to_string()
            } else {
                std::mem::take(&mut self.event_name)
            },
            data: std::mem::take(&mut self.data),
            id: (!self.last_event_id.is_empty()).then(|| self.last_event_id.clone()),
            retry_ms: self.retry_ms,
        };
        self.event_name.clear();
        Ok(Some(event))
    }
}

fn parse_retry(value: &str) -> Result<u64, ParseError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ParseError::InvalidRetry);
    }
    let value = value.parse::<u64>().map_err(|_| ParseError::InvalidRetry)?;
    if value > MAX_RETRY_MS {
        return Err(ParseError::InvalidRetry);
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(chunks: &[&[u8]]) -> Vec<SseEvent> {
        let mut parser = SseParser::new();
        let mut events = Vec::new();
        for chunk in chunks {
            events.extend(parser.feed(chunk).unwrap());
        }
        events.extend(parser.finish().unwrap());
        events
    }

    #[test]
    fn parses_comments_multiline_data_and_metadata() {
        let events = parse(&[b"\xef\xbb\xbf: hi\r\nevent: update\r\ndata: one\r\ndata: two\r\nid: 42\r\nretry: 1500\r\n\r\n"]);
        assert_eq!(
            events,
            vec![SseEvent {
                event: "update".into(),
                data: "one\ntwo".into(),
                id: Some("42".into()),
                retry_ms: Some(1500),
            }]
        );
    }

    #[test]
    fn supports_cr_lf_and_eof_flush() {
        let events = parse(&[b"data: a\r\rdata: b\n\ndata: c"]);
        assert_eq!(events[0].data, "a");
        assert_eq!(events[1].data, "b");
        assert_eq!(events[2].data, "c");
    }

    #[test]
    fn a_single_cr_separates_lines_without_dispatching_an_event() {
        let events = parse(&[b"data: first\rdata: second\r\r"]);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "first\nsecond");
    }

    #[test]
    fn split_utf8_and_bom_are_handled_incrementally() {
        let mut parser = SseParser::new();
        let mut events = Vec::new();
        for chunk in [
            b"\xef".as_slice(),
            b"\xbb",
            b"\xbfdata: caf",
            "é\n\n".as_bytes(),
        ] {
            events.extend(parser.feed(chunk).unwrap());
        }
        assert_eq!(parser.decoded_bytes(), 16);
        events.extend(parser.finish().unwrap());
        assert_eq!(events[0].data, "café");
    }

    #[test]
    fn empty_id_clears_previous_id() {
        let events = parse(&[b"id: old\ndata: first\n\nid:\ndata: second\n\n"]);
        assert_eq!(events[0].id.as_deref(), Some("old"));
        assert_eq!(events[1].id, None);
    }

    #[test]
    fn retry_metadata_survives_a_retry_only_chunk() {
        let mut parser = SseParser::new();
        assert!(parser.feed(b"retry: 2400\n").unwrap().is_empty());
        assert_eq!(parser.retry_ms(), Some(2400));
        assert!(parser.finish().unwrap().is_empty());
        assert_eq!(parser.retry_ms(), Some(2400));
    }

    #[test]
    fn malformed_retry_and_nul_id_fail_closed() {
        let mut parser = SseParser::new();
        assert_eq!(parser.feed(b"retry: 1.5\n"), Err(ParseError::InvalidRetry));
        let mut parser = SseParser::new();
        assert_eq!(
            parser.feed(b"id: bad\0id\n"),
            Err(ParseError::InvalidEventId)
        );
    }

    #[test]
    fn invalid_utf8_and_bounds_do_not_get_replaced() {
        let mut parser = SseParser::new();
        assert_eq!(parser.feed(&[0xff]), Err(ParseError::InvalidUtf8));
        let mut parser = SseParser::new();
        assert_eq!(
            parser.feed(format!("data: {}\n", "x".repeat(MAX_LINE_BYTES)).as_bytes()),
            Err(ParseError::LineTooLong)
        );
    }

    #[test]
    fn event_buffer_evicts_oldest_by_count_and_bytes() {
        let mut buffer = EventBuffer::default();
        for index in 0..=MAX_RETAINED_EVENTS {
            buffer.push(SseEvent {
                event: "message".into(),
                data: index.to_string(),
                id: None,
                retry_ms: None,
            });
        }
        assert_eq!(buffer.events().len(), MAX_RETAINED_EVENTS);
        assert_eq!(buffer.evicted(), 1);
        assert!(buffer.bytes() <= MAX_DECODED_BYTES);
    }
}
