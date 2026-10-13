use crate::{model::*, service::Result};
use devbox_protocol::Fail;
use serde::Serialize;
pub(crate) const INPUT_BYTES: usize = 64 * 1024;
pub(crate) const TASK_BYTES: usize = 512 * 1024;
pub(crate) const PAGE_BYTES: usize = 768 * 1024;
pub(crate) fn input(text: &str) -> Result<()> {
    if text.len() > INPUT_BYTES {
        return Err(AgentsError::InputTooLarge {
            limit_bytes: INPUT_BYTES as u32,
        }
        .into());
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
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn input_uses_utf8_bytes_and_serialized_limit_includes_escaping() {
        assert!(input(&"가".repeat(INPUT_BYTES / 3)).is_ok());
        assert!(input(&"가".repeat(INPUT_BYTES / 3 + 1)).is_err());
        assert!(serialized(&"\u{0}".repeat(10), 20).is_err());
    }
}
