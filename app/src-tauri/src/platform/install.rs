use crate::core::{
    connection::IpcError,
    install::{WindowsPackage, winget_arguments},
};
use std::{os::windows::process::CommandExt, process::Command};
#[tauri::command]
pub fn install_windows_tool(package: WindowsPackage) -> Result<(), IpcError> {
    Command::new("winget.exe")
        .args(winget_arguments(package))
        .creation_flags(0x0000_0010)
        .spawn()
        .map_err(|_| {
            IpcError::new("Windows 설치 창을 열지 못했습니다. winget 설치를 확인해 주세요")
        })?;
    Ok(())
}
