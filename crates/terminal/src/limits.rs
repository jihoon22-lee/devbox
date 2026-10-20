use crate::TerminalError;
use devbox_core::input::{CommandTextError, validate_command_text};
use devbox_protocol::Fail;

pub use devbox_core::input::MAX_COMMAND_BYTES;
pub(crate) const COMMAND_PREVIEW_CHARS: usize = 256;
pub(crate) const PAGE_BYTES: usize = 768 * 1024;
pub(crate) const PAGE_ROWS: usize = 256;
pub(crate) const CURSOR_BYTES: usize = 128;

/// Shared by terminal admission and the whole-project launch preflight.
pub fn validate_command(command: &str) -> Result<(), Fail<TerminalError>> {
    validate_command_text(command).map_err(|error| match error {
        CommandTextError::ContainsNul => TerminalError::CommandInvalid.into(),
        CommandTextError::TooLong { limit_bytes } => TerminalError::CommandTooLong {
            limit_bytes: limit_bytes as u32,
        }
        .into(),
    })
}

/// List display only. The executed command and stored record remain complete.
pub(crate) fn command_preview(command: &str) -> String {
    let mut chars = command.chars();
    let mut preview: String = chars.by_ref().take(COMMAND_PREVIEW_CHARS).collect();
    if chars.next().is_some() {
        preview.push_str("… (생략)");
    }
    preview
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_errors_keep_terminal_codes_and_nul_precedence() {
        assert!(matches!(
            validate_command(&"\0".repeat(MAX_COMMAND_BYTES + 1)),
            Err(Fail::Domain(TerminalError::CommandInvalid))
        ));
        assert!(matches!(
            validate_command(&"x".repeat(MAX_COMMAND_BYTES + 1)),
            Err(Fail::Domain(TerminalError::CommandTooLong { limit_bytes }))
                if limit_bytes == 65_536
        ));
    }
}
