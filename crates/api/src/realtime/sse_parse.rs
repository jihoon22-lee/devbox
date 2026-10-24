//! 상한 있는 증분 SSE 파서(v0.9.0 `api-protocols/core/sse.rs` 이식).
//!
//! 조각 경계와 상관없이 완성된 이벤트만 돌려주고, 끝나지 않은 UTF-8 코드 포인트는 다음 조각까지 둔다.
//! 줄·필드·이벤트 크기 상한을 넘거나 텍스트가 잘못되면 스트림 실패다(부분 데이터로 이어 가지 않는다).
//! v1 적응: 오래 열린 스트림이므로 v0.9.0의 스트림 전체 20MiB 상한과 보관 링 버퍼는 두지 않는다
//! (보관은 데몬 큐 `buffer.rs`와 화면 목록 상한이 맡는다).

pub(crate) const MAX_LINE_BYTES: usize = 64 * 1024;
pub(crate) const MAX_FIELD_BYTES: usize = 64 * 1024;
pub(crate) const MAX_EVENT_NAME_BYTES: usize = 256;
pub(crate) const MAX_EVENT_DATA_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_EVENT_ID_BYTES: usize = 256;
pub(crate) const MAX_RETRY_MS: u64 = 60_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SseEvent {
    pub(crate) event: String,
    pub(crate) data: String,
    pub(crate) id: Option<String>,
    pub(crate) retry_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParseError {
    InvalidUtf8,
    LineTooLong,
    FieldTooLong,
    DataTooLong,
    EventNameTooLong,
    EventIdTooLong,
    InvalidEventId,
    InvalidRetry,
}

impl ParseError {
    /// `api.network{ kind }`에 쓰는 고정 사유(값을 담지 않는다).
    pub(crate) fn kind(self) -> &'static str {
        match self {
            Self::InvalidUtf8 => "sse_invalid_utf8",
            Self::LineTooLong | Self::FieldTooLong => "sse_line_too_long",
            Self::DataTooLong => "sse_event_too_large",
            Self::EventNameTooLong | Self::EventIdTooLong => "sse_field_too_long",
            Self::InvalidEventId => "sse_invalid_id",
            Self::InvalidRetry => "sse_invalid_retry",
        }
    }
}

/// 줄·필드 값·이벤트 data 상한. 기본은 실시간 SSE 값이고 MCP는 메시지 상한(4MiB)을 쓴다.
#[derive(Debug, Clone, Copy)]
struct Limits {
    line: usize,
    value: usize,
    data: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            line: MAX_LINE_BYTES,
            value: MAX_FIELD_BYTES,
            data: MAX_EVENT_DATA_BYTES,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct SseParser {
    limits: Limits,
    pending_utf8: Vec<u8>,
    line: String,
    pending_cr: bool,
    stream_started: bool,
    decoded_bytes: usize,
    event_name: String,
    data: String,
    last_event_id: String,
    retry_ms: Option<u64>,
    ready: Vec<SseEvent>,
}

impl SseParser {
    /// 재연결 때 마지막 id를 이어 받는다(새 연결의 BOM·버퍼는 새로 시작).
    pub(crate) fn resumed(last_event_id: Option<&str>, retry_ms: Option<u64>) -> Self {
        Self {
            last_event_id: last_event_id.unwrap_or_default().to_owned(),
            retry_ms,
            ..Self::default()
        }
    }

    /// 한 줄·한 이벤트 data를 `max` 바이트까지 받는다(필드 이름·event·id 상한은 그대로).
    pub(crate) fn with_limit(max: usize) -> Self {
        Self {
            limits: Limits {
                line: max.saturating_add(16),
                value: max,
                data: max,
            },
            ..Self::default()
        }
    }

    #[cfg(test)]
    fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    fn decoded_bytes(&self) -> usize {
        self.decoded_bytes
    }

    pub(crate) fn retry_ms(&self) -> Option<u64> {
        self.retry_ms
    }

    pub(crate) fn feed(&mut self, bytes: &[u8]) -> Result<Vec<SseEvent>, ParseError> {
        self.decoded_bytes = self.decoded_bytes.saturating_add(bytes.len());
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

    /// 끝나지 않은 마지막 줄·이벤트를 내보낸다. 끝나지 않은 UTF-8은 오류다.
    pub(crate) fn finish(&mut self) -> Result<Vec<SseEvent>, ParseError> {
        if !self.pending_utf8.is_empty() {
            return Err(ParseError::InvalidUtf8);
        }
        self.pending_cr = false;
        if !self.line.is_empty() {
            self.finish_line()?
        }
        if let Some(event) = self.dispatch() {
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
                return self.consume_chars(text.chars());
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
                    if self.line.len() > self.limits.line {
                        return Err(ParseError::LineTooLong);
                    }
                }
            }
        }
        Ok(())
    }

    fn take_events(&mut self) -> Vec<SseEvent> {
        std::mem::take(&mut self.ready)
    }

    fn finish_line(&mut self) -> Result<(), ParseError> {
        let line = std::mem::take(&mut self.line);
        if line.is_empty() {
            if let Some(event) = self.dispatch() {
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
        if let Some(rest) = value.strip_prefix(' ') {
            value = rest;
        }
        if value.len() > self.limits.value {
            return Err(ParseError::FieldTooLong);
        }
        match field {
            "event" => {
                if value.len() > MAX_EVENT_NAME_BYTES {
                    return Err(ParseError::EventNameTooLong);
                }
                if value.contains('\0') {
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
                if next > self.limits.data {
                    return Err(ParseError::DataTooLong);
                }
                self.data.push_str(value);
                self.data.push('\n');
            }
            "id" => {
                if value.len() > MAX_EVENT_ID_BYTES {
                    return Err(ParseError::EventIdTooLong);
                }
                if value.contains('\0') {
                    return Err(ParseError::InvalidEventId);
                }
                self.last_event_id.clear();
                self.last_event_id.push_str(value);
            }
            "retry" => self.retry_ms = Some(parse_retry(value)?),
            // 확장 필드는 무시한다(위의 줄·필드 상한은 그대로 적용).
            _ => {}
        }
        Ok(())
    }

    fn dispatch(&mut self) -> Option<SseEvent> {
        if self.data.is_empty() {
            self.event_name.clear();
            return None;
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
        Some(event)
    }

    /// 마지막으로 정해진 id(이벤트를 내보내지 않았어도 `id:` 줄이 바꾼다).
    pub(crate) fn last_event_id(&self) -> Option<&str> {
        (!self.last_event_id.is_empty()).then_some(self.last_event_id.as_str())
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
            events.extend(parser.feed(chunk).expect("feed"));
        }
        events.extend(parser.finish().expect("finish"));
        events
    }

    #[test]
    fn parses_comments_multiline_data_and_metadata() {
        let events = parse(&[
            b"\xef\xbb\xbf: hi\r\nevent: update\r\ndata: one\r\ndata: two\r\nid: 42\r\nretry: 1500\r\n\r\n",
        ]);
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
            events.extend(parser.feed(chunk).expect("feed"));
        }
        assert_eq!(parser.decoded_bytes(), 16);
        events.extend(parser.finish().expect("finish"));
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
        assert!(parser.feed(b"retry: 2400\n").expect("feed").is_empty());
        assert_eq!(parser.retry_ms(), Some(2400));
        assert!(parser.finish().expect("finish").is_empty());
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

    /// v1 추가: 줄바꿈 없이 계속 오는 바이트는 줄 상한에서 멈춘다(메모리 증가 없음).
    #[test]
    fn a_stream_without_newlines_stops_at_the_line_limit() {
        let mut parser = SseParser::new();
        let chunk = vec![b'x'; 8 * 1024];
        let mut failed = None;
        for _ in 0..(MAX_LINE_BYTES / chunk.len() + 2) {
            if let Err(error) = parser.feed(&chunk) {
                failed = Some(error);
                break;
            }
        }
        assert_eq!(failed, Some(ParseError::LineTooLong));
        let mut parser = SseParser::new();
        let mut data = String::new();
        while data.len() <= MAX_EVENT_DATA_BYTES + 64 * 1024 {
            data.push_str(&format!("data: {}\n", "y".repeat(1000)));
        }
        assert_eq!(parser.feed(data.as_bytes()), Err(ParseError::DataTooLong));
    }

    #[test]
    fn resumed_parser_keeps_the_last_id_for_events_without_one() {
        let mut parser = SseParser::resumed(Some("7"), Some(50));
        let events = parser.feed(b"data: next\n\n").expect("feed");
        assert_eq!(events[0].id.as_deref(), Some("7"));
        assert_eq!(parser.retry_ms(), Some(50));
        assert_eq!(parser.last_event_id(), Some("7"));
    }
}
