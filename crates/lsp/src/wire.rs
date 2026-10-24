use crate::{LspError, RelayMessage, actor::Command};
use devbox_protocol::stream::{StreamEnds, StreamTx};
use std::{sync::Arc, time::Duration};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc};
use tokio_util::task::TaskTracker;
const MAX_PACKET: usize = 4 * 1024 * 1024;
const CREDIT_WAIT: Duration = Duration::from_millis(250);
#[derive(Default)]
struct Decoder {
    buffer: Vec<u8>,
}
impl Decoder {
    fn push(&mut self, bytes: &[u8]) -> Result<Vec<RelayMessage>, LspError> {
        let mut messages = Vec::new();
        for chunk in bytes.split_inclusive(|b| *b == b'\n') {
            if self.buffer.len() + chunk.len() > MAX_PACKET {
                return Err(LspError::LimitExceeded);
            }
            self.buffer.extend_from_slice(chunk);
            if self.buffer.last() == Some(&b'\n') {
                let packet: RelayMessage =
                    serde_json::from_slice(&self.buffer).map_err(|_| LspError::Protocol)?;
                if packet.json.len() > crate::framing::MAX_BODY
                    || packet.lease_id.as_ref().is_some_and(|s| s.len() > 128)
                {
                    return Err(LspError::LimitExceeded);
                }
                messages.push(packet);
                self.buffer.clear();
            }
        }
        Ok(messages)
    }
}
pub(crate) struct Client {
    pub tx: StreamTx,
    output: mpsc::Sender<(Vec<u8>, OwnedSemaphorePermit)>,
    budget: Arc<Semaphore>,
}
impl Client {
    pub fn start(ends: StreamEnds, commands: mpsc::Sender<Command>, tasks: &TaskTracker) -> Self {
        let id = ends.id;
        let tx = ends.tx;
        let input_tx = tx.clone();
        let mut input = ends.input;
        let input_commands = commands.clone();
        tasks.spawn(async move {
            let mut decoder = Decoder::default();
            let closed = input_tx.closed();
            'read: loop {
                let bytes = tokio::select! {_=closed.cancelled()=>break,bytes=input.recv()=>bytes};
                let Some(bytes) = bytes else {
                    break;
                };
                let Ok(messages) = decoder.push(&bytes) else {
                    break;
                };
                for message in messages {
                    if input_commands
                        .try_send(Command::Message { id, message })
                        .is_err()
                    {
                        break 'read;
                    }
                }
            }
            let _ = tokio::time::timeout(CREDIT_WAIT, input_tx.finish()).await;
            let _ = input_commands.try_send(Command::Detach { id });
        });
        let (output, mut receive) = mpsc::channel::<(Vec<u8>, OwnedSemaphorePermit)>(4);
        let output_tx = tx.clone();
        tasks.spawn(async move {
            let closed = output_tx.closed();
            loop {
                let packet = tokio::select! {_=closed.cancelled()=>break,p=receive.recv()=>p};
                let Some((bytes, _permit)) = packet else {
                    break;
                };
                if !matches!(
                    tokio::time::timeout(CREDIT_WAIT, output_tx.write(&bytes)).await,
                    Ok(Ok(()))
                ) {
                    break;
                }
            }
            let _ = tokio::time::timeout(CREDIT_WAIT, output_tx.finish()).await;
            let _ = commands.try_send(Command::Detach { id });
        });
        Self {
            tx,
            output,
            budget: Arc::new(Semaphore::new(MAX_PACKET)),
        }
    }
    pub fn send(&self, json: String) -> Result<(), LspError> {
        let mut bytes = serde_json::to_vec(&RelayMessage {
            json,
            lease_id: None,
        })
        .map_err(|_| LspError::Protocol)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_PACKET {
            return Err(LspError::LimitExceeded);
        }
        let permit = self
            .budget
            .clone()
            .try_acquire_many_owned(bytes.len() as u32)
            .map_err(|_| LspError::LimitExceeded)?;
        self.output
            .try_send((bytes, permit))
            .map_err(|_| LspError::LimitExceeded)
    }
    pub fn close(self, tasks: &TaskTracker) {
        tasks.spawn(async move {
            let _ = tokio::time::timeout(CREDIT_WAIT, self.tx.finish()).await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packets_handle_split_multibyte_and_coalesced_json_strings() {
        let packet = RelayMessage {
            json: r#"{"jsonrpc":"2.0","method":"test","params":"가😀\n"}"#.into(),
            lease_id: Some("L1".into()),
        };
        let line = format!("{}\n", serde_json::to_string(&packet).expect("packet"));
        let bytes = [line.as_bytes(), line.as_bytes()].concat();
        for chunk in [1, 3, bytes.len()] {
            let mut decoder = Decoder::default();
            let mut out = Vec::new();
            for part in bytes.chunks(chunk) {
                out.extend(decoder.push(part).expect("packet"));
            }
            assert_eq!(out.len(), 2);
            assert_eq!(out[0].json, packet.json);
        }
        assert!(Decoder::default().push(b"not json\n").is_err());
        assert!(
            Decoder::default()
                .push(&vec![b'x'; MAX_PACKET + 1])
                .is_err()
        );
    }
}
