use super::{
    connection::IpcError,
    external::{ExternalKind, arguments},
};
use serde::Deserialize;
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceEditor {
    Code,
    Cursor,
    Zed,
}
pub fn source_arguments(
    editor: SourceEditor,
    distro: &str,
    path: &str,
) -> Result<Vec<String>, IpcError> {
    arguments(
        if matches!(editor, SourceEditor::Zed) {
            ExternalKind::Explorer
        } else {
            ExternalKind::Vscode
        },
        distro,
        path,
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_fixed_editors_and_literal_wsl_paths_are_accepted() {
        let path = "/owned/가 & $(x); file.ts";
        for editor in [SourceEditor::Code, SourceEditor::Cursor] {
            assert_eq!(
                source_arguments(editor, "Ubuntu", path).unwrap(),
                ["--remote", "wsl+Ubuntu", path]
            );
        }
        assert_eq!(
            source_arguments(SourceEditor::Zed, "Ubuntu", path).unwrap(),
            [r"\\wsl.localhost\Ubuntu\owned\가 & $(x); file.ts"]
        );
        for path in ["--command", "/x/../y", "/x\ny"] {
            assert!(source_arguments(SourceEditor::Zed, "Ubuntu", path).is_err());
        }
        assert!(serde_json::from_str::<SourceEditor>("\"powershell\"").is_err());
    }
}
