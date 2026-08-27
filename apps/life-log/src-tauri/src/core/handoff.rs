//! Life Log -> Knowledge `knowledge-draft/v1` payload.
//!
//! The digest response is already native-only, privacy-sanitized, bounded, and
//! validated by `core::digest`.  This module deliberately projects that
//! response to a smaller handoff shape: aggregate metrics, a deterministic
//! Markdown body, and source provenance.  Sessions, window titles, and Git
//! project paths never cross the handoff boundary.

use crate::core::digest::DigestResponse;
use crate::core::export::SourceMetadata;
use serde::{Deserialize, Serialize};

pub const KNOWLEDGE_DRAFT_KIND: &str = "knowledge-draft/v1";
pub const KNOWLEDGE_DRAFT_SCHEMA_VERSION: u32 = 1;
pub const MAX_DRAFT_TITLE_BYTES: usize = 256;
pub const MAX_DRAFT_BODY_BYTES: usize = 512 * 1024;
pub const MAX_DRAFT_PAYLOAD_BYTES: usize = 768 * 1024;
pub const MAX_DRAFT_SOURCES: usize = 4;

const BODY_HEADER: &str = "# Life Log local digest\n\n";
const EXPECTED_SOURCE_IDS: [&str; MAX_DRAFT_SOURCES] =
    ["life-log", "git", "run-manager", "knowledge-base"];

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeDraftPayload {
    pub schema_version: u32,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    pub summary: KnowledgeDraftSummary,
    pub sources: Vec<KnowledgeDraftSource>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeDraftSummary {
    pub period: String,
    pub start_date: String,
    pub end_date: String,
    pub timezone: String,
    pub filter: Option<String>,
    pub pc_usage_ms: i64,
    pub session_count: usize,
    pub active_days: usize,
    pub total_days: usize,
    pub average_daily_usage_ms: i64,
    pub git_commits: u32,
    pub top_app: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeDraftSource {
    pub id: String,
    pub available: bool,
    pub schema_version: Option<u32>,
    pub snapshot_version: Option<u32>,
    pub producer_version: Option<String>,
    pub generated_at: Option<String>,
    pub freshness_ms: Option<u64>,
    pub view: Option<String>,
    pub scope: String,
    pub error_code: Option<String>,
}

/// Project a validated native digest into the handoff payload.
pub fn build_knowledge_draft(response: &DigestResponse) -> Result<KnowledgeDraftPayload, String> {
    if !crate::core::digest::validate_response(response) {
        return Err("handoff digest 결과를 검증하지 못했습니다".into());
    }
    let document = &response.document;
    let summary = KnowledgeDraftSummary {
        period: document.period.as_str().to_string(),
        start_date: document.range.start_date.clone(),
        end_date: document.range.end_date.clone(),
        timezone: document.range.timezone.clone(),
        filter: document.filter.app.clone(),
        pc_usage_ms: document.summary.pc_usage_ms,
        session_count: document.summary.session_count,
        active_days: document.summary.active_days,
        total_days: document.summary.total_days,
        average_daily_usage_ms: document.summary.average_daily_usage_ms,
        git_commits: document.summary.git_commits,
        top_app: document.summary.top_app.clone(),
    };
    let title = format!(
        "Life Log digest · {} ~ {}",
        summary.start_date, summary.end_date
    );
    let tags = vec![
        "life-log".to_string(),
        "digest".to_string(),
        summary.period.clone(),
    ];
    let sources = document
        .sources
        .iter()
        .map(source_from_export)
        .collect::<Vec<_>>();
    let body = render_body(&summary, &sources);
    let payload = KnowledgeDraftPayload {
        schema_version: KNOWLEDGE_DRAFT_SCHEMA_VERSION,
        title,
        body,
        tags,
        summary,
        sources,
    };
    validate_knowledge_draft(&payload)?;
    let serialized = serde_json::to_vec(&payload)
        .map_err(|_| "handoff draft를 직렬화하지 못했습니다".to_string())?;
    if serialized.len() > MAX_DRAFT_PAYLOAD_BYTES {
        return Err("handoff draft가 크기 제한을 초과했습니다".into());
    }
    Ok(payload)
}

fn source_from_export(source: &SourceMetadata) -> KnowledgeDraftSource {
    KnowledgeDraftSource {
        id: source.id.clone(),
        available: source.available,
        schema_version: source.schema_version,
        snapshot_version: source.snapshot_version,
        producer_version: source.producer_version.clone(),
        generated_at: source.generated_at.clone(),
        freshness_ms: source.freshness_ms,
        view: source.view.clone(),
        scope: source.scope.clone(),
        error_code: source.error_code.clone(),
    }
}

/// Validate the business payload independently of the generic applink
/// envelope.  Knowledge repeats this check on receipt, so a future producer
/// cannot silently widen the contract.
pub fn validate_knowledge_draft(payload: &KnowledgeDraftPayload) -> Result<(), String> {
    if payload.schema_version != KNOWLEDGE_DRAFT_SCHEMA_VERSION
        || payload.sources.len() != MAX_DRAFT_SOURCES
        || payload.tags != ["life-log", "digest", payload.summary.period.as_str()]
    {
        return Err("handoff draft 형식이 올바르지 않습니다".into());
    }
    if !bounded_text(&payload.title, MAX_DRAFT_TITLE_BYTES, false)
        || payload.title
            != format!(
                "Life Log digest · {} ~ {}",
                payload.summary.start_date, payload.summary.end_date
            )
    {
        return Err("handoff draft 제목이 올바르지 않습니다".into());
    }
    if payload.body.len() > MAX_DRAFT_BODY_BYTES
        || !payload.body.starts_with(BODY_HEADER)
        || payload.body != render_body(&payload.summary, &payload.sources)
    {
        return Err("handoff draft 본문이 올바르지 않습니다".into());
    }
    validate_summary(&payload.summary)?;
    for (source, expected_id) in payload.sources.iter().zip(EXPECTED_SOURCE_IDS) {
        validate_source(source, expected_id)?;
    }
    Ok(())
}

fn validate_summary(summary: &KnowledgeDraftSummary) -> Result<(), String> {
    if !matches!(summary.period.as_str(), "day" | "week" | "month")
        || !valid_date_key(&summary.start_date)
        || !valid_date_key(&summary.end_date)
        || summary.start_date > summary.end_date
        || !bounded_text(&summary.timezone, 128, false)
        || looks_like_path(&summary.timezone)
        || summary.filter.as_deref().is_some_and(|value| {
            !bounded_text(value, 256, false)
                || contains_secret_marker(value)
                || looks_like_path(value)
        })
        || summary.pc_usage_ms < 0
        || summary.session_count > 100_000
        || summary.active_days > summary.total_days
        || summary.total_days == 0
        || summary.total_days > 366
        || summary.average_daily_usage_ms < 0
        || summary.average_daily_usage_ms != summary.pc_usage_ms / summary.total_days as i64
        || summary.top_app.as_deref().is_some_and(|value| {
            !bounded_text(value, 256, false)
                || contains_secret_marker(value)
                || looks_like_path(value)
        })
    {
        return Err("handoff draft 요약이 올바르지 않습니다".into());
    }
    let expected_days = match summary.period.as_str() {
        "day" if summary.start_date == summary.end_date => 1,
        "week" if is_monday(&summary.start_date) => 7,
        "month"
            if summary.start_date.ends_with("-01")
                && summary.start_date[..7] == summary.end_date[..7]
                && (28..=31).contains(&summary.total_days) =>
        {
            summary.total_days
        }
        _ => 0,
    };
    if expected_days == 0 || expected_days != summary.total_days {
        return Err("handoff draft 기간이 올바르지 않습니다".into());
    }
    Ok(())
}

fn validate_source(source: &KnowledgeDraftSource, expected_id: &str) -> Result<(), String> {
    if source.id != expected_id
        || !bounded_text(&source.scope, 64, false)
        || source
            .error_code
            .as_deref()
            .is_some_and(|value| !bounded_text(value, 64, false) || !safe_source_error(value))
        || source
            .producer_version
            .as_deref()
            .is_some_and(|value| !bounded_text(value, 64, false) || !valid_semver(value))
        || source
            .generated_at
            .as_deref()
            .is_some_and(|value| !bounded_text(value, 32, false) || !valid_generated_at(value))
        || source.view.as_deref().is_some_and(|value| {
            !bounded_text(value, 64, false) || !matches!(value, "activity" | "legacy-data")
        })
    {
        return Err("handoff draft 출처가 올바르지 않습니다".into());
    }
    let expected_scope = if matches!(expected_id, "run-manager" | "knowledge-base") {
        "latest-snapshot-out-of-range"
    } else {
        "requested-range"
    };
    if source.scope != expected_scope {
        return Err("handoff draft 출처 범위가 올바르지 않습니다".into());
    }
    match expected_id {
        "life-log" => {
            if !source.available
                || source.error_code.is_some()
                || source.schema_version != Some(1)
                || source.snapshot_version.is_some()
                || source.producer_version.is_none()
                || source.generated_at.is_some()
                || source.freshness_ms.is_some()
                || source.view.is_some()
            {
                return Err("handoff draft 출처가 올바르지 않습니다".into());
            }
        }
        "git" => {
            if source.schema_version.is_some()
                || source.snapshot_version.is_some()
                || source.producer_version.is_some()
                || source.generated_at.is_some()
                || source.freshness_ms.is_some()
                || source.view.is_some()
                || source.available != source.error_code.is_none()
            {
                return Err("handoff draft 출처가 올바르지 않습니다".into());
            }
        }
        "run-manager" | "knowledge-base" => {
            if source.available != source.error_code.is_none()
                || source
                    .schema_version
                    .is_some_and(|version| version != source.snapshot_version.unwrap_or(0))
                || source.available
                    && (source.schema_version != Some(1)
                        || source.snapshot_version != Some(1)
                        || source.producer_version.is_none()
                        || source.generated_at.is_none()
                        || source.freshness_ms.is_none())
            {
                return Err("handoff draft 출처가 올바르지 않습니다".into());
            }
        }
        _ => return Err("handoff draft 출처가 올바르지 않습니다".into()),
    }
    Ok(())
}

pub fn render_body(summary: &KnowledgeDraftSummary, sources: &[KnowledgeDraftSource]) -> String {
    let mut body = String::from(BODY_HEADER);
    body.push_str("- Period: `");
    body.push_str(&markdown_cell(&summary.period));
    body.push_str("`\n- Range: `");
    body.push_str(&summary.start_date);
    body.push_str("` to `");
    body.push_str(&summary.end_date);
    body.push_str("` (exclusive end)\n- Timezone: `");
    body.push_str(&markdown_cell(&summary.timezone));
    body.push_str("`\n- Filter: ");
    body.push_str(&markdown_cell(
        summary.filter.as_deref().unwrap_or("all apps"),
    ));
    body.push_str("\n\n## Summary\n\n| Metric | Value |\n| --- | ---: |\n");
    markdown_row(&mut body, "PC usage (ms)", &summary.pc_usage_ms.to_string());
    markdown_row(&mut body, "Sessions", &summary.session_count.to_string());
    markdown_row(
        &mut body,
        "Active days",
        &format!("{} / {}", summary.active_days, summary.total_days),
    );
    markdown_row(
        &mut body,
        "Average daily usage (ms)",
        &summary.average_daily_usage_ms.to_string(),
    );
    markdown_row(&mut body, "Git commits", &summary.git_commits.to_string());
    markdown_row(
        &mut body,
        "Top app",
        summary.top_app.as_deref().unwrap_or("-"),
    );
    body.push_str(
        "\n## Sources\n\n| Source | Available | Scope | Error code |\n| --- | --- | --- | --- |\n",
    );
    for source in sources {
        body.push_str("| ");
        body.push_str(&markdown_cell(&source.id));
        body.push_str(" | ");
        body.push_str(if source.available { "true" } else { "false" });
        body.push_str(" | ");
        body.push_str(&markdown_cell(&source.scope));
        body.push_str(" | ");
        body.push_str(&markdown_cell(source.error_code.as_deref().unwrap_or("-")));
        body.push_str(" |\n");
    }
    body
}

fn markdown_row(body: &mut String, label: &str, value: &str) {
    body.push_str("| ");
    body.push_str(&markdown_cell(label));
    body.push_str(" | ");
    body.push_str(&markdown_cell(value));
    body.push_str(" |\n");
}

fn markdown_cell(value: &str) -> String {
    value
        .replace(['|', '`', '\\'], "")
        .replace(['\r', '\n'], " ")
}

fn bounded_text(value: &str, max_bytes: usize, allow_newlines: bool) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && !value.chars().any(|character| {
            character.is_control() && (!allow_newlines || !matches!(character, '\n' | '\r'))
        })
}

fn valid_date_key(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| !matches!(index, 4 | 7) && !byte.is_ascii_digit())
    {
        return false;
    }
    let year = value[0..4].parse::<i32>().ok();
    let month = value[5..7].parse::<u32>().ok();
    let day = value[8..10].parse::<u32>().ok();
    let (Some(year), Some(month), Some(day)) = (year, month, day) else {
        return false;
    };
    year > 0 && (1..=12).contains(&month) && (1..=days_in_month(year, month)).contains(&day)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn is_monday(value: &str) -> bool {
    let year = value[0..4].parse::<i32>().unwrap_or(0);
    let month = value[5..7].parse::<usize>().unwrap_or(0);
    let day = value[8..10].parse::<i32>().unwrap_or(0);
    if !(1..=12).contains(&month) || day < 1 {
        return false;
    }
    const OFFSETS: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let adjusted_year = if month < 3 { year - 1 } else { year };
    (adjusted_year + adjusted_year / 4 - adjusted_year / 100
        + adjusted_year / 400
        + OFFSETS[month - 1]
        + day)
        .rem_euclid(7)
        == 1
}

fn looks_like_path(value: &str) -> bool {
    value.starts_with('/')
        || value.starts_with('\\')
        || value.contains(":\\")
        || value.contains(":/")
        || value.split('/').any(|part| part == "..")
}

fn contains_secret_marker(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "password",
        "passwd",
        "secret",
        "token",
        "access_token",
        "refresh_token",
        "api_key",
        "apikey",
        "credential",
        "authorization",
        "bearer ",
        "basic ",
        "sk-",
        "ghp_",
        "gho_",
        "ghs_",
        "ghu_",
        "github_pat_",
        "xoxb-",
        "xoxp-",
        "npm_",
        "pypi-",
        "akia",
        "ya29.",
        "-----begin ",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn safe_source_error(value: &str) -> bool {
    matches!(
        value,
        "no_safe_project_paths"
            | "snapshot_unavailable"
            | "snapshot_invalid"
            | "snapshot_schema_unsupported"
            | "snapshot_payload_invalid"
            | "snapshot_changed_during_read"
            | "git_invalid_arguments"
            | "git_spawn_failed"
            | "git_stdout_unavailable"
            | "git_wait_failed"
            | "git_timeout"
            | "git_reader_failed"
            | "git_output_read_failed"
            | "git_failed"
            | "git_output_invalid_utf8"
            | "git_output_too_large"
            | "git_output_invalid"
    )
}

fn valid_semver(value: &str) -> bool {
    let (without_build, build) = match value.split_once('+') {
        Some((core, build)) if !build.is_empty() => (core, Some(build)),
        Some(_) => return false,
        None => (value, None),
    };
    let (core, pre) = match without_build.split_once('-') {
        Some((core, pre)) if !pre.is_empty() => (core, Some(pre)),
        Some(_) => return false,
        None => (without_build, None),
    };
    let parts = core.split('.').collect::<Vec<_>>();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (part == &"0" || !part.starts_with('0'))
        })
        && pre.is_none_or(valid_semver_suffix)
        && build.is_none_or(valid_semver_suffix)
}

fn valid_semver_suffix(value: &str) -> bool {
    value.split('.').all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    })
}

fn valid_generated_at(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 20
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'Z'
        && bytes.iter().enumerate().all(|(index, byte)| {
            matches!(index, 4 | 7 | 10 | 13 | 16 | 19) || byte.is_ascii_digit()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary() -> KnowledgeDraftSummary {
        KnowledgeDraftSummary {
            period: "week".into(),
            start_date: "2026-08-10".into(),
            end_date: "2026-08-16".into(),
            timezone: "Asia/Seoul".into(),
            filter: None,
            pc_usage_ms: 7_200_000,
            session_count: 4,
            active_days: 2,
            total_days: 7,
            average_daily_usage_ms: 1_028_571,
            git_commits: 3,
            top_app: Some("Code.exe".into()),
        }
    }

    fn sources() -> Vec<KnowledgeDraftSource> {
        vec![
            KnowledgeDraftSource {
                id: "life-log".into(),
                available: true,
                schema_version: Some(1),
                snapshot_version: None,
                producer_version: Some("0.3.1".into()),
                generated_at: None,
                freshness_ms: None,
                view: None,
                scope: "requested-range".into(),
                error_code: None,
            },
            KnowledgeDraftSource {
                id: "git".into(),
                available: true,
                schema_version: None,
                snapshot_version: None,
                producer_version: None,
                generated_at: None,
                freshness_ms: None,
                view: None,
                scope: "requested-range".into(),
                error_code: None,
            },
            KnowledgeDraftSource {
                id: "run-manager".into(),
                available: false,
                schema_version: None,
                snapshot_version: None,
                producer_version: None,
                generated_at: None,
                freshness_ms: None,
                view: None,
                scope: "latest-snapshot-out-of-range".into(),
                error_code: Some("snapshot_unavailable".into()),
            },
            KnowledgeDraftSource {
                id: "knowledge-base".into(),
                available: true,
                schema_version: Some(1),
                snapshot_version: Some(1),
                producer_version: Some("0.3.1".into()),
                generated_at: Some("2026-08-27T01:02:03Z".into()),
                freshness_ms: Some(1_000),
                view: Some("activity".into()),
                scope: "latest-snapshot-out-of-range".into(),
                error_code: None,
            },
        ]
    }

    #[test]
    fn builds_bounded_summary_only_payload() {
        let summary = summary();
        let sources = sources();
        let payload = KnowledgeDraftPayload {
            schema_version: 1,
            title: "Life Log digest · 2026-08-10 ~ 2026-08-16".into(),
            body: render_body(&summary, &sources),
            tags: vec!["life-log".into(), "digest".into(), "week".into()],
            summary,
            sources,
        };
        validate_knowledge_draft(&payload).unwrap();
        let encoded = serde_json::to_string(&payload).unwrap();
        assert!(!encoded.contains("\"daily\""));
        assert!(!encoded.contains("\"projects\""));
        assert!(!encoded.contains("path"));
        assert!(encoded.len() < MAX_DRAFT_PAYLOAD_BYTES);
    }

    #[test]
    fn rejects_tampered_body_and_secret_or_path_metadata() {
        let summary = summary();
        let sources = sources();
        let mut payload = KnowledgeDraftPayload {
            schema_version: 1,
            title: "Life Log digest · 2026-08-10 ~ 2026-08-16".into(),
            body: render_body(&summary, &sources),
            tags: vec!["life-log".into(), "digest".into(), "week".into()],
            summary,
            sources,
        };
        payload.body.push_str("\nraw credential");
        assert!(validate_knowledge_draft(&payload).is_err());
        payload.body = render_body(&payload.summary, &payload.sources);
        payload.summary.timezone = "/private/path".into();
        assert!(validate_knowledge_draft(&payload).is_err());
    }
}
