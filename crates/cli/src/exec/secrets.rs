//! Secret input never enters argv, diagnostic text, or files. Mask bytes before line splitting.
use aho_corasick::{AhoCorasick, MatchKind};
use std::{collections::BTreeMap, ffi::OsString, fmt::Write as _, os::unix::ffi::OsStringExt};
use tokio::io::AsyncReadExt;
use zeroize::{Zeroize, Zeroizing};

const MAX_INPUT: usize = 1024 * 1024;
#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("invalid secret stdin (expected unique NAME=value NUL records, at most 1 MiB)")]
    Invalid,
    #[error("secret stdin I/O failed")]
    Io(#[from] std::io::Error),
    #[error("secret redaction could not be initialized")]
    Matcher,
}
// Deliberately no Debug implementation: diagnostics must never format these values.
#[derive(Default)]
pub struct Secrets {
    values: BTreeMap<String, Zeroizing<Vec<u8>>>,
    masks: Vec<Zeroizing<Vec<u8>>>,
}
impl Drop for Secrets {
    fn drop(&mut self) {
        for value in self.values.values_mut() {
            value.zeroize();
        }
    }
}
impl Secrets {
    pub async fn read() -> Result<Self, SecretError> {
        let mut bytes = Zeroizing::new(Vec::new());
        tokio::io::stdin()
            .take((MAX_INPUT + 1) as u64)
            .read_to_end(&mut bytes)
            .await?;
        let result = Self::parse(&bytes);
        bytes.zeroize();
        result
    }
    fn parse(bytes: &[u8]) -> Result<Self, SecretError> {
        if bytes.len() > MAX_INPUT || (!bytes.is_empty() && bytes.last() != Some(&0)) {
            return Err(SecretError::Invalid);
        }
        let mut values = BTreeMap::new();
        let mut masks = Vec::new();
        for record in bytes
            .strip_suffix(&[0])
            .into_iter()
            .flat_map(|b| b.split(|b| *b == 0))
        {
            if let Some(mask) = record.strip_prefix(b"@=") {
                masks.push(Zeroizing::new(mask.to_vec()));
                continue;
            }
            let split = record
                .iter()
                .position(|b| *b == b'=')
                .ok_or(SecretError::Invalid)?;
            let key = std::str::from_utf8(&record[..split]).map_err(|_| SecretError::Invalid)?;
            let mut name = key.bytes();
            if !name
                .next()
                .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
                || !name.all(|b| b.is_ascii_alphanumeric() || b == b'_')
                || values.contains_key(key)
            {
                return Err(SecretError::Invalid);
            }
            values.insert(key.into(), Zeroizing::new(record[split + 1..].to_vec()));
        }
        Ok(Self { values, masks })
    }
    pub fn envs(&self) -> impl Iterator<Item = (&str, OsString)> {
        self.values
            .iter()
            .map(|(key, value)| (key.as_str(), OsString::from_vec(value.to_vec())))
    }
    pub fn redactor(&self) -> Result<Redactor, SecretError> {
        let mut patterns: Vec<Zeroizing<Vec<u8>>> = Vec::new();
        for value in self
            .values
            .values()
            .chain(self.masks.iter())
            .filter(|v| !v.is_empty())
        {
            patterns.push(Zeroizing::new(value.to_vec()));
            // Serializers escape quotes, backslashes and control characters; mask those
            // interiors too (outer quotes stay visible in the log).
            let Ok(text) = std::str::from_utf8(value) else {
                continue;
            };
            // Pre-size to the worst case (6 bytes per input byte plus quotes) so the
            // buffers never regrow and no freed chunk keeps a copy of the secret.
            let capacity = text.len().saturating_mul(6).saturating_add(2);
            let mut json = Zeroizing::new(Vec::with_capacity(capacity));
            serde_json::to_writer(&mut *json, text).map_err(|_| SecretError::Matcher)?;
            let mut debug = Zeroizing::new(String::with_capacity(capacity));
            write!(&mut *debug, "{text:?}").map_err(|_| SecretError::Matcher)?;
            for escaped in [json.as_slice(), debug.as_bytes()] {
                let inner = &escaped[1..escaped.len() - 1];
                if inner != value.as_slice() {
                    patterns.push(Zeroizing::new(inner.to_vec()));
                }
            }
        }
        let longest = patterns.iter().map(|p| p.len()).max().unwrap_or(0);
        let matcher = if patterns.is_empty() {
            None
        } else {
            Some(
                AhoCorasick::builder()
                    .match_kind(MatchKind::LeftmostLongest)
                    .build(patterns.iter().map(|p| p.as_slice()))
                    .map_err(|_| SecretError::Matcher)?,
            )
        };
        Ok(Redactor {
            matcher,
            patterns,
            longest,
            pending: Vec::new(),
        })
    }
}
pub struct Redactor {
    matcher: Option<AhoCorasick>,
    // Kept (zeroized on drop) to tell whether a held tail can still begin a secret.
    patterns: Vec<Zeroizing<Vec<u8>>>,
    longest: usize,
    pending: Vec<u8>,
}
impl Drop for Redactor {
    fn drop(&mut self) {
        self.pending.zeroize();
    }
}
impl Redactor {
    pub fn push(&mut self, bytes: &[u8], eof: bool) -> Vec<u8> {
        let Some(matcher) = &self.matcher else {
            return bytes.to_vec();
        };
        self.pending.extend_from_slice(bytes);
        let safe_end = if eof {
            self.pending.len()
        } else {
            hold_start(&self.pending, &self.patterns, self.longest)
        };
        let mut output = Vec::new();
        let mut consumed = 0;
        for matched in matcher.find_iter(&self.pending) {
            if matched.start() >= safe_end {
                break;
            }
            output.extend_from_slice(&self.pending[consumed..matched.start()]);
            output.extend_from_slice(b"[REDACTED]");
            consumed = matched.end();
        }
        if consumed < safe_end {
            output.extend_from_slice(&self.pending[consumed..safe_end]);
            consumed = safe_end;
        }
        self.pending.drain(..consumed);
        output
    }
}
/// Earliest offset whose rest of `pending` is a proper prefix of a pattern, so later output could
/// still complete a secret there; `pending.len()` if there is none. Every match that starts before
/// it is already fully visible, so LeftmostLongest decides it exactly as on the whole stream.
fn hold_start(pending: &[u8], patterns: &[Zeroizing<Vec<u8>>], longest: usize) -> usize {
    let len = pending.len();
    (len.saturating_sub(longest.saturating_sub(1))..len)
        .find(|&start| {
            let tail = &pending[start..];
            patterns
                .iter()
                .any(|pattern| pattern.len() > tail.len() && pattern.starts_with(tail))
        })
        .unwrap_or(len)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secret_parser_rejects_malformed_without_echoing_input() {
        for bytes in [
            b"TOKEN=owned-token".as_slice(),
            b"=owned-token\0",
            b"A=one\0A=two\0",
            b"A=ok\0\0",
            b"2A=owned-token\0",
        ] {
            let err = Secrets::parse(bytes).err().expect("invalid input");
            assert!(!err.to_string().contains("owned-token"));
        }
        assert!(Secrets::parse(b"EMPTY=\0TOKEN=a=b\nsecond\0").is_ok());
    }
    #[test]
    fn original_secret_is_masked_independently_of_composed_environment() {
        let secrets =
            Secrets::parse(b"@=owned-token\0HEADER=Bearer owned-token suffix\0").expect("payload");
        assert_eq!(
            secrets.envs().map(|(key, _)| key).collect::<Vec<_>>(),
            vec!["HEADER"]
        );
        let text = b"owned-token Bearer owned-token suffix";
        for split in 0..=text.len() {
            let mut redactor = secrets.redactor().expect("redactor");
            let mut output = redactor.push(&text[..split], false);
            output.extend(redactor.push(&text[split..], true));
            assert_eq!(output, b"[REDACTED] [REDACTED]");
        }
    }
    #[test]
    fn secret_redaction_handles_every_chunk_and_multiline_boundary() {
        let secrets = Secrets::parse(b"A=owned-token\0B=multi\nline\0C=owned\0").expect("secrets");
        let input = b"before owned-token multi\nline owned after";
        for split in 0..=input.len() {
            let mut redactor = secrets.redactor().expect("redactor");
            let mut out = redactor.push(&input[..split], false);
            out.extend(redactor.push(&input[split..], true));
            assert_eq!(
                out, b"before [REDACTED] [REDACTED] [REDACTED] after",
                "split {split}"
            );
        }
    }
    #[test]
    fn redactor_releases_tail_that_cannot_begin_a_secret() {
        let secrets = Secrets::parse(b"TOKEN=owned-token-for-task3\0").expect("secrets");
        let mut redactor = secrets.redactor().expect("redactor");
        assert_eq!(redactor.push(b"ready\n", false), b"ready\n");
        assert_eq!(redactor.push(b"x owned-", false), b"x ");
        assert_eq!(redactor.push(b"token-for-task3\n", true), b"[REDACTED]\n");
    }
    #[test]
    fn redactor_holds_a_short_secret_end_that_can_start_a_longer_one() {
        let secrets = Secrets::parse(b"A=ab\0B=abcd\0").expect("secrets");
        let mut redactor = secrets.redactor().expect("redactor");
        assert_eq!(redactor.push(b"zab", false), b"z");
        assert_eq!(redactor.push(b"cd\n", true), b"[REDACTED]\n");
    }
    fn escaped_secret_inputs() -> (Secrets, String, String) {
        let raw = "ow\"n\\ed\nx\u{1}";
        let mut record = b"@=".to_vec();
        record.extend_from_slice(raw.as_bytes());
        record.push(0);
        let json = serde_json::to_string(raw).expect("json");
        let debug = format!("{raw:?}");
        assert_ne!(json, debug);
        (
            Secrets::parse(&record).expect("secrets"),
            json[1..json.len() - 1].to_owned(),
            debug[1..debug.len() - 1].to_owned(),
        )
    }
    fn assert_masked_at_every_split(secrets: &Secrets, input: &[u8], expected: &[u8]) {
        for split in 0..=input.len() {
            let mut redactor = secrets.redactor().expect("redactor");
            let mut out = redactor.push(&input[..split], false);
            out.extend(redactor.push(&input[split..], true));
            assert_eq!(out, expected, "split {split}");
        }
    }
    #[test]
    fn json_escaped_secret_is_masked_at_every_split() {
        let (secrets, json, _) = escaped_secret_inputs();
        let input = format!("{{\"t\":\"{json}\"}}");
        assert_masked_at_every_split(&secrets, input.as_bytes(), br#"{"t":"[REDACTED]"}"#);
    }
    #[test]
    fn debug_escaped_secret_is_masked_at_every_split() {
        let (secrets, _, debug) = escaped_secret_inputs();
        let input = format!("t=Some(\"{debug}\")");
        assert_masked_at_every_split(&secrets, input.as_bytes(), br#"t=Some("[REDACTED]")"#);
    }
    /// Many held pushes in a row (one byte each) and every two-piece split mask like one pass.
    fn assert_streaming_matches_one_pass(secrets: &Secrets, input: &[u8]) {
        let mut redactor = secrets.redactor().expect("redactor");
        let expected = redactor.push(input, true);
        let mut out = Vec::new();
        for byte in input {
            out.extend(redactor.push(std::slice::from_ref(byte), false));
        }
        out.extend(redactor.push(&[], true));
        assert_eq!(out, expected, "byte stream {input:?}");
        for split in 0..=input.len() {
            let mut out = redactor.push(&input[..split], false);
            out.extend(redactor.push(&input[split..], true));
            assert_eq!(out, expected, "split {split} of {input:?}");
        }
    }
    #[test]
    fn streamed_redaction_matches_one_pass_for_nested_and_overlapping_secrets() {
        let plain = Secrets::parse(b"A=owned-token\0B=multi\nline\0C=owned\0").expect("secrets");
        assert_streaming_matches_one_pass(&plain, b"before owned-token multi\nline owned after");
        let (escaped, json, debug) = escaped_secret_inputs();
        let input = format!("{{\"t\":\"{json}\"}} t=Some(\"{debug}\")");
        assert_streaming_matches_one_pass(&escaped, input.as_bytes());
        // Every input up to six bytes over a small alphabet: nested, self- and cross-overlapping.
        let nested = Secrets::parse(b"A=ab\0B=abcd\0C=bca\0D=abab\0").expect("secrets");
        for len in 0..=6 {
            for index in 0..4usize.pow(len) {
                let input: Vec<u8> = (0..len)
                    .map(|digit| b"abcd"[index / 4usize.pow(digit) % 4])
                    .collect();
                assert_streaming_matches_one_pass(&nested, &input);
            }
        }
    }
    #[test]
    fn redactor_releases_a_shorter_secret_completed_at_the_end_of_a_push() {
        let secrets = Secrets::parse(b"A=xyz\0B=abcd\0").expect("secrets");
        assert_eq!(
            secrets.redactor().expect("redactor").push(b"zxyz", false),
            b"z[REDACTED]"
        );
    }
}
