//! A narrow fail-closed guard for raw request bodies, including decoded base64.
//! Never rewrite collection bytes: callers retain their original digest and unknown fields.
use crate::{ApiError, REDACTED, template::is_reference};
use serde::{
    Deserialize,
    de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor},
};
use std::fmt;

#[derive(Deserialize)]
struct Document {
    #[serde(default)]
    request: Option<Request>,
}
#[derive(Deserialize)]
struct Request {
    #[serde(default)]
    body: Option<RawBody>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawBody {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    content_type: Option<String>,
    #[serde(default)]
    encoding: Option<String>,
}
fn reject() -> ApiError {
    ApiError::StorageSensitiveBody
}
fn sensitive(name: &str) -> bool {
    let name = name.to_ascii_lowercase().replace(['_', '-'], "");
    [
        "authorization",
        "cookie",
        "apikey",
        "token",
        "secret",
        "password",
        "passwd",
        "privatekey",
    ]
    .iter()
    .any(|s| name.contains(s))
}
fn safe(value: &str) -> bool {
    value.is_empty() || value == REDACTED || (is_reference(value) && !known_token(value))
}
fn known_token(text: &str) -> bool {
    // Match prefixed tokens anywhere, but keep JWTs to a maximal token-alphabet run.
    // The frontend uses the same grammar without an unanchored backtracking JWT regex.
    static PREFIX: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static JWT: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let prefixed = PREFIX.get_or_init(|| regex::Regex::new(
        r"(?P<reference>\{\{secret:[A-Za-z_][A-Za-z0-9_]{0,127}\}\})|(?P<secret>(?:sk[_-]|ghp_|github_pat_|glpat-|xox[baprs]-)[A-Za-z0-9_.-]{12,}|AKIA[A-Z0-9]{16}|-----BEGIN [A-Z ]*PRIVATE KEY-----)"
    ).expect("constant stored-token pattern"));
    let jwt = JWT.get_or_init(|| regex::Regex::new(
        r"(?:^|[^A-Za-z0-9_.-])[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}(?:$|[^A-Za-z0-9_.-])"
    ).expect("constant JWT pattern"));
    prefixed
        .captures_iter(text)
        .any(|matched| matched.name("secret").is_some())
        || jwt.is_match(text)
}
/// Inspect every occurrence, including duplicate JSON keys, before serde could discard any value.
struct JsonSecrets(bool);
impl<'de> DeserializeSeed<'de> for JsonSecrets {
    type Value = bool;
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<bool, D::Error> {
        d.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for JsonSecrets {
    type Value = bool;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("JSON data")
    }
    fn visit_str<E: de::Error>(self, s: &str) -> Result<bool, E> {
        Ok((self.0 && !safe(s)) || known_token(s))
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> Result<bool, E> {
        Ok(self.0)
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> Result<bool, E> {
        Ok(self.0)
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> Result<bool, E> {
        Ok(self.0)
    }
    fn visit_f64<E: de::Error>(self, _: f64) -> Result<bool, E> {
        Ok(self.0)
    }
    fn visit_unit<E: de::Error>(self) -> Result<bool, E> {
        Ok(false)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<bool, A::Error> {
        let mut unsafe_value = false;
        while let Some(v) = seq.next_element_seed(JsonSecrets(self.0))? {
            unsafe_value |= v;
        }
        Ok(unsafe_value)
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<bool, A::Error> {
        let mut unsafe_value = false;
        while let Some(key) = map.next_key::<String>()? {
            unsafe_value |= known_token(&key);
            unsafe_value |= map.next_value_seed(JsonSecrets(self.0 || sensitive(&key)))?;
        }
        Ok(unsafe_value)
    }
}
fn inspect(text: &str, mime: &str) -> Result<(), ApiError> {
    if safe(text) {
        return Ok(());
    }
    let mut deserializer = serde_json::Deserializer::from_str(text.trim_start_matches('\u{feff}'));
    let json = JsonSecrets(false)
        .deserialize(&mut deserializer)
        .and_then(|unsafe_value| deserializer.end().map(|_| unsafe_value));
    match json {
        Ok(true) => return Err(reject()),
        Ok(false) => return Ok(()),
        Err(_)
            if mime.ends_with("/json")
                || mime.ends_with("+json")
                || text.trim_start().starts_with(['{', '[']) =>
        {
            return Err(reject());
        }
        Err(_) => {}
    }
    if known_token(text) {
        return Err(reject());
    }
    for (name, value) in url::form_urlencoded::parse(text.as_bytes()) {
        if (sensitive(&name) && !safe(&value)) || known_token(&value) {
            return Err(reject());
        }
    }
    // A malformed JSON fragment must not evade inspection by declaring itself plain text.
    if sensitive(text) && crate::history::sanitize::json(text) != text {
        return Err(reject());
    }
    Ok(())
}
pub(crate) fn validate(bytes: &[u8]) -> Result<(), ApiError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| reject())?
        .trim_start_matches('\u{feff}');
    // Duplicate known envelope fields and malformed collection JSON are rejected, never normalized.
    let document: Document = serde_json::from_str(text).map_err(|_| reject())?;
    let Some(body) = document
        .request
        .and_then(|r| r.body)
        .filter(|b| b.kind == "raw")
    else {
        return Ok(());
    };
    let mime = body
        .content_type
        .as_deref()
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match body.encoding.as_deref() {
        None => inspect(&body.text, &mime),
        Some("base64") => {
            let bytes = data_encoding::BASE64
                .decode(body.text.as_bytes())
                .map_err(|_| ApiError::body("invalid_base64"))?;
            if (mime.ends_with("/json")
                || mime.ends_with("+json")
                || mime == "application/x-www-form-urlencoded")
                && std::str::from_utf8(&bytes).is_err()
            {
                return Err(reject());
            }
            inspect(&String::from_utf8_lossy(&bytes), &mime)
        }
        Some(_) => Err(ApiError::body("unsupported_encoding")),
    }
}
