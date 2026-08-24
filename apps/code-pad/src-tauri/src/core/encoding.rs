//! Supported text encodings and the lossless/explicit conversion boundary.
//!
//! Code Pad deliberately supports a small set of encodings. Detection is strict:
//! known BOMs win first, valid UTF-8 is accepted next, and CP949 is accepted only
//! when `chardetng` selects EUC-KR and decoding has no replacement errors.

use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
use encoding_rs::{Encoding as RsEncoding, EUC_KR, UTF_16BE, UTF_16LE, UTF_8};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt;

/// The encodings exposed by the Code Pad status bar and encoding picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EncodingKind {
    #[serde(rename = "utf8", alias = "UTF-8", alias = "utf-8")]
    Utf8,
    #[serde(rename = "utf16Le", alias = "UTF-16LE", alias = "utf-16le")]
    Utf16Le,
    #[serde(rename = "utf16Be", alias = "UTF-16BE", alias = "utf-16be")]
    Utf16Be,
    #[serde(rename = "cp949", alias = "CP949", alias = "euc-kr", alias = "EUC-KR")]
    Cp949,
}

impl EncodingKind {
    /// Upper-case aliases are useful at the command/JSON boundary while keeping
    /// the Rust enum idiomatic.
    pub const UTF8: Self = Self::Utf8;
    pub const UTF16_LE: Self = Self::Utf16Le;
    pub const UTF16_BE: Self = Self::Utf16Be;
    pub const CP949: Self = Self::Cp949;

    pub const fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf16Le => "UTF-16 LE",
            Self::Utf16Be => "UTF-16 BE",
            Self::Cp949 => "CP949",
        }
    }
}

/// Encoding plus the independent BOM-preservation bit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Encoding {
    #[serde(rename = "encodingKind", alias = "encoding_kind", alias = "kind")]
    pub encoding_kind: EncodingKind,
    pub bom: bool,
}

impl Encoding {
    pub const fn new(encoding_kind: EncodingKind, bom: bool) -> Self {
        Self { encoding_kind, bom }
    }

    pub const fn kind(self) -> EncodingKind {
        self.encoding_kind
    }

    pub const fn utf8() -> Self {
        Self::new(EncodingKind::Utf8, false)
    }

    pub const fn utf8_bom() -> Self {
        Self::new(EncodingKind::Utf8, true)
    }

    pub const fn utf16_le(bom: bool) -> Self {
        Self::new(EncodingKind::Utf16Le, bom)
    }

    pub const fn utf16_be(bom: bool) -> Self {
        Self::new(EncodingKind::Utf16Be, bom)
    }

    pub const fn cp949() -> Self {
        Self::new(EncodingKind::Cp949, false)
    }
}

/// The result of automatic detection. `lossy` means the bytes could not be
/// classified as one of the supported encodings and were opened as UTF-8 with
/// replacement characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodingDetection {
    pub encoding: Encoding,
    pub lossy: bool,
}

/// Text returned by automatic decoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedText {
    pub text: String,
    pub encoding: Encoding,
    pub lossy: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    InvalidUtf8,
    InvalidUtf16,
    MissingBom,
    UnexpectedBom,
    UnsupportedBom,
    InvalidEncoding,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUtf8 => f.write_str("input is not valid UTF-8"),
            Self::InvalidUtf16 => f.write_str("input is not valid UTF-16"),
            Self::MissingBom => f.write_str("selected encoding requires its matching BOM"),
            Self::UnexpectedBom => {
                f.write_str("input BOM does not match the selected encoding metadata")
            }
            Self::UnsupportedBom => f.write_str("CP949 cannot have a BOM"),
            Self::InvalidEncoding => {
                f.write_str("input contains bytes invalid for the selected encoding")
            }
        }
    }
}

impl std::error::Error for DecodeError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeError {
    UnrepresentableCharacter,
    UnsupportedBom,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnrepresentableCharacter => {
                f.write_str("text contains a character that the selected encoding cannot represent")
            }
            Self::UnsupportedBom => f.write_str("CP949 cannot have a BOM"),
        }
    }
}

impl std::error::Error for EncodeError {}

/// Detects one of the supported encodings, in the order required by the design:
/// BOM, strict UTF-8, then a strict CP949 detector result.
pub fn detect(bytes: &[u8]) -> EncodingDetection {
    if let Some((bom_encoding, _)) = RsEncoding::for_bom(bytes) {
        if bom_encoding == UTF_8 {
            return EncodingDetection {
                encoding: Encoding::utf8_bom(),
                lossy: false,
            };
        }
        if bom_encoding == UTF_16LE {
            return EncodingDetection {
                encoding: Encoding::utf16_le(true),
                lossy: false,
            };
        }
        if bom_encoding == UTF_16BE {
            return EncodingDetection {
                encoding: Encoding::utf16_be(true),
                lossy: false,
            };
        }
    }

    if std::str::from_utf8(bytes).is_ok() {
        return EncodingDetection {
            encoding: Encoding::utf8(),
            lossy: false,
        };
    }

    let mut detector = EncodingDetector::new(Iso2022JpDetection::Deny);
    detector.feed(bytes, true);
    let guessed = detector.guess(None, Utf8Detection::Deny);
    if guessed == EUC_KR {
        let (decoded, _, had_errors) = EUC_KR.decode(bytes);
        if !had_errors && decoded.chars().all(|ch| ch != '\u{FFFD}') {
            return EncodingDetection {
                encoding: Encoding::cp949(),
                lossy: false,
            };
        }
    }

    EncodingDetection {
        encoding: Encoding::utf8(),
        lossy: true,
    }
}

/// Automatically detects and decodes bytes. Unsupported/ambiguous bytes follow
/// the documented UTF-8-lossy fallback so the user can still inspect the file.
pub fn decode_detect(bytes: &[u8]) -> DecodedText {
    let detected = detect(bytes);
    if detected.lossy {
        return DecodedText {
            text: String::from_utf8_lossy(bytes).into_owned(),
            encoding: detected.encoding,
            lossy: true,
        };
    }

    match decode(bytes, detected.encoding) {
        Ok(text) => DecodedText {
            text,
            encoding: detected.encoding,
            lossy: false,
        },
        Err(_) => DecodedText {
            text: String::from_utf8_lossy(bytes).into_owned(),
            encoding: Encoding::utf8(),
            lossy: true,
        },
    }
}

/// Decodes bytes using an explicit encoding metadata value.
pub fn decode(bytes: &[u8], encoding: Encoding) -> Result<String, DecodeError> {
    match encoding.encoding_kind {
        EncodingKind::Utf8 => {
            let payload = explicit_payload(bytes, encoding.bom, b"\xEF\xBB\xBF", 3)?;
            std::str::from_utf8(payload)
                .map(str::to_owned)
                .map_err(|_| DecodeError::InvalidUtf8)
        }
        EncodingKind::Utf16Le => decode_utf16(bytes, encoding.bom, true),
        EncodingKind::Utf16Be => decode_utf16(bytes, encoding.bom, false),
        EncodingKind::Cp949 => {
            if encoding.bom {
                return Err(DecodeError::UnsupportedBom);
            }
            let (text, _, had_errors) = EUC_KR.decode(bytes);
            if had_errors {
                Err(DecodeError::InvalidEncoding)
            } else {
                Ok(text.into_owned())
            }
        }
    }
}

fn decode_utf16(bytes: &[u8], has_bom: bool, little_endian: bool) -> Result<String, DecodeError> {
    let expected = if little_endian {
        b"\xFF\xFE"
    } else {
        b"\xFE\xFF"
    };
    let payload = explicit_payload(bytes, has_bom, expected, 2)?;
    if payload.len() % 2 != 0 {
        return Err(DecodeError::InvalidUtf16);
    }

    let units: Vec<u16> = payload
        .as_chunks::<2>()
        .0
        .iter()
        .map(|chunk| {
            if little_endian {
                u16::from_le_bytes(*chunk)
            } else {
                u16::from_be_bytes(*chunk)
            }
        })
        .collect();
    String::from_utf16(&units).map_err(|_| DecodeError::InvalidUtf16)
}

fn explicit_payload<'a>(
    bytes: &'a [u8],
    has_bom: bool,
    expected: &[u8],
    bom_len: usize,
) -> Result<&'a [u8], DecodeError> {
    let has_any_supported_bom = bytes.starts_with(b"\xEF\xBB\xBF")
        || bytes.starts_with(b"\xFF\xFE")
        || bytes.starts_with(b"\xFE\xFF");
    if has_bom {
        if bytes.starts_with(expected) {
            Ok(&bytes[bom_len..])
        } else if has_any_supported_bom {
            Err(DecodeError::UnexpectedBom)
        } else {
            Err(DecodeError::MissingBom)
        }
    } else if has_any_supported_bom {
        Err(DecodeError::UnexpectedBom)
    } else {
        Ok(bytes)
    }
}

/// Encodes text while preserving the caller's explicit BOM choice.
pub fn encode(text: &str, encoding: Encoding) -> Result<Vec<u8>, EncodeError> {
    match encoding.encoding_kind {
        EncodingKind::Utf8 => {
            let mut bytes = Vec::with_capacity(text.len() + usize::from(encoding.bom) * 3);
            if encoding.bom {
                bytes.extend_from_slice(b"\xEF\xBB\xBF");
            }
            bytes.extend_from_slice(text.as_bytes());
            Ok(bytes)
        }
        EncodingKind::Utf16Le => encode_utf16(text, encoding.bom, true),
        EncodingKind::Utf16Be => encode_utf16(text, encoding.bom, false),
        EncodingKind::Cp949 => {
            if encoding.bom {
                return Err(EncodeError::UnsupportedBom);
            }
            let (bytes, _, had_errors) = EUC_KR.encode(text);
            if had_errors {
                return Err(EncodeError::UnrepresentableCharacter);
            }
            Ok(match bytes {
                Cow::Borrowed(bytes) => bytes.to_vec(),
                Cow::Owned(bytes) => bytes,
            })
        }
    }
}

fn encode_utf16(text: &str, bom: bool, little_endian: bool) -> Result<Vec<u8>, EncodeError> {
    let mut bytes = Vec::with_capacity(text.encode_utf16().count() * 2 + usize::from(bom) * 2);
    if bom {
        if little_endian {
            bytes.extend_from_slice(b"\xFF\xFE");
        } else {
            bytes.extend_from_slice(b"\xFE\xFF");
        }
    }
    for unit in text.encode_utf16() {
        let encoded = if little_endian {
            unit.to_le_bytes()
        } else {
            unit.to_be_bytes()
        };
        bytes.extend_from_slice(&encoded);
    }
    Ok(bytes)
}

/// Alias used by callers that want to make the automatic nature explicit.
pub fn detect_and_decode(bytes: &[u8]) -> DecodedText {
    decode_detect(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(bytes: &[u8], expected_encoding: Encoding) {
        let detected = detect(bytes);
        assert_eq!(detected.encoding, expected_encoding);
        assert!(!detected.lossy);
        let decoded = decode(bytes, detected.encoding).unwrap();
        assert_eq!(encode(&decoded, detected.encoding).unwrap(), bytes);
    }

    #[test]
    fn detects_utf8_before_legacy_detector() {
        roundtrip("hello 안녕\n".as_bytes(), Encoding::utf8());
    }

    #[test]
    fn detects_and_roundtrips_utf8_bom() {
        let bytes = [b"\xEF\xBB\xBF".as_slice(), "hello".as_bytes()].concat();
        roundtrip(&bytes, Encoding::utf8_bom());
    }

    #[test]
    fn detects_and_roundtrips_utf16_le_bom() {
        let bytes = encode("hello 안녕", Encoding::utf16_le(true)).unwrap();
        roundtrip(&bytes, Encoding::utf16_le(true));
    }

    #[test]
    fn detects_and_roundtrips_utf16_be_bom() {
        let bytes = encode("hello 안녕", Encoding::utf16_be(true)).unwrap();
        roundtrip(&bytes, Encoding::utf16_be(true));
    }

    #[test]
    fn explicit_utf16_without_bom_is_manual_and_lossless() {
        for encoding in [Encoding::utf16_le(false), Encoding::utf16_be(false)] {
            let bytes = encode("hello 안녕", encoding).unwrap();
            assert_eq!(decode(&bytes, encoding).unwrap(), "hello 안녕");
        }
    }

    #[test]
    fn detects_cp949_and_roundtrips_it() {
        let (bytes, _, had_errors) = EUC_KR.encode("안녕하세요");
        assert!(!had_errors);
        let bytes = bytes.into_owned();
        roundtrip(&bytes, Encoding::cp949());
    }

    #[test]
    fn cp949_rejects_unrepresentable_text() {
        assert_eq!(
            encode("안녕🙂", Encoding::cp949()),
            Err(EncodeError::UnrepresentableCharacter)
        );
    }

    #[test]
    fn unknown_bytes_use_lossy_utf8_fallback() {
        let decoded = decode_detect(&[0xFF, 0xFE, 0xFA]);
        assert!(decoded.lossy);
        assert_eq!(decoded.encoding, Encoding::utf8());
        assert!(decoded.text.contains('\u{FFFD}'));
    }

    #[test]
    fn malformed_utf16_is_rejected_when_selected_explicitly() {
        assert_eq!(
            decode(&[0xFF, 0xFE, 0x00], Encoding::utf16_le(true)),
            Err(DecodeError::InvalidUtf16)
        );
    }

    #[test]
    fn explicit_encoding_requires_bom_metadata_to_match_bytes() {
        assert_eq!(
            decode(b"hello", Encoding::utf8_bom()),
            Err(DecodeError::MissingBom)
        );
        assert_eq!(
            decode(b"\xEF\xBB\xBFhello", Encoding::utf8()),
            Err(DecodeError::UnexpectedBom)
        );
        assert_eq!(
            decode(b"\xFE\xFF\0h", Encoding::utf16_le(true)),
            Err(DecodeError::UnexpectedBom)
        );
    }
}
