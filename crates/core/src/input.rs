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

/// The exec wrapper counts every argv byte, including each argument's terminator.
pub fn validate_command_argv<'a>(
    arguments: impl IntoIterator<Item = &'a [u8]>,
) -> Result<(), CommandTextError> {
    let mut total = 0usize;
    for argument in arguments {
        if argument.contains(&0) {
            return Err(CommandTextError::ContainsNul);
        }
        total = total.saturating_add(argument.len()).saturating_add(1);
        if total > MAX_COMMAND_BYTES {
            return Err(CommandTextError::TooLong {
                limit_bytes: MAX_COMMAND_BYTES,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argv_budget_includes_all_arguments_and_terminators() {
        let exact = vec![b'x'; MAX_COMMAND_BYTES - 1];
        assert_eq!(validate_command_argv([exact.as_slice()]), Ok(()));
        assert!(matches!(
            validate_command_argv([b"a".as_slice(), exact.as_slice()]),
            Err(CommandTextError::TooLong { .. })
        ));
        assert_eq!(
            validate_command_argv([b"a\0b".as_slice()]),
            Err(CommandTextError::ContainsNul)
        );
    }
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
