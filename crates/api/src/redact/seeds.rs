//! Operation-owned raw values and bounded byte variants; temporary patterns are zeroized too.
use crate::ApiError;
use zeroize::Zeroizing;
pub(crate) const MAX_SEEDS: usize = 1024;
pub(crate) const MAX_SEED_BYTES: usize = 256 * 1024;
pub(crate) const MAX_PATTERNS: usize = 4096;
pub(crate) const MAX_PATTERN_BYTES: usize = 1024 * 1024;
const HEX: &[u8; 16] = b"0123456789ABCDEF";
#[derive(Default)]
pub(super) struct Seeds {
    values: Zeroizing<Vec<Vec<u8>>>,
    bytes: usize,
}
pub(super) fn failed(reason: &str) -> ApiError {
    ApiError::RedactionFailed {
        reason: reason.into(),
    }
}
impl Seeds {
    pub fn add(&mut self, value: &[u8]) -> Result<bool, ApiError> {
        if value.is_empty() || self.values.iter().any(|v| v.as_slice() == value) {
            return Ok(false);
        }
        if self.values.len() >= MAX_SEEDS || value.len() > MAX_SEED_BYTES.saturating_sub(self.bytes)
        {
            return Err(failed("capacity"));
        }
        self.bytes += value.len();
        self.values.push(value.to_vec());
        Ok(true)
    }
    pub fn patterns(&self) -> Result<Zeroizing<Vec<Vec<u8>>>, ApiError> {
        let mut patterns = Zeroizing::new(Vec::new());
        let mut bytes = 0;
        for value in self.values.iter() {
            pattern(&mut patterns, &mut bytes, value.iter().copied())?;
            pattern(&mut patterns, &mut bytes, encoded(value, true))?;
            pattern(&mut patterns, &mut bytes, encoded(value, false))?;
            if std::str::from_utf8(value).is_ok() {
                pattern(&mut patterns, &mut bytes, json(value))?;
            }
        }
        Ok(patterns)
    }
}
fn pattern<I: Iterator<Item = u8> + Clone>(
    patterns: &mut Vec<Vec<u8>>,
    bytes: &mut usize,
    value: I,
) -> Result<(), ApiError> {
    let length = value.clone().count();
    if length == 0
        || patterns
            .iter()
            .any(|p| p.len() == length && value.clone().eq(p.iter().copied()))
    {
        return Ok(());
    }
    if patterns.len() >= MAX_PATTERNS || length > MAX_PATTERN_BYTES.saturating_sub(*bytes) {
        return Err(failed("capacity"));
    }
    // All retained allocation sizes are admitted before collecting the bytes.
    *bytes += length;
    patterns.push(value.collect());
    Ok(())
}
fn encoded(value: &[u8], form: bool) -> impl Iterator<Item = u8> + Clone + '_ {
    value.iter().flat_map(move |b| {
        let mut out = [0; 3];
        let n = if b.is_ascii_alphanumeric() || b"*-._".contains(b) {
            out[0] = *b;
            1
        } else if *b == b' ' && form {
            out[0] = b'+';
            1
        } else {
            out = [b'%', HEX[(b >> 4) as usize], HEX[(b & 15) as usize]];
            3
        };
        out.into_iter().take(n)
    })
}
fn json(value: &[u8]) -> impl Iterator<Item = u8> + Clone + '_ {
    value.iter().flat_map(|b| {
        let mut out = [0; 6];
        let escaped = match *b {
            b'"' => Some(b'"'),
            b'\\' => Some(b'\\'),
            b'\n' => Some(b'n'),
            b'\r' => Some(b'r'),
            b'\t' => Some(b't'),
            8 => Some(b'b'),
            12 => Some(b'f'),
            _ => None,
        };
        let n = if let Some(escaped) = escaped {
            out[0] = b'\\';
            out[1] = escaped;
            2
        } else if *b < 0x20 {
            out = [
                b'\\',
                b'u',
                b'0',
                b'0',
                b"0123456789abcdef"[(b >> 4) as usize],
                b"0123456789abcdef"[(b & 15) as usize],
            ];
            6
        } else {
            out[0] = *b;
            1
        };
        out.into_iter().take(n)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_encoders_match_existing_library_forms() {
        let ascii: String = (0u8..=127).map(char::from).collect();
        for value in [
            ascii.as_str(),
            "",
            "a b+c/%~",
            "quotes\"back\\slash",
            "한글 😀 é",
        ] {
            let form: String = url::form_urlencoded::byte_serialize(value.as_bytes()).collect();
            assert_eq!(
                encoded(value.as_bytes(), true).collect::<Vec<_>>(),
                form.as_bytes()
            );
            assert_eq!(
                encoded(value.as_bytes(), false).collect::<Vec<_>>(),
                form.replace('+', "%20").as_bytes()
            );
            let expected = serde_json::to_string(value).expect("json string");
            assert_eq!(
                json(value.as_bytes()).collect::<Vec<_>>(),
                expected.as_bytes()[1..expected.len() - 1]
            );
        }
    }

    #[test]
    fn seed_count_bytes_and_duplicates_are_bounded_before_retaining() {
        let mut count = Seeds::default();
        assert!(!count.add(b"").expect("empty"));
        for n in 0..MAX_SEEDS {
            assert!(
                count
                    .add(format!("fixture-{n}").as_bytes())
                    .expect("in budget")
            );
        }
        assert!(!count.add(b"fixture-0").expect("duplicate"));
        assert!(count.add(b"outside-count").is_err());
        assert_eq!(count.values.len(), MAX_SEEDS);
        let mut bytes = Seeds::default();
        assert!(bytes.add(&vec![b'a'; MAX_SEED_BYTES]).expect("at limit"));
        assert!(bytes.add(b"b").is_err());
        assert_eq!(bytes.bytes, MAX_SEED_BYTES);
    }

    #[test]
    fn expanded_patterns_fail_closed_at_byte_and_count_limits() {
        let mut seeds = Seeds::default();
        seeds.add(&vec![0; MAX_SEED_BYTES]).expect("raw budget");
        assert!(
            seeds.patterns().is_err(),
            "escaped variants exceed aggregate budget"
        );
        let mut patterns = Vec::new();
        let mut bytes = 0;
        for n in 0..MAX_PATTERNS {
            pattern(&mut patterns, &mut bytes, n.to_be_bytes().into_iter()).expect("at count");
        }
        assert!(
            pattern(
                &mut patterns,
                &mut bytes,
                MAX_PATTERNS.to_be_bytes().into_iter()
            )
            .is_err()
        );
        assert_eq!(patterns.len(), MAX_PATTERNS);
        let length = patterns.len();
        assert!(
            pattern(
                &mut patterns,
                &mut bytes,
                std::iter::repeat_n(b'x', MAX_PATTERN_BYTES)
            )
            .is_err()
        );
        assert_eq!(patterns.len(), length, "rejected pattern is not retained");
    }
}
