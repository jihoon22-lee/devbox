use std::path::{Path, PathBuf};
pub fn file_uri(path: &Path) -> Option<String> {
    let text = path.to_str()?;
    if !valid_path(text) {
        return None;
    }
    let mut uri = String::from("file://");
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    Some(uri)
}
fn valid_path(text: &str) -> bool {
    text.starts_with('/')
        && !text.contains('\\')
        && !text.chars().any(char::is_control)
        && !text.split('/').any(|part| part == "." || part == "..")
}
pub fn file_path(uri: &str) -> Option<PathBuf> {
    let text = uri.strip_prefix("file://")?;
    if text.contains(['?', '#']) || !text.starts_with('/') || text.len() > 16384 {
        return None;
    }
    let mut bytes = Vec::new();
    let mut input = text.bytes();
    while let Some(b) = input.next() {
        if b != b'%' {
            bytes.push(b);
            continue;
        }
        let a = (input.next()? as char).to_digit(16)?;
        let b = (input.next()? as char).to_digit(16)?;
        let decoded = (a * 16 + b) as u8;
        if decoded == b'/' || decoded == b'\\' {
            return None;
        }
        bytes.push(decoded);
    }
    let text = String::from_utf8(bytes).ok()?;
    valid_path(&text).then(|| PathBuf::from(text))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_uris_encode_unicode_and_reject_other_authorities() {
        let p = Path::new("/owned/가 😀#%.ts");
        let uri = file_uri(p).expect("uri");
        assert!(uri.contains("%20") && uri.contains("%23") && uri.contains("%25"));
        assert_eq!(file_path(&uri).as_deref(), Some(p));
        for uri in [
            "file://host/a",
            "file:///a/../b",
            "file:///a%2fb",
            "file:///a?b",
            "file:///a#b",
            "https://x/a",
            "file:///a%00b",
            "file:///a/%2e%2e/b",
        ] {
            assert!(file_path(uri).is_none(), "{uri}");
        }
    }
}

/// Canonicalize LSP locations and workspace-edit targets through daemon authority.
/// Text and opaque data remain opaque; there is no Windows URI translation in a renderer.
pub(crate) fn normalize<'a>(
    value: &'a mut serde_json::Value,
    access: &'a crate::adapters::DocumentAccess,
    depth: usize,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), crate::LspError>> + Send + 'a>> {
    Box::pin(async move {
        use serde_json::Value;
        if depth > 32 {
            return Err(crate::LspError::Protocol);
        }
        match value {
            Value::Object(map) => {
                for (key, value) in map {
                    if ["uri", "targetUri", "oldUri", "newUri"].contains(&key.as_str()) {
                        if let Some(uri) = value.as_str() {
                            *value = Value::String(access(uri.into(), None).await?.uri);
                        }
                    } else if key == "changes" && value.is_object() {
                        let old = value.as_object().expect("object").clone();
                        let mut changed = serde_json::Map::new();
                        for (uri, mut edits) in old {
                            let canonical = access(uri, None).await?.uri;
                            normalize(&mut edits, access, depth + 1).await?;
                            if changed.insert(canonical, edits).is_some() {
                                return Err(crate::LspError::Protocol);
                            }
                        }
                        *value = Value::Object(changed);
                    } else {
                        normalize(value, access, depth + 1).await?;
                    }
                }
            }
            Value::Array(items) => {
                for item in items {
                    normalize(item, access, depth + 1).await?;
                }
            }
            _ => {}
        }
        Ok(())
    })
}
