use super::{connection::Connection, external::launch_editor};
use crate::core::{
    connection::IpcError,
    external::EditorKind,
    source_external::{SourceEditor, source_arguments},
};
use std::{
    os::windows::process::CommandExt,
    path::PathBuf,
    process::{Command, Stdio},
};
#[tauri::command]
pub fn open_external_editor(
    editor: SourceEditor,
    path: String,
    conn: tauri::State<'_, Connection>,
) -> Result<(), IpcError> {
    let distro = conn.state().ready_distro()?.to_owned();
    let args = source_arguments(editor, &distro, &path)?;
    match editor {
        SourceEditor::Code => launch_editor(EditorKind::Vscode, args),
        SourceEditor::Cursor => launch_editor(EditorKind::Cursor, args),
        SourceEditor::Zed => {
            let mut candidates: Vec<PathBuf> = std::env::var_os("PATH")
                .map(|path| {
                    std::env::split_paths(&path)
                        .map(|path| path.join("zed.exe"))
                        .collect()
                })
                .unwrap_or_default();
            if let Some(root) = std::env::var_os("LOCALAPPDATA") {
                candidates.push(PathBuf::from(root).join("Programs/Zed/zed.exe"));
            }
            let exe = candidates
                .into_iter()
                .find(|path| path.is_absolute() && path.is_file())
                .ok_or_else(|| IpcError::new("Zed 설치 경로를 찾지 못했습니다"))?;
            Command::new(exe)
                .args(args)
                .creation_flags(0x0800_0000)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| IpcError::new("Zed를 시작하지 못했습니다. 설치를 확인해 주세요"))?;
            Ok(())
        }
    }
}
