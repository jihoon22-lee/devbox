//! Knowledge receiver for versioned Life Log and Developer Toolbox drafts.
//!
//! This is a deliberately strict, app-local copy of the wire contract.  The
//! generic applink envelope checks protocol, target, size, and storage safety;
//! this module checks the producer identity, schema, aggregate-only shape,
//! source provenance, and deterministic Markdown body before a preview is
//! shown.

use devbox_applink::{HandoffClaim, HandoffError};
use serde::{Deserialize, Serialize};

pub const KNOWLEDGE_DRAFT_KIND: &str = "knowledge-draft/v1";
pub const KNOWLEDGE_DRAFT_SCHEMA_VERSION: u32 = 1;
pub const TOOLBOX_DRAFT_KIND: &str = "knowledge-draft/v2";
pub const TOOLBOX_DRAFT_SCHEMA_VERSION: u32 = 2;
pub const MAX_DRAFT_TITLE_BYTES: usize = 256;
pub const MAX_DRAFT_BODY_BYTES: usize = 512 * 1024;
pub const MAX_DRAFT_PAYLOAD_BYTES: usize = 768 * 1024;
pub const MAX_DRAFT_SOURCES: usize = 4;
pub const MAX_PROVENANCE_FRESHNESS_MS: u64 = 30 * 24 * 60 * 60 * 1_000;

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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolboxDraftPayload {
    pub schema_version: u32,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    pub created_date: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IncomingKnowledgeDraft {
    LifeLog(KnowledgeDraftPayload),
    Toolbox(ToolboxDraftPayload),
}

impl IncomingKnowledgeDraft {
    pub fn title(&self) -> &str {
        match self {
            Self::LifeLog(payload) => &payload.title,
            Self::Toolbox(payload) => &payload.title,
        }
    }

    pub fn body(&self) -> &str {
        match self {
            Self::LifeLog(payload) => &payload.body,
            Self::Toolbox(payload) => &payload.body,
        }
    }

    pub fn tags(&self) -> &[String] {
        match self {
            Self::LifeLog(payload) => &payload.tags,
            Self::Toolbox(payload) => &payload.tags,
        }
    }

    pub fn note_stem(&self) -> String {
        match self {
            Self::LifeLog(payload) => format!(
                "Journal/{}-life-log-{}",
                payload.summary.start_date, payload.summary.period
            ),
            Self::Toolbox(payload) => {
                format!("Journal/{}-developer-toolbox-result", payload.created_date)
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeDraftPreview {
    pub id: String,
    pub kind: String,
    pub producer_id: String,
    pub expires_at_ms: u64,
    pub lease_until_ms: u64,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    pub summary: Option<KnowledgeDraftSummary>,
    pub sources: Vec<KnowledgeDraftSource>,
}

impl KnowledgeDraftPreview {
    pub fn from_claim(claim: &HandoffClaim, payload: &IncomingKnowledgeDraft) -> Self {
        let (summary, sources) = match payload {
            IncomingKnowledgeDraft::LifeLog(payload) => {
                (Some(payload.summary.clone()), payload.sources.clone())
            }
            IncomingKnowledgeDraft::Toolbox(_) => (None, Vec::new()),
        };
        Self {
            id: claim.envelope.id.clone(),
            kind: claim.envelope.kind.clone(),
            producer_id: claim.envelope.source_app.clone(),
            expires_at_ms: claim.envelope.expires_at_ms,
            lease_until_ms: claim.lease_until_ms,
            title: payload.title().to_string(),
            body: payload.body().to_string(),
            tags: payload.tags().to_vec(),
            summary,
            sources,
        }
    }
}

/// Convert a generic claimed envelope into a validated business payload.
pub fn parse_claim(claim: &HandoffClaim) -> Result<IncomingKnowledgeDraft, String> {
    if claim.envelope.target_app.as_deref() != Some("knowledge-base") {
        return Err("handoff draft 출처 또는 대상이 올바르지 않습니다".into());
    }
    let payload = match (
        claim.envelope.kind.as_str(),
        claim.envelope.source_app.as_str(),
    ) {
        (KNOWLEDGE_DRAFT_KIND, "life-log") => {
            let payload: KnowledgeDraftPayload =
                serde_json::from_value(claim.envelope.payload.clone())
                    .map_err(|_| "handoff draft 형식이 올바르지 않습니다".to_string())?;
            validate_knowledge_draft(&payload)?;
            IncomingKnowledgeDraft::LifeLog(payload)
        }
        (TOOLBOX_DRAFT_KIND, "developer-toolbox") => {
            let payload: ToolboxDraftPayload =
                serde_json::from_value(claim.envelope.payload.clone())
                    .map_err(|_| "handoff draft 형식이 올바르지 않습니다".to_string())?;
            validate_toolbox_draft(&payload)?;
            IncomingKnowledgeDraft::Toolbox(payload)
        }
        _ => return Err("handoff draft 출처 또는 대상이 올바르지 않습니다".into()),
    };
    let serialized = serde_json::to_vec(&claim.envelope.payload)
        .map_err(|_| "handoff draft를 읽을 수 없습니다".to_string())?;
    if serialized.len() > MAX_DRAFT_PAYLOAD_BYTES {
        return Err("handoff draft가 크기 제한을 초과했습니다".into());
    }
    Ok(payload)
}

pub fn validate_toolbox_draft(payload: &ToolboxDraftPayload) -> Result<(), String> {
    if payload.schema_version != TOOLBOX_DRAFT_SCHEMA_VERSION
        || payload.title != format!("Developer Toolbox result · {}", payload.created_date)
        || payload.tags != ["developer-toolbox".to_string(), "draft".to_string()]
        || !valid_date_key(&payload.created_date)
        || !bounded_text(&payload.title, MAX_DRAFT_TITLE_BYTES, false)
        || payload.body.trim().is_empty()
        || !bounded_text(&payload.body, MAX_DRAFT_BODY_BYTES, true)
        || devbox_applink::validate_handoff_text(&payload.body).is_err()
    {
        return Err("handoff draft 형식이 올바르지 않습니다".into());
    }
    Ok(())
}

pub fn map_claim_error(error: &HandoffError) -> &'static str {
    match error {
        HandoffError::Missing | HandoffError::Expired | HandoffError::LeaseExpired => {
            "Knowledge draft를 사용할 수 없거나 만료되었습니다. 보낸 앱에서 새로 생성하세요."
        }
        HandoffError::AlreadyClaimed => "Knowledge draft가 이미 미리보기 중입니다.",
        HandoffError::WrongTarget | HandoffError::WrongKind => {
            "Knowledge draft 대상이 올바르지 않습니다."
        }
        _ => "Knowledge draft를 처리할 수 없습니다.",
    }
}

pub fn validate_knowledge_draft(payload: &KnowledgeDraftPayload) -> Result<(), String> {
    let expected_tags = vec![
        "life-log".to_string(),
        "digest".to_string(),
        payload.summary.period.clone(),
    ];
    if payload.schema_version != KNOWLEDGE_DRAFT_SCHEMA_VERSION
        || payload.sources.len() != MAX_DRAFT_SOURCES
        || payload.tags != expected_tags
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
    validate_summary(&payload.summary)?;
    let source_contract = payload.sources[0].schema_version.unwrap_or_default();
    if !matches!(source_contract, 1 | 2) {
        return Err("handoff draft 출처가 올바르지 않습니다".into());
    }
    for (source, expected_id) in payload.sources.iter().zip(EXPECTED_SOURCE_IDS) {
        validate_source(source, expected_id, source_contract)?;
    }
    if payload.body.len() > MAX_DRAFT_BODY_BYTES
        || !payload.body.starts_with(BODY_HEADER)
        || payload.body != render_body(&payload.summary, &payload.sources)
    {
        return Err("handoff draft 본문이 올바르지 않습니다".into());
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
        || contains_secret_marker(&summary.timezone)
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

fn validate_source(
    source: &KnowledgeDraftSource,
    expected_id: &str,
    source_contract: u32,
) -> Result<(), String> {
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
        || source
            .freshness_ms
            .is_some_and(|value| value > MAX_PROVENANCE_FRESHNESS_MS)
        || source.view.as_deref().is_some_and(|value| {
            !bounded_text(value, 64, false)
                || !matches!(value, "activity" | "legacy-data" | "daily-activity")
        })
    {
        return Err("handoff draft 출처가 올바르지 않습니다".into());
    }
    let valid_scope = if matches!(expected_id, "run-manager" | "knowledge-base") {
        if source_contract == 1 {
            source.scope == "latest-snapshot-out-of-range"
        } else {
            matches!(
                source.scope.as_str(),
                "requested-range" | "requested-range-partial"
            )
        }
    } else {
        source.scope == "requested-range"
    };
    if !valid_scope {
        return Err("handoff draft 출처 범위가 올바르지 않습니다".into());
    }
    match expected_id {
        "life-log" => {
            if !source.available
                || source.error_code.is_some()
                || source.schema_version != Some(source_contract)
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
                || (source_contract == 2
                    && (source.view.as_deref() != Some("daily-activity")
                        || (source.available && source.scope != "requested-range")
                        || (source.scope == "requested-range-partial" && source.available)))
                || (source_contract == 1
                    && source.view.as_deref().is_some_and(|view| {
                        expected_id != "knowledge-base"
                            || !matches!(view, "activity" | "legacy-data")
                    }))
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
    body.push_str("` (date keys inclusive; end timestamp exclusive)\n- Timezone: `");
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
            character.is_control() && (!allow_newlines || !matches!(character, '\n' | '\r' | '\t'))
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
        || value.split(['/', '\\']).any(|part| part == "..")
}

fn contains_secret_marker(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    if [
        "password",
        "passwd",
        "secret",
        "secretkey",
        "token",
        "access_token",
        "refresh_token",
        "api_key",
        "apikey",
        "x-api-key",
        "x_api_key",
        "accesskey",
        "client_secret",
        "clientsecret",
        "session_token",
        "sessiontoken",
        "id_token",
        "idtoken",
        "credential",
        "authorization",
        "cookie",
        "set-cookie",
        "private key",
        "private_key",
        "ssh_private_key",
        "signing_key",
        "oauth",
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
    {
        return true;
    }
    if lower.split_once("://").is_some_and(|(_, rest)| {
        rest.split(['/', '?', '#'])
            .next()
            .is_some_and(|authority| authority.contains('@'))
    }) {
        return true;
    }
    value
        .split(|character: char| {
            character.is_whitespace() || matches!(character, '&' | ',' | ';' | '?')
        })
        .filter_map(|segment| segment.split_once('=').or_else(|| segment.split_once(':')))
        .any(|(key, assigned)| {
            let key = key
                .chars()
                .filter(|character| character.is_ascii_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect::<String>();
            matches!(
                key.as_str(),
                "authorization"
                    | "cookie"
                    | "password"
                    | "passwd"
                    | "secret"
                    | "secretkey"
                    | "token"
                    | "accesstoken"
                    | "refreshtoken"
                    | "apikey"
                    | "xapikey"
                    | "accesskey"
                    | "clientsecret"
                    | "sessiontoken"
                    | "idtoken"
                    | "privatekey"
                    | "signingkey"
                    | "credential"
            ) && !assigned.trim().is_empty()
        })
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
            | "snapshot_stale"
            | "snapshot_range_partial"
            | "snapshot_range_unavailable"
            | "snapshot_boundary_mismatch"
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
    use devbox_applink::{HandoffEnvelope, PROTOCOL_VERSION};

    fn fixture() -> KnowledgeDraftPayload {
        let summary = KnowledgeDraftSummary {
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
        };
        let sources = vec![
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
        ];
        KnowledgeDraftPayload {
            schema_version: 1,
            title: "Life Log digest · 2026-08-10 ~ 2026-08-16".into(),
            body: render_body(&summary, &sources),
            tags: vec!["life-log".into(), "digest".into(), "week".into()],
            summary,
            sources,
        }
    }

    #[test]
    fn validates_fixture_and_rejects_tampering() {
        let mut payload = fixture();
        validate_knowledge_draft(&payload).unwrap();
        payload.body.push_str("\nraw credential");
        assert!(validate_knowledge_draft(&payload).is_err());
        payload.body = render_body(&payload.summary, &payload.sources);
        payload.summary.timezone = "/private/path".into();
        assert!(validate_knowledge_draft(&payload).is_err());
    }

    #[test]
    fn receiver_rejects_credential_shaped_metadata_with_fixed_errors() {
        let mut payload = fixture();
        payload.summary.top_app = Some("client_secret=must-not-cross".into());
        payload.body = render_body(&payload.summary, &payload.sources);
        let error = validate_knowledge_draft(&payload).unwrap_err();
        assert!(!error.contains("must-not-cross"));

        let mut payload = fixture();
        payload.summary.top_app = Some("X_API_KEY=must-not-cross".into());
        payload.body = render_body(&payload.summary, &payload.sources);
        assert!(validate_knowledge_draft(&payload).is_err());

        let mut payload = fixture();
        payload.summary.filter = Some(r"C:\Users\me\..\secret".into());
        payload.body = render_body(&payload.summary, &payload.sources);
        assert!(validate_knowledge_draft(&payload).is_err());

        let mut payload = fixture();
        payload.summary.timezone = "token=must-not-cross".into();
        payload.body = render_body(&payload.summary, &payload.sources);
        assert!(validate_knowledge_draft(&payload).is_err());
    }

    #[test]
    fn checked_in_wire_fixture_is_accepted() {
        let fixture = include_str!("../../tests/fixtures/knowledge-draft-v1.json");
        let payload: KnowledgeDraftPayload = serde_json::from_str(fixture).unwrap();
        validate_knowledge_draft(&payload).unwrap();
    }

    #[test]
    fn maps_claim_failures_to_fixed_messages() {
        assert!(map_claim_error(&HandoffError::Expired).contains("만료"));
        assert!(!map_claim_error(&HandoffError::Corrupt).contains("Corrupt"));
    }

    #[test]
    fn accepts_snapshot_stale_and_rejects_unbounded_freshness() {
        let mut payload = fixture();
        let knowledge = payload.sources.last_mut().unwrap();
        knowledge.available = false;
        knowledge.error_code = Some("snapshot_stale".into());
        payload.body = render_body(&payload.summary, &payload.sources);
        validate_knowledge_draft(&payload).unwrap();

        payload.sources.last_mut().unwrap().freshness_ms = Some(MAX_PROVENANCE_FRESHNESS_MS + 1);
        payload.body = render_body(&payload.summary, &payload.sources);
        assert!(validate_knowledge_draft(&payload).is_err());
    }

    #[test]
    fn toolbox_v2_claim_is_separate_from_the_life_log_contract() {
        let payload = ToolboxDraftPayload {
            schema_version: TOOLBOX_DRAFT_SCHEMA_VERSION,
            title: "Developer Toolbox result · 2026-08-30".into(),
            body: "safe\ntransformed output".into(),
            tags: vec!["developer-toolbox".into(), "draft".into()],
            created_date: "2026-08-30".into(),
        };
        let claim = HandoffClaim {
            envelope: HandoffEnvelope {
                protocol_version: PROTOCOL_VERSION,
                id: "a".repeat(32),
                kind: TOOLBOX_DRAFT_KIND.into(),
                source_app: "developer-toolbox".into(),
                target_app: Some("knowledge-base".into()),
                created_at_ms: 1,
                expires_at_ms: 10,
                payload: serde_json::to_value(&payload).unwrap(),
            },
            claim_token: "b".repeat(32),
            lease_until_ms: 9,
        };

        assert_eq!(
            parse_claim(&claim),
            Ok(IncomingKnowledgeDraft::Toolbox(payload.clone()))
        );
        let preview =
            KnowledgeDraftPreview::from_claim(&claim, &IncomingKnowledgeDraft::Toolbox(payload));
        assert_eq!(preview.kind, TOOLBOX_DRAFT_KIND);
        assert_eq!(preview.producer_id, "developer-toolbox");
        assert!(preview.summary.is_none());
        assert!(preview.sources.is_empty());

        let mut wrong_source = claim.clone();
        wrong_source.envelope.source_app = "life-log".into();
        assert!(parse_claim(&wrong_source).is_err());
        let mut wrong_shape = claim;
        wrong_shape.envelope.payload["summary"] = serde_json::json!({});
        assert!(parse_claim(&wrong_shape).is_err());
    }

    #[test]
    fn toolbox_v2_rejects_raw_credentials_and_unbounded_or_forged_metadata() {
        let fixture = || ToolboxDraftPayload {
            schema_version: TOOLBOX_DRAFT_SCHEMA_VERSION,
            title: "Developer Toolbox result · 2026-08-30".into(),
            body: "safe output".into(),
            tags: vec!["developer-toolbox".into(), "draft".into()],
            created_date: "2026-08-30".into(),
        };
        assert!(validate_toolbox_draft(&fixture()).is_ok());
        let mut raw = fixture();
        raw.body = "Bearer raw-value".into();
        assert!(validate_toolbox_draft(&raw).is_err());
        let mut forged = fixture();
        forged.tags.push("untrusted".into());
        assert!(validate_toolbox_draft(&forged).is_err());
        let mut invalid_date = fixture();
        invalid_date.created_date = "2026-02-29".into();
        assert!(validate_toolbox_draft(&invalid_date).is_err());
    }
}
