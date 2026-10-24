//! Bounded LSP stdio framing; lengths are UTF-8 byte counts.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FrameError {
    #[error("invalid LSP frame")]
    Invalid,
    #[error("LSP frame exceeds limit")]
    Limit,
    #[error("truncated LSP frame")]
    Eof,
}
pub const MAX_BODY: usize = 2 * 1024 * 1024;
const MAX_HEADER: usize = 8192;
#[derive(Default)]
pub struct Decoder {
    buffer: Vec<u8>,
    length: Option<usize>,
    failed: bool,
}
impl Decoder {
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<String>, FrameError> {
        if self.failed {
            return Err(FrameError::Invalid);
        }
        let result = self.decode(bytes);
        self.failed = result.is_err();
        result
    }
    fn decode(&mut self, bytes: &[u8]) -> Result<Vec<String>, FrameError> {
        let mut messages = Vec::new();
        for chunk in bytes.chunks(MAX_HEADER) {
            self.buffer.extend_from_slice(chunk);
            loop {
                if self.length.is_none() {
                    let end = self.buffer.windows(4).position(|p| p == b"\r\n\r\n");
                    let Some(end) = end else {
                        if self.buffer.len() > MAX_HEADER {
                            return Err(FrameError::Limit);
                        }
                        break;
                    };
                    if end > MAX_HEADER {
                        return Err(FrameError::Limit);
                    }
                    let header = std::str::from_utf8(&self.buffer[..end])
                        .map_err(|_| FrameError::Invalid)?;
                    if !header.is_ascii() {
                        return Err(FrameError::Invalid);
                    }
                    let mut length = None;
                    for line in header.split("\r\n") {
                        let (key, value) = line.split_once(':').ok_or(FrameError::Invalid)?;
                        if key.eq_ignore_ascii_case("Content-Length") {
                            let value = value.trim();
                            if length.is_some()
                                || value.is_empty()
                                || !value.bytes().all(|b| b.is_ascii_digit())
                            {
                                return Err(FrameError::Invalid);
                            }
                            let n = value.parse::<usize>().map_err(|_| FrameError::Limit)?;
                            if n == 0 || n > MAX_BODY {
                                return Err(FrameError::Limit);
                            }
                            length = Some(n);
                        }
                    }
                    self.length = Some(length.ok_or(FrameError::Invalid)?);
                    self.buffer.drain(..end + 4);
                }
                let length = self.length.expect("parsed header");
                if self.buffer.len() < length {
                    break;
                }
                let text = std::str::from_utf8(&self.buffer[..length])
                    .map_err(|_| FrameError::Invalid)?
                    .to_owned();
                self.buffer.drain(..length);
                self.length = None;
                messages.push(text);
            }
        }
        Ok(messages)
    }
    pub fn finish(&self) -> Result<(), FrameError> {
        if self.failed || !self.buffer.is_empty() || self.length.is_some() {
            Err(FrameError::Eof)
        } else {
            Ok(())
        }
    }
}
pub fn encode(json: &str) -> Result<Vec<u8>, FrameError> {
    if json.is_empty() || json.len() > MAX_BODY {
        return Err(FrameError::Limit);
    }
    let mut bytes = format!("Content-Length: {}\r\n\r\n", json.len()).into_bytes();
    bytes.extend_from_slice(json.as_bytes());
    Ok(bytes)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn framing_round_trips_split_and_coalesced_messages() {
        let a = r#"{"text":"가😀"}"#;
        let b = r#"{"id":2,"result":null}"#;
        let bytes = [encode(a).expect("frame"), encode(b).expect("frame")].concat();
        for size in [1, 2, 7, bytes.len()] {
            let mut d = Decoder::default();
            let mut messages = Vec::new();
            for part in bytes.chunks(size) {
                messages.extend(d.push(part).expect("decode"));
            }
            assert_eq!(messages, [a, b]);
            assert_eq!(d.finish(), Ok(()));
        }
        assert!(
            String::from_utf8(encode(a).expect("frame"))
                .expect("utf8")
                .starts_with(&format!("Content-Length: {}\r\n", a.len()))
        );
    }
    #[test]
    fn malformed_length_oversize_and_eof_fail_closed() {
        for header in [
            "Content-Length: -1\r\n\r\n",
            "Content-Length: nope\r\n\r\n",
            "Content-Length: 0\r\nContent-Length: 0\r\n\r\n",
            "Unknown: 4\r\n\r\n",
        ] {
            assert!(Decoder::default().push(header.as_bytes()).is_err());
        }
        assert!(
            Decoder::default()
                .push(format!("Content-Length: {}\r\n\r\n", MAX_BODY + 1).as_bytes())
                .is_err()
        );
        let mut d = Decoder::default();
        d.push(b"Content-Length: 3\r\n\r\n{").expect("partial");
        assert_eq!(d.finish(), Err(FrameError::Eof));
    }
}
