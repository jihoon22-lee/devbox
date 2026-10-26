//! Only recognized actual header fields contribute inferred credential values.
use super::seeds::Seeds;
use crate::ApiError;
use zeroize::Zeroizing;
pub(crate) fn sensitive_header(name: &str) -> bool {
    [
        "authorization",
        "proxy-authorization",
        "x-api-key",
        "api-key",
        "x-auth-token",
        "cookie",
        "set-cookie",
    ]
    .iter()
    .any(|known| name.eq_ignore_ascii_case(known))
}
fn trim(mut value: &[u8]) -> &[u8] {
    while value.first().is_some_and(u8::is_ascii_whitespace) {
        value = &value[1..];
    }
    while value.last().is_some_and(u8::is_ascii_whitespace) {
        value = &value[..value.len() - 1];
    }
    value
}
fn semantic_cookie(value: &[u8]) -> &[u8] {
    let value = trim(value);
    if value.len() >= 2 && value[0] == b'"' && value[value.len() - 1] == b'"' {
        &value[1..value.len() - 1]
    } else {
        value
    }
}
fn cookie_value(seeds: &mut Seeds, value: &[u8]) -> Result<bool, ApiError> {
    let value = trim(value);
    if semantic_cookie(value).is_empty() {
        return Ok(false);
    }
    let mut changed = seeds.add_cookie(value)?;
    if value.len() >= 2 && value[0] == b'"' && value[value.len() - 1] == b'"' {
        changed |= seeds.add_cookie(&value[1..value.len() - 1])?;
    }
    Ok(changed)
}
pub(super) fn request(seeds: &mut Seeds, name: &str, value: &[u8]) -> Result<bool, ApiError> {
    if name.eq_ignore_ascii_case("cookie") {
        let mut changed = false;
        for part in value.split(|b| *b == b';') {
            if let Some(at) = part.iter().position(|b| *b == b'=') {
                let v = trim(&part[at + 1..]);
                changed |= cookie_value(seeds, v)?;
            } else if !trim(part).is_empty() {
                changed |= seeds.add_cookie(trim(part))?;
            }
        }
        return Ok(changed);
    }
    if name.eq_ignore_ascii_case("authorization")
        || name.eq_ignore_ascii_case("proxy-authorization")
    {
        let mut changed = seeds.add(value)?;
        let value = trim(value);
        if let Some(at) = value.iter().position(u8::is_ascii_whitespace) {
            let credential = trim(&value[at..]);
            changed |= seeds.add(credential)?;
            if value[..at].eq_ignore_ascii_case(b"basic")
                && let Ok(decoded) = data_encoding::BASE64.decode(credential)
            {
                let decoded = Zeroizing::new(decoded);
                changed |= seeds.add(&decoded)?;
                if let Some(colon) = decoded.iter().position(|b| *b == b':') {
                    changed |= seeds.add(&decoded[colon + 1..])?;
                }
            }
        }
        return Ok(changed);
    }
    if ["x-api-key", "api-key", "x-auth-token"]
        .iter()
        .any(|n| name.eq_ignore_ascii_case(n))
    {
        return seeds.add(value);
    }
    Ok(false)
}
pub(super) fn response(seeds: &mut Seeds, name: &str, value: &[u8]) -> Result<bool, ApiError> {
    if !name.eq_ignore_ascii_case("set-cookie") {
        return Ok(false);
    }
    let pair = value.split(|b| *b == b';').next().unwrap_or_default();
    if let Some(at) = pair.iter().position(|b| *b == b'=') {
        cookie_value(seeds, &pair[at + 1..])
    } else {
        seeds.add_cookie(trim(pair))
    }
}
