//! gRPC 메타데이터 검사(HTTP 헤더와 같은 오류 모양). 이름은 소문자 `[0-9a-z_.-]`로 바꾸고,
//! 전송 계층이 정하는 이름(`te`·`content-type`·`grpc-*`·가상 헤더 등)은 받지 않는다. `-bin` 값은 Base64다.
use crate::{ApiError, HeaderRow, validate};
use tonic::metadata::{
    AsciiMetadataKey, AsciiMetadataValue, BinaryMetadataKey, BinaryMetadataValue,
};
use tonic::metadata::{KeyAndValueRef, MetadataMap};

fn reserved(name: &str) -> bool {
    name.starts_with("grpc-")
        || matches!(
            name,
            "te" | "content-type"
                | "host"
                | "connection"
                | "upgrade"
                | "transfer-encoding"
                | "content-length"
                | "keep-alive"
                | "proxy-connection"
                | "user-agent"
        )
}

fn key_char(b: u8) -> bool {
    b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.')
}

pub(crate) fn prepare(rows: &[HeaderRow]) -> Result<MetadataMap, ApiError> {
    if rows.len() > validate::MAX_ROWS {
        return Err(validate::too_large(validate::MAX_ROWS));
    }
    let mut map = MetadataMap::new();
    let mut total = 0;
    for row in rows {
        let value = validate::header(row.row, &row.name, &row.value)?;
        let name = row.name.to_ascii_lowercase();
        if !name.bytes().all(key_char) {
            return Err(ApiError::header(row.row, "name", "invalid_token"));
        }
        if reserved(&name) {
            return Err(ApiError::header(row.row, "name", "reserved"));
        }
        total += name.len() + value.len() + 4;
        if name.ends_with("-bin") {
            let bytes = decode_base64(&value)
                .ok_or_else(|| ApiError::header(row.row, "value", "base64"))?;
            let key = BinaryMetadataKey::from_bytes(name.as_bytes())
                .map_err(|_| ApiError::header(row.row, "name", "invalid_token"))?;
            map.append_bin(key, BinaryMetadataValue::from_bytes(&bytes));
        } else {
            let key = AsciiMetadataKey::from_bytes(name.as_bytes())
                .map_err(|_| ApiError::header(row.row, "name", "invalid_token"))?;
            let value = AsciiMetadataValue::try_from(value.as_str())
                .map_err(|_| ApiError::header(row.row, "value", "control_character"))?;
            map.append(key, value);
        }
    }
    if total > validate::MAX_HEADER_BYTES {
        return Err(validate::too_large(validate::MAX_HEADER_BYTES));
    }
    Ok(map)
}

fn decode_base64(text: &str) -> Option<Vec<u8>> {
    data_encoding::BASE64
        .decode(text.as_bytes())
        .or_else(|_| data_encoding::BASE64_NOPAD.decode(text.as_bytes()))
        .ok()
}

/// 받은 메타데이터를 화면 표시용 쌍으로(이진 값은 Base64). 값은 가린다.
pub(crate) fn pairs(map: &MetadataMap, redactor: &crate::redact::Redactor) -> Vec<crate::Pair> {
    map.iter()
        .map(|entry| match entry {
            KeyAndValueRef::Ascii(key, value) => (
                key.as_str().to_owned(),
                String::from_utf8_lossy(value.as_encoded_bytes()).into_owned(),
            ),
            KeyAndValueRef::Binary(key, value) => (
                key.as_str().to_owned(),
                String::from_utf8_lossy(value.as_encoded_bytes()).into_owned(),
            ),
        })
        .map(|(name, value)| crate::Pair {
            name,
            value: redactor.text(&value),
        })
        .collect()
}
