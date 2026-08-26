//! Tauri boundary for the local Life Log digest.
//!
//! The command only coordinates state access and the explicit Markdown save
//! action.  Aggregation, privacy, source validation, and output bounds live in
//! `core::digest`/`core::export`.

use crate::commands::tracking::AppState;
use crate::core::db;
use crate::core::digest::{self, DigestInput, DigestResponse};
use std::sync::Arc;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveDigestResult {
    pub saved: bool,
    pub byte_length: usize,
}

fn prepare(
    state: &tauri::State<'_, Arc<AppState>>,
    input: &DigestInput,
) -> Result<crate::core::export::PreparedExport, String> {
    let conn = state
        .db
        .lock()
        .map_err(|_| "digest 데이터를 잠글 수 없습니다".to_string())?;
    let projects = db::get_setting(&conn, "projects", "")
        .lines()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    digest::prepare(&conn, &projects, input)
}

async fn build_for_state(
    state: &tauri::State<'_, Arc<AppState>>,
    input: DigestInput,
) -> Result<DigestResponse, String> {
    let prepared = prepare(state, &input)?;
    digest::build_response(prepared, &input).await
}

/// Build a bounded, deterministic local digest.  This command has no file,
/// clipboard, history, network, or external-LLM side effect.
#[tauri::command]
pub async fn get_digest(
    state: tauri::State<'_, Arc<AppState>>,
    input: DigestInput,
) -> Result<DigestResponse, String> {
    build_for_state(&state, input).await
}

/// Save the already rendered digest only after the user confirms a native
/// Markdown save dialog.  Cancellation creates no file.
#[tauri::command]
pub async fn save_digest(
    state: tauri::State<'_, Arc<AppState>>,
    input: DigestInput,
) -> Result<SaveDigestResult, String> {
    let response = build_for_state(&state, input).await?;
    if !digest::validate_response(&response) {
        return Err("digest_output_invalid".into());
    }

    #[cfg(target_os = "windows")]
    {
        let Some(path) = choose_save_path() else {
            return Ok(SaveDigestResult {
                saved: false,
                byte_length: response.markdown.len(),
            });
        };
        validate_save_path(&path)?;
        devbox_filesystem::atomic_write(&path, response.markdown.as_bytes())
            .map_err(|_| "digest 파일을 저장하지 못했습니다".to_string())?;
        return Ok(SaveDigestResult {
            saved: true,
            byte_length: response.markdown.len(),
        });
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = response;
        Err("native digest 저장은 Windows 데스크톱에서 사용할 수 없습니다".into())
    }
}

#[cfg(target_os = "windows")]
fn choose_save_path() -> Option<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::UI::Controls::Dialogs::{
        GetSaveFileNameW, OFN_EXPLORER, OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST,
        OPENFILENAMEW,
    };

    let filter: Vec<u16> = "Markdown (*.md)\0*.md\0All files (*.*)\0*.*\0"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let title: Vec<u16> = "Life Log digest 저장"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let extension: Vec<u16> = "md".encode_utf16().chain(std::iter::once(0)).collect();
    let mut buffer = [0u16; 32_768];
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        nFilterIndex: 1,
        lpstrFile: PWSTR(buffer.as_mut_ptr()),
        nMaxFile: buffer.len() as u32,
        lpstrTitle: PCWSTR(title.as_ptr()),
        lpstrDefExt: PCWSTR(extension.as_ptr()),
        Flags: OFN_EXPLORER | OFN_NOCHANGEDIR | OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST,
        ..Default::default()
    };
    let opened = unsafe { GetSaveFileNameW(&mut dialog).as_bool() };
    if !opened {
        return None;
    }
    let length = buffer
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(buffer.len());
    if length == 0 {
        return None;
    }
    Some(std::ffi::OsString::from_wide(&buffer[..length]).into())
}

#[cfg(target_os = "windows")]
fn validate_save_path(path: &std::path::Path) -> Result<(), String> {
    use std::path::Component;
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        || path.file_name().is_none()
        || path.parent().is_none_or(|parent| !parent.is_dir())
        || path.to_str().is_none()
        || path.to_string_lossy().chars().any(char::is_control)
    {
        return Err("digest 저장 경로가 안전하지 않습니다".into());
    }
    if let Ok(metadata) = std::fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
            return Err("digest 저장 경로가 안전하지 않습니다".into());
        }
    }
    if path
        .extension()
        .and_then(|value| value.to_str())
        .is_none_or(|value| !value.eq_ignore_ascii_case("md"))
    {
        return Err("digest 저장 형식이 올바르지 않습니다".into());
    }
    Ok(())
}
