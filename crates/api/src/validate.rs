//! 모양 검사(RFC 9110 token·field-value, URL, 상한). 오류에는 값이 아니라 고정 사유만 담는다.
use crate::ApiError;

pub(crate) const MAX_TEXT_BODY: usize = 8 * 1024 * 1024;
pub(crate) const MAX_PARTS: usize = 50;
pub(crate) const MAX_ROWS: usize = 100;
pub(crate) const MAX_HEADER_BYTES: usize = 64 * 1024;
pub(crate) const MAX_URL_BYTES: usize = 64 * 1024;
pub(crate) const MAX_TIMEOUT_MS: i64 = 600_000;
pub(crate) const MAX_REDIRECTS: usize = 10;

fn tchar(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)
}

/// RFC 9110 token(메서드·헤더 이름).
pub(crate) fn is_token(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(tchar)
}

pub(crate) fn method(text: &str) -> Result<reqwest::Method, ApiError> {
    if !is_token(text) {
        return Err(ApiError::InvalidMethod);
    }
    reqwest::Method::from_bytes(text.as_bytes()).map_err(|_| ApiError::InvalidMethod)
}

/// field-value: 앞뒤 OWS(SP·HTAB)를 뺀 뒤 CTL을 거절한다(HTAB 허용).
pub(crate) fn field_value(text: &str) -> Result<&str, &'static str> {
    let trimmed = text.trim_matches([' ', '\t']);
    if trimmed
        .bytes()
        .any(|b| (b < 0x20 && b != b'\t') || b == 0x7f)
    {
        return Err("control_character");
    }
    Ok(trimmed)
}

/// 헤더 행 하나를 검사해 (이름, 값)을 돌려준다.
pub(crate) fn header(index: u32, name: &str, value: &str) -> Result<String, ApiError> {
    if name.is_empty() {
        return Err(ApiError::header(index, "name", "empty"));
    }
    if !is_token(name) {
        return Err(ApiError::header(index, "name", "invalid_token"));
    }
    field_value(value)
        .map(str::to_owned)
        .map_err(|reason| ApiError::header(index, "value", reason))
}

/// RFC 6265 cookie-name(token)·cookie-octet.
pub(crate) fn cookie(index: u32, name: &str, value: &str) -> Result<(), ApiError> {
    let reason = |reason: &str| ApiError::InvalidCookie {
        index,
        reason: reason.into(),
    };
    if !is_token(name) {
        return Err(reason("invalid_name"));
    }
    let octet = |b: u8| matches!(b, 0x21 | 0x23..=0x2b | 0x2d..=0x3a | 0x3c..=0x5b | 0x5d..=0x7e);
    let inner = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value);
    if !inner.bytes().all(octet) {
        return Err(reason("invalid_value"));
    }
    Ok(())
}

/// http/https 절대 URL. userinfo는 거절하고 fragment는 보내지 않는다.
pub(crate) fn url(text: &str) -> Result<url::Url, ApiError> {
    url_with(text, &["http", "https"])
}

/// `schemes` 중 하나인 절대 URL(WebSocket은 `ws`·`wss`). 나머지 규칙은 `url`과 같다.
pub(crate) fn url_with(text: &str, schemes: &[&str]) -> Result<url::Url, ApiError> {
    if text.len() > MAX_URL_BYTES {
        return Err(ApiError::url("too_long"));
    }
    let mut url = url::Url::parse(text.trim()).map_err(|_| ApiError::url("parse"))?;
    if !schemes.contains(&url.scheme()) {
        return Err(ApiError::url("scheme"));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(ApiError::url("host"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ApiError::url("userinfo"));
    }
    url.set_fragment(None);
    Ok(url)
}

/// 값 이름이 자격 증명처럼 보이는지(v0.9.0 `is_sensitive_name`).
pub(crate) fn is_sensitive_name(name: &str) -> bool {
    let compact = name.to_ascii_lowercase().replace(['_', '-'], "");
    [
        "authorization",
        "cookie",
        "apikey",
        "apivalue",
        "token",
        "secret",
        "password",
        "passwd",
        "privatekey",
        "username",
    ]
    .iter()
    .any(|needle| compact.contains(needle))
}

/// GraphQL 엔드포인트는 fragment·자격 증명 query를 받지 않는다(v0.9.0 규칙).
pub(crate) fn graphql_endpoint(raw: &str, query: &[(String, String)]) -> Result<(), ApiError> {
    if raw.contains('#') {
        return Err(ApiError::url("fragment"));
    }
    let url = url::Url::parse(raw.trim()).map_err(|_| ApiError::url("parse"))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ApiError::url("userinfo"));
    }
    if url.query_pairs().any(|(name, _)| is_sensitive_name(&name))
        || query.iter().any(|(name, _)| is_sensitive_name(name))
    {
        return Err(ApiError::url("credential_query"));
    }
    Ok(())
}

pub(crate) fn timeout(ms: i64) -> Result<std::time::Duration, ApiError> {
    if !(1..=MAX_TIMEOUT_MS).contains(&ms) {
        return Err(ApiError::input("timeoutMs", "range"));
    }
    Ok(std::time::Duration::from_millis(ms as u64))
}

pub(crate) fn too_large(limit: usize) -> ApiError {
    ApiError::TooLarge {
        limit: limit as u64,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn tokens_and_field_values_follow_rfc_9110() {
        assert!(super::is_token("X-API-Key"));
        assert!(!super::is_token("X API Key"));
        assert!(!super::is_token("X:Key"));
        assert_eq!(super::field_value("  ok\t "), Ok("ok"));
        assert_eq!(super::field_value("a\tb"), Ok("a\tb"));
        assert_eq!(super::field_value("한글"), Ok("한글"));
        assert!(super::field_value("a\nb").is_err());
        assert!(super::field_value("a\u{7f}").is_err());
        assert!(super::method("GET").is_ok());
        assert!(super::method("BAD METHOD").is_err());
    }
}
