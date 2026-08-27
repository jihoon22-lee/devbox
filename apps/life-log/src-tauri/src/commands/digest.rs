//! Tauri boundary for the local Life Log digest.
//!
//! The command only coordinates state access and the explicit Markdown save
//! action.  Aggregation, privacy, source validation, and output bounds live in
//! `core::digest`/`core::export`.

use crate::commands::tracking::AppState;
use crate::core::db;
use crate::core::digest::{self, DigestInput, DigestResponse};
use serde::Deserialize;
use std::sync::Arc;
use tokio::time::{sleep, Duration, Instant};

const DIGEST_CANCEL_WAIT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveDigestResult {
    pub saved: bool,
    pub byte_length: usize,
}

fn prepare(
    state: &tauri::State<'_, Arc<AppState>>,
    input: &DigestInput,
    cancellation: Arc<std::sync::atomic::AtomicBool>,
) -> Result<crate::core::export::PreparedExport, String> {
    let conn = state
        .db
        .lock()
        .map_err(|_| "digest 데이터를 잠글 수 없습니다".to_string())?;
    let raw_projects = db::get_setting_bounded(
        &conn,
        "projects",
        "",
        crate::core::export::MAX_PROJECT_SETTING_BYTES,
    )?;
    let projects = crate::core::export::parse_project_setting(&raw_projects)?;
    digest::prepare_with_cancel(&conn, &projects, input, cancellation)
}

pub(crate) async fn build_for_state(
    state: &tauri::State<'_, Arc<AppState>>,
    input: DigestInput,
    cancellation: Arc<std::sync::atomic::AtomicBool>,
) -> Result<DigestResponse, String> {
    let prepared = prepare(state, &input, Arc::clone(&cancellation))?;
    digest::build_response_with_cancel(prepared, &input, cancellation).await
}

/// Build a bounded, deterministic local digest.  This command has no file,
/// clipboard, history, network, or external-LLM side effect.
#[tauri::command]
pub async fn get_digest(
    state: tauri::State<'_, Arc<AppState>>,
    input: DigestInput,
) -> Result<DigestResponse, String> {
    let operation = state.digest_operations.begin()?;
    let cancellation = operation.cancellation();
    let response = build_for_state(&state, input, cancellation).await?;
    if operation.is_cancelled() {
        return Err("digest_cancelled".into());
    }
    let issued = state.digest_handles.issue(response)?;
    if operation.is_cancelled() {
        return Err("digest_cancelled".into());
    }
    Ok(issued)
}

/// Cancel the currently running native digest. Cancellation is cooperative:
/// the DB progress hook and Git child observe the same generation token, and
/// the single-flight guard remains held until both have stopped.
#[tauri::command]
pub async fn cancel_digest(state: tauri::State<'_, Arc<AppState>>) -> Result<bool, String> {
    let generation = state.digest_operations.cancel_generation();
    let Some(generation) = generation else {
        // `cancel_generation` returns None both when there is no active
        // operation and when the state lock is poisoned. Treat the latter as
        // a hard failure so a caller can never start a new generation while
        // the old one may still own the single-flight slot.
        if state.digest_operations.is_active() {
            return Err("digest_cancel_timeout".into());
        }
        return Ok(false);
    };
    let deadline = Instant::now() + DIGEST_CANCEL_WAIT;
    while state.digest_operations.is_active_generation(generation) && Instant::now() < deadline {
        sleep(Duration::from_millis(5)).await;
    }
    if state.digest_operations.is_active_generation(generation) {
        Err("digest_cancel_timeout".into())
    } else {
        Ok(true)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveDigestRequest {
    pub handle: String,
}

/// Save the already rendered digest only after the user confirms a native
/// Markdown save dialog.  Cancellation creates no file.
#[tauri::command]
pub async fn save_digest(
    state: tauri::State<'_, Arc<AppState>>,
    request: SaveDigestRequest,
) -> Result<SaveDigestResult, String> {
    let operation = state.digest_operations.begin()?;
    let response = state.digest_handles.get(&request.handle)?;
    if operation.is_cancelled() {
        return Err("digest_cancelled".into());
    }
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
        if operation.is_cancelled() {
            return Err("digest_cancelled".into());
        }
        validate_save_path(&path)?;
        operation.commit_if_not_cancelled(|| {
            devbox_filesystem::atomic_write(&path, response.markdown.as_bytes())
                .map_err(|_| "digest 파일을 저장하지 못했습니다".to_string())
        })?;
        Ok(SaveDigestResult {
            saved: true,
            byte_length: response.markdown.len(),
        })
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
