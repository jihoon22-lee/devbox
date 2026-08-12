//! Lossless conversions between UTF-8 Rust strings and negotiated LSP positions.

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PositionEncoding {
    Utf16,
    Utf8,
}

impl PositionEncoding {
    pub const fn lsp_name(self) -> &'static str {
        match self {
            Self::Utf16 => "utf-16",
            Self::Utf8 => "utf-8",
        }
    }

    pub fn negotiate(server_value: Option<&str>) -> Result<(Self, bool), PositionError> {
        match server_value {
            Some(value) if value.eq_ignore_ascii_case("utf-8") => Ok((Self::Utf8, false)),
            Some(value) if value.eq_ignore_ascii_case("utf-16") => Ok((Self::Utf16, false)),
            Some(value) => Err(PositionError::UnsupportedEncoding(value.to_owned())),
            None => Ok((Self::Utf16, true)),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspPosition {
    pub line: u32,
    pub character: u32,
}

impl LspPosition {
    pub const fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspRange {
    pub start: LspPosition,
    pub end: LspPosition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PositionError {
    ByteOffsetOutOfBounds { offset: usize, length: usize },
    InvalidUtf8Boundary { offset: usize },
    LineOutOfBounds { line: u32 },
    CharacterOutOfBounds { line: u32, character: u32 },
    InvalidCharacterBoundary { line: u32, character: u32 },
    UnsupportedEncoding(String),
    PositionOverflow,
}

impl fmt::Display for PositionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ByteOffsetOutOfBounds { offset, length } => {
                write!(f, "byte offset {offset} exceeds document length {length}")
            }
            Self::InvalidUtf8Boundary { offset } => {
                write!(f, "byte offset {offset} is not a UTF-8 boundary")
            }
            Self::LineOutOfBounds { line } => write!(f, "line {line} is outside the document"),
            Self::CharacterOutOfBounds { line, character } => {
                write!(f, "character {character} is outside line {line}")
            }
            Self::InvalidCharacterBoundary { line, character } => write!(
                f,
                "character {character} points inside an encoded character on line {line}"
            ),
            Self::UnsupportedEncoding(encoding) => {
                write!(
                    f,
                    "server selected unsupported position encoding {encoding:?}"
                )
            }
            Self::PositionOverflow => f.write_str("document position exceeds the LSP u32 range"),
        }
    }
}

impl std::error::Error for PositionError {}

pub fn offset_to_position(
    text: &str,
    offset: usize,
    encoding: PositionEncoding,
) -> Result<LspPosition, PositionError> {
    if offset > text.len() {
        return Err(PositionError::ByteOffsetOutOfBounds {
            offset,
            length: text.len(),
        });
    }
    if !text.is_char_boundary(offset) {
        return Err(PositionError::InvalidUtf8Boundary { offset });
    }

    let before = &text[..offset];
    let line = u32::try_from(before.bytes().filter(|byte| *byte == b'\n').count())
        .map_err(|_| PositionError::PositionOverflow)?;
    let line_start = before.rfind('\n').map_or(0, |index| index + 1);
    let character = encoded_len(&text[line_start..offset], encoding)?;
    Ok(LspPosition { line, character })
}

pub fn position_to_offset(
    text: &str,
    position: LspPosition,
    encoding: PositionEncoding,
) -> Result<usize, PositionError> {
    let (start, end) = line_bounds(text, position.line)?;
    let line = &text[start..end];
    let mut units = 0_u32;
    for (byte_offset, character) in line.char_indices() {
        if units == position.character {
            return Ok(start + byte_offset);
        }
        let next = units
            .checked_add(character_units(character, encoding))
            .ok_or(PositionError::PositionOverflow)?;
        if position.character < next {
            return Err(PositionError::InvalidCharacterBoundary {
                line: position.line,
                character: position.character,
            });
        }
        units = next;
    }
    if units == position.character {
        Ok(end)
    } else {
        Err(PositionError::CharacterOutOfBounds {
            line: position.line,
            character: position.character,
        })
    }
}

/// Clamp display-only positions to the nearest valid boundary. Mutating edits
/// must use [`position_to_offset`] and reject malformed positions instead.
pub fn position_to_offset_clamped(
    text: &str,
    position: LspPosition,
    encoding: PositionEncoding,
) -> usize {
    let last_line =
        u32::try_from(text.bytes().filter(|byte| *byte == b'\n').count()).unwrap_or(u32::MAX);
    let line_number = position.line.min(last_line);
    let (start, end) = line_bounds(text, line_number).unwrap_or((text.len(), text.len()));
    let line = &text[start..end];
    let mut units = 0_u32;
    for (byte_offset, character) in line.char_indices() {
        if position.character == units {
            return start + byte_offset;
        }
        let width = character_units(character, encoding);
        let next = units.saturating_add(width);
        if position.character < next {
            let distance_before = position.character.saturating_sub(units);
            let distance_after = next.saturating_sub(position.character);
            return if distance_before <= distance_after {
                start + byte_offset
            } else {
                start + byte_offset + character.len_utf8()
            };
        }
        units = next;
    }
    end
}

fn line_bounds(text: &str, line: u32) -> Result<(usize, usize), PositionError> {
    let mut start = 0_usize;
    for _ in 0..line {
        let Some(relative) = text[start..].find('\n') else {
            return Err(PositionError::LineOutOfBounds { line });
        };
        start += relative + 1;
    }
    let end = text[start..]
        .find('\n')
        .map_or(text.len(), |relative| start + relative);
    Ok((start, end))
}

fn encoded_len(value: &str, encoding: PositionEncoding) -> Result<u32, PositionError> {
    let units = match encoding {
        PositionEncoding::Utf16 => value.encode_utf16().count(),
        PositionEncoding::Utf8 => value.len(),
    };
    u32::try_from(units).map_err(|_| PositionError::PositionOverflow)
}

const fn character_units(character: char, encoding: PositionEncoding) -> u32 {
    match encoding {
        PositionEncoding::Utf16 => character.len_utf16() as u32,
        PositionEncoding::Utf8 => character.len_utf8() as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_counts_surrogate_pairs_and_korean_code_units() {
        let text = "a😀한b\n";
        assert_eq!(
            offset_to_position(text, "a😀한".len(), PositionEncoding::Utf16).unwrap(),
            LspPosition::new(0, 4)
        );
        assert_eq!(
            position_to_offset(text, LspPosition::new(0, 4), PositionEncoding::Utf16).unwrap(),
            "a😀한".len()
        );
        assert!(matches!(
            position_to_offset(text, LspPosition::new(0, 2), PositionEncoding::Utf16),
            Err(PositionError::InvalidCharacterBoundary { .. })
        ));
    }

    #[test]
    fn utf8_uses_bytes_and_rejects_mid_codepoint_positions() {
        let text = "a😀한b";
        assert_eq!(
            offset_to_position(text, "a😀".len(), PositionEncoding::Utf8).unwrap(),
            LspPosition::new(0, 5)
        );
        assert_eq!(
            position_to_offset(text, LspPosition::new(0, 5), PositionEncoding::Utf8).unwrap(),
            "a😀".len()
        );
        assert!(matches!(
            position_to_offset(text, LspPosition::new(0, 3), PositionEncoding::Utf8),
            Err(PositionError::InvalidCharacterBoundary { .. })
        ));
    }

    #[test]
    fn lf_lines_empty_lines_and_trailing_lines_are_exact() {
        let text = "one\n\n세\n";
        assert_eq!(
            offset_to_position(text, 4, PositionEncoding::Utf16).unwrap(),
            LspPosition::new(1, 0)
        );
        assert_eq!(
            position_to_offset(text, LspPosition::new(2, 1), PositionEncoding::Utf16).unwrap(),
            "one\n\n세".len()
        );
        assert_eq!(
            position_to_offset(text, LspPosition::new(3, 0), PositionEncoding::Utf16).unwrap(),
            text.len()
        );
    }

    #[test]
    fn diagnostic_clamp_never_applies_to_strict_edit_conversion() {
        let text = "a😀b\nlast";
        let invalid = LspPosition::new(0, 2);
        assert!(position_to_offset(text, invalid, PositionEncoding::Utf16).is_err());
        assert_eq!(
            position_to_offset_clamped(text, invalid, PositionEncoding::Utf16),
            1
        );
        assert_eq!(
            position_to_offset_clamped(text, LspPosition::new(99, 99), PositionEncoding::Utf16),
            text.len()
        );
    }

    #[test]
    fn legacy_or_unknown_negotiation_falls_back_to_utf16() {
        assert_eq!(
            PositionEncoding::negotiate(Some("utf-8")).unwrap(),
            (PositionEncoding::Utf8, false),
        );
        assert_eq!(
            PositionEncoding::negotiate(None).unwrap(),
            (PositionEncoding::Utf16, true),
        );
        assert!(matches!(
            PositionEncoding::negotiate(Some("utf-32")),
            Err(PositionError::UnsupportedEncoding(value)) if value == "utf-32"
        ));
    }
}
