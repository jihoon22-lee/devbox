use crate::{model::*, service::Result};
use devbox_protocol::Fail;
use serde::Serialize;
pub(crate) const INPUT_BYTES: usize = 64 * 1024;
// External create/send admissions retain the original serialized task budget.
pub(crate) const TASK_BYTES: usize = 512 * 1024;
// Lifecycle metadata may grow after admission without evicting accepted user history.
pub(crate) const STORED_TASK_BYTES: usize = TASK_BYTES + 32 * 1024;
pub(crate) const ATTENTION_BYTES: usize = 1024;
pub(crate) const IDENTITY_BYTES: usize = 128;
pub(crate) const PAGE_BYTES: usize = 768 * 1024;
/// Pure admission check shared with compound launch preflight.
pub fn input(text: &str) -> Result<()> {
    if text.len() > INPUT_BYTES {
        return Err(AgentsError::InputTooLarge {
            limit_bytes: INPUT_BYTES as u32,
        }
        .into());
    }
    Ok(())
}
// PR titles are independently editable argv values, not task-title slugs.
pub(crate) fn pr_title(title: &str) -> Result<()> {
    if title.len() > INPUT_BYTES {
        return Err(AgentsError::PrTitleTooLong {
            limit_bytes: INPUT_BYTES as u32,
        }
        .into());
    }
    if title.contains('\0') {
        return Err(AgentsError::InvalidInput.into());
    }
    Ok(())
}
pub(crate) fn squash_message(message: &str) -> Result<()> {
    if message.len() > INPUT_BYTES {
        return Err(AgentsError::SquashMessageTooLong {
            limit_bytes: INPUT_BYTES as u32,
        }
        .into());
    }
    if message.contains('\0') {
        return Err(AgentsError::InvalidInput.into());
    }
    Ok(())
}
pub(crate) fn teardown_commands(commands: &[String]) -> Result<()> {
    for command in commands {
        if command.len() > INPUT_BYTES {
            return Err(AgentsError::TeardownCommandTooLong {
                limit_bytes: INPUT_BYTES as u32,
            }
            .into());
        }
        if command.contains('\0') {
            return Err(AgentsError::TeardownCommandInvalid.into());
        }
    }
    Ok(())
}
pub(crate) fn serialized<T: Serialize>(value: &T, limit: usize) -> Result<()> {
    let bytes = serde_json::to_vec(value)
        .map_err(|e| Fail::Internal(format!("measure agent response: {e}")))?;
    if bytes.len() > limit {
        return Err(AgentsError::InputTooLarge {
            limit_bytes: limit as u32,
        }
        .into());
    }
    Ok(())
}
// IDs are exact correlation and filename components, never display excerpts.
pub(crate) fn identity(text: &str) -> Result<()> {
    if text.len() > IDENTITY_BYTES {
        return Err(AgentsError::InputTooLarge {
            limit_bytes: IDENTITY_BYTES as u32,
        }
        .into());
    }
    if text.is_empty()
        || !text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err(AgentsError::InvalidInput.into());
    }
    Ok(())
}
fn json_char_bytes(c: char) -> usize {
    match c {
        '"' | '\\' | '\u{8}' | '\t' | '\n' | '\u{c}' | '\r' => 2,
        '\u{0}'..='\u{1f}' => 6,
        _ => c.len_utf8(),
    }
}
/// Bound the actual JSON string, including quotes, and visibly mark omitted hook text.
/// Stop within the small display budget instead of serializing the full payload repeatedly.
pub(crate) fn attention_message(text: &str) -> String {
    const OMITTED: &str = "… (생략)";
    let mut used = 2;
    let mut end = 0;
    for (offset, c) in text.char_indices() {
        let size = json_char_bytes(c);
        if used + size > ATTENTION_BYTES {
            break;
        }
        used += size;
        end = offset + c.len_utf8();
    }
    let mut message = text[..end].to_owned();
    if end != text.len() {
        let suffix = OMITTED.chars().map(json_char_bytes).sum::<usize>();
        while used + suffix > ATTENTION_BYTES {
            used -= json_char_bytes(message.pop().expect("bounded prefix"));
        }
        message.push_str(OMITTED);
    }
    message
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hook_excerpt_matches_json_bytes_and_preserves_short_text() {
        for text in ["ordinary", "가😀", "\u{0}\u{8}\t\n\u{c}\r\u{1f}\"\\"] {
            assert_eq!(attention_message(text), text);
            let long = text.repeat(4096);
            let excerpt = attention_message(&long);
            assert!(serde_json::to_vec(&excerpt).expect("JSON excerpt").len() <= ATTENTION_BYTES);
            assert!(excerpt.ends_with("… (생략)"));
        }
    }
    #[test]
    fn input_uses_utf8_bytes_and_serialized_limit_includes_escaping() {
        assert!(input(&"가".repeat(INPUT_BYTES / 3)).is_ok());
        assert!(input(&"가".repeat(INPUT_BYTES / 3 + 1)).is_err());
        assert!(serialized(&"\u{0}".repeat(10), 20).is_err());
    }
}
