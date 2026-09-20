//! Line-ending metadata and the LF-only editor buffer invariant.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineEnding {
    #[serde(rename = "lf", alias = "LF")]
    Lf,
    #[serde(rename = "crlf", alias = "CRLF")]
    CrLf,
    #[serde(rename = "cr", alias = "CR")]
    Cr,
}

impl LineEnding {
    pub const LF: Self = Self::Lf;
    pub const CRLF: Self = Self::CrLf;
    pub const CR: Self = Self::Cr;

    pub const fn label(self) -> &'static str {
        match self {
            Self::Lf => "LF",
            Self::CrLf => "CRLF",
            Self::Cr => "CR",
        }
    }
}

/// Detects the dominant line ending. A CRLF pair is counted once, and in a tie
/// the first complete style encountered wins (with LF as the empty-file default).
pub fn detect(text: &str) -> LineEnding {
    let bytes = text.as_bytes();
    let mut counts = [0usize; 3]; // LF, CRLF, CR
    let mut first = None;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\r' if bytes.get(i + 1) == Some(&b'\n') => {
                counts[1] += 1;
                first.get_or_insert(LineEnding::CrLf);
                i += 2;
            }
            b'\n' => {
                counts[0] += 1;
                first.get_or_insert(LineEnding::Lf);
                i += 1;
            }
            b'\r' => {
                counts[2] += 1;
                first.get_or_insert(LineEnding::Cr);
                i += 1;
            }
            _ => i += 1,
        }
    }

    let max = counts.iter().copied().max().unwrap_or(0);
    if max == 0 {
        return LineEnding::Lf;
    }
    if let Some(style) = first {
        let index = match style {
            LineEnding::Lf => 0,
            LineEnding::CrLf => 1,
            LineEnding::Cr => 2,
        };
        if counts[index] == max {
            return style;
        }
    }
    if counts[1] == max {
        LineEnding::CrLf
    } else if counts[0] == max {
        LineEnding::Lf
    } else {
        LineEnding::Cr
    }
}

/// Converts every supported line ending to the CodeMirror LF invariant.
pub fn normalize(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\r' if bytes.get(i + 1) == Some(&b'\n') => {
                normalized.push('\n');
                i += 2;
            }
            b'\r' => {
                normalized.push('\n');
                i += 1;
            }
            _ => {
                // `text` is valid UTF-8, so advancing by the current char is
                // required rather than treating every byte as a char.
                let ch = text[i..].chars().next().expect("valid UTF-8 boundary");
                normalized.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    normalized
}

/// Restores a normalized LF buffer to its document's original line-ending style.
/// Normalizing first makes this idempotent even if a caller accidentally passes
/// mixed line endings.
pub fn restore(text: &str, ending: LineEnding) -> String {
    let normalized = normalize(text);
    match ending {
        LineEnding::Lf => normalized,
        LineEnding::CrLf => normalized.replace('\n', "\r\n"),
        LineEnding::Cr => normalized.replace('\n', "\r"),
    }
}

pub fn to_lf(text: &str) -> String {
    normalize(text)
}

pub fn from_lf(text: &str, ending: LineEnding) -> String {
    restore(text, ending)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_all_line_endings() {
        assert_eq!(detect("one\ntwo\n"), LineEnding::Lf);
        assert_eq!(detect("one\r\ntwo\r\n"), LineEnding::CrLf);
        assert_eq!(detect("one\rtwo\r"), LineEnding::Cr);
        assert_eq!(detect(""), LineEnding::Lf);
    }

    #[test]
    fn normalizes_mixed_input_to_lf() {
        assert_eq!(normalize("a\r\nb\rc\nd"), "a\nb\nc\nd");
    }

    #[test]
    fn restore_roundtrips_each_style() {
        let lf = "a\nb\nc";
        for ending in [LineEnding::Lf, LineEnding::CrLf, LineEnding::Cr] {
            let restored = restore(lf, ending);
            assert_eq!(normalize(&restored), lf);
            assert_eq!(detect(&restored), ending);
        }
    }

    #[test]
    fn restore_does_not_double_crlf() {
        assert_eq!(restore("a\r\nb", LineEnding::CrLf), "a\r\nb");
    }
}
