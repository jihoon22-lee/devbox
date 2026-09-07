//! Bounded, offline HMAC operations for Developer Toolbox.
//!
//! The command layer deliberately passes only the typed request to this module.
//! Keys and messages are decoded in memory for the duration of one operation;
//! this module never writes, logs, or serializes them.

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use hmac::digest::block_api::EagerHash;
use hmac::{Hmac, KeyInit, Mac};
use serde::Deserialize;
use sha2::{Sha256, Sha384, Sha512};

/// Stable, intentionally non-descriptive error exposed to the UI.
pub const HMAC_ERROR: &str = "HMAC 입력을 처리할 수 없습니다.";

/// Maximum decoded key or message size. This keeps an accidental paste from
/// turning the desktop app into an unbounded hashing worker.
pub const MAX_HMAC_INPUT_BYTES: usize = 1_000_000;

/// Maximum encoded text accepted for one key or message field.
pub const MAX_HMAC_TEXT_BYTES: usize = 2_100_000;

/// The largest supported HMAC tag (SHA-512, in hex).
pub const MAX_HMAC_OUTPUT_CHARS: usize = 128;

const MAX_HMAC_TAG_BYTES: usize = 64;

/// HMAC generate wire request. Field names are camelCase in the Tauri JSON
/// contract; values intentionally use a small, exact lower-case vocabulary.
/// This type does not implement `Debug` or `Serialize` so a secret cannot be
/// accidentally formatted by a command or log helper.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HmacRequest {
    pub algorithm: String,
    pub key: String,
    pub key_encoding: String,
    pub message: String,
    pub message_encoding: String,
    pub output_encoding: String,
}

/// HMAC verify wire request. Verification returns only a boolean and never
/// returns the calculated tag to the frontend.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HmacVerifyRequest {
    pub algorithm: String,
    pub key: String,
    pub key_encoding: String,
    pub message: String,
    pub message_encoding: String,
    pub output_encoding: String,
    pub expected_tag: String,
}

#[derive(Clone, Copy)]
enum Algorithm {
    Sha256,
    Sha384,
    Sha512,
}

impl Algorithm {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "sha256" => Ok(Self::Sha256),
            "sha384" => Ok(Self::Sha384),
            "sha512" => Ok(Self::Sha512),
            _ => Err(fixed_error()),
        }
    }

    fn tag_len(self) -> usize {
        match self {
            Self::Sha256 => 32,
            Self::Sha384 => 48,
            Self::Sha512 => 64,
        }
    }
}

#[derive(Clone, Copy)]
enum InputEncoding {
    Utf8,
    Hex,
    Base64,
    Base64Url,
}

impl InputEncoding {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "utf8" => Ok(Self::Utf8),
            "hex" => Ok(Self::Hex),
            "base64" => Ok(Self::Base64),
            "base64url" => Ok(Self::Base64Url),
            _ => Err(fixed_error()),
        }
    }
}

#[derive(Clone, Copy)]
enum OutputEncoding {
    Hex,
    Base64,
    Base64Url,
}

impl OutputEncoding {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "hex" => Ok(Self::Hex),
            "base64" => Ok(Self::Base64),
            "base64url" => Ok(Self::Base64Url),
            _ => Err(fixed_error()),
        }
    }
}

struct DecodedRequest {
    algorithm: Algorithm,
    key: Vec<u8>,
    message: Vec<u8>,
    output_encoding: OutputEncoding,
}

fn fixed_error() -> String {
    HMAC_ERROR.to_string()
}

/// Generate one HMAC tag using the selected standard RustCrypto primitive.
pub fn generate(request: &HmacRequest) -> Result<String, String> {
    let decoded = decode_request(
        &request.algorithm,
        &request.key,
        &request.key_encoding,
        &request.message,
        &request.message_encoding,
        &request.output_encoding,
    )?;
    let tag = compute(decoded.algorithm, &decoded.key, &decoded.message)?;
    encode_output(&tag, decoded.output_encoding)
}

/// Verify one HMAC tag with the primitive's constant-time `verify_slice`
/// implementation. A well-formed tag with the wrong length/value is a normal
/// signature mismatch and returns `false`; malformed encoding is an error.
pub fn verify(request: &HmacVerifyRequest) -> Result<bool, String> {
    let decoded = decode_request(
        &request.algorithm,
        &request.key,
        &request.key_encoding,
        &request.message,
        &request.message_encoding,
        &request.output_encoding,
    )?;
    let expected = decode_tag(&request.expected_tag, decoded.output_encoding)?;
    if expected.len() != decoded.algorithm.tag_len() {
        return Ok(false);
    }
    verify_with(decoded.algorithm, &decoded.key, &decoded.message, &expected)
}

fn decode_request(
    algorithm: &str,
    key: &str,
    key_encoding: &str,
    message: &str,
    message_encoding: &str,
    output_encoding: &str,
) -> Result<DecodedRequest, String> {
    let algorithm = Algorithm::parse(algorithm)?;
    let key_encoding = InputEncoding::parse(key_encoding)?;
    let message_encoding = InputEncoding::parse(message_encoding)?;
    let output_encoding = OutputEncoding::parse(output_encoding)?;
    let key = decode_input(key, key_encoding)?;
    let message = decode_input(message, message_encoding)?;
    if key.is_empty() {
        return Err(fixed_error());
    }
    Ok(DecodedRequest {
        algorithm,
        key,
        message,
        output_encoding,
    })
}

fn decode_input(value: &str, encoding: InputEncoding) -> Result<Vec<u8>, String> {
    ensure_text_bound(value)?;
    match encoding {
        InputEncoding::Utf8 => {
            ensure_input_bound(value.len())?;
            Ok(value.as_bytes().to_vec())
        }
        InputEncoding::Hex => decode_hex(value),
        InputEncoding::Base64 => decode_base64(value, false),
        InputEncoding::Base64Url => decode_base64(value, true),
    }
}

fn decode_tag(value: &str, encoding: OutputEncoding) -> Result<Vec<u8>, String> {
    ensure_text_bound(value)?;
    if value.len() > MAX_HMAC_OUTPUT_CHARS {
        return Err(fixed_error());
    }
    match encoding {
        OutputEncoding::Hex => decode_hex_bounded(value, MAX_HMAC_TAG_BYTES),
        OutputEncoding::Base64 => decode_base64_bounded(value, false, MAX_HMAC_TAG_BYTES),
        OutputEncoding::Base64Url => decode_base64_bounded(value, true, MAX_HMAC_TAG_BYTES),
    }
}

fn ensure_text_bound(value: &str) -> Result<(), String> {
    if value.len() > MAX_HMAC_TEXT_BYTES {
        Err(fixed_error())
    } else {
        Ok(())
    }
}

fn ensure_input_bound(length: usize) -> Result<(), String> {
    if length > MAX_HMAC_INPUT_BYTES {
        Err(fixed_error())
    } else {
        Ok(())
    }
}

fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    decode_hex_bounded(value, MAX_HMAC_INPUT_BYTES)
}

fn decode_hex_bounded(value: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
    if !value.len().is_multiple_of(2) || value.len() / 2 > max_bytes {
        return Err(fixed_error());
    }
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.as_chunks::<2>().0 {
        let high = hex_digit(pair[0]).ok_or_else(fixed_error)?;
        let low = hex_digit(pair[1]).ok_or_else(fixed_error)?;
        decoded.push((high << 4) | low);
    }
    Ok(decoded)
}

fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn decode_base64(value: &str, url_safe: bool) -> Result<Vec<u8>, String> {
    decode_base64_bounded(value, url_safe, MAX_HMAC_INPUT_BYTES)
}

fn decode_base64_bounded(value: &str, url_safe: bool, max_bytes: usize) -> Result<Vec<u8>, String> {
    let valid_alphabet = if url_safe {
        value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    } else {
        value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='))
    };
    if !valid_alphabet {
        return Err(fixed_error());
    }

    let decoded = if url_safe {
        URL_SAFE_NO_PAD.decode(value)
    } else {
        STANDARD.decode(value)
    }
    .map_err(|_| fixed_error())?;
    if decoded.len() > max_bytes {
        return Err(fixed_error());
    }

    // Re-encoding rejects non-canonical pad bits and, for standard Base64,
    // missing/extra padding. This makes the wire representation deterministic.
    let canonical = if url_safe {
        URL_SAFE_NO_PAD.encode(&decoded)
    } else {
        STANDARD.encode(&decoded)
    };
    if canonical != value {
        return Err(fixed_error());
    }
    Ok(decoded)
}

fn encode_output(value: &[u8], encoding: OutputEncoding) -> Result<String, String> {
    let encoded = match encoding {
        OutputEncoding::Hex => encode_hex(value),
        OutputEncoding::Base64 => STANDARD.encode(value),
        OutputEncoding::Base64Url => URL_SAFE_NO_PAD.encode(value),
    };
    if encoded.len() > MAX_HMAC_OUTPUT_CHARS {
        return Err(fixed_error());
    }
    Ok(encoded)
}

fn encode_hex(value: &[u8]) -> String {
    let mut encoded = String::with_capacity(value.len() * 2);
    for byte in value {
        encoded.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
        encoded.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
    }
    encoded
}

fn compute(algorithm: Algorithm, key: &[u8], message: &[u8]) -> Result<Vec<u8>, String> {
    match algorithm {
        Algorithm::Sha256 => compute_with::<Sha256>(key, message),
        Algorithm::Sha384 => compute_with::<Sha384>(key, message),
        Algorithm::Sha512 => compute_with::<Sha512>(key, message),
    }
}

fn compute_with<D: EagerHash>(key: &[u8], message: &[u8]) -> Result<Vec<u8>, String> {
    let mut mac = Hmac::<D>::new_from_slice(key).map_err(|_| fixed_error())?;
    mac.update(message);
    Ok(mac.finalize().into_bytes().to_vec())
}

fn verify_with(
    algorithm: Algorithm,
    key: &[u8],
    message: &[u8],
    expected: &[u8],
) -> Result<bool, String> {
    match algorithm {
        Algorithm::Sha256 => verify_with_digest::<Sha256>(key, message, expected),
        Algorithm::Sha384 => verify_with_digest::<Sha384>(key, message, expected),
        Algorithm::Sha512 => verify_with_digest::<Sha512>(key, message, expected),
    }
}

fn verify_with_digest<D: EagerHash>(
    key: &[u8],
    message: &[u8],
    expected: &[u8],
) -> Result<bool, String> {
    let mut mac = Hmac::<D>::new_from_slice(key).map_err(|_| fixed_error())?;
    mac.update(message);
    Ok(mac.verify_slice(expected).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(
        algorithm: &str,
        key: &str,
        key_encoding: &str,
        message: &str,
        message_encoding: &str,
        output_encoding: &str,
    ) -> HmacRequest {
        HmacRequest {
            algorithm: algorithm.to_string(),
            key: key.to_string(),
            key_encoding: key_encoding.to_string(),
            message: message.to_string(),
            message_encoding: message_encoding.to_string(),
            output_encoding: output_encoding.to_string(),
        }
    }

    fn verify_request(base: HmacRequest, expected_tag: &str) -> HmacVerifyRequest {
        HmacVerifyRequest {
            algorithm: base.algorithm,
            key: base.key,
            key_encoding: base.key_encoding,
            message: base.message,
            message_encoding: base.message_encoding,
            output_encoding: base.output_encoding,
            expected_tag: expected_tag.to_string(),
        }
    }

    #[test]
    fn matches_rfc_4231_vectors_for_all_supported_algorithms() {
        let key = "Jefe";
        let message = "what do ya want for nothing?";
        assert_eq!(
            generate(&request("sha256", key, "utf8", message, "utf8", "hex")).unwrap(),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        assert_eq!(
            generate(&request("sha384", key, "utf8", message, "utf8", "hex")).unwrap(),
            "af45d2e376484031617f78d2b58a6b1b9c7ef464f5a01b47e42ec3736322445e8e2240ca5e69e2c78b3239ecfab21649"
        );
        assert_eq!(
            generate(&request("sha512", key, "utf8", message, "utf8", "hex")).unwrap(),
            "164b7a7bfcf819e2e395fbe73b56e0a387bd64222e831fd610270cd7ea2505549758bf75c05a994a6d034f65f8f0e6fdcaeab1a34d4a6b4b636e070a38bce737"
        );
    }

    #[test]
    fn supports_key_message_and_output_encodings() {
        let utf8 = request("sha256", "secret", "utf8", "payload", "utf8", "hex");
        let hex = request(
            "sha256",
            "736563726574",
            "hex",
            "7061796c6f6164",
            "hex",
            "base64",
        );
        let base64 = request(
            "sha256",
            "c2VjcmV0",
            "base64",
            "cGF5bG9hZA==",
            "base64",
            "base64url",
        );
        let expected = generate(&utf8).unwrap();
        assert_eq!(
            generate(&hex).unwrap(),
            STANDARD.encode(hex_decode(&expected))
        );
        assert_eq!(
            generate(&base64).unwrap(),
            URL_SAFE_NO_PAD.encode(hex_decode(&expected))
        );
    }

    #[test]
    fn verification_is_true_for_matching_tag_and_false_for_mismatch() {
        let base = request("sha256", "secret", "utf8", "payload", "utf8", "hex");
        let tag = generate(&base).unwrap();
        assert!(verify(&verify_request(base, &tag)).unwrap());

        for output_encoding in ["base64", "base64url"] {
            let base = request(
                "sha256",
                "secret",
                "utf8",
                "payload",
                "utf8",
                output_encoding,
            );
            let tag = generate(&base).unwrap();
            assert!(verify(&verify_request(base, &tag)).unwrap());
        }

        let base = request("sha256", "secret", "utf8", "payload", "utf8", "hex");
        assert!(!verify(&verify_request(base, &"0".repeat(64))).unwrap());

        let base = request("sha256", "secret", "utf8", "payload", "utf8", "hex");
        assert_eq!(
            verify(&verify_request(
                base,
                &"0".repeat(MAX_HMAC_OUTPUT_CHARS + 1)
            )),
            Err(HMAC_ERROR.to_string())
        );
    }

    #[test]
    fn rejects_malformed_wire_values_with_one_fixed_error() {
        let mut invalid = request("SHA256", "secret", "utf8", "payload", "utf8", "hex");
        assert_eq!(generate(&invalid), Err(HMAC_ERROR.to_string()));
        invalid.algorithm = "sha256".to_string();
        invalid.key_encoding = "hex".to_string();
        invalid.key = "not-hex".to_string();
        assert_eq!(generate(&invalid), Err(HMAC_ERROR.to_string()));
        invalid.key_encoding = "base64".to_string();
        invalid.key = "Zh==".to_string();
        assert_eq!(generate(&invalid), Err(HMAC_ERROR.to_string()));
        invalid.key = "secret".to_string();
        invalid.key_encoding = "utf8".to_string();
        invalid.output_encoding = "utf8".to_string();
        assert_eq!(generate(&invalid), Err(HMAC_ERROR.to_string()));
    }

    #[test]
    fn enforces_non_empty_key_and_input_limits_without_reflecting_input() {
        let empty_key = request("sha256", "", "utf8", "payload", "utf8", "hex");
        assert_eq!(generate(&empty_key), Err(HMAC_ERROR.to_string()));

        let empty_message = request("sha256", "secret", "utf8", "", "utf8", "hex");
        assert!(generate(&empty_message).is_ok());

        let oversized = request(
            "sha256",
            &"k".repeat(MAX_HMAC_INPUT_BYTES + 1),
            "utf8",
            "payload",
            "utf8",
            "hex",
        );
        let error = generate(&oversized).unwrap_err();
        assert_eq!(error, HMAC_ERROR);
        assert!(!error.contains('k'));

        assert!(ensure_text_bound(&"x".repeat(MAX_HMAC_TEXT_BYTES)).is_ok());
        assert_eq!(
            ensure_text_bound(&"x".repeat(MAX_HMAC_TEXT_BYTES + 1)),
            Err(HMAC_ERROR.to_string())
        );
        assert!(ensure_input_bound(MAX_HMAC_INPUT_BYTES).is_ok());
        assert_eq!(
            ensure_input_bound(MAX_HMAC_INPUT_BYTES + 1),
            Err(HMAC_ERROR.to_string())
        );
    }

    #[test]
    fn serde_contract_rejects_unknown_fields() {
        let parsed = serde_json::from_str::<HmacRequest>(
            r#"{"algorithm":"sha256","key":"secret","keyEncoding":"utf8","message":"payload","messageEncoding":"utf8","outputEncoding":"hex","secret":"unexpected"}"#,
        );
        assert!(parsed.is_err());
    }

    fn hex_decode(value: &str) -> Vec<u8> {
        decode_hex(value).unwrap()
    }
}
