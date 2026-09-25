//! Request bodies are kept as text. Bytes that are not UTF-8 are stored as
//! base64 with an explicit encoding tag so nothing is lost or guessed.
use base64::Engine as _;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BodyEncoding {
    #[default]
    Utf8,
    Base64,
}

impl BodyEncoding {
    pub fn is_utf8(&self) -> bool {
        *self == Self::Utf8
    }
}

pub fn encode_body(bytes: Vec<u8>) -> (String, BodyEncoding) {
    match String::from_utf8(bytes) {
        Ok(text) => (text, BodyEncoding::Utf8),
        Err(error) => (
            base64::engine::general_purpose::STANDARD.encode(error.as_bytes()),
            BodyEncoding::Base64,
        ),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidBodyEncoding;

pub fn decode_body(body: &str, encoding: BodyEncoding) -> Result<Vec<u8>, InvalidBodyEncoding> {
    match encoding {
        BodyEncoding::Utf8 => Ok(body.as_bytes().to_vec()),
        BodyEncoding::Base64 => base64::engine::general_purpose::STANDARD
            .decode(body)
            .map_err(|_| InvalidBodyEncoding),
    }
}

/// Number of bytes the body occupies on the wire.
pub fn decoded_len(body: &str, encoding: BodyEncoding) -> Option<usize> {
    match encoding {
        BodyEncoding::Utf8 => Some(body.len()),
        BodyEncoding::Base64 => decode_body(body, encoding).ok().map(|bytes| bytes.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_stays_text_and_binary_round_trips() {
        assert_eq!(
            encode_body(b"{\"ok\":true}".to_vec()),
            ("{\"ok\":true}".to_string(), BodyEncoding::Utf8)
        );
        let binary = vec![0x1f, 0x8b, 0x08, 0x00, 0xff];
        let (stored, encoding) = encode_body(binary.clone());
        assert_eq!(encoding, BodyEncoding::Base64);
        assert_eq!(decode_body(&stored, encoding).unwrap(), binary);
        assert_eq!(decoded_len(&stored, encoding), Some(5));
        assert!(decode_body("not base64!", BodyEncoding::Base64).is_err());
    }

    #[test]
    fn missing_encoding_field_means_utf8() {
        #[derive(Deserialize)]
        struct Record {
            #[serde(default)]
            body_encoding: BodyEncoding,
        }
        let record: Record = serde_json::from_str("{}").unwrap();
        assert!(record.body_encoding.is_utf8());
        assert_eq!(
            serde_json::to_string(&BodyEncoding::Base64).unwrap(),
            "\"base64\""
        );
    }
}
