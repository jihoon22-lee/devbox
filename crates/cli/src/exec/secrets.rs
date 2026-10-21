//! Secret input never enters argv, diagnostic text, or files. Mask bytes before line splitting.
use aho_corasick::{AhoCorasick, MatchKind};
use std::{collections::BTreeMap, ffi::OsString, os::unix::ffi::OsStringExt};
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
        let patterns: Vec<&[u8]> = self
            .values
            .values()
            .chain(self.masks.iter())
            .filter(|v| !v.is_empty())
            .map(|v| v.as_slice())
            .collect();
        let longest = patterns.iter().map(|p| p.len()).max().unwrap_or(0);
        let matcher = if patterns.is_empty() {
            None
        } else {
            Some(
                AhoCorasick::builder()
                    .match_kind(MatchKind::LeftmostLongest)
                    .build(patterns)
                    .map_err(|_| SecretError::Matcher)?,
            )
        };
        Ok(Redactor {
            matcher,
            longest,
            pending: Vec::new(),
        })
    }
}
pub struct Redactor {
    matcher: Option<AhoCorasick>,
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
            self.pending
                .len()
                .saturating_sub(self.longest.saturating_sub(1))
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
}
