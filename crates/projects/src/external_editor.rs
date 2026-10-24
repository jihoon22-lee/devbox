//! Fixed editor argv; the UI never supplies a program or shell command.
use crate::FilesError;
use devbox_protocol::Fail;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum SourceEditor {
    Code,
    Cursor,
    Zed,
}

pub fn editor_arguments(
    editor: SourceEditor,
    distro: Option<&str>,
    path: &str,
) -> Result<(&'static str, Vec<String>), Fail<FilesError>> {
    if !path.starts_with('/')
        || path.chars().any(char::is_control)
        || path.split('/').any(|s| matches!(s, "." | ".."))
    {
        return Err(FilesError::InvalidInput.into());
    }
    if matches!(editor, SourceEditor::Zed) {
        return Ok(("zed", vec![path.into()]));
    }
    let distro = distro
        .filter(|name| {
            !name.is_empty()
                && !name.chars().any(char::is_control)
                && !name.contains(['\\', '/', '"', ';'])
        })
        .ok_or(FilesError::EditorUnavailable)?;
    Ok((
        if matches!(editor, SourceEditor::Code) {
            "code"
        } else {
            "cursor"
        },
        vec!["--remote".into(), format!("wsl+{distro}"), path.into()],
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editor_paths_are_literal_and_only_fixed_programs_are_allowed() {
        let path = "/owned/가 & $(not-run); file.ts";
        for editor in [SourceEditor::Code, SourceEditor::Cursor] {
            let (program, args) = editor_arguments(editor, Some("Ubuntu"), path).expect("argv");
            assert!(matches!(program, "code" | "cursor"));
            assert_eq!(args, ["--remote", "wsl+Ubuntu", path]);
        }
        assert_eq!(
            editor_arguments(SourceEditor::Zed, None, path).expect("zed"),
            ("zed", vec![path.into()])
        );
        assert!(editor_arguments(SourceEditor::Code, None, path).is_err());
        for bad in ["--exec", "/a/../b", "/a\nb"] {
            assert!(editor_arguments(SourceEditor::Zed, None, bad).is_err());
        }
        assert!(serde_json::from_str::<SourceEditor>("\"sh\"").is_err());
    }
}
