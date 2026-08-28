//! Pure validation and rendering for Knowledge note templates.
//!
//! Templates are intentionally small, local, and deterministic.  The command
//! layer owns persistence and the preview approval slot; this module never
//! touches the filesystem or performs interpolation from process environment.

use serde::{Deserialize, Serialize};

pub const MAX_TEMPLATE_NAME_BYTES: usize = 128;
pub const MAX_TEMPLATE_CONTENT_BYTES: usize = 64 * 1024;
pub const MAX_TEMPLATE_OUTPUT_BYTES: usize = 256 * 1024;
pub const MAX_TEMPLATE_TITLE_BYTES: usize = 256;
pub const MAX_TEMPLATE_PATH_BYTES: usize = 512;
pub const MAX_TEMPLATES: usize = 100;

pub const PLACEHOLDERS: [&str; 4] = [
    "{{title}}",
    "{{date}}",
    "{{time}}",
    "{{vault-relative-path}}",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NoteTemplate {
    pub id: i64,
    pub name: String,
    pub content: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemplateDraft {
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemplateApplyInput {
    pub template_id: i64,
    pub target: String,
    pub title: String,
    pub date: String,
    pub time: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TemplatePreview {
    pub preview_id: String,
    pub template_id: i64,
    pub template_updated_at_ms: i64,
    pub target: String,
    pub content: String,
    pub byte_length: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateError {
    InvalidName,
    InvalidContent,
    InvalidId,
    InvalidTarget,
    InvalidTitle,
    InvalidDate,
    InvalidTime,
    UnknownPlaceholder,
    OutputTooLarge,
}

impl TemplateError {
    pub const fn message(self) -> &'static str {
        match self {
            Self::InvalidName => "템플릿 이름이 올바르지 않습니다",
            Self::InvalidContent => "템플릿 본문이 올바르지 않습니다",
            Self::InvalidId => "템플릿을 찾을 수 없습니다",
            Self::InvalidTarget => "템플릿 저장 경로가 올바르지 않습니다",
            Self::InvalidTitle => "템플릿 제목이 올바르지 않습니다",
            Self::InvalidDate => "템플릿 날짜가 올바르지 않습니다",
            Self::InvalidTime => "템플릿 시간이 올바르지 않습니다",
            Self::UnknownPlaceholder => "지원하지 않는 템플릿 변수가 있습니다",
            Self::OutputTooLarge => "템플릿 결과가 크기 제한을 초과했습니다",
        }
    }
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for TemplateError {}

pub fn validate_draft(draft: &TemplateDraft) -> Result<(), TemplateError> {
    if !bounded_text(&draft.name, MAX_TEMPLATE_NAME_BYTES, true)
        || draft.name.contains(['/', '\\'])
        || !single_line(&draft.name)
    {
        return Err(TemplateError::InvalidName);
    }
    if !bounded_text(&draft.content, MAX_TEMPLATE_CONTENT_BYTES, false) {
        return Err(TemplateError::InvalidContent);
    }
    validate_placeholders(&draft.content)
}

pub fn render(
    template_id: i64,
    content: &str,
    input: &TemplateApplyInput,
) -> Result<String, TemplateError> {
    if template_id <= 0 {
        return Err(TemplateError::InvalidId);
    }
    if !bounded_text(&input.target, MAX_TEMPLATE_PATH_BYTES, true)
        || !safe_relative_path(&input.target)
        || !input.target.to_ascii_lowercase().ends_with(".md")
    {
        return Err(TemplateError::InvalidTarget);
    }
    if !bounded_text(&input.title, MAX_TEMPLATE_TITLE_BYTES, false) || !single_line(&input.title) {
        return Err(TemplateError::InvalidTitle);
    }
    if !valid_date(&input.date) {
        return Err(TemplateError::InvalidDate);
    }
    if !valid_time(&input.time) {
        return Err(TemplateError::InvalidTime);
    }
    if !bounded_text(content, MAX_TEMPLATE_CONTENT_BYTES, false) {
        return Err(TemplateError::InvalidContent);
    }
    validate_placeholders(content)?;

    let values = [
        ("{{title}}", input.title.as_str()),
        ("{{date}}", input.date.as_str()),
        ("{{time}}", input.time.as_str()),
        ("{{vault-relative-path}}", input.target.as_str()),
    ];
    let mut output = content.to_owned();
    for (placeholder, value) in values {
        output = output.replace(placeholder, value);
    }
    if output.len() > MAX_TEMPLATE_OUTPUT_BYTES {
        return Err(TemplateError::OutputTooLarge);
    }
    Ok(output)
}

fn validate_placeholders(content: &str) -> Result<(), TemplateError> {
    let mut rest = content;
    while let Some(start) = rest.find("{{") {
        let candidate = &rest[start..];
        let Some(end) = candidate.find("}}") else {
            return Err(TemplateError::UnknownPlaceholder);
        };
        let token = &candidate[..end + 2];
        if !PLACEHOLDERS.contains(&token) {
            return Err(TemplateError::UnknownPlaceholder);
        }
        rest = &candidate[end + 2..];
    }
    Ok(())
}

fn bounded_text(value: &str, max_bytes: usize, non_empty: bool) -> bool {
    !value.contains('\0')
        && value
            .chars()
            .all(|character| !character.is_control() || matches!(character, '\n' | '\r' | '\t'))
        && value.len() <= max_bytes
        && (!non_empty || !value.trim().is_empty())
}

fn safe_relative_path(value: &str) -> bool {
    let normalized = value.replace('\\', "/");
    !normalized.is_empty()
        && normalized == value
        && !normalized.starts_with('/')
        && !normalized.contains("//")
        && !normalized.contains(':')
        && normalized
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
        && normalized.chars().all(|character| !character.is_control())
}

fn valid_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
    {
        return false;
    }
    let Some(year) = value[0..4].parse::<u32>().ok() else {
        return false;
    };
    let Some(month) = value[5..7].parse::<u32>().ok() else {
        return false;
    };
    let Some(day) = value[8..10].parse::<u32>().ok() else {
        return false;
    };
    if year == 0 || !(1..=12).contains(&month) {
        return false;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    (1..=days).contains(&day)
}

fn valid_time(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 5
        || bytes[2] != b':'
        || !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 2 || byte.is_ascii_digit())
    {
        return false;
    }
    let hour = value[0..2].parse::<u32>().ok();
    let minute = value[3..5].parse::<u32>().ok();
    matches!((hour, minute), (Some(0..=23), Some(0..=59)))
}

fn single_line(value: &str) -> bool {
    value.chars().all(|character| !character.is_control())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> TemplateApplyInput {
        TemplateApplyInput {
            template_id: 1,
            target: "Journal/2026-08-28.md".into(),
            title: "Focus log".into(),
            date: "2026-08-28".into(),
            time: "09:30".into(),
        }
    }

    #[test]
    fn substitutes_all_supported_values() {
        let result = render(
            1,
            "# {{title}}\n{{date}} {{time}}\n{{vault-relative-path}}",
            &input(),
        )
        .unwrap();
        assert_eq!(
            result,
            "# Focus log\n2026-08-28 09:30\nJournal/2026-08-28.md"
        );
    }

    #[test]
    fn rejects_unknown_and_malformed_placeholders() {
        assert_eq!(
            validate_placeholders("{{author}}").unwrap_err(),
            TemplateError::UnknownPlaceholder
        );
        assert_eq!(
            validate_placeholders("{{title").unwrap_err(),
            TemplateError::UnknownPlaceholder
        );
    }

    #[test]
    fn rejects_unsafe_target_and_oversized_output() {
        let mut unsafe_input = input();
        unsafe_input.target = "../outside.md".into();
        assert_eq!(
            render(1, "x", &unsafe_input).unwrap_err(),
            TemplateError::InvalidTarget
        );
        let huge = "x".repeat(MAX_TEMPLATE_OUTPUT_BYTES);
        assert_eq!(
            render(1, &huge, &input()).unwrap_err(),
            TemplateError::InvalidContent
        );
    }

    #[test]
    fn rejects_calendar_invalid_dates_and_windows_paths() {
        let mut invalid_date = input();
        invalid_date.date = "2026-02-29".into();
        assert_eq!(
            render(1, "{{date}}", &invalid_date).unwrap_err(),
            TemplateError::InvalidDate
        );
        let mut windows_path = input();
        windows_path.target = r"Journal\outside.md".into();
        assert_eq!(
            render(1, "x", &windows_path).unwrap_err(),
            TemplateError::InvalidTarget
        );
    }
}
