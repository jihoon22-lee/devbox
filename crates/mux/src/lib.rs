//! Windows 앱의 창과 데몬 채널을 잇는 라우팅. JSON 본문은 해석하지 않는다.
use devbox_protocol::frame::{FrameCodec, FrameKind, HEADER_LEN};
pub use devbox_protocol::frame::{frame_channel, set_frame_channel};
use devbox_protocol::message::{Control, control_frame};
use std::collections::HashMap;
use std::hash::Hash;
use std::time::Duration;
use tokio_util::codec::Encoder;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MuxError {
    #[error("window has no open channel")]
    UnknownWindow,
    #[error("frame is shorter than its header")]
    ShortFrame,
    #[error("malformed frame stream: {0}")]
    Malformed(String),
}

pub struct Mux<K> {
    by_key: HashMap<K, u32>,
    by_channel: HashMap<u32, K>,
    next: u32,
}

impl<K: Eq + Hash + Clone> Default for Mux<K> {
    fn default() -> Self {
        Self::new()
    }
}

fn control_bytes(channel: u32, c: &Control) -> Vec<u8> {
    let mut out = bytes::BytesMut::new();
    FrameCodec::default()
        .encode(control_frame(channel, c), &mut out)
        .expect("control frames are small");
    out.to_vec()
}

impl<K: Eq + Hash + Clone> Mux<K> {
    pub fn new() -> Self {
        Self {
            by_key: HashMap::new(),
            by_channel: HashMap::new(),
            next: 1,
        }
    }

    /// 창에 새 채널을 준다. 이미 열린 창이면(새로 고침) 옛 채널을 먼저 닫는다.
    pub fn open(&mut self, key: K) -> (u32, Vec<Vec<u8>>) {
        let mut frames = Vec::new();
        if let Some(old) = self.close(&key) {
            frames.push(old);
        }
        while self.by_channel.contains_key(&self.next) {
            self.next = self.next.wrapping_add(1).max(1);
        }
        let ch = self.next;
        self.next = self.next.wrapping_add(1).max(1);
        self.by_key.insert(key.clone(), ch);
        self.by_channel.insert(ch, key);
        frames.push(control_bytes(ch, &Control::ChannelOpen));
        (ch, frames)
    }

    pub fn close(&mut self, key: &K) -> Option<Vec<u8>> {
        let ch = self.by_key.remove(key)?;
        self.by_channel.remove(&ch);
        Some(control_bytes(ch, &Control::ChannelClose))
    }

    pub fn outbound(&self, key: &K, frame: &mut [u8]) -> Result<(), MuxError> {
        let ch = *self.by_key.get(key).ok_or(MuxError::UnknownWindow)?;
        if set_frame_channel(frame, ch) {
            Ok(())
        } else {
            Err(MuxError::ShortFrame)
        }
    }

    pub fn inbound(&self, frame: &[u8]) -> Option<&K> {
        self.by_channel.get(&frame_channel(frame)?)
    }

    /// 데몬 재연결 뒤 열린 채널을 모두 다시 연다.
    pub fn reopen_all(&self) -> Vec<Vec<u8>> {
        let mut chans: Vec<u32> = self.by_channel.keys().copied().collect();
        chans.sort_unstable();
        chans
            .into_iter()
            .map(|c| control_bytes(c, &Control::ChannelOpen))
            .collect()
    }
}

/// 바이트 흐름을 완성된 프레임 단위로 자른다(본문은 해석하지 않음).
#[derive(Default)]
pub struct FrameSplitter {
    buf: bytes::BytesMut,
    poisoned: bool,
}

impl FrameSplitter {
    fn fail(&mut self, reason: String) -> MuxError {
        self.poisoned = true;
        self.buf.clear();
        MuxError::Malformed(reason)
    }

    pub fn push(&mut self, data: &[u8]) -> Result<Vec<Vec<u8>>, MuxError> {
        if self.poisoned {
            return Err(MuxError::Malformed(
                "splitter stopped after an earlier malformed frame".into(),
            ));
        }
        self.buf.extend_from_slice(data);
        let mut out = Vec::new();
        loop {
            if self.buf.len() < 4 {
                return Ok(out);
            }
            let len =
                u32::from_le_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]) as usize;
            if len < HEADER_LEN - 4 {
                return Err(self.fail(format!("length {len}")));
            }
            if self.buf.len() < 5 {
                return Ok(out);
            }
            let Some(kind) = FrameKind::from_u8(self.buf[4]) else {
                return Err(self.fail(format!("kind {}", self.buf[4])));
            };
            if len - 5 > kind.max_body() {
                return Err(self.fail(format!("length {len}")));
            }
            if self.buf.len() < 4 + len {
                return Ok(out);
            }
            out.push(self.buf.split_to(4 + len).to_vec());
        }
    }
}

#[derive(Default)]
pub struct Backoff {
    idx: usize,
}

impl Backoff {
    pub const DELAYS_MS: [u64; 5] = [200, 500, 1000, 2000, 5000];
    pub fn next(&mut self) -> Duration {
        let d = Self::DELAYS_MS[self.idx.min(Self::DELAYS_MS.len() - 1)];
        self.idx = self.idx.saturating_add(1);
        Duration::from_millis(d)
    }
    pub fn reset(&mut self) {
        self.idx = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;
    use devbox_protocol::frame::{Frame, FrameCodec, FrameKind};
    use devbox_protocol::message::{Control, parse_json};
    use tokio_util::codec::{Decoder, Encoder};

    fn raw(kind: FrameKind, channel: u32, body: &[u8]) -> Vec<u8> {
        let mut out = BytesMut::new();
        FrameCodec::default()
            .encode(
                Frame {
                    kind,
                    channel,
                    body: bytes::Bytes::copy_from_slice(body),
                },
                &mut out,
            )
            .unwrap();
        out.to_vec()
    }
    fn decode(raw: &[u8]) -> Frame {
        FrameCodec::default()
            .decode(&mut BytesMut::from(raw))
            .unwrap()
            .unwrap()
    }

    #[test]
    fn windows_get_distinct_channels_and_open_frames() {
        let mut m = Mux::new();
        let (a, fa) = m.open("main");
        let (b, _) = m.open("popout");
        assert_ne!(a, b);
        assert_eq!(fa.len(), 1);
        let f = decode(&fa[0]);
        assert_eq!((f.kind, f.channel), (FrameKind::Control, a));
        assert_eq!(parse_json::<Control>(&f).unwrap(), Control::ChannelOpen);
    }

    #[test]
    fn reopening_a_window_closes_its_old_channel_first() {
        let mut m = Mux::new();
        let (a, _) = m.open("main");
        let (a2, frames) = m.open("main");
        assert_ne!(a, a2);
        assert_eq!(frames.len(), 2);
        assert_eq!(
            parse_json::<Control>(&decode(&frames[0])).unwrap(),
            Control::ChannelClose
        );
        assert_eq!(decode(&frames[0]).channel, a);
    }

    #[test]
    fn rewrites_outbound_and_routes_inbound_by_header_only() {
        let mut m = Mux::new();
        let (a, _) = m.open("main");
        let mut out = raw(FrameKind::Json, 0, b"{\"not\":\"parsed\"");
        m.outbound(&"main", &mut out).unwrap();
        assert_eq!(frame_channel(&out), Some(a));
        assert_eq!(m.inbound(&out), Some(&"main"));
        assert!(
            m.outbound(&"ghost", &mut raw(FrameKind::Json, 0, b"{}"))
                .is_err()
        );
    }

    #[test]
    fn close_returns_a_close_frame_and_forgets_routing() {
        let mut m = Mux::new();
        let (a, _) = m.open("main");
        let f = m.close(&"main").unwrap();
        assert_eq!(decode(&f).channel, a);
        assert_eq!(m.inbound(&raw(FrameKind::Json, a, b"{}")), None);
        assert!(m.close(&"main").is_none());
    }

    #[test]
    fn reopen_all_lists_every_open_channel() {
        let mut m = Mux::new();
        m.open("a");
        m.open("b");
        assert_eq!(m.reopen_all().len(), 2);
    }

    #[test]
    fn splitter_reassembles_frames_across_reads() {
        let mut s = FrameSplitter::default();
        let mut stream = raw(FrameKind::Json, 1, b"{}");
        stream.extend(raw(FrameKind::Stream, 2, &[0, 0, 0, 1, 0, 9, 9]));
        let (left, right) = stream.split_at(7);
        assert!(s.push(left).unwrap().is_empty());
        let frames = s.push(right).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frame_channel(&frames[1]), Some(2));
        assert!(s.push(&[0, 0, 0, 0]).is_err());
    }

    #[test]
    fn backoff_follows_the_design_schedule() {
        let mut b = Backoff::default();
        let ms: Vec<u128> = (0..7).map(|_| b.next().as_millis()).collect();
        assert_eq!(ms, vec![200, 500, 1000, 2000, 5000, 5000, 5000]);
        b.reset();
        assert_eq!(b.next().as_millis(), 200);
    }

    #[test]
    fn wrapping_channel_ids_skip_active_channels_and_zero() {
        let mut m = Mux::new();
        let (first, _) = m.open("first");
        m.next = u32::MAX;
        assert_eq!(m.open("last").0, u32::MAX);
        let (wrapped, _) = m.open("wrapped");
        assert_eq!((first, wrapped), (1, 2));
        assert_eq!(
            m.inbound(&raw(FrameKind::Json, first, b"{}")),
            Some(&"first")
        );
    }

    #[test]
    fn routing_preserves_payload_and_short_frames() {
        let mut m = Mux::new();
        m.open("main");
        for kind in [FrameKind::Json, FrameKind::Stream, FrameKind::Control] {
            let mut frame = raw(kind, 0, &[0, 255, 3, 7]);
            let original = frame.clone();
            m.outbound(&"main", &mut frame).unwrap();
            assert_eq!(&frame[..5], &original[..5]);
            assert_eq!(&frame[9..], &original[9..]);
        }
        for len in 0..HEADER_LEN {
            let mut short = vec![255; len];
            assert_eq!(m.outbound(&"main", &mut short), Err(MuxError::ShortFrame));
            assert_eq!(short, vec![255; len]);
            assert_eq!(m.inbound(&short), None);
        }
    }

    #[test]
    fn splitter_rejects_bad_headers_and_stays_poisoned() {
        let mut bad: Vec<Vec<u8>> = (0u32..5).map(|n| n.to_le_bytes().to_vec()).collect();
        bad.push(vec![5, 0, 0, 0, 255]);
        for kind in [FrameKind::Json, FrameKind::Stream, FrameKind::Control] {
            let mut oversized = ((5 + kind.max_body() + 1) as u32).to_le_bytes().to_vec();
            oversized.push(kind as u8);
            bad.push(oversized);
        }
        for header in bad {
            let mut s = FrameSplitter::default();
            assert!(matches!(s.push(&header), Err(MuxError::Malformed(_))));
            assert!(matches!(
                s.push(&raw(FrameKind::Json, 1, b"{}")),
                Err(MuxError::Malformed(_))
            ));
            assert!(s.push(&[]).is_err());
        }
    }

    #[test]
    fn splitter_preserves_bytes_at_every_boundary_and_kind_limit() {
        for kind in [FrameKind::Json, FrameKind::Stream, FrameKind::Control] {
            let frame = raw(kind, 7, &[255, 0, 1]);
            for boundary in 0..=frame.len() {
                let mut s = FrameSplitter::default();
                let mut frames = s.push(&frame[..boundary]).unwrap();
                frames.extend(s.push(&frame[boundary..]).unwrap());
                assert_eq!(frames, vec![frame.clone()]);
            }
            let frame = raw(kind, 8, &vec![0; kind.max_body()]);
            assert_eq!(FrameSplitter::default().push(&frame).unwrap(), vec![frame]);
        }
    }

    #[test]
    fn backoff_counter_saturates() {
        let mut b = Backoff { idx: usize::MAX };
        assert_eq!(b.next().as_millis(), 5000);
        assert_eq!(b.next().as_millis(), 5000);
        b.reset();
        assert_eq!(b.next().as_millis(), 200);
    }
}
