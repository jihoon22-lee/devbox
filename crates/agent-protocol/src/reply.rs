//! Bounded large-result transport over the existing Stream/Ack envelopes.
use crate::ProtocolError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
const CHUNK_BYTES: usize = 24 * 1024;
pub const MAX_REPLY_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_BATCH_BYTES: usize = 64 * 1024;
type Result<T> = std::result::Result<T, ProtocolError>;
#[derive(Clone, Debug, Deserialize, Serialize)]
enum Kind {
    #[serde(rename = "replyChunk")]
    ReplyChunk,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Chunk {
    kind: Kind,
    pub offset: u64,
    pub total_bytes: u64,
    pub data: String,
}
fn bounded_json(value: &Value) -> Result<String> {
    struct Output {
        bytes: Vec<u8>,
        full: bool,
    }
    impl std::io::Write for Output {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > MAX_REPLY_BYTES.saturating_sub(self.bytes.len()) {
                self.full = true;
                return Err(std::io::Error::other("reply_too_large"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut output = Output {
        bytes: Vec::new(),
        full: false,
    };
    serde_json::to_writer(&mut output, value).map_err(|_| {
        if output.full {
            ProtocolError::TooLarge
        } else {
            ProtocolError::Malformed
        }
    })?;
    String::from_utf8(output.bytes).map_err(|_| ProtocolError::Malformed)
}
pub struct Sender {
    json: String,
    offset: usize,
    awaiting: Option<usize>,
}
impl Sender {
    pub fn new(value: &Value) -> Result<Self> {
        Ok(Self {
            json: bounded_json(value)?,
            offset: 0,
            awaiting: None,
        })
    }
    pub fn awaiting_cursor(&self) -> Option<u64> {
        self.awaiting.map(|value| value as u64)
    }
    pub fn buffered_bytes(&self) -> usize {
        self.json.len()
    }
    pub fn next_chunk(&mut self) -> Result<Option<Chunk>> {
        if self.awaiting.is_some() {
            return Err(ProtocolError::Duplicate);
        }
        if self.offset == self.json.len() {
            return Ok(None);
        }
        let mut end = (self.offset + CHUNK_BYTES).min(self.json.len());
        while !self.json.is_char_boundary(end) {
            end -= 1;
        }
        self.awaiting = Some(end);
        Ok(Some(Chunk {
            kind: Kind::ReplyChunk,
            offset: self.offset as u64,
            total_bytes: self.json.len() as u64,
            data: self.json[self.offset..end].into(),
        }))
    }
    pub fn ack(&mut self, cursor: u64) -> Result<()> {
        if self.awaiting.map(|value| value as u64) != Some(cursor) {
            return Err(ProtocolError::Malformed);
        }
        self.offset = self.awaiting.take().ok_or(ProtocolError::Malformed)?;
        Ok(())
    }
}
#[derive(Default)]
pub struct Receiver {
    json: String,
    total: Option<usize>,
}
impl Receiver {
    pub fn expected_bytes(&self) -> usize {
        self.total.unwrap_or(0)
    }
    pub fn push(&mut self, chunk: Chunk) -> Result<u64> {
        let total = usize::try_from(chunk.total_bytes).map_err(|_| ProtocolError::TooLarge)?;
        if total == 0
            || total > MAX_REPLY_BYTES
            || chunk.data.is_empty()
            || chunk.data.len() > CHUNK_BYTES
        {
            return Err(ProtocolError::TooLarge);
        }
        if chunk.offset != self.json.len() as u64
            || self.total.is_some_and(|expected| expected != total)
            || chunk.data.len() > total.saturating_sub(self.json.len())
        {
            return Err(ProtocolError::Malformed);
        }
        // Reserve room for the outer Stream envelope even for escaped controls.
        if serde_json::to_vec(&chunk)
            .map_err(|_| ProtocolError::Malformed)?
            .len()
            > MAX_BATCH_BYTES - 128
        {
            return Err(ProtocolError::TooLarge);
        }
        self.total = Some(total);
        self.json.push_str(&chunk.data);
        Ok(self.json.len() as u64)
    }
    pub fn finish(self) -> Result<Value> {
        if self.total != Some(self.json.len()) {
            return Err(ProtocolError::Malformed);
        }
        serde_json::from_str(&self.json).map_err(|_| ProtocolError::Malformed)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_and_escaped_fields_round_trip_with_one_unacknowledged_batch() {
        let value = serde_json::json!({"content":"λ\\\"\n".repeat(300_000)});
        let mut sender = Sender::new(&value).unwrap();
        let mut receiver = Receiver::default();
        let mut batches = 0;
        while let Some(chunk) = sender.next_chunk().unwrap() {
            assert!(sender.next_chunk().is_err());
            let frame = crate::AgentMessage::Stream {
                stream: 7,
                payload: serde_json::to_value(&chunk).unwrap(),
            };
            assert!(crate::encode(&frame).unwrap().len() <= MAX_BATCH_BYTES + 4);
            let cursor = receiver.push(chunk).unwrap();
            assert!(sender.ack(cursor + 1).is_err());
            sender.ack(cursor).unwrap();
            assert!(sender.ack(cursor).is_err());
            batches += 1;
        }
        assert!(batches > 1);
        assert_eq!(receiver.finish().unwrap(), value);
    }
    #[test]
    fn serialized_reply_limit_counts_json_escaping_and_never_truncates() {
        let value = serde_json::json!("\0".repeat(MAX_REPLY_BYTES / 6 + 1));
        assert!(matches!(Sender::new(&value), Err(ProtocolError::TooLarge)));
    }
    #[test]
    fn incomplete_reordered_and_unbounded_replies_are_rejected() {
        let mut sender = Sender::new(&serde_json::json!("x".repeat(CHUNK_BYTES * 2))).unwrap();
        let chunk = sender.next_chunk().unwrap().unwrap();
        let mut receiver = Receiver::default();
        receiver.push(chunk.clone()).unwrap();
        assert!(receiver.push(chunk.clone()).is_err());
        assert!(receiver.finish().is_err());
        let mut too_large = chunk;
        too_large.total_bytes = MAX_REPLY_BYTES as u64 + 1;
        assert!(Receiver::default().push(too_large).is_err());
    }
}

/// Encode borrowed envelopes without cloning potentially large response trees.
pub fn encode_reply(id: u64, response: &Value) -> Result<Vec<u8>> {
    #[derive(Serialize)]
    struct Borrowed<'a> {
        #[serde(rename = "type")]
        kind: &'static str,
        id: u64,
        response: &'a Value,
    }
    crate::encode(&Borrowed {
        kind: "reply",
        id,
        response,
    })
}
pub fn check_call(component: &str, request: &Value) -> Result<()> {
    #[derive(Serialize)]
    struct Borrowed<'a> {
        #[serde(rename = "type")]
        kind: &'static str,
        id: u64,
        component: &'a str,
        request: &'a Value,
    }
    crate::encode(&Borrowed {
        kind: "call",
        id: u64::MAX,
        component,
        request,
    })
    .map(|_| ())
}
