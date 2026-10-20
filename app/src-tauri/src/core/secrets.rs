//! Native-only key negotiation. Renderer channels never enter this state machine.
use devbox_protocol::{
    Done, PROTOCOL_VERSION,
    frame::{FrameKind, HEADER_LEN, frame_channel},
    message::{ClientKind, ClientMessage, ServerMessage},
    secrets::{Bootstrap, KeyStatus, NATIVE_CHANNEL, Unlock},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

#[derive(Debug)]
pub struct Locked;

pub trait KeyProtector {
    fn generate(&self) -> Result<Zeroizing<Vec<u8>>, Locked>;
    fn seal(&self, key: &[u8]) -> Result<Vec<u8>, Locked>;
    fn unseal(&self, blob: &[u8]) -> Result<Zeroizing<Vec<u8>>, Locked>;
}

#[derive(Default)]
enum Phase {
    #[default]
    Finished,
    Welcome,
    Status,
    Bootstrap(Zeroizing<Vec<u8>>),
    Unlock,
}

#[derive(Default)]
pub struct NativeSecrets {
    phase: Phase,
}

pub fn is_native(frame: &[u8]) -> bool {
    frame_channel(frame) == Some(NATIVE_CHANNEL)
}

fn fingerprint(key: &[u8]) -> String {
    format!("{:x}", Sha256::digest(key))
}

// Direct borrowed serialization avoids storing plaintext in a RawValue/String
// or in a Debug-derived ClientMessage. The transport wipes the final byte buffer.
#[derive(Serialize)]
struct Request<'a, P> {
    #[serde(rename = "type")]
    kind: &'static str,
    id: u32,
    method: &'static str,
    params: &'a P,
}
fn wire(message: &impl Serialize) -> Vec<u8> {
    // Enough for a 64 KiB DPAPI blob encoded as a JSON byte array, preventing
    // reallocations while writing key-bearing requests into this buffer.
    let mut frame = Vec::with_capacity(512 * 1024);
    frame.resize(HEADER_LEN, 0);
    serde_json::to_writer(&mut frame, message).expect("native DTO serialization");
    let length = (frame.len() - 4) as u32;
    frame[..4].copy_from_slice(&length.to_le_bytes());
    frame[4] = FrameKind::Json as u8;
    frame[5..9].copy_from_slice(&NATIVE_CHANNEL.to_le_bytes());
    frame
}
fn request(id: u32, method: &'static str, params: &impl Serialize) -> Vec<u8> {
    wire(&Request {
        kind: "request",
        id,
        method,
        params,
    })
}

impl NativeSecrets {
    pub fn start(&mut self, instance: &str) -> Vec<u8> {
        self.phase = Phase::Welcome;
        wire(&ClientMessage::Hello {
            version: env!("CARGO_PKG_VERSION").into(),
            protocol: PROTOCOL_VERSION,
            client: ClientKind::App,
            instance: instance.into(),
        })
    }

    pub fn abort(&mut self) {
        self.phase = Phase::Finished;
    }

    #[cfg(test)]
    fn finished(&self) -> bool {
        matches!(self.phase, Phase::Finished)
    }

    /// A native frame is always consumed by the supervisor, including unknown
    /// responses and failures. This method never produces a renderer message.
    pub fn receive(&mut self, frame: &[u8], provider: &impl KeyProtector) -> Option<Vec<u8>> {
        if !is_native(frame) || matches!(self.phase, Phase::Finished) {
            return None;
        }
        if frame[4] != FrameKind::Json as u8
            || u32::from_le_bytes(frame[..4].try_into().ok()?) as usize != frame.len() - 4
            || frame.len() > 512 * 1024
        {
            self.abort();
            return None;
        }
        let Ok(message) = serde_json::from_slice::<ServerMessage>(&frame[HEADER_LEN..]) else {
            self.abort();
            return None;
        };
        if matches!(self.phase, Phase::Welcome) {
            self.abort();
            if let ServerMessage::Welcome {
                version, protocol, ..
            } = message
                && version == env!("CARGO_PKG_VERSION")
                && protocol == PROTOCOL_VERSION
            {
                self.phase = Phase::Status;
                #[derive(Serialize)]
                struct Empty {}
                return Some(request(1, "secrets.key_status", &Empty {}));
            }
            return None;
        }
        let ServerMessage::Response {
            id, result, error, ..
        } = message
        else {
            return None;
        };
        let expected = match &self.phase {
            Phase::Status => 1,
            Phase::Bootstrap(_) => 2,
            Phase::Unlock => 3,
            _ => return None,
        };
        if id != expected {
            return None;
        }
        let phase = std::mem::take(&mut self.phase);
        if error.is_some() {
            return None;
        }
        let result = result?;
        match phase {
            Phase::Status => {
                let status: KeyStatus = serde_json::from_str(result.get()).ok()?;
                if !status.locked {
                    return None;
                }
                match (status.key_id, status.blob) {
                    (Some(id), Some(blob)) if !blob.is_empty() && blob.len() <= 64 * 1024 => {
                        let key = provider.unseal(&blob).ok()?;
                        if key.len() != 32 || fingerprint(&key) != id {
                            return None;
                        }
                        Some(self.unlock(key))
                    }
                    (None, None) => {
                        let key = provider.generate().ok()?;
                        if key.len() != 32 {
                            return None;
                        }
                        let blob = provider.seal(&key).ok()?;
                        if blob.is_empty() || blob.len() > 64 * 1024 {
                            return None;
                        }
                        let bootstrap = Bootstrap {
                            blob,
                            key_id: fingerprint(&key),
                        };
                        self.phase = Phase::Bootstrap(key);
                        Some(request(2, "secrets.bootstrap", &bootstrap))
                    }
                    _ => None,
                }
            }
            Phase::Bootstrap(key) => {
                let _: Done = serde_json::from_str(result.get()).ok()?;
                Some(self.unlock(key))
            }
            Phase::Unlock => None,
            _ => None,
        }
    }

    fn unlock(&mut self, key: Zeroizing<Vec<u8>>) -> Vec<u8> {
        self.phase = Phase::Unlock;
        // Both the owned key and DTO copy wipe themselves after serialization.
        request(3, "secrets.unlock", &Unlock { key: key.to_vec() })
    }
}

#[cfg(test)]
#[path = "secrets_tests.rs"]
mod tests;
