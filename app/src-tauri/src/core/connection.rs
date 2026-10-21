#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn external_distro_requires_ready_current_bridge_and_clears_on_close() {
        let mut c = State::<()> {
            active_distro: Some("Debian".into()),
            ..Default::default()
        };
        assert!(c.ready_distro().is_err());
        c.open = true;
        assert!(c.ready_distro().is_err());
        c.gate.state = "ready";
        assert_eq!(c.ready_distro().unwrap(), "Debian");
        c.generation = 2;
        assert!(!c.close_generation(1));
        assert_eq!(c.ready_distro().unwrap(), "Debian");
        c.cancel.cancel();
        assert!(c.ready_distro().is_err());
        assert!(c.close_generation(2));
        assert!(c.active_distro.is_none());
        assert!(c.ready_distro().is_err());
    }
    #[test]
    fn stale_lease_cannot_detach_or_send_and_new_bridge_survives_old_cleanup() {
        let mut c = State::<()>::default();
        c.attach("main".into(), "old-lease".into(), ()).unwrap();
        c.attach("main".into(), "new-lease".into(), ()).unwrap();
        assert!(!c.detach("main", Some("old-lease")));
        assert!(
            c.send("main", "old-lease", vec![5, 0, 0, 0, 1, 0, 0, 0, 0])
                .is_err()
        );
        assert_eq!(c.channels.get("main").unwrap().channel, ());
        assert_eq!(c.gate.state, "checking");
        c.generation = 2;
        c.open = true;
        assert!(!c.close_generation(1));
        assert!(c.open);
        assert!(c.close_generation(2));
        assert!(!c.open);
    }
    #[test]
    fn full_writer_queue_closes_transport_and_rejects_send() {
        let mut c = State::<()>::default();
        c.attach("main".into(), "lease".into(), ()).unwrap();
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        tx.try_send(vec![]).unwrap();
        c.writer = Some(tx);
        c.open = true;
        assert!(
            c.send("main", "lease", vec![5, 0, 0, 0, 1, 0, 0, 0, 0])
                .is_err()
        );
        assert!(!c.open);
        assert!(c.writer.is_none());
    }
    #[test]
    fn reconnect_reopens_more_windows_than_writer_slots_before_renderer_frames() {
        let mut state = State::<()>::default();
        for number in 0..17 {
            state
                .attach(format!("window-{number}"), format!("lease-{number}"), ())
                .unwrap();
        }
        let expected = state.mux.reopen_all();
        let (writer, mut writes) = mpsc::channel(16);
        state.writer = Some(writer);
        state.open = true;
        state
            .enqueue_reopens()
            .expect("all existing windows reopen");
        assert!(state.open);
        assert!(!state.cancel.is_cancelled());
        let mut splitter = FrameSplitter::default();
        let mut frames = Vec::new();
        while let Ok(bytes) = writes.try_recv() {
            frames.extend(splitter.push(&bytes).unwrap());
        }
        assert_eq!(frames, expected);
        assert_eq!(frames.len(), 17);
        state
            .send("window-0", "lease-0", vec![5, 0, 0, 0, 1, 0, 0, 0, 0])
            .unwrap();
        let renderer = writes.try_recv().unwrap();
        assert_eq!(splitter.push(&renderer).unwrap().len(), 1);
    }
    #[test]
    fn send_rejects_multiple_or_incomplete_frames() {
        let mut c = State::<()>::default();
        c.attach("main".into(), "lease".into(), ()).unwrap();
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        c.writer = Some(tx);
        c.open = true;
        assert!(
            c.send("main", "lease", vec![5, 0, 0, 0, 1, 0, 0, 0])
                .is_err()
        );
        assert!(
            c.send("main", "lease", vec![5, 0, 0, 0, 255, 0, 0, 0, 0])
                .is_err()
        );
    }
}

use devbox_mux::{FrameSplitter, Mux};
use serde::Serialize;
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
pub const INSTANCE: &str = super::channel::CHANNEL.config().instance;
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GateStatus {
    pub state: &'static str,
    pub detail: Option<String>,
    pub distros: Option<Vec<super::wsl::Distro>>,
    pub instance: &'static str,
    pub revision: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    pub attachment_id: String,
    pub state: &'static str,
    pub revision: String,
}
#[derive(Debug, Clone, Serialize, thiserror::Error)]
#[error("{message}")]
pub struct IpcError {
    pub message: String,
}
impl IpcError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}
pub struct Lease<C> {
    pub id: String,
    pub channel: C,
}
pub struct State<C> {
    pub gate: GateStatus,
    pub revision: u64,
    pub generation: u64,
    pub open: bool,
    /// Assigned only after this generation's validated distro reaches bridge Ready.
    pub active_distro: Option<String>,
    pub mux: Mux<String>,
    pub channels: HashMap<String, Lease<C>>,
    pub writer: Option<mpsc::Sender<Vec<u8>>>,
    pub cancel: CancellationToken,
}
impl<C> Default for State<C> {
    fn default() -> Self {
        Self {
            gate: GateStatus {
                state: "checking",
                detail: None,
                distros: None,
                instance: INSTANCE,
                revision: "0".into(),
            },
            revision: 0,
            generation: 0,
            open: false,
            active_distro: None,
            mux: Mux::new(),
            channels: HashMap::new(),
            writer: None,
            cancel: CancellationToken::new(),
        }
    }
}
impl<C> State<C> {
    pub fn ready_distro(&self) -> Result<&str, IpcError> {
        if !self.open || self.gate.state != "ready" || self.cancel.is_cancelled() {
            return Err(IpcError::new("연결이 준비되지 않았습니다"));
        }
        self.active_distro
            .as_deref()
            .ok_or_else(|| IpcError::new("연결된 배포판이 없습니다"))
    }
    pub fn bump(&mut self) {
        self.revision = self.revision.checked_add(1).expect("revision exhausted");
    }
    pub fn notice(&self, id: &str) -> Notice {
        Notice {
            attachment_id: id.into(),
            state: if self.open { "open" } else { "closed" },
            revision: self.revision.to_string(),
        }
    }
    pub fn attach(&mut self, label: String, id: String, channel: C) -> Result<Notice, IpcError> {
        if id.is_empty()
            || id.len() > 96
            || !id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
        {
            return Err(IpcError::new("잘못된 연결 ID"));
        }
        let (_, frames) = self.mux.open(label.clone());
        self.channels.insert(
            label,
            Lease {
                id: id.clone(),
                channel,
            },
        );
        self.bump();
        if self.open {
            for f in frames {
                self.enqueue(f)?;
            }
        }
        Ok(self.notice(&id))
    }
    pub fn detach(&mut self, label: &str, id: Option<&str>) -> bool {
        if !self
            .channels
            .get(label)
            .is_some_and(|l| id.is_none_or(|id| id == l.id))
        {
            return false;
        }
        self.channels.remove(label);
        if let Some(f) = self.mux.close(&label.to_owned())
            && self.open
        {
            let _ = self.enqueue(f);
        }
        self.bump();
        true
    }
    pub fn send(&mut self, label: &str, id: &str, mut frame: Vec<u8>) -> Result<(), IpcError> {
        if !self.channels.get(label).is_some_and(|l| l.id == id) {
            return Err(IpcError::new("만료된 화면 연결"));
        }
        let mut splitter = FrameSplitter::default();
        let frames = splitter
            .push(&frame)
            .map_err(|e| IpcError::new(e.to_string()))?;
        if frames.len() != 1 || frames[0].len() != frame.len() {
            return Err(IpcError::new("완전한 프레임 하나가 필요합니다"));
        }
        self.mux
            .outbound(&label.to_owned(), &mut frame)
            .map_err(|e| IpcError::new(e.to_string()))?;
        self.enqueue(frame)
    }
    pub fn enqueue_reopens(&mut self) -> Result<(), IpcError> {
        // The bridge writes bytes, so existing small control frames can share
        // one queue slot without changing their wire order or framing.
        let bytes = self.mux.reopen_all().concat();
        if bytes.is_empty() {
            return Ok(());
        }
        self.enqueue(bytes)
    }
    pub fn enqueue(&mut self, frame: Vec<u8>) -> Result<(), IpcError> {
        if self.open
            && self
                .writer
                .as_ref()
                .is_some_and(|tx| tx.try_send(frame).is_ok())
        {
            return Ok(());
        }
        self.open = false;
        self.active_distro = None;
        self.writer = None;
        self.cancel.cancel();
        self.bump();
        Err(IpcError::new(
            "브리지 연결이 닫혔거나 전송 큐가 가득 찼습니다",
        ))
    }
    pub fn close_generation(&mut self, generation: u64) -> bool {
        if generation != self.generation {
            return false;
        }
        self.writer = None;
        self.open = false;
        self.active_distro = None;
        self.bump();
        true
    }
}
