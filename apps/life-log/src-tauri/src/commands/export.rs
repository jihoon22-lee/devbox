//! Life Log export command boundary.
//!
//! Data preparation/formatting lives in `core::export`. This module owns only
//! Tauri state access and the explicit native save action. The Windows dialog
//! uses the already bundled `windows` crate; no runtime download or new plugin
//! is required.

use crate::commands::tracking::AppState;
use crate::core::db;
use crate::core::export::{self, ExportInput, RenderedExport};
#[cfg(any(target_os = "windows", test))]
use crate::core::export::{ExportDocument, ExportOrigin};
use serde::Serialize;
use std::sync::Arc;

#[cfg(target_os = "windows")]
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveExportResult {
    pub saved: bool,
    pub format: export::ExportFormat,
    pub byte_length: usize,
}

fn prepare(
    state: &tauri::State<'_, Arc<AppState>>,
    input: &ExportInput,
) -> Result<export::PreparedExport, String> {
    let conn = state
        .db
        .lock()
        .map_err(|_| "Life Log DB를 잠글 수 없습니다".to_string())?;
    let raw_projects =
        db::get_setting_bounded(&conn, "projects", "", export::MAX_PROJECT_SETTING_BYTES)?;
    let projects = export::parse_project_setting(&raw_projects)?;
    export::prepare_document(&conn, &projects, input)
}

async fn render_for_state(
    state: &tauri::State<'_, Arc<AppState>>,
    input: ExportInput,
) -> Result<RenderedExport, String> {
    let prepared = prepare(state, &input)?;
    let document = export::build_document(prepared).await?;
    export::render(&document, input.format)
}

/// Validate the in-memory artifact once more immediately before a native
/// write. This is deliberately independent of the renderer so a future
/// renderer change cannot make the save path accept a malformed/truncated
/// payload. All failures collapse to a stable code and never include a path
/// or parser/OS detail.
#[cfg(any(target_os = "windows", test))]
fn validate_rendered_export(rendered: &RenderedExport) -> Result<(), String> {
    if rendered.origin != ExportOrigin::Native
        || rendered.byte_length != rendered.content.len()
        || rendered.byte_length > export::MAX_EXPORT_BYTES
        || rendered.extension != rendered.format.extension()
        || rendered.mime_type != rendered.format.mime_type()
    {
        return Err("export_output_invalid".into());
    }
    let valid = match rendered.format {
        export::ExportFormat::Json => {
            let document = serde_json::from_str::<ExportDocument>(&rendered.content).ok();
            document.is_some_and(valid_export_document)
        }
        export::ExportFormat::Markdown => valid_markdown_output(&rendered.content),
        export::ExportFormat::Csv => valid_csv_output(&rendered.content),
    };
    valid
        .then_some(())
        .ok_or_else(|| "export_output_invalid".into())
}

#[cfg(any(target_os = "windows", test))]
fn valid_markdown_output(content: &str) -> bool {
    [
        "# Life Log digest\n\n",
        "## Aggregation rules\n\n",
        "## Summary\n\n",
        "## Daily digest\n\n",
        "## Applications\n\n",
        "## Git projects\n\n",
        "## Sources\n\n",
        "## Sessions\n\n",
    ]
    .into_iter()
    .all(|marker| content.contains(marker))
        && content
            .chars()
            .all(|character| !character.is_control() || matches!(character, '\r' | '\n' | '\t'))
}

#[cfg(any(target_os = "windows", test))]
fn valid_export_document(document: ExportDocument) -> bool {
    export::validate_document(&document)
}

#[cfg(any(target_os = "windows", test))]
fn valid_csv_output(content: &str) -> bool {
    let header = export::EXPORT_CSV_HEADER.as_bytes();
    let bytes = content.as_bytes();
    if !bytes.starts_with(header)
        || bytes.get(header.len()..header.len() + 2) != Some(b"\r\n")
        || !bytes.ends_with(b"\r\n")
        || bytes.contains(&0)
    {
        return false;
    }

    // Reparse quote-aware records and require the renderer's fixed width. A
    // quoted title may contain CR/LF; those bytes are not record boundaries.
    let mut fields = 1usize;
    let mut records = 0usize;
    let mut field_start = true;
    let mut quoted = false;
    let mut after_quote = false;
    let mut index = header.len() + 2;
    while index < bytes.len() {
        let byte = bytes[index];
        if quoted {
            if byte == b'"' {
                if bytes.get(index + 1) == Some(&b'"') {
                    index += 2;
                    continue;
                }
                quoted = false;
                after_quote = true;
            }
            index += 1;
            continue;
        }
        if after_quote {
            if byte == b',' {
                fields += 1;
                field_start = true;
                after_quote = false;
                index += 1;
                continue;
            }
            if byte == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
                if fields != 24 {
                    return false;
                }
                fields = 1;
                field_start = true;
                after_quote = false;
                records += 1;
                index += 2;
                continue;
            }
            return false;
        }
        match byte {
            b'"' if field_start => {
                quoted = true;
                field_start = false;
            }
            b'"' => return false,
            b',' => {
                fields += 1;
                field_start = true;
            }
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => {
                if fields != 24 {
                    return false;
                }
                fields = 1;
                field_start = true;
                records += 1;
                index += 1;
            }
            b'\n' | b'\r' => return false,
            _ => field_start = false,
        }
        index += 1;
    }
    !quoted && !after_quote && fields == 1 && records > 0
}

/// Export content를 미리 생성한다. 이 command는 파일·clipboard·history를
/// 변경하지 않으며, 브라우저 fixture에서도 같은 payload를 사용할 수 있다.
#[tauri::command]
pub async fn export_life_log(
    state: tauri::State<'_, Arc<AppState>>,
    input: ExportInput,
) -> Result<RenderedExport, String> {
    render_for_state(&state, input).await
}

/// 사용자가 context menu 또는 export dialog에서 저장을 확정했을 때만 호출된다.
/// Windows native save dialog가 취소되면 `saved: false`를 반환하고 아무 파일도
/// 만들지 않는다. 선택 후 기록은 sibling temp + atomic replace다.
#[tauri::command]
pub async fn save_life_log(
    state: tauri::State<'_, Arc<AppState>>,
    input: ExportInput,
) -> Result<SaveExportResult, String> {
    let rendered = render_for_state(&state, input).await?;
    #[cfg(target_os = "windows")]
    {
        let Some(path) = choose_save_path(rendered.format) else {
            return Ok(SaveExportResult {
                saved: false,
                format: rendered.format,
                byte_length: rendered.byte_length,
            });
        };
        validate_save_path(&path, rendered.format)?;
        validate_rendered_export(&rendered)?;
        devbox_filesystem::atomic_write(&path, rendered.content.as_bytes())
            .map_err(|_| "export 파일을 저장하지 못했습니다".to_string())?;
        Ok(SaveExportResult {
            saved: true,
            format: rendered.format,
            byte_length: rendered.byte_length,
        })
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = rendered;
        Err("native export 저장은 Windows 데스크톱에서 사용할 수 없습니다".into())
    }
}

#[cfg(target_os = "windows")]
fn choose_save_path(format: export::ExportFormat) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::UI::Controls::Dialogs::{
        GetSaveFileNameW, OFN_EXPLORER, OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST,
        OPENFILENAMEW,
    };

    let (filter_index, extension, title) = match format {
        export::ExportFormat::Markdown => (1, "md", "Life Log Markdown 저장"),
        export::ExportFormat::Json => (2, "json", "Life Log JSON 저장"),
        export::ExportFormat::Csv => (3, "csv", "Life Log CSV 저장"),
    };
    let filter: Vec<u16> =
        "Markdown (*.md)\0*.md\0JSON (*.json)\0*.json\0CSV (*.csv)\0*.csv\0All files (*.*)\0*.*\0"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
    let title: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    let default_extension: Vec<u16> = extension.encode_utf16().chain(std::iter::once(0)).collect();
    let mut buffer = [0u16; 32_768];
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        nFilterIndex: filter_index,
        lpstrFile: PWSTR(buffer.as_mut_ptr()),
        nMaxFile: buffer.len() as u32,
        lpstrTitle: PCWSTR(title.as_ptr()),
        lpstrDefExt: PCWSTR(default_extension.as_ptr()),
        Flags: OFN_EXPLORER | OFN_NOCHANGEDIR | OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST,
        ..Default::default()
    };
    // A false result includes user cancellation. The dialog owns no file and
    // therefore both cancellation and a native dialog failure are fail-closed.
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
fn validate_save_path(path: &Path, format: export::ExportFormat) -> Result<(), String> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        || path.file_name().is_none()
        || path.parent().is_none_or(|parent| !parent.is_dir())
    {
        return Err("export 저장 경로가 안전하지 않습니다".into());
    }
    if path.to_str().is_none() || path.to_string_lossy().chars().any(char::is_control) {
        return Err("export 저장 경로가 안전하지 않습니다".into());
    }
    if let Ok(metadata) = std::fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
            return Err("export 저장 경로가 안전하지 않습니다".into());
        }
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase());
    if extension.as_deref() != Some(format.extension()) {
        return Err("선택한 파일 확장자가 export 형식과 일치하지 않습니다".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_boundary_accepts_complete_markdown_and_rejects_corrupt_metadata() {
        let content = [
            "# Life Log digest\n\n",
            "## Aggregation rules\n\n",
            "## Summary\n\n",
            "## Daily digest\n\n",
            "## Applications\n\n",
            "## Git projects\n\n",
            "## Sources\n\n",
            "## Sessions\n\n",
        ]
        .concat();
        let mut rendered = RenderedExport {
            origin: ExportOrigin::Native,
            format: export::ExportFormat::Markdown,
            extension: "md".into(),
            mime_type: "text/markdown;charset=utf-8".into(),
            byte_length: content.len(),
            content,
        };
        assert_eq!(validate_rendered_export(&rendered), Ok(()));

        rendered.byte_length += 1;
        assert_eq!(
            validate_rendered_export(&rendered),
            Err("export_output_invalid".into())
        );
    }
}
