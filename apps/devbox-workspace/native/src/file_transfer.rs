//! Bounded text paging inside an authenticated Files connection. File authority
//! remains with the caller; these tokens never grant access to another file.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, &'static str>;
pub const CHUNK_BYTES: usize = 1024 * 1024;
pub const INLINE_BYTES: usize = 5 * 1024 * 1024;
// Invalid single input bytes can each decode to a three-byte replacement char.
pub const MAX_TEXT_BYTES: usize = code_pad_lib::core::guard::MAX_OPENABLE_BYTES as usize * 3;

fn digest(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Descriptor {
    token: String,
    bytes: usize,
    sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Chunk {
    token: String,
    offset: usize,
    text: String,
    done: bool,
}

pub struct Pending {
    pub path: String,
    pub revision: String,
    descriptor: Descriptor,
    text: String,
    offset: usize,
    created: Instant,
}
impl Pending {
    pub fn new(path: String, revision: String, text: String) -> Result<Self> {
        if text.len() <= INLINE_BYTES || text.len() > MAX_TEXT_BYTES {
            return Err("file_transfer_invalid");
        }
        Ok(Self {
            path,
            revision,
            descriptor: Descriptor {
                token: uuid::Uuid::new_v4().to_string(),
                bytes: text.len(),
                sha256: digest(&text),
            },
            text,
            offset: 0,
            created: Instant::now(),
        })
    }
    pub fn descriptor(&self) -> Result<Value> {
        serde_json::to_value(&self.descriptor).map_err(|_| "file_transfer_invalid")
    }
    pub fn expired(&self) -> bool {
        self.created.elapsed() >= Duration::from_secs(30)
    }
    pub fn next(&mut self, token: &str, offset: usize) -> Result<(Value, bool)> {
        if token != self.descriptor.token
            || offset != self.offset
            || offset >= self.text.len()
            || self.expired()
        {
            return Err("file_transfer_stale");
        }
        let mut end = (offset + CHUNK_BYTES).min(self.text.len());
        while !self.text.is_char_boundary(end) {
            end -= 1;
        }
        let done = end == self.text.len();
        let value = serde_json::to_value(Chunk {
            token: token.to_owned(),
            offset,
            text: self.text[offset..end].to_owned(),
            done,
        })
        .map_err(|_| "file_transfer_invalid")?;
        self.offset = end;
        Ok((value, done))
    }
}

/// Assemble only an explicitly marked native response. The caller keeps the
/// connection lock and original deadline throughout every chunk request.
pub fn receive(
    mut opened: Value,
    mut next: impl FnMut(&str, usize) -> Result<Value>,
) -> Result<Value> {
    let Some(descriptor) = opened
        .as_object_mut()
        .and_then(|o| o.remove("textTransfer"))
    else {
        return Ok(opened);
    };
    let descriptor: Descriptor =
        serde_json::from_value(descriptor).map_err(|_| "file_transfer_invalid")?;
    if !crate::token(&descriptor.token)
        || descriptor.bytes <= INLINE_BYTES
        || descriptor.bytes > MAX_TEXT_BYTES
        || descriptor.sha256.len() != 64
        || !descriptor
            .sha256
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        || opened.get("text").and_then(Value::as_str) != Some("")
    {
        return Err("file_transfer_invalid");
    }
    let mut text = String::with_capacity(descriptor.bytes);
    while text.len() < descriptor.bytes {
        let chunk: Chunk = serde_json::from_value(next(&descriptor.token, text.len())?)
            .map_err(|_| "file_transfer_invalid")?;
        if chunk.token != descriptor.token
            || chunk.offset != text.len()
            || chunk.text.is_empty()
            || chunk.text.len() > CHUNK_BYTES
            || chunk.text.len() > descriptor.bytes - text.len()
            || chunk.done != (text.len() + chunk.text.len() == descriptor.bytes)
        {
            return Err("file_transfer_invalid");
        }
        text.push_str(&chunk.text);
    }
    if digest(&text) != descriptor.sha256 {
        return Err("file_transfer_invalid");
    }
    opened["text"] = Value::String(text);
    Ok(opened)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pending() -> Pending {
        Pending::new(
            "/fixture".into(),
            "revision".into(),
            "한글\u{1}\\\"".repeat(INLINE_BYTES / 6),
        )
        .unwrap()
    }
    #[test]
    fn utf8_boundaries_and_json_expansion_preserve_exact_text() {
        let mut pending = pending();
        let expected = pending.text.clone();
        let opened = json!({"text":"", "nativeRevision":"revision", "textTransfer":pending.descriptor().unwrap()});
        let result = receive(opened, |token, offset| {
            let (chunk, _) = pending.next(token, offset)?;
            let mut frame = Vec::new();
            crate::write_frame(&mut frame, &chunk).unwrap();
            assert!(frame.len() < CHUNK_BYTES * 6 + 1024);
            Ok(crate::read_frame(&mut frame.as_slice()).unwrap().unwrap())
        })
        .unwrap();
        assert_eq!(result["text"], expected);
        assert!(result.get("textTransfer").is_none());
        assert_eq!(result["nativeRevision"], "revision");
        assert!(pending.next(&pending.descriptor.token.clone(), 0).is_err());
    }
    #[test]
    fn forged_reordered_truncated_and_corrupt_chunks_never_publish_text() {
        for variant in 0..5 {
            let mut pending = pending();
            let opened = json!({"text":"", "textTransfer":pending.descriptor().unwrap()});
            let result = receive(opened, |token, offset| {
                let (mut chunk, _) = pending.next(token, offset)?;
                match variant {
                    0 => chunk["offset"] = json!(offset + 1),
                    1 => chunk["token"] = json!(uuid::Uuid::new_v4().to_string()),
                    2 => chunk["done"] = json!(true),
                    3 => chunk["text"] = json!(""),
                    // Same byte count reaches the final whole-text hash check.
                    _ => chunk["text"] = json!("x".repeat(chunk["text"].as_str().unwrap().len())),
                }
                Ok(chunk)
            });
            assert!(result.is_err(), "variant {variant}");
        }
        let mut pending = pending();
        pending.created -= Duration::from_secs(31);
        assert!(pending.next(&pending.descriptor.token.clone(), 0).is_err());
        let mut descriptor = pending.descriptor().unwrap();
        descriptor["bytes"] = json!(MAX_TEXT_BYTES + 1);
        assert!(
            receive(json!({"text":"","textTransfer":descriptor}), |_, _| panic!(
                "invalid length must fail before request/allocation"
            ))
            .is_err()
        );
    }
}
