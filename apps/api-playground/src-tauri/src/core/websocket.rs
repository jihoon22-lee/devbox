//! Pure bounds and retention primitives for the WebSocket transport.
//!
//! The transport keeps raw payloads in process memory only so an explicit binary save can
//! reference a bounded message without sending a filesystem path through the webview.  This
//! module deliberately has no Tauri, network, or filesystem dependency.

use std::collections::VecDeque;

pub const MAX_RETAINED_MESSAGES: usize = 10_000;
pub const MAX_BUFFER_BYTES: usize = 20 * 1024 * 1024;
pub const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_TEXT_PREVIEW_BYTES: usize = 64 * 1024;
pub const MAX_BINARY_PREVIEW_BYTES: usize = 4096;
pub const MAX_CONTROL_PAYLOAD_BYTES: usize = 125;
pub const MAX_CLOSE_REASON_BYTES: usize = 123;

pub const CLOSE_CODE_INVALID: &str = "WebSocket close code가 올바르지 않습니다";
pub const CLOSE_REASON_INVALID: &str = "WebSocket close reason이 올바르지 않습니다";
pub const MESSAGE_TOO_LARGE: &str = "WebSocket message가 허용된 크기를 초과했습니다";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Text,
    Binary,
    Ping,
    Pong,
    Close,
}

impl MessageKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Binary => "binary",
            Self::Ping => "ping",
            Self::Pong => "pong",
            Self::Close => "close",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageDirection {
    Sent,
    Received,
}

impl MessageDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sent => "sent",
            Self::Received => "received",
        }
    }
}

/// Raw payload retained for the current session.  It is never serialized directly.
#[derive(Debug, Clone)]
pub struct BufferedMessage {
    pub id: u64,
    pub kind: MessageKind,
    pub direction: MessageDirection,
    pub payload: Vec<u8>,
    pub close_code: Option<u16>,
    pub close_reason: String,
}

impl BufferedMessage {
    pub fn size_bytes(&self) -> usize {
        self.payload.len()
    }
}

#[derive(Debug, Default)]
pub struct MessageBuffer {
    messages: VecDeque<BufferedMessage>,
    bytes: usize,
    evicted: usize,
}

impl MessageBuffer {
    /// Push one bounded message and evict the oldest entries until both limits hold.
    pub fn push(&mut self, message: BufferedMessage) -> usize {
        self.bytes = self.bytes.saturating_add(message.size_bytes());
        self.messages.push_back(message);
        let mut removed = 0;
        while self.messages.len() > MAX_RETAINED_MESSAGES || self.bytes > MAX_BUFFER_BYTES {
            let Some(oldest) = self.messages.pop_front() else {
                self.bytes = 0;
                break;
            };
            self.bytes = self.bytes.saturating_sub(oldest.size_bytes());
            self.evicted = self.evicted.saturating_add(1);
            removed += 1;
        }
        removed
    }

    pub fn get(&self, id: u64) -> Option<&BufferedMessage> {
        self.messages.iter().find(|message| message.id == id)
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    #[allow(dead_code)]
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn evicted(&self) -> usize {
        self.evicted
    }
}

pub fn validate_payload(kind: MessageKind, bytes: &[u8]) -> Result<(), &'static str> {
    let limit = match kind {
        MessageKind::Ping | MessageKind::Pong => MAX_CONTROL_PAYLOAD_BYTES,
        MessageKind::Text | MessageKind::Binary | MessageKind::Close => MAX_MESSAGE_BYTES,
    };
    if bytes.len() > limit {
        return Err(MESSAGE_TOO_LARGE);
    }
    Ok(())
}

pub fn validate_close_code(code: Option<u16>) -> Result<u16, &'static str> {
    let code = code.unwrap_or(1000);
    // RFC 6455 reserves 1004–1006 and 1015, and private/application codes are 3000–4999.
    let allowed = matches!(code, 1000..=1003 | 1007..=1014 | 3000..=4999);
    if allowed {
        Ok(code)
    } else {
        Err(CLOSE_CODE_INVALID)
    }
}

pub fn validate_close_reason(reason: &str) -> Result<(), &'static str> {
    if reason.len() > MAX_CLOSE_REASON_BYTES || reason.chars().any(|character| character == '\0') {
        return Err(CLOSE_REASON_INVALID);
    }
    Ok(())
}

pub fn utf8_truncate(value: &str, max_bytes: usize) -> (String, bool) {
    if value.len() <= max_bytes {
        return (value.to_string(), false);
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].to_string(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(id: u64, bytes: usize) -> BufferedMessage {
        BufferedMessage {
            id,
            kind: MessageKind::Binary,
            direction: MessageDirection::Received,
            payload: vec![0; bytes],
            close_code: None,
            close_reason: String::new(),
        }
    }

    #[test]
    fn count_limit_evicts_oldest_and_reports_cumulative_count() {
        let mut buffer = MessageBuffer::default();
        for id in 0..=MAX_RETAINED_MESSAGES as u64 {
            buffer.push(message(id, 1));
        }
        assert_eq!(buffer.len(), MAX_RETAINED_MESSAGES);
        assert_eq!(buffer.evicted(), 1);
        assert!(buffer.get(0).is_none());
        assert!(buffer.get(MAX_RETAINED_MESSAGES as u64).is_some());
    }

    #[test]
    fn byte_limit_evicts_oldest_until_under_twenty_mib() {
        let mut buffer = MessageBuffer::default();
        buffer.push(message(1, MAX_BUFFER_BYTES - 1));
        buffer.push(message(2, 2));
        assert_eq!(buffer.bytes(), 2);
        assert_eq!(buffer.evicted(), 1);
        assert!(buffer.get(1).is_none());
        assert!(buffer.get(2).is_some());
    }

    #[test]
    fn message_and_control_bounds_are_distinct() {
        assert!(validate_payload(MessageKind::Text, &[0; MAX_MESSAGE_BYTES]).is_ok());
        assert_eq!(
            validate_payload(MessageKind::Text, &[0; MAX_MESSAGE_BYTES + 1]),
            Err(MESSAGE_TOO_LARGE)
        );
        assert!(validate_payload(MessageKind::Ping, &[0; MAX_CONTROL_PAYLOAD_BYTES]).is_ok());
        assert_eq!(
            validate_payload(MessageKind::Ping, &[0; MAX_CONTROL_PAYLOAD_BYTES + 1]),
            Err(MESSAGE_TOO_LARGE)
        );
    }

    #[test]
    fn close_codes_and_reasons_are_fail_closed() {
        assert_eq!(validate_close_code(None), Ok(1000));
        assert!(validate_close_code(Some(1006)).is_err());
        assert!(validate_close_code(Some(2999)).is_err());
        assert!(validate_close_code(Some(3000)).is_ok());
        assert!(validate_close_reason(&"x".repeat(MAX_CLOSE_REASON_BYTES)).is_ok());
        assert!(validate_close_reason(&"x".repeat(MAX_CLOSE_REASON_BYTES + 1)).is_err());
        assert!(validate_close_reason("bad\0reason").is_err());
    }

    #[test]
    fn utf8_preview_does_not_split_a_code_point() {
        let (preview, truncated) = utf8_truncate("aé", 2);
        assert_eq!(preview, "a");
        assert!(truncated);
    }
}
