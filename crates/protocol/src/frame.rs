//! 길이 접두 프레임: u32 LE 길이(뒤따르는 바이트 수) + u8 종류 + u32 LE 채널 + 본문.
use bytes::{Buf, BufMut, Bytes, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

pub const HEADER_LEN: usize = 9;
pub const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;
pub const SOFT_JSON_BYTES: usize = 1024 * 1024;
pub const MAX_STREAM_CHUNK: usize = 64 * 1024;
pub const MAX_CONTROL_BYTES: usize = 4 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameKind {
    Json = 0,
    Stream = 1,
    Control = 2,
}

impl FrameKind {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Json),
            1 => Some(Self::Stream),
            2 => Some(Self::Control),
            _ => None,
        }
    }
    /// 본문 상한. 스트림 본문은 스트림 ID(4) + 플래그(1) + 데이터.
    pub fn max_body(self) -> usize {
        match self {
            Self::Json => MAX_JSON_BYTES,
            Self::Stream => 5 + MAX_STREAM_CHUNK,
            Self::Control => MAX_CONTROL_BYTES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub kind: FrameKind,
    pub channel: u32,
    pub body: Bytes,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FrameError {
    #[error("frame length is zero")]
    Empty,
    #[error("frame length {0} is shorter than its header")]
    Short(usize),
    #[error("unknown frame kind {0}")]
    UnknownKind(u8),
    #[error("{kind:?} frame body is {len} bytes, limit {limit}")]
    TooLarge {
        kind: FrameKind,
        len: usize,
        limit: usize,
    },
    #[error("decoder stopped after an earlier malformed frame")]
    Poisoned,
}

#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error(transparent)]
    Frame(#[from] FrameError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Default)]
pub struct FrameCodec {
    poisoned: bool,
}

impl FrameCodec {
    fn fail(&mut self, e: FrameError) -> CodecError {
        self.poisoned = true;
        e.into()
    }
}

impl Decoder for FrameCodec {
    type Item = Frame;
    type Error = CodecError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Frame>, CodecError> {
        if self.poisoned {
            return Err(FrameError::Poisoned.into());
        }
        if src.len() < 4 {
            return Ok(None);
        }
        let len = u32::from_le_bytes([src[0], src[1], src[2], src[3]]) as usize;
        if len == 0 {
            return Err(self.fail(FrameError::Empty));
        }
        if len < 5 {
            return Err(self.fail(FrameError::Short(len)));
        }
        if src.len() < 5 {
            return Ok(None);
        }
        let Some(kind) = FrameKind::from_u8(src[4]) else {
            return Err(self.fail(FrameError::UnknownKind(src[4])));
        };
        let body_len = len - 5;
        if body_len > kind.max_body() {
            return Err(self.fail(FrameError::TooLarge {
                kind,
                len: body_len,
                limit: kind.max_body(),
            }));
        }
        if src.len() < 4 + len {
            src.reserve(4 + len - src.len());
            return Ok(None);
        }
        src.advance(5);
        let channel = src.get_u32_le();
        let body = src.split_to(body_len).freeze();
        Ok(Some(Frame {
            kind,
            channel,
            body,
        }))
    }
}

impl Encoder<Frame> for FrameCodec {
    type Error = CodecError;

    fn encode(&mut self, f: Frame, dst: &mut BytesMut) -> Result<(), CodecError> {
        if f.body.len() > f.kind.max_body() {
            return Err(FrameError::TooLarge {
                kind: f.kind,
                len: f.body.len(),
                limit: f.kind.max_body(),
            }
            .into());
        }
        dst.reserve(HEADER_LEN + f.body.len());
        dst.put_u32_le((5 + f.body.len()) as u32);
        dst.put_u8(f.kind as u8);
        dst.put_u32_le(f.channel);
        dst.put_slice(&f.body);
        Ok(())
    }
}

/// 완성된 프레임 바이트에서 채널 번호를 읽는다(본문은 해석하지 않음).
pub fn frame_channel(raw: &[u8]) -> Option<u32> {
    (raw.len() >= HEADER_LEN).then(|| u32::from_le_bytes([raw[5], raw[6], raw[7], raw[8]]))
}

/// 완성된 프레임 바이트의 채널 번호를 바꾼다.
pub fn set_frame_channel(raw: &mut [u8], channel: u32) -> bool {
    if raw.len() < HEADER_LEN {
        return false;
    }
    raw[5..9].copy_from_slice(&channel.to_le_bytes());
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;
    use tokio_util::codec::{Decoder, Encoder};

    fn json(channel: u32, text: &str) -> Frame {
        Frame {
            kind: FrameKind::Json,
            channel,
            body: Bytes::copy_from_slice(text.as_bytes()),
        }
    }
    fn encode(frames: &[Frame]) -> BytesMut {
        let mut codec = FrameCodec::default();
        let mut out = BytesMut::new();
        for f in frames {
            codec.encode(f.clone(), &mut out).expect("encode");
        }
        out
    }

    #[test]
    fn round_trips_several_frames_fed_one_byte_at_a_time() {
        let frames = vec![
            json(0, "{\"a\":1}"),
            json(7, "{}"),
            Frame {
                kind: FrameKind::Stream,
                channel: 3,
                body: Bytes::from_static(&[0, 1, 2, 255]),
            },
        ];
        let bytes = encode(&frames);
        let mut codec = FrameCodec::default();
        let mut buf = BytesMut::new();
        let mut got = Vec::new();
        for b in bytes.iter() {
            buf.extend_from_slice(&[*b]);
            while let Some(f) = codec.decode(&mut buf).expect("decode") {
                got.push(f);
            }
        }
        assert_eq!(got, frames);
    }

    #[test]
    fn decodes_two_frames_from_one_buffer() {
        let mut buf = encode(&[json(1, "1"), json(2, "2")]);
        let mut codec = FrameCodec::default();
        assert_eq!(codec.decode(&mut buf).unwrap().unwrap().channel, 1);
        assert_eq!(codec.decode(&mut buf).unwrap().unwrap().channel, 2);
        assert!(codec.decode(&mut buf).unwrap().is_none());
    }

    #[test]
    fn zero_length_poisons_the_decoder() {
        let mut buf = BytesMut::from(&[0u8, 0, 0, 0][..]);
        buf.extend_from_slice(&encode(&[json(0, "{}")]));
        let mut codec = FrameCodec::default();
        assert!(matches!(
            codec.decode(&mut buf),
            Err(CodecError::Frame(FrameError::Empty))
        ));
        assert!(matches!(
            codec.decode(&mut buf),
            Err(CodecError::Frame(FrameError::Poisoned))
        ));
    }

    #[test]
    fn rejects_unknown_kind_and_oversized_chunks() {
        let mut bad = BytesMut::new();
        bad.extend_from_slice(&5u32.to_le_bytes());
        bad.extend_from_slice(&[9, 0, 0, 0, 0]);
        assert!(matches!(
            FrameCodec::default().decode(&mut bad),
            Err(CodecError::Frame(FrameError::UnknownKind(9)))
        ));

        let mut big = BytesMut::new();
        let len = (5 + 5 + MAX_STREAM_CHUNK + 1) as u32;
        big.extend_from_slice(&len.to_le_bytes());
        big.extend_from_slice(&[FrameKind::Stream as u8, 0, 0, 0, 0]);
        assert!(matches!(
            FrameCodec::default().decode(&mut big),
            Err(CodecError::Frame(FrameError::TooLarge { .. }))
        ));
    }

    #[test]
    fn encoder_refuses_oversized_bodies() {
        let mut out = BytesMut::new();
        let f = Frame {
            kind: FrameKind::Control,
            channel: 0,
            body: Bytes::from(vec![b' '; MAX_CONTROL_BYTES + 1]),
        };
        assert!(FrameCodec::default().encode(f, &mut out).is_err());
    }

    #[test]
    fn reads_and_rewrites_the_channel_in_raw_bytes() {
        let mut raw = encode(&[json(4, "{}")]).to_vec();
        assert_eq!(frame_channel(&raw), Some(4));
        assert!(set_frame_channel(&mut raw, 9));
        assert_eq!(frame_channel(&raw), Some(9));
        assert_eq!(frame_channel(&raw[..5]), None);
    }

    #[test]
    fn rejects_short_lengths_and_keeps_decoder_poisoned() {
        for length in 1u32..5 {
            let mut buf = BytesMut::from(length.to_le_bytes().as_slice());
            buf.extend_from_slice(&encode(&[json(0, "{}")]));
            let mut codec = FrameCodec::default();
            assert!(
                matches!(codec.decode(&mut buf), Err(CodecError::Frame(FrameError::Short(n))) if n == length as usize)
            );
            assert!(matches!(
                codec.decode(&mut buf),
                Err(CodecError::Frame(FrameError::Poisoned))
            ));
        }
    }

    #[test]
    fn accepts_each_kind_at_limit_and_rejects_above_before_body_arrives() {
        for kind in [FrameKind::Json, FrameKind::Stream, FrameKind::Control] {
            let frame = Frame {
                kind,
                channel: u32::MAX,
                body: Bytes::from(vec![0; kind.max_body()]),
            };
            let mut buf = encode(&[frame.clone()]);
            assert_eq!(
                FrameCodec::default().decode(&mut buf).expect("decode"),
                Some(frame)
            );
            let mut bad =
                BytesMut::from(((5 + kind.max_body() + 1) as u32).to_le_bytes().as_slice());
            bad.extend_from_slice(&[kind as u8]);
            let mut codec = FrameCodec::default();
            assert!(
                matches!(codec.decode(&mut bad), Err(CodecError::Frame(FrameError::TooLarge { kind: actual, len, limit })) if actual == kind && len == limit + 1)
            );
            bad.extend_from_slice(&encode(&[json(0, "{}")]));
            assert!(matches!(
                codec.decode(&mut bad),
                Err(CodecError::Frame(FrameError::Poisoned))
            ));
        }
    }

    #[test]
    fn unknown_kind_poisons_and_failed_encode_leaves_output_unchanged() {
        let mut bad = BytesMut::from(&[5, 0, 0, 0, 255][..]);
        bad.extend_from_slice(&encode(&[json(0, "{}")]));
        let mut codec = FrameCodec::default();
        assert!(matches!(
            codec.decode(&mut bad),
            Err(CodecError::Frame(FrameError::UnknownKind(255)))
        ));
        assert!(matches!(
            codec.decode(&mut bad),
            Err(CodecError::Frame(FrameError::Poisoned))
        ));
        for kind in [FrameKind::Json, FrameKind::Stream, FrameKind::Control] {
            let mut out = BytesMut::from(&b"prefix"[..]);
            let frame = Frame {
                kind,
                channel: 0,
                body: Bytes::from(vec![0; kind.max_body() + 1]),
            };
            assert!(FrameCodec::default().encode(frame, &mut out).is_err());
            assert_eq!(&out[..], b"prefix");
        }
    }

    #[test]
    fn incomplete_channel_helpers_leave_bytes_unchanged() {
        for length in 0..HEADER_LEN {
            let mut raw = vec![0; length];
            assert_eq!(frame_channel(&raw), None);
            assert!(!set_frame_channel(&mut raw, 9));
            assert_eq!(raw, vec![0; length]);
        }
    }
}
