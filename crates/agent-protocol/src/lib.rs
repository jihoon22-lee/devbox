//! Frames and messages between product UIs and the per-user devbox-agent.
//! A frame is a 4-byte little-endian length followed by a JSON body.
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::HashSet;

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolError {
    Empty,
    TooLarge,
    Malformed,
    Mismatch,
    Duplicate,
}

impl ProtocolError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Empty => "frame_empty",
            Self::TooLarge => "frame_too_large",
            Self::Malformed => "frame_malformed",
            Self::Mismatch => "protocol_mismatch",
            Self::Duplicate => "duplicate_request",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ClientMessage {
    Hello {
        protocol: u32,
        product: String,
        session: String,
    },
    Call {
        id: u64,
        component: String,
        request: serde_json::Value,
    },
    Cancel {
        id: u64,
    },
    Ack {
        stream: u64,
        cursor: u64,
    },
    Unsubscribe {
        stream: u64,
    },
    Shutdown {},
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AgentMessage {
    Welcome {
        protocol: u32,
        agent_version: String,
        generation: String,
    },
    Rejected {
        reason: String,
    },
    Reply {
        id: u64,
        response: serde_json::Value,
    },
    Stream {
        stream: u64,
        payload: serde_json::Value,
    },
    StreamEnd {
        stream: u64,
        reason: String,
    },
}

/// Serializes with a bounded writer, before allocating an oversized JSON body.
pub fn encode<T: Serialize>(message: &T) -> Result<Vec<u8>, ProtocolError> {
    struct Writer {
        frame: Vec<u8>,
        too_large: bool,
    }
    impl std::io::Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > MAX_FRAME_BYTES + 4 - self.frame.len() {
                self.too_large = true;
                return Err(std::io::Error::other("frame_too_large"));
            }
            self.frame.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = Writer {
        frame: vec![0; 4],
        too_large: false,
    };
    if serde_json::to_writer(&mut writer, message).is_err() {
        return Err(if writer.too_large {
            ProtocolError::TooLarge
        } else {
            ProtocolError::Malformed
        });
    }
    let length = writer.frame.len() - 4;
    if length == 0 {
        return Err(ProtocolError::Empty);
    }
    writer.frame[..4].copy_from_slice(&(length as u32).to_le_bytes());
    Ok(writer.frame)
}

pub fn decode<T: DeserializeOwned>(body: &[u8]) -> Result<T, ProtocolError> {
    if body.is_empty() {
        return Err(ProtocolError::Empty);
    }
    if body.len() > MAX_FRAME_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    serde_json::from_slice(body).map_err(|_| ProtocolError::Malformed)
}

/// Holds only the incomplete frame. After a framing error discard the connection.
#[derive(Default)]
pub struct FrameDecoder {
    header: [u8; 4],
    header_len: usize,
    expected: Option<usize>,
    body: Vec<u8>,
    failed: Option<ProtocolError>,
}
impl FrameDecoder {
    pub fn push(&mut self, mut bytes: &[u8]) -> Result<Vec<Vec<u8>>, ProtocolError> {
        if let Some(error) = self.failed {
            return Err(error);
        }
        let mut bodies = Vec::new();
        while !bytes.is_empty() {
            if self.header_len < 4 {
                let count = (4 - self.header_len).min(bytes.len());
                self.header[self.header_len..self.header_len + count]
                    .copy_from_slice(&bytes[..count]);
                self.header_len += count;
                bytes = &bytes[count..];
                if self.header_len < 4 {
                    break;
                }
                let length = u32::from_le_bytes(self.header) as usize;
                let error = if length == 0 {
                    Some(ProtocolError::Empty)
                } else if length > MAX_FRAME_BYTES {
                    Some(ProtocolError::TooLarge)
                } else {
                    None
                };
                if let Some(error) = error {
                    self.failed = Some(error);
                    return Err(error);
                }
                self.expected = Some(length);
            }
            // expected is established only by the validated complete header.
            let length = self.expected.ok_or(ProtocolError::Malformed)?;
            let count = (length - self.body.len()).min(bytes.len());
            self.body.extend_from_slice(&bytes[..count]);
            bytes = &bytes[count..];
            if self.body.len() == length {
                bodies.push(std::mem::take(&mut self.body));
                self.header_len = 0;
                self.expected = None;
            }
        }
        Ok(bodies)
    }
}

/// Shape/version check only; the caller must separately authenticate the OS peer.
pub fn check_hello(message: &ClientMessage, expected_product: &str) -> Result<(), ProtocolError> {
    match message {
        ClientMessage::Hello {
            protocol,
            product,
            session,
        } => {
            if *protocol != PROTOCOL_VERSION {
                return Err(ProtocolError::Mismatch);
            }
            if product != expected_product || session.is_empty() || session.len() > 64 {
                return Err(ProtocolError::Malformed);
            }
            Ok(())
        }
        _ => Err(ProtocolError::Malformed),
    }
}

#[derive(Default)]
pub struct RequestIds(HashSet<u64>);

impl RequestIds {
    pub fn insert(&mut self, id: u64) -> Result<(), ProtocolError> {
        if self.0.insert(id) {
            Ok(())
        } else {
            Err(ProtocolError::Duplicate)
        }
    }

    pub fn remove(&mut self, id: u64) {
        self.0.remove(&id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_survive_arbitrary_splits_and_batching() {
        let one = encode(&ClientMessage::Cancel { id: 7 }).unwrap();
        let two = encode(&ClientMessage::Ack {
            stream: 1,
            cursor: 9,
        })
        .unwrap();
        let mut joined = one.clone();
        joined.extend_from_slice(&two);
        for split in 1..joined.len() {
            let mut decoder = FrameDecoder::default();
            let mut bodies = decoder.push(&joined[..split]).unwrap();
            bodies.extend(decoder.push(&joined[split..]).unwrap());
            let messages: Vec<ClientMessage> =
                bodies.iter().map(|body| decode(body).unwrap()).collect();
            assert_eq!(
                messages,
                vec![
                    ClientMessage::Cancel { id: 7 },
                    ClientMessage::Ack {
                        stream: 1,
                        cursor: 9
                    }
                ],
                "split at {split}"
            );
        }
    }

    #[test]
    fn empty_oversized_and_malformed_frames_fail() {
        let mut decoder = FrameDecoder::default();
        assert_eq!(
            decoder.push(&0u32.to_le_bytes()).unwrap_err(),
            ProtocolError::Empty
        );
        let mut decoder = FrameDecoder::default();
        assert_eq!(
            decoder
                .push(&((MAX_FRAME_BYTES as u32) + 1).to_le_bytes())
                .unwrap_err(),
            ProtocolError::TooLarge
        );
        assert_eq!(
            decode::<ClientMessage>(b"not json").unwrap_err(),
            ProtocolError::Malformed
        );
        assert_eq!(
            decode::<ClientMessage>(br#"{"type":"cancel","id":1,"extra":true}"#).unwrap_err(),
            ProtocolError::Malformed
        );
    }

    #[test]
    fn hello_checks_version_and_product() {
        let hello = ClientMessage::Hello {
            protocol: PROTOCOL_VERSION,
            product: "workspace".into(),
            session: "s".into(),
        };
        assert!(check_hello(&hello, "workspace").is_ok());
        let old = ClientMessage::Hello {
            protocol: PROTOCOL_VERSION + 1,
            product: "workspace".into(),
            session: "s".into(),
        };
        assert_eq!(
            check_hello(&old, "workspace").unwrap_err(),
            ProtocolError::Mismatch
        );
        assert_eq!(
            check_hello(&ClientMessage::Cancel { id: 1 }, "workspace").unwrap_err(),
            ProtocolError::Malformed
        );
        assert_eq!(ProtocolError::Mismatch.code(), "protocol_mismatch");
    }

    #[test]
    fn request_ids_are_unique_per_connection() {
        let mut ids = RequestIds::default();
        ids.insert(1).unwrap();
        assert_eq!(ids.insert(1).unwrap_err(), ProtocolError::Duplicate);
        ids.remove(1);
        assert!(ids.insert(1).is_ok());
    }
    #[test]
    fn one_byte_chunks_and_exact_frame_limit_are_supported() {
        let frame = encode(&ClientMessage::Shutdown {}).unwrap();
        let mut decoder = FrameDecoder::default();
        let mut bodies = Vec::new();
        for byte in frame {
            bodies.extend(decoder.push(&[byte]).unwrap());
        }
        assert_eq!(
            decode::<ClientMessage>(&bodies[0]).unwrap(),
            ClientMessage::Shutdown {}
        );
        let payload = "a".repeat(MAX_FRAME_BYTES - 2);
        let frame = encode(&payload).unwrap();
        assert_eq!(frame.len(), MAX_FRAME_BYTES + 4);
        assert_eq!(decode::<String>(&frame[4..]).unwrap(), payload);
        assert_eq!(
            encode(&"a".repeat(MAX_FRAME_BYTES)).unwrap_err(),
            ProtocolError::TooLarge
        );
        assert_eq!(decode::<String>(&[]).unwrap_err(), ProtocolError::Empty);
        assert_eq!(
            decode::<String>(&vec![b'a'; MAX_FRAME_BYTES + 1]).unwrap_err(),
            ProtocolError::TooLarge
        );
    }
    #[test]
    fn invalid_frame_poisoning_prevents_resuming_the_connection() {
        let mut decoder = FrameDecoder::default();
        assert_eq!(
            decoder.push(&u32::MAX.to_le_bytes()).unwrap_err(),
            ProtocolError::TooLarge
        );
        assert_eq!(
            decoder
                .push(&encode(&ClientMessage::Cancel { id: 1 }).unwrap())
                .unwrap_err(),
            ProtocolError::TooLarge
        );
    }
    #[test]
    fn both_envelopes_are_strict_and_hello_does_not_authorize_a_foreign_product() {
        let hello = ClientMessage::Hello {
            protocol: PROTOCOL_VERSION,
            product: "knowledge".into(),
            session: "s".into(),
        };
        assert_eq!(
            check_hello(&hello, "workspace"),
            Err(ProtocolError::Malformed)
        );
        let empty = ClientMessage::Hello {
            protocol: PROTOCOL_VERSION,
            product: "workspace".into(),
            session: String::new(),
        };
        assert_eq!(
            check_hello(&empty, "workspace"),
            Err(ProtocolError::Malformed)
        );
        let long = ClientMessage::Hello {
            protocol: PROTOCOL_VERSION,
            product: "workspace".into(),
            session: "x".repeat(65),
        };
        assert_eq!(
            check_hello(&long, "workspace"),
            Err(ProtocolError::Malformed)
        );
        assert_eq!(decode::<AgentMessage>(br#"{"type":"welcome","protocol":1,"agentVersion":"0.8.1","generation":"g","extra":true}"#), Err(ProtocolError::Malformed));
        let call = ClientMessage::Call {
            id: 1,
            component: "workspace.runtime".into(),
            request: serde_json::json!({"header":{"requestId":"r1"},"method":"list","args":{}}),
        };
        let frame = encode(&call).unwrap();
        assert_eq!(decode::<ClientMessage>(&frame[4..]).unwrap(), call);
    }
}
