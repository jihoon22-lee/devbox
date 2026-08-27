//! Bounded, offline JWT signature verification for Developer Toolbox.
//!
//! Decoding of the compact token and claim presentation lives in the
//! frontend so browser preview and the packaged app have the same display
//! contract.  This module is deliberately only the native cryptographic
//! boundary: it accepts a strict request, verifies an allow-listed HMAC
//! algorithm with RustCrypto, and returns a boolean.  It never logs,
//! persists, or serializes a key, token, or calculated signature.

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use hmac::digest::block_api::EagerHash;
use hmac::{Hmac, KeyInit, Mac};
use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;
use sha2::{Sha256, Sha384, Sha512};
use std::collections::HashSet;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// Stable, intentionally non-descriptive error exposed to the UI.
pub const JWT_VERIFY_ERROR: &str = "JWT 검증을 처리할 수 없습니다.";

/// Maximum compact signing input accepted by the native command.
pub const MAX_SIGNING_INPUT_BYTES: usize = 256 * 1024;
pub const MAX_SEGMENT_BYTES: usize = 96 * 1024;
/// Maximum encoded signature text. HS512 is 86 unpadded base64url chars.
pub const MAX_SIGNATURE_TEXT_BYTES: usize = 128;
/// Maximum encoded key text and decoded key bytes, shared with HMAC policy.
pub const MAX_KEY_TEXT_BYTES: usize = 2_100_000;
const MAX_JSON_BYTES: usize = 64 * 1024;
const MAX_JSON_DEPTH: usize = 32;
const MAX_JSON_NODES: usize = 10_000;
const MAX_JSON_STRING_BYTES: usize = 16 * 1024;
const MAX_CRITICAL_HEADERS: usize = 8;
const MAX_NUMERIC_DATE_SECONDS: f64 = 8_640_000_000_000.0;
const CLOCK_SKEW_SECONDS: f64 = 60.0;
pub const MAX_KEY_BYTES: usize = 1_000_000;

/// Strict native DTO.  Secret-bearing fields intentionally do not implement
/// `Debug` or `Serialize`, preventing accidental formatting at the command
/// boundary.  `deny_unknown_fields` keeps browser/native wire drift visible.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JwtVerifyRequest {
    pub algorithm: String,
    pub signing_input: String,
    pub signature: String,
    pub key: String,
    pub key_encoding: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Algorithm {
    Hs256,
    Hs384,
    Hs512,
}

impl Algorithm {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "HS256" => Ok(Self::Hs256),
            "HS384" => Ok(Self::Hs384),
            "HS512" => Ok(Self::Hs512),
            _ => Err(fixed_error()),
        }
    }

    fn tag_len(self) -> usize {
        match self {
            Self::Hs256 => 32,
            Self::Hs384 => 48,
            Self::Hs512 => 64,
        }
    }

    fn wire_name(self) -> &'static str {
        match self {
            Self::Hs256 => "HS256",
            Self::Hs384 => "HS384",
            Self::Hs512 => "HS512",
        }
    }
}

#[derive(Clone, Copy)]
enum KeyEncoding {
    Utf8,
    Hex,
    Base64,
    Base64Url,
}

impl KeyEncoding {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "utf8" => Ok(Self::Utf8),
            "hex" => Ok(Self::Hex),
            "base64" => Ok(Self::Base64),
            "base64url" => Ok(Self::Base64Url),
            // PEM and JWK are intentionally not parsed in this feature.  In
            // particular, an RSA/EC public key must never silently fall back
            // to an HMAC secret, which is the classic algorithm-confusion
            // failure mode.
            _ => Err(fixed_error()),
        }
    }
}

fn fixed_error() -> String {
    JWT_VERIFY_ERROR.to_string()
}

/// Verify a compact JWT's already parsed signing input with HMAC.
///
/// The caller still gets only `true`/`false` for a well-formed request.  A
/// malformed encoding, unsupported algorithm, wrong signature length, or key
/// policy violation is a fixed error so malformed data cannot be mistaken for
/// a normal signature mismatch.
pub fn verify(request: &JwtVerifyRequest) -> Result<bool, String> {
    let now_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| fixed_error())?
        .as_secs_f64();
    verify_at(request, now_seconds)
}

fn verify_at(request: &JwtVerifyRequest, now_seconds: f64) -> Result<bool, String> {
    if !now_seconds.is_finite() || now_seconds.abs() > MAX_NUMERIC_DATE_SECONDS {
        return Err(fixed_error());
    }
    let algorithm = Algorithm::parse(&request.algorithm)?;
    let key_encoding = KeyEncoding::parse(&request.key_encoding)?;
    let signing_input = validate_signing_input_at(&request.signing_input, algorithm, now_seconds)?;
    let signature = decode_signature(&request.signature, algorithm.tag_len())?;
    let key = decode_key(&request.key, key_encoding)?;
    if key.len() < algorithm.tag_len() {
        return Err(fixed_error());
    }

    verify_with(algorithm, &key, signing_input.as_bytes(), &signature)
}

#[cfg(test)]
fn validate_signing_input(value: &str, expected: Algorithm) -> Result<String, String> {
    let now_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| fixed_error())?
        .as_secs_f64();
    validate_signing_input_at(value, expected, now_seconds)
}

fn validate_signing_input_at(
    value: &str,
    expected: Algorithm,
    now_seconds: f64,
) -> Result<String, String> {
    if value.is_empty() || value.len() > MAX_SIGNING_INPUT_BYTES || !value.is_ascii() {
        return Err(fixed_error());
    }
    let Some((header, payload)) = value.split_once('.') else {
        return Err(fixed_error());
    };
    if header.is_empty()
        || payload.is_empty()
        || payload.contains('.')
        || header.len() > MAX_SEGMENT_BYTES
        || payload.len() > MAX_SEGMENT_BYTES
    {
        return Err(fixed_error());
    }
    // Re-encoding through the pinned decoder rejects '=' padding, alternate
    // alphabets, bad length modulo, and non-zero unused pad bits.
    let header_bytes = decode_canonical_base64url(header, false)?;
    let payload_bytes = decode_canonical_base64url(payload, false)?;
    if header_algorithm(&header_bytes)? != expected.wire_name() {
        return Err(fixed_error());
    }
    validate_json(&payload_bytes)?;
    validate_temporal_claims(&payload_bytes, now_seconds)?;
    Ok(value.to_string())
}

fn validate_temporal_claims(payload: &[u8], now_seconds: f64) -> Result<(), String> {
    if !now_seconds.is_finite() || now_seconds.abs() > MAX_NUMERIC_DATE_SECONDS {
        return Err(fixed_error());
    }
    let value = serde_json::from_slice::<serde_json::Value>(payload).map_err(|_| fixed_error())?;
    let Some(object) = value.as_object() else {
        return Ok(());
    };
    let claim = |name: &str| -> Result<Option<f64>, String> {
        let Some(value) = object.get(name) else {
            return Ok(None);
        };
        let number = value.as_f64().ok_or_else(fixed_error)?;
        if !number.is_finite() || number.abs() > MAX_NUMERIC_DATE_SECONDS {
            return Err(fixed_error());
        }
        Ok(Some(number))
    };

    if claim("exp")?.is_some_and(|exp| now_seconds > exp + CLOCK_SKEW_SECONDS)
        || claim("nbf")?.is_some_and(|nbf| now_seconds + CLOCK_SKEW_SECONDS < nbf)
        || claim("iat")?.is_some_and(|iat| iat > now_seconds + CLOCK_SKEW_SECONDS)
    {
        return Err(fixed_error());
    }
    Ok(())
}

fn header_algorithm(bytes: &[u8]) -> Result<String, String> {
    validate_json(bytes)?;
    let value = serde_json::from_slice::<serde_json::Value>(bytes).map_err(|_| fixed_error())?;
    let object = value.as_object().ok_or_else(fixed_error)?;
    for name in ["typ", "kid", "cty"] {
        if let Some(value) = object.get(name) {
            if !value.is_string() {
                return Err(fixed_error());
            }
        }
    }
    if let Some(critical) = object.get("crit") {
        let values = critical.as_array().ok_or_else(fixed_error)?;
        if values.len() > MAX_CRITICAL_HEADERS {
            return Err(fixed_error());
        }
        let mut names = HashSet::new();
        for value in values {
            let name = value.as_str().ok_or_else(fixed_error)?;
            if name.is_empty()
                || name == "crit"
                || !matches!(name, "alg" | "typ" | "kid" | "cty")
                || !object.contains_key(name)
                || !names.insert(name)
            {
                return Err(fixed_error());
            }
        }
    }
    object
        .get("alg")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(fixed_error)
}

struct JsonBounds {
    nodes: usize,
}

struct JsonSeed<'a> {
    bounds: &'a mut JsonBounds,
    depth: usize,
}

struct JsonVisitor<'a> {
    bounds: &'a mut JsonBounds,
    depth: usize,
}

fn enter_json_value<E>(bounds: &mut JsonBounds, depth: usize) -> Result<(), E>
where
    E: de::Error,
{
    bounds.nodes += 1;
    if bounds.nodes > MAX_JSON_NODES || depth > MAX_JSON_DEPTH {
        return Err(E::custom("JWT JSON bounds"));
    }
    Ok(())
}

fn check_json_string<E>(value: &str) -> Result<(), E>
where
    E: de::Error,
{
    if value.len() > MAX_JSON_STRING_BYTES {
        return Err(E::custom("JWT JSON string bounds"));
    }
    Ok(())
}

impl<'de, 'a> DeserializeSeed<'de> for JsonSeed<'a> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(JsonVisitor {
            bounds: self.bounds,
            depth: self.depth,
        })
    }
}

impl<'de, 'a> Visitor<'de> for JsonVisitor<'a> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a bounded JSON value")
    }

    fn visit_bool<E>(self, _value: bool) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        enter_json_value::<E>(self.bounds, self.depth)
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        if value.unsigned_abs() > 9_007_199_254_740_991 {
            return Err(E::custom("JWT JSON number bounds"));
        }
        enter_json_value::<E>(self.bounds, self.depth)
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        if value > 9_007_199_254_740_991 {
            return Err(E::custom("JWT JSON number bounds"));
        }
        enter_json_value::<E>(self.bounds, self.depth)
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        if !value.is_finite() || (value.fract() == 0.0 && value.abs() > 9_007_199_254_740_991.0) {
            return Err(E::custom("JWT JSON number bounds"));
        }
        enter_json_value::<E>(self.bounds, self.depth)
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        check_json_string::<E>(value)?;
        enter_json_value::<E>(self.bounds, self.depth)
    }

    fn visit_borrowed_str<E>(self, value: &'de str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.visit_str(value)
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.visit_str(&value)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        enter_json_value::<E>(self.bounds, self.depth)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        enter_json_value::<A::Error>(self.bounds, self.depth)?;
        while sequence
            .next_element_seed(JsonSeed {
                bounds: self.bounds,
                depth: self.depth + 1,
            })?
            .is_some()
        {}
        Ok(())
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        enter_json_value::<A::Error>(self.bounds, self.depth)?;
        let mut names = HashSet::new();
        while let Some(name) = map.next_key::<String>()? {
            check_json_string::<A::Error>(&name)?;
            self.bounds.nodes += 1;
            if self.bounds.nodes > MAX_JSON_NODES || !names.insert(name) {
                return Err(de::Error::custom("JWT JSON object bounds"));
            }
            map.next_value_seed(JsonSeed {
                bounds: self.bounds,
                depth: self.depth + 1,
            })?;
        }
        Ok(())
    }
}

fn validate_json(bytes: &[u8]) -> Result<(), String> {
    if bytes.is_empty() || bytes.len() > MAX_JSON_BYTES {
        return Err(fixed_error());
    }
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    deserializer
        .deserialize_any(JsonVisitor {
            bounds: &mut JsonBounds { nodes: 0 },
            depth: 0,
        })
        .map_err(|_| fixed_error())?;
    deserializer.end().map_err(|_| fixed_error())
}

fn decode_signature(value: &str, expected_len: usize) -> Result<Vec<u8>, String> {
    if value.is_empty() || value.len() > MAX_SIGNATURE_TEXT_BYTES {
        return Err(fixed_error());
    }
    let decoded = decode_canonical_base64url(value, false)?;
    if decoded.len() != expected_len {
        return Err(fixed_error());
    }
    Ok(decoded)
}

fn decode_key(value: &str, encoding: KeyEncoding) -> Result<Vec<u8>, String> {
    if value.len() > MAX_KEY_TEXT_BYTES {
        return Err(fixed_error());
    }
    let decoded = match encoding {
        KeyEncoding::Utf8 => value.as_bytes().to_vec(),
        KeyEncoding::Hex => decode_hex(value)?,
        KeyEncoding::Base64 => decode_canonical_base64(value)?,
        KeyEncoding::Base64Url => decode_canonical_base64url(value, true)?,
    };
    if decoded.is_empty() || decoded.len() > MAX_KEY_BYTES {
        return Err(fixed_error());
    }
    Ok(decoded)
}

fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    if value.is_empty() || !value.len().is_multiple_of(2) || value.len() / 2 > MAX_KEY_BYTES {
        return Err(fixed_error());
    }
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
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

fn decode_canonical_base64(value: &str) -> Result<Vec<u8>, String> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='))
    {
        return Err(fixed_error());
    }
    let decoded = STANDARD.decode(value).map_err(|_| fixed_error())?;
    if decoded.is_empty() || decoded.len() > MAX_KEY_BYTES || STANDARD.encode(&decoded) != value {
        return Err(fixed_error());
    }
    Ok(decoded)
}

fn decode_canonical_base64url(value: &str, allow_empty: bool) -> Result<Vec<u8>, String> {
    if (!allow_empty && value.is_empty())
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(fixed_error());
    }
    let decoded = URL_SAFE_NO_PAD.decode(value).map_err(|_| fixed_error())?;
    if decoded.len() > MAX_KEY_BYTES || URL_SAFE_NO_PAD.encode(&decoded) != value {
        return Err(fixed_error());
    }
    Ok(decoded)
}

fn verify_with(
    algorithm: Algorithm,
    key: &[u8],
    signing_input: &[u8],
    signature: &[u8],
) -> Result<bool, String> {
    match algorithm {
        Algorithm::Hs256 => verify_with_digest::<Sha256>(key, signing_input, signature),
        Algorithm::Hs384 => verify_with_digest::<Sha384>(key, signing_input, signature),
        Algorithm::Hs512 => verify_with_digest::<Sha512>(key, signing_input, signature),
    }
}

fn verify_with_digest<D: EagerHash>(
    key: &[u8],
    signing_input: &[u8],
    signature: &[u8],
) -> Result<bool, String> {
    let mut mac = Hmac::<D>::new_from_slice(key).map_err(|_| fixed_error())?;
    mac.update(signing_input);
    Ok(mac.verify_slice(signature).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIGNING_INPUT: &str =
        "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ";
    const HS384_SIGNING_INPUT: &str =
        "eyJhbGciOiJIUzM4NCIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ";
    const HS512_SIGNING_INPUT: &str =
        "eyJhbGciOiJIUzUxMiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ";
    const KEY_HEX: &str = "3031323334353637383930313233343536373839303132333435363738393031";
    const LONG_KEY_HEX: &str = "30313233343536373839303132333435363738393031323334353637383930313031323334353637383930313233343536373839303132333435363738393031";
    const HS256_SIGNATURE: &str = "AL_nmexgcwawKDK5uJ0RtfAxT1GguksdPuaahEACpHc";

    fn request(
        algorithm: &str,
        signing_input: &str,
        signature: &str,
        key: &str,
        key_encoding: &str,
    ) -> JwtVerifyRequest {
        JwtVerifyRequest {
            algorithm: algorithm.to_string(),
            signing_input: signing_input.to_string(),
            signature: signature.to_string(),
            key: key.to_string(),
            key_encoding: key_encoding.to_string(),
        }
    }

    fn signing_input_for(payload: &str) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"HS256"}"#);
        let payload = URL_SAFE_NO_PAD.encode(payload.as_bytes());
        format!("{header}.{payload}")
    }

    #[test]
    fn verifies_known_hs256_vector_and_rejects_modified_signature() {
        let valid = request("HS256", SIGNING_INPUT, HS256_SIGNATURE, KEY_HEX, "hex");
        assert!(verify(&valid).unwrap());

        let mut invalid = request("HS256", SIGNING_INPUT, HS256_SIGNATURE, KEY_HEX, "hex");
        invalid.signature = format!("{}A", &HS256_SIGNATURE[..HS256_SIGNATURE.len() - 1]);
        assert!(!verify(&invalid).unwrap());
    }

    #[test]
    fn accepts_all_allow_listed_hmac_algorithms_with_exact_tag_lengths() {
        let vectors = [
            (
                "HS256",
                KEY_HEX,
                HS256_SIGNATURE,
            ),
            (
                "HS384",
                LONG_KEY_HEX,
                "58Hc1lXLsSwvo-Mor4Son_yMVfSf4OA5qsVBjYpWacUeSlLSMVjLgTZ-rk5ORQrr",
            ),
            (
                "HS512",
                LONG_KEY_HEX,
                "Ck5IG3CaU-sZxfd1TzD9VxRVRbNb45Hv5mO0wzo8cJlVFKgUhVH8ofN1XBNgpq8J9kzS7zfDLKXA-y9bjc4EBw",
            ),
        ];
        for (index, (algorithm, key, signature)) in vectors.into_iter().enumerate() {
            let signing_input = match index {
                0 => SIGNING_INPUT,
                1 => HS384_SIGNING_INPUT,
                _ => HS512_SIGNING_INPUT,
            };
            let request = request(algorithm, signing_input, signature, key, "hex");
            assert!(verify(&request).unwrap());
        }
    }

    #[test]
    fn rejects_algorithm_confusion_and_unimplemented_key_formats() {
        for algorithm in ["none", "RS256", "ES256", "hs256"] {
            let request = request(algorithm, SIGNING_INPUT, HS256_SIGNATURE, KEY_HEX, "hex");
            assert_eq!(verify(&request), Err(JWT_VERIFY_ERROR.to_string()));
        }
        for encoding in ["pem", "jwk", "raw"] {
            let request = request("HS256", SIGNING_INPUT, HS256_SIGNATURE, KEY_HEX, encoding);
            assert_eq!(verify(&request), Err(JWT_VERIFY_ERROR.to_string()));
        }
    }

    #[test]
    fn enforces_key_and_signature_bounds_without_reflecting_values() {
        let short = request("HS256", SIGNING_INPUT, HS256_SIGNATURE, "short", "utf8");
        assert_eq!(verify(&short), Err(JWT_VERIFY_ERROR.to_string()));

        let mut malformed = request("HS256", SIGNING_INPUT, HS256_SIGNATURE, KEY_HEX, "hex");
        malformed.signature = "A".repeat(MAX_SIGNATURE_TEXT_BYTES + 1);
        assert_eq!(verify(&malformed), Err(JWT_VERIFY_ERROR.to_string()));

        malformed.signature = "A".to_string();
        assert_eq!(verify(&malformed), Err(JWT_VERIFY_ERROR.to_string()));

        malformed.key = "secret-value-that-must-not-appear".to_string();
        malformed.key_encoding = "base64".to_string();
        let error = verify(&malformed).unwrap_err();
        assert_eq!(error, JWT_VERIFY_ERROR);
        assert!(!error.contains("secret-value"));
    }

    #[test]
    fn requires_canonical_compact_base64url_signing_input() {
        let mut malformed = request("HS256", SIGNING_INPUT, HS256_SIGNATURE, KEY_HEX, "hex");
        malformed.signing_input = format!("={SIGNING_INPUT}");
        assert_eq!(verify(&malformed), Err(JWT_VERIFY_ERROR.to_string()));

        malformed.signing_input = format!("{SIGNING_INPUT}.");
        assert_eq!(verify(&malformed), Err(JWT_VERIFY_ERROR.to_string()));
    }

    #[test]
    fn rejects_header_algorithm_mismatch_and_duplicate_alg_members() {
        let mismatch = request("HS512", SIGNING_INPUT, HS256_SIGNATURE, LONG_KEY_HEX, "hex");
        assert_eq!(verify(&mismatch), Err(JWT_VERIFY_ERROR.to_string()));

        let duplicate_header = "eyJhbGciOiJIUzI1NiIsImFsZyI6IkhTNTEyIn0";
        let duplicate_input = format!(
            "{duplicate_header}.{}",
            SIGNING_INPUT.split('.').nth(1).unwrap()
        );
        let duplicate = request("HS256", &duplicate_input, HS256_SIGNATURE, KEY_HEX, "hex");
        assert_eq!(verify(&duplicate), Err(JWT_VERIFY_ERROR.to_string()));
    }

    #[test]
    fn native_signing_input_matches_bounded_json_and_critical_header_rules() {
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"{"alg":"HS256"}"#);
        let invalid_payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"not-json");
        let invalid = format!("{header}.{invalid_payload}");
        assert_eq!(
            validate_signing_input(&invalid, Algorithm::Hs256),
            Err(JWT_VERIFY_ERROR.to_string())
        );

        let critical_header = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(br#"{"alg":"HS256","crit":["b64"]}"#);
        let valid_payload = SIGNING_INPUT.split('.').nth(1).unwrap();
        let unsupported_critical = format!("{critical_header}.{valid_payload}");
        assert_eq!(
            validate_signing_input(&unsupported_critical, Algorithm::Hs256),
            Err(JWT_VERIFY_ERROR.to_string())
        );

        let deeply_nested = format!(
            "{}null{}",
            "[".repeat(MAX_JSON_DEPTH + 2),
            "]".repeat(MAX_JSON_DEPTH + 2)
        );
        assert_eq!(
            validate_json(deeply_nested.as_bytes()),
            Err(JWT_VERIFY_ERROR.to_string())
        );

        let oversized_string = format!(
            "{{\"value\":\"{}\"}}",
            "x".repeat(MAX_JSON_STRING_BYTES + 1)
        );
        assert_eq!(
            validate_json(oversized_string.as_bytes()),
            Err(JWT_VERIFY_ERROR.to_string())
        );
    }

    #[test]
    fn native_boundary_validates_exp_nbf_and_iat_with_fixed_skew() {
        let now = 1_700_000_000.0;
        for payload in [
            r#"{"exp":1699999940}"#,
            r#"{"nbf":1700000060}"#,
            r#"{"iat":1700000060}"#,
        ] {
            assert!(
                validate_signing_input_at(&signing_input_for(payload), Algorithm::Hs256, now,)
                    .is_ok()
            );
        }

        for payload in [
            r#"{"exp":1699999939}"#,
            r#"{"nbf":1700000061}"#,
            r#"{"iat":1700000061}"#,
            r#"{"exp":"not-a-number"}"#,
        ] {
            assert_eq!(
                validate_signing_input_at(&signing_input_for(payload), Algorithm::Hs256, now,),
                Err(JWT_VERIFY_ERROR.to_string())
            );
        }

        let expired = request(
            "HS256",
            &signing_input_for(r#"{"exp":1699999939}"#),
            HS256_SIGNATURE,
            KEY_HEX,
            "hex",
        );
        assert_eq!(verify_at(&expired, now), Err(JWT_VERIFY_ERROR.to_string()));
    }

    #[test]
    fn serde_contract_rejects_unknown_fields() {
        let parsed = serde_json::from_str::<JwtVerifyRequest>(
            r#"{"algorithm":"HS256","signingInput":"header.payload","signature":"sig","key":"secret","keyEncoding":"utf8","token":"unexpected"}"#,
        );
        assert!(parsed.is_err());
    }
}
