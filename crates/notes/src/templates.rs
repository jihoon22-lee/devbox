//! Note template validation and one-pass rendering. Ported from v0.9.0
//! `knowledge-vault-engine/core/templates.rs`; v1 templates are vault files
//! (`<templates_folder>/<name>.md`) instead of database rows, and `{{pc}}` is new.
//! Never touches the filesystem or the process environment.
use crate::model::TemplateProblem;

pub const MAX_TEMPLATE_CONTENT_BYTES: usize = 64 * 1024;
pub const MAX_TEMPLATE_OUTPUT_BYTES: usize = 256 * 1024;
pub const MAX_TEMPLATE_TITLE_BYTES: usize = 256;
pub const MAX_TEMPLATE_PATH_BYTES: usize = 512;
pub const MAX_PC_BYTES: usize = 128;

pub const PLACEHOLDERS: [&str; 5] = [
    "{{title}}",
    "{{date}}",
    "{{time}}",
    "{{pc}}",
    "{{vault-relative-path}}",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateInput {
    /// Vault-relative path of the note being created.
    pub target: String,
    pub title: String,
    /// `YYYY-MM-DD`
    pub date: String,
    /// `HH:MM`
    pub time: String,
    pub pc: String,
}

pub fn render(content: &str, input: &TemplateInput) -> Result<String, TemplateProblem> {
    if !bounded_text(&input.target, MAX_TEMPLATE_PATH_BYTES, true)
        || !safe_relative_path(&input.target)
        || !input.target.to_ascii_lowercase().ends_with(".md")
    {
        return Err(TemplateProblem::InvalidTarget);
    }
    if !bounded_text(&input.title, MAX_TEMPLATE_TITLE_BYTES, false) || !single_line(&input.title) {
        return Err(TemplateProblem::InvalidTitle);
    }
    if !valid_date(&input.date) {
        return Err(TemplateProblem::InvalidDate);
    }
    if !valid_time(&input.time) {
        return Err(TemplateProblem::InvalidTime);
    }
    if !bounded_text(&input.pc, MAX_PC_BYTES, false) || !single_line(&input.pc) {
        return Err(TemplateProblem::InvalidContent);
    }
    if !bounded_text(content, MAX_TEMPLATE_CONTENT_BYTES, false) {
        return Err(TemplateProblem::InvalidContent);
    }
    validate_placeholders(content)?;

    // Expand in one pass: a substituted value is user data, not template syntax,
    // so placeholder-looking text inside a value stays literal.
    let mut output = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(start) = rest.find("{{") {
        push_bounded(&mut output, &rest[..start])?;
        let candidate = &rest[start..];
        let end = candidate
            .find("}}")
            .ok_or(TemplateProblem::UnknownPlaceholder)?;
        let token = &candidate[..end + 2];
        let value = match token {
            "{{title}}" => input.title.as_str(),
            "{{date}}" => input.date.as_str(),
            "{{time}}" => input.time.as_str(),
            "{{pc}}" => input.pc.as_str(),
            "{{vault-relative-path}}" => input.target.as_str(),
            _ => return Err(TemplateProblem::UnknownPlaceholder),
        };
        push_bounded(&mut output, value)?;
        rest = &candidate[end + 2..];
    }
    push_bounded(&mut output, rest)?;
    Ok(output)
}

fn push_bounded(output: &mut String, value: &str) -> Result<(), TemplateProblem> {
    if output
        .len()
        .checked_add(value.len())
        .is_none_or(|length| length > MAX_TEMPLATE_OUTPUT_BYTES)
    {
        return Err(TemplateProblem::OutputTooLarge);
    }
    output.push_str(value);
    Ok(())
}

pub fn validate_placeholders(content: &str) -> Result<(), TemplateProblem> {
    let mut rest = content;
    while let Some(start) = rest.find("{{") {
        let candidate = &rest[start..];
        let Some(end) = candidate.find("}}") else {
            return Err(TemplateProblem::UnknownPlaceholder);
        };
        let token = &candidate[..end + 2];
        if !PLACEHOLDERS.contains(&token) {
            return Err(TemplateProblem::UnknownPlaceholder);
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

pub(crate) fn safe_relative_path(value: &str) -> bool {
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

pub(crate) fn valid_date(value: &str) -> bool {
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

    fn input() -> TemplateInput {
        TemplateInput {
            target: "Journal/2026-08-28.md".into(),
            title: "Focus log".into(),
            date: "2026-08-28".into(),
            time: "09:30".into(),
            pc: "desk".into(),
        }
    }

    #[test]
    fn substitutes_all_supported_values() {
        let result = render(
            "# {{title}}\n{{date}} {{time}} {{pc}}\n{{vault-relative-path}}",
            &input(),
        )
        .expect("render");
        assert_eq!(
            result,
            "# Focus log\n2026-08-28 09:30 desk\nJournal/2026-08-28.md"
        );
    }

    #[test]
    fn substituted_values_are_not_interpreted_as_template_syntax() {
        let mut input = input();
        input.title = "literal {{date}}".into();
        input.target = "Journal/{{title}}.md".into();
        input.pc = "{{time}}".into();
        assert_eq!(
            render("{{title}} · {{vault-relative-path}} · {{pc}}", &input).expect("render"),
            "literal {{date}} · Journal/{{title}}.md · {{time}}"
        );
    }

    #[test]
    fn rejects_unknown_and_malformed_placeholders() {
        assert_eq!(
            validate_placeholders("{{author}}").expect_err("must be rejected"),
            TemplateProblem::UnknownPlaceholder
        );
        assert_eq!(
            validate_placeholders("{{title").expect_err("must be rejected"),
            TemplateProblem::UnknownPlaceholder
        );
    }

    #[test]
    fn rejects_unsafe_target_and_oversized_output() {
        let mut unsafe_input = input();
        unsafe_input.target = "../outside.md".into();
        assert_eq!(
            render("x", &unsafe_input).expect_err("must be rejected"),
            TemplateProblem::InvalidTarget
        );
        let huge = "x".repeat(MAX_TEMPLATE_OUTPUT_BYTES);
        assert_eq!(
            render(&huge, &input()).expect_err("must be rejected"),
            TemplateProblem::InvalidContent
        );
    }

    #[test]
    fn rejects_calendar_invalid_dates_and_windows_paths() {
        let mut invalid_date = input();
        invalid_date.date = "2026-02-29".into();
        assert_eq!(
            render("{{date}}", &invalid_date).expect_err("must be rejected"),
            TemplateProblem::InvalidDate
        );
        let mut windows_path = input();
        windows_path.target = r"Journal\outside.md".into();
        assert_eq!(
            render("x", &windows_path).expect_err("must be rejected"),
            TemplateProblem::InvalidTarget
        );
    }
}
