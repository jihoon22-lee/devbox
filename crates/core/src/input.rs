//! Pure command admission shared by execution domains.
pub const MAX_COMMAND_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CommandTextError {
    #[error("command contains a NUL byte")]
    ContainsNul,
    #[error("command exceeds {limit_bytes} UTF-8 bytes")]
    TooLong { limit_bytes: usize },
}

pub fn validate_command_text(command: &str) -> Result<(), CommandTextError> {
    if command.contains('\0') {
        return Err(CommandTextError::ContainsNul);
    }
    if command.len() > MAX_COMMAND_BYTES {
        return Err(CommandTextError::TooLong {
            limit_bytes: MAX_COMMAND_BYTES,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_length_uses_utf8_bytes_at_the_exact_boundary() {
        for command in [
            "x".repeat(MAX_COMMAND_BYTES),
            "😀".repeat(MAX_COMMAND_BYTES / 4),
        ] {
            assert_eq!(validate_command_text(&command), Ok(()));
            assert_eq!(
                validate_command_text(&(command + "x")),
                Err(CommandTextError::TooLong {
                    limit_bytes: MAX_COMMAND_BYTES,
                })
            );
        }
        assert_eq!(validate_command_text(""), Ok(()));
        assert_eq!(validate_command_text("echo a\necho b"), Ok(()));
    }

    #[test]
    fn nul_is_invalid_even_when_the_command_is_also_too_long() {
        for command in ["echo\0bad".into(), "\0".repeat(MAX_COMMAND_BYTES + 1)] {
            assert_eq!(
                validate_command_text(&command),
                Err(CommandTextError::ContainsNul)
            );
        }
    }
}
