use crate::TerminalError;
use devbox_protocol::Fail;

pub const MAX_COMMAND_BYTES: usize = 64 * 1024;
pub(crate) const COMMAND_PREVIEW_CHARS: usize = 256;
pub(crate) const PAGE_BYTES: usize = 768 * 1024;
pub(crate) const PAGE_ROWS: usize = 256;
pub(crate) const CURSOR_BYTES: usize = 128;

/// Shared by terminal admission and the whole-project launch preflight.
pub fn validate_command(command: &str) -> Result<(), Fail<TerminalError>> {
    if command.contains('\0') {
        return Err(TerminalError::CommandInvalid.into());
    }
    if command.len() > MAX_COMMAND_BYTES {
        return Err(TerminalError::CommandTooLong {
            limit_bytes: MAX_COMMAND_BYTES as u32,
        }
        .into());
    }
    Ok(())
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
