use super::connection::Connection;
use crate::core::{
    connection::IpcError,
    external::{self, ExternalKind},
};
use std::{
    os::windows::process::CommandExt,
    path::PathBuf,
    process::{Command, Stdio},
};

#[tauri::command]
pub fn open_external(
    kind: ExternalKind,
    path: String,
    conn: tauri::State<'_, Connection>,
) -> Result<(), IpcError> {
    // Readiness and distro are owned by the current validated bridge generation.
    // The renderer cannot supply an arbitrary distro or executable.
    let distro = conn.state().ready_distro()?.to_owned();
    let args = external::arguments(kind, &distro, &path)?;
    let mut command = match kind {
        ExternalKind::Vscode => {
            let (exe, cli) =
                vscode().ok_or_else(|| IpcError::new("VS Code 설치 경로를 찾지 못했습니다"))?;
            // Official code.cmd runs Code.exe cli.js with this environment.
            // Invoke it directly so paths never pass through cmd.exe expansion.
            let mut c = Command::new(exe);
            c.arg(cli)
                .env("ELECTRON_RUN_AS_NODE", "1")
                .env_remove("VSCODE_DEV");
            c
        }
        ExternalKind::Explorer => {
            let root = std::env::var_os("SystemRoot")
                .ok_or_else(|| IpcError::new("Windows 경로를 찾지 못했습니다"))?;
            Command::new(PathBuf::from(root).join("explorer.exe"))
        }
        ExternalKind::Terminal => Command::new("wt.exe"),
    };
    command
        .args(args)
        .creation_flags(0x0800_0000)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| IpcError::new("외부 앱을 시작하지 못했습니다. 앱 설치를 확인해 주세요"))?;
    // Windows closes the process handles when Child drops; no Unix zombie process.
    Ok(())
}

fn vscode() -> Option<(PathBuf, PathBuf)> {
    let mut candidates: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    for (variable, relative) in [
        ("LOCALAPPDATA", "Programs/Microsoft VS Code/bin"),
        ("ProgramFiles", "Microsoft VS Code/bin"),
        ("ProgramFiles(x86)", "Microsoft VS Code/bin"),
    ] {
        if let Some(root) = std::env::var_os(variable) {
            candidates.push(PathBuf::from(root).join(relative));
        }
    }
    candidates
        .into_iter()
        .find_map(|path| external::vscode_cli(&path))
}

#[tauri::command]
pub fn open_file_location(
    editor: external::EditorKind,
    path: String,
    line: u32,
    column: u32,
    conn: tauri::State<'_, Connection>,
) -> Result<(), IpcError> {
    let distro = conn.state().ready_distro()?.to_owned();
    let args = external::file_arguments(&distro, &path, line, column)?;
    launch_editor(editor, args)
}

pub(super) fn launch_editor(
    editor: external::EditorKind,
    args: Vec<String>,
) -> Result<(), IpcError> {
    let (exe, cli) = match editor {
        external::EditorKind::Vscode => vscode(),
        external::EditorKind::Cursor => {
            let mut candidates: Vec<PathBuf> = std::env::var_os("PATH")
                .map(|p| std::env::split_paths(&p).collect())
                .unwrap_or_default();
            for (variable, relative) in [
                ("LOCALAPPDATA", "Programs/cursor/resources/app/bin"),
                ("LOCALAPPDATA", "Programs/cursor/bin"),
                ("ProgramFiles", "cursor/bin"),
            ] {
                if let Some(root) = std::env::var_os(variable) {
                    candidates.push(PathBuf::from(root).join(relative));
                }
            }
            candidates
                .into_iter()
                .find_map(|bin| external::editor_cli(&bin, editor))
        }
    }
    .ok_or_else(|| IpcError::new("편집기 설치 경로를 찾지 못했습니다"))?;
    Command::new(exe)
        .arg(cli)
        .args(args)
        .env("ELECTRON_RUN_AS_NODE", "1")
        .env_remove("VSCODE_DEV")
        .creation_flags(0x0800_0000)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| IpcError::new("편집기를 시작하지 못했습니다. 앱 설치를 확인해 주세요"))?;
    Ok(())
}
