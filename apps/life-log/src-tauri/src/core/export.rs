//! Life Log의 로컬 export 계약.
//!
//! Export는 DB와 검증된 integration snapshot에서만 데이터를 읽는다. 이 모듈은
//! 저장 경로·대화상자·Tauri 상태를 알지 않으며, 같은 입력이면 세 형식 모두 같은
//! 순서와 같은 source metadata를 만든다. 파일 기록은 command 계층의 명시적인
//! 사용자 저장 action에서만 수행한다.

use crate::core::db;
use crate::core::models::Session;
use crate::core::privacy::{apply as apply_privacy, PrivacyRules};
use devbox_filesystem::{parse_safe_project_path, MAX_PROJECT_PATH_BYTES};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

pub const EXPORT_SCHEMA_VERSION: u32 = 1;
pub const MAX_EXPORT_DAYS: usize = 366;
pub const MAX_EXPORT_SESSIONS: usize = 50_000;
pub const MAX_EXPORT_BYTES: usize = 4 * 1024 * 1024;
pub const EXPORT_CSV_HEADER: &str = "record_type,date,range_start_date,range_end_date,id,app,title,start_ts_ms,end_ts_ms,duration_ms,project_path,commits,metric,value,source,available,schema_version,snapshot_version,producer_version,generated_at,freshness_ms,view,scope,error_code";
const DAY_MS: i64 = 86_400_000;
const MAX_TIMEZONE_BYTES: usize = 128;
const MAX_APP_BYTES: usize = 256;
const MAX_TITLE_BYTES: usize = 4 * 1024;
const MAX_PATH_BYTES: usize = MAX_PROJECT_PATH_BYTES;
const MAX_PROJECTS: usize = 64;
const MAX_GIT_OUTPUT_BYTES: usize = 256 * 1024;
const GIT_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_PRIVACY_JSON_BYTES: usize = 64 * 1024;
const MAX_PRIVACY_RULES: usize = 128;
const MAX_REGEX_BYTES: usize = 512;
const MAX_RUN_SERVICES: usize = 256;
const MAX_SERVICE_ID_BYTES: usize = 256;
const MAX_RUN_COUNT: i64 = 1_000_000_000;
const MAX_SERVICE_UPTIME_MS: i64 = DAY_MS * 366 * 100;
const MAX_NOTE_IDS: usize = 512;
const MAX_NOTE_ID_BYTES: usize = 128;
const MAX_NOTES_MODIFIED: u64 = 1_000_000_000;
const LATEST_SNAPSHOT_OUT_OF_RANGE_SCOPE: &str = "latest-snapshot-out-of-range";

/// Frontend가 전달하는 범위. `end_date`는 inclusive 날짜이고 `day_end`는
/// exclusive epoch millisecond 경계다. `day_boundaries`는 system-local 날짜의
/// 실제 경계를 담아 DST에서도 날짜별 집계를 보존한다.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportInput {
    pub start_date: String,
    pub end_date: String,
    pub timezone: String,
    pub day_start: i64,
    pub day_end: i64,
    pub day_boundaries: Vec<ExportDayBoundary>,
    pub format: ExportFormat,
}

/// 프론트엔드가 계산한 system-local civil-day 경계다. UTC의 고정 24시간을
/// 가정하지 않으므로 DST 전환일도 선택한 달력 날짜에 정확히 귀속할 수 있다.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportDayBoundary {
    pub date: String,
    pub start_ms: i64,
    pub end_ms: i64,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Markdown,
    Json,
    Csv,
}

impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Json => "json",
            Self::Csv => "csv",
        }
    }

    pub fn mime_type(self) -> &'static str {
        match self {
            Self::Markdown => "text/markdown;charset=utf-8",
            Self::Json => "application/json;charset=utf-8",
            Self::Csv => "text/csv;charset=utf-8",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportRange {
    pub start_date: String,
    pub end_date: String,
    pub timezone: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub day_boundaries: Vec<ExportDayBoundary>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportSession {
    pub id: i64,
    pub app: String,
    pub title: String,
    pub start_ts_ms: i64,
    pub end_ts_ms: i64,
    pub duration_ms: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportAppTotal {
    pub app: String,
    pub duration_ms: i64,
    pub sessions: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportGit {
    pub projects: Vec<ExportGitProject>,
    pub total_commits: u32,
    pub error_codes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportGitProject {
    pub path: String,
    pub commits: u32,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DailyDigest {
    pub date: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub pc_usage_ms: i64,
    pub session_count: usize,
    pub git_commits: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunDigest {
    pub success: i64,
    pub failed: i64,
    pub active_service_count: usize,
    pub active_service_uptime_ms: i64,
    pub last_run_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeDigest {
    pub notes_modified_today: u64,
    pub last_modified_at_ms: Option<i64>,
    pub identifiers_truncated: bool,
}

/// Export에 포함되는 source 상태. 현재 시각으로 freshness를 다시 계산하지 않고
/// snapshot의 generatedAt을 그대로 보존해 동일한 fixture가 동일한 결과를 낸다.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceMetadata {
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
pub struct ExportSummary {
    pub pc_usage_ms: i64,
    pub session_count: usize,
    pub app_totals: Vec<ExportAppTotal>,
    pub git: ExportGit,
    pub run: Option<RunDigest>,
    pub knowledge: Option<KnowledgeDigest>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportRules {
    pub session_window: String,
    pub session_duration: String,
    pub daily_buckets: String,
    pub privacy: String,
    pub app_totals: String,
    pub git_commits: String,
    pub snapshot_scope: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportDocument {
    pub schema_version: u32,
    pub range: ExportRange,
    pub rules: ExportRules,
    pub summary: ExportSummary,
    pub daily: Vec<DailyDigest>,
    pub sessions: Vec<ExportSession>,
    pub sources: Vec<SourceMetadata>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ExportOrigin {
    Native,
    BrowserPreview,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenderedExport {
    pub origin: ExportOrigin,
    pub format: ExportFormat,
    pub extension: String,
    pub mime_type: String,
    pub byte_length: usize,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ValidatedRange {
    start_date: DateKey,
    end_date: DateKey,
    start_ms: i64,
    end_ms: i64,
    timezone: String,
    days: Vec<ValidatedDayBoundary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ValidatedDayBoundary {
    date: DateKey,
    start_ms: i64,
    end_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct DateKey {
    year: i32,
    month: u32,
    day: u32,
}

impl DateKey {
    fn parse(value: &str) -> Result<Self, String> {
        let bytes = value.as_bytes();
        if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
            return Err("날짜 형식이 올바르지 않습니다".into());
        }
        if !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
        {
            return Err("날짜 형식이 올바르지 않습니다".into());
        }
        let year = value[0..4]
            .parse::<i32>()
            .map_err(|_| "날짜 형식이 올바르지 않습니다")?;
        let month = value[5..7]
            .parse::<u32>()
            .map_err(|_| "날짜 형식이 올바르지 않습니다")?;
        let day = value[8..10]
            .parse::<u32>()
            .map_err(|_| "날짜 형식이 올바르지 않습니다")?;
        if year < 1
            || !(1..=12).contains(&month)
            || !(1..=days_in_month(year, month)).contains(&day)
        {
            return Err("날짜 형식이 올바르지 않습니다".into());
        }
        Ok(Self { year, month, day })
    }

    fn as_string(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    fn next(self) -> Self {
        if self.day < days_in_month(self.year, self.month) {
            return Self {
                day: self.day + 1,
                ..self
            };
        }
        if self.month == 12 {
            Self {
                year: self.year + 1,
                month: 1,
                day: 1,
            }
        } else {
            Self {
                month: self.month + 1,
                day: 1,
                ..self
            }
        }
    }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn validate_range(input: &ExportInput) -> Result<ValidatedRange, String> {
    let start_date = DateKey::parse(&input.start_date)?;
    let end_date = DateKey::parse(&input.end_date)?;
    if input.timezone.trim().is_empty()
        || input.timezone.len() > MAX_TIMEZONE_BYTES
        || input.timezone.chars().any(char::is_control)
        || input.timezone != input.timezone.trim()
    {
        return Err("export 시간대가 올바르지 않습니다".into());
    }
    let span = input
        .day_end
        .checked_sub(input.day_start)
        .ok_or_else(|| "export 기간이 올바르지 않습니다".to_string())?;
    if start_date > end_date
        || span <= 0
        // DST/일광절약 전환으로 civil-day 범위가 24시간에서 벗어날 수 있다.
        // 하루 수 제한은 아래의 달력 날짜 계산으로 별도 검증한다.
        || span > DAY_MS.saturating_mul(MAX_EXPORT_DAYS as i64 + 1)
    {
        return Err("export 기간이 올바르지 않습니다".into());
    }
    let mut expected_days = 1usize;
    let mut date = start_date;
    while date < end_date {
        date = date.next();
        expected_days += 1;
        if expected_days > MAX_EXPORT_DAYS {
            return Err("export 기간이 올바르지 않습니다".into());
        }
    }
    if input.day_boundaries.len() != expected_days {
        return Err("export 기간이 올바르지 않습니다".into());
    }
    let mut days = Vec::with_capacity(expected_days);
    let mut expected_date = start_date;
    let mut previous_end = None;
    for boundary in &input.day_boundaries {
        let date = DateKey::parse(&boundary.date)?;
        let boundary_span = boundary
            .end_ms
            .checked_sub(boundary.start_ms)
            .ok_or_else(|| "export 날짜 경계가 올바르지 않습니다".to_string())?;
        if date != expected_date
            || boundary_span <= 0
            // The frontend owns timezone/DST conversion. A single civil day
            // may be 23/24/25 hours, but an arbitrary multi-day boundary is
            // not accepted as a substitute for a timezone calculation.
            || boundary_span > DAY_MS.saturating_mul(2)
            || previous_end.is_some_and(|value| value != boundary.start_ms)
        {
            return Err("export 날짜 경계가 올바르지 않습니다".into());
        }
        days.push(ValidatedDayBoundary {
            date,
            start_ms: boundary.start_ms,
            end_ms: boundary.end_ms,
        });
        previous_end = Some(boundary.end_ms);
        expected_date = expected_date.next();
    }
    if days
        .first()
        .is_none_or(|day| day.start_ms != input.day_start)
        || days.last().is_none_or(|day| day.end_ms != input.day_end)
    {
        return Err("export 날짜 경계가 기간과 일치하지 않습니다".into());
    }
    Ok(ValidatedRange {
        start_date,
        end_date,
        start_ms: input.day_start,
        end_ms: input.day_end,
        timezone: input.timezone.trim().to_string(),
        days,
    })
}

/// DB와 snapshot에서 await 이전에 수집할 데이터를 준비한다. Tauri command는
/// `PreparedExport`를 만든 뒤 DB mutex를 풀고 비동기 git 집계를 수행해야 한다.
pub fn prepare_document(
    conn: &Connection,
    projects: &[String],
    input: &ExportInput,
) -> Result<PreparedExport, String> {
    let range = validate_range(input)?;
    let rules = read_privacy_rules(conn);
    let rows =
        db::get_timeline_limited(conn, range.start_ms, range.end_ms, MAX_EXPORT_SESSIONS + 1)
            .map_err(|_| "Life Log 활동 데이터를 읽을 수 없습니다".to_string())?;
    if rows.len() > MAX_EXPORT_SESSIONS {
        return Err("export 활동 수가 제한을 초과했습니다".into());
    }
    let sessions = rows
        .into_iter()
        .map(|session| sanitized_session(session, &rules))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let session_text_bytes = sessions.iter().fold(0usize, |total, session| {
        total
            .saturating_add(session.app.len())
            .saturating_add(session.title.len())
    });
    if session_text_bytes > MAX_EXPORT_BYTES {
        return Err("export 결과가 크기 제한을 초과했습니다".into());
    }

    let mut safe_projects = projects
        .iter()
        .filter_map(|path| safe_project_path(path))
        .filter_map(|path| parse_safe_project_path(&path))
        .collect::<Vec<_>>();
    // The configured list is user-editable. Collapse duplicate paths before
    // invoking git so the total and per-project rows cannot double count the
    // same repository while still retaining deterministic byte ordering.
    safe_projects.sort_by(|left, right| {
        left.identity()
            .as_bytes()
            .cmp(right.identity().as_bytes())
            .then_with(|| left.as_str().as_bytes().cmp(right.as_str().as_bytes()))
    });
    safe_projects.dedup_by(|left, right| left.identity() == right.identity());
    if safe_projects.len() > MAX_PROJECTS {
        return Err("export 프로젝트 수 제한을 초과했습니다".into());
    }
    let safe_projects = safe_projects
        .into_iter()
        .map(|path| path.into_string())
        .collect::<Vec<_>>();
    Ok(PreparedExport {
        range,
        sessions,
        safe_projects,
        run_source: read_run_source(),
        knowledge_source: read_knowledge_source(),
    })
}

pub struct PreparedExport {
    range: ValidatedRange,
    sessions: Vec<ExportSession>,
    safe_projects: Vec<String>,
    run_source: SourceResult<RunDigest>,
    knowledge_source: SourceResult<KnowledgeDigest>,
}

/// 준비된 DB/snapshot 자료만 받아 export document를 만든다. 이 함수가
/// `Connection`을 보유하지 않으므로 Tauri async command가 안전하게 await할 수 있다.
pub async fn build_document(prepared: PreparedExport) -> Result<ExportDocument, String> {
    let PreparedExport {
        range,
        sessions,
        safe_projects,
        run_source,
        knowledge_source,
    } = prepared;
    // One bounded query per configured repository is enough. Git output is
    // post-filtered against the exact millisecond range, then assigned to the
    // supplied civil-day boundaries; this avoids a 366-day N+1 process storm.
    let git_projects = safe_projects.clone();
    let git_range = range.clone();
    let git = tokio::task::spawn_blocking(move || collect_git_export(&git_projects, &git_range))
        .await
        .map_err(|_| "git export 작업을 완료하지 못했습니다".to_string())?;

    let mut daily = Vec::with_capacity(range.days.len());
    for (day_index, day) in range.days.iter().enumerate() {
        let cursor = day.start_ms;
        let day_end = day.end_ms;
        let day_sessions = sessions
            .iter()
            .filter(|session| session.start_ts_ms >= cursor && session.start_ts_ms < day_end)
            .collect::<Vec<_>>();
        daily.push(DailyDigest {
            date: day.date.as_string(),
            start_ms: cursor,
            end_ms: day_end,
            pc_usage_ms: sum_durations(day_sessions.iter().map(|session| session.duration_ms)),
            session_count: day_sessions.len(),
            git_commits: git.daily_commits.get(day_index).copied().unwrap_or(0),
        });
    }

    let app_totals = build_app_totals(&sessions);
    let sources = vec![
        SourceMetadata {
            id: "life-log".into(),
            available: true,
            schema_version: Some(EXPORT_SCHEMA_VERSION),
            snapshot_version: None,
            producer_version: Some(env!("CARGO_PKG_VERSION").into()),
            generated_at: None,
            freshness_ms: None,
            view: None,
            scope: "requested-range".into(),
            error_code: None,
        },
        SourceMetadata {
            id: "git".into(),
            available: !safe_projects.is_empty() && git.error_codes.is_empty(),
            schema_version: None,
            snapshot_version: None,
            producer_version: None,
            generated_at: None,
            freshness_ms: None,
            view: None,
            scope: "requested-range".into(),
            error_code: if safe_projects.is_empty() {
                Some("no_safe_project_paths".into())
            } else {
                git.error_codes.first().cloned()
            },
        },
        run_source.metadata.clone(),
        knowledge_source.metadata.clone(),
    ];

    Ok(ExportDocument {
        schema_version: EXPORT_SCHEMA_VERSION,
        range: ExportRange {
            start_date: range.start_date.as_string(),
            end_date: range.end_date.as_string(),
            timezone: range.timezone,
            start_ms: range.start_ms,
            end_ms: range.end_ms,
            day_boundaries: range
                .days
                .iter()
                .map(|day| ExportDayBoundary {
                    date: day.date.as_string(),
                    start_ms: day.start_ms,
                    end_ms: day.end_ms,
                })
                .collect(),
        },
        rules: export_rules(),
        summary: ExportSummary {
            pc_usage_ms: sum_durations(sessions.iter().map(|session| session.duration_ms)),
            session_count: sessions.len(),
            app_totals,
            git: export_git(git),
            // Current Run Manager and Knowledge snapshots are “today/latest”
            // summaries, not range-keyed history. Keep their validated
            // provenance in `sources`, but never put those unrelated values
            // into a requested-date summary. A future range-scoped producer
            // can opt in by defining a matching snapshot contract.
            run: None,
            knowledge: None,
        },
        daily,
        sessions,
        sources,
    })
}

fn export_rules() -> ExportRules {
    ExportRules {
        session_window: "start_ts_ms >= range.startMs && start_ts_ms < range.endMs".into(),
        session_duration:
            "stored durationMs is retained; a session is assigned by start timestamp and is not clipped to the range".into(),
        daily_buckets:
            "daily rows use the supplied local civil-day boundaries; each session belongs to the bucket containing its start timestamp".into(),
        privacy: "current Life Log privacy rules and obvious credential markers are reapplied before aggregation".into(),
        app_totals: "sanitized sessions grouped by app; duration descending then app byte order"
            .into(),
        git_commits: "read-only git log with fixed argv per safe configured absolute project path; output is filtered to [range.startMs, range.endMs) and bounded by timeout/output limits".into(),
        snapshot_scope:
            "Run Manager and Knowledge currently expose only latest local snapshots; validated provenance is reported as latest-snapshot-out-of-range and their values are omitted from requested-range summary until a matching range-scoped snapshot exists".into(),
    }
}

pub fn render(document: &ExportDocument, format: ExportFormat) -> Result<RenderedExport, String> {
    let content = match format {
        ExportFormat::Markdown => render_markdown(document),
        ExportFormat::Json => {
            serde_json::to_string_pretty(document)
                .map_err(|_| "JSON export를 만들 수 없습니다".to_string())?
                + "\n"
        }
        ExportFormat::Csv => render_csv(document),
    };
    let byte_length = content.len();
    if byte_length > MAX_EXPORT_BYTES {
        return Err("export 결과가 크기 제한을 초과했습니다".into());
    }
    Ok(RenderedExport {
        origin: ExportOrigin::Native,
        format,
        extension: format.extension().into(),
        mime_type: format.mime_type().into(),
        byte_length,
        content,
    })
}

/// 저장 직전 JSON을 다시 읽었을 때 현재 `life-log/export/v1` producer가
/// 보장해야 하는 불변식을 확인한다. Renderer 내부 자료만 믿지 않고, future
/// renderer 변경이나 손상된 in-memory DTO가 native 파일 경계를 통과하지
/// 못하도록 command 계층에서 호출한다.
#[cfg(any(target_os = "windows", test))]
pub fn validate_document(document: &ExportDocument) -> bool {
    if document.schema_version != EXPORT_SCHEMA_VERSION
        || document.sources.len() != 4
        || document.summary.run.is_some()
        || document.summary.knowledge.is_some()
        || document.rules != export_rules()
    {
        return false;
    }
    let input = ExportInput {
        start_date: document.range.start_date.clone(),
        end_date: document.range.end_date.clone(),
        timezone: document.range.timezone.clone(),
        day_start: document.range.start_ms,
        day_end: document.range.end_ms,
        day_boundaries: document.range.day_boundaries.clone(),
        format: ExportFormat::Json,
    };
    let Ok(range) = validate_range(&input) else {
        return false;
    };
    if document.sessions.len() > MAX_EXPORT_SESSIONS
        || document.daily.len() != range.days.len()
        || document.summary.session_count != document.sessions.len()
        || document.summary.pc_usage_ms
            != sum_durations(document.sessions.iter().map(|session| session.duration_ms))
        || document.summary.app_totals != build_app_totals(&document.sessions)
    {
        return false;
    }
    if document.sessions.iter().any(|session| {
        session.id < 0
            || !is_bounded_export_text(&session.app, MAX_APP_BYTES, false)
            || !is_bounded_export_text(&session.title, MAX_TITLE_BYTES, true)
            || session.start_ts_ms < range.start_ms
            || session.start_ts_ms >= range.end_ms
            || session.end_ts_ms < session.start_ts_ms
            || session.duration_ms < 0
            || session.duration_ms > session.end_ts_ms.saturating_sub(session.start_ts_ms)
    }) {
        return false;
    }
    for (index, day) in document.daily.iter().enumerate() {
        let expected = &range.days[index];
        let expected_sessions = document
            .sessions
            .iter()
            .filter(|session| {
                session.start_ts_ms >= expected.start_ms && session.start_ts_ms < expected.end_ms
            })
            .collect::<Vec<_>>();
        if day.date != expected.date.as_string()
            || day.start_ms != expected.start_ms
            || day.end_ms != expected.end_ms
            || day.session_count != expected_sessions.len()
            || day.pc_usage_ms
                != sum_durations(expected_sessions.iter().map(|session| session.duration_ms))
        {
            return false;
        }
    }
    if document
        .daily
        .iter()
        .fold(0u32, |total, day| total.saturating_add(day.git_commits))
        != document.summary.git.total_commits
    {
        return false;
    }

    if document.summary.git.projects.len() > MAX_PROJECTS
        || !projects_are_deterministic(&document.summary.git.projects)
        || document.summary.git.total_commits
            != document
                .summary
                .git
                .projects
                .iter()
                .fold(0u32, |total, project| total.saturating_add(project.commits))
        || document.summary.git.error_codes.len() > MAX_PROJECTS
        || document
            .summary
            .git
            .error_codes
            .windows(2)
            .any(|pair| pair[0].as_bytes() >= pair[1].as_bytes())
        || document
            .summary
            .git
            .error_codes
            .iter()
            .any(|code| !is_safe_error_code(code))
        || document.summary.git.projects.iter().any(|project| {
            project
                .error_code
                .as_deref()
                .is_some_and(|code| !is_safe_error_code(code))
                || (project.error_code.is_some() && project.commits != 0)
        })
    {
        return false;
    }

    let mut expected_git_errors = document
        .summary
        .git
        .projects
        .iter()
        .filter_map(|project| project.error_code.clone())
        .collect::<Vec<_>>();
    expected_git_errors.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    expected_git_errors.dedup();
    if document.summary.git.error_codes != expected_git_errors {
        return false;
    }

    let [life_log, git, run_manager, knowledge_base] = document.sources.as_slice() else {
        return false;
    };
    valid_life_log_source(life_log)
        && valid_git_source(git, &document.summary.git)
        && valid_snapshot_source(run_manager, "run-manager", false)
        && valid_snapshot_source(knowledge_base, "knowledge-base", true)
}

#[cfg(any(target_os = "windows", test))]
fn valid_life_log_source(source: &SourceMetadata) -> bool {
    source.id == "life-log"
        && source.available
        && source.schema_version == Some(EXPORT_SCHEMA_VERSION)
        && source.snapshot_version.is_none()
        && source.producer_version.as_deref() == Some(env!("CARGO_PKG_VERSION"))
        && source.generated_at.is_none()
        && source.freshness_ms.is_none()
        && source.view.is_none()
        && source.scope == "requested-range"
        && source.error_code.is_none()
}

#[cfg(any(target_os = "windows", test))]
fn valid_git_source(source: &SourceMetadata, git: &ExportGit) -> bool {
    let expected_error = if git.projects.is_empty() {
        Some("no_safe_project_paths")
    } else {
        git.error_codes.first().map(String::as_str)
    };
    source.id == "git"
        && source.available == expected_error.is_none()
        && source.schema_version.is_none()
        && source.snapshot_version.is_none()
        && source.producer_version.is_none()
        && source.generated_at.is_none()
        && source.freshness_ms.is_none()
        && source.view.is_none()
        && source.scope == "requested-range"
        && source.error_code.as_deref() == expected_error
        && source.error_code.as_deref().is_none_or(is_git_error_code)
}

#[cfg(any(target_os = "windows", test))]
fn valid_snapshot_source(source: &SourceMetadata, expected_id: &str, knowledge: bool) -> bool {
    let provenance_complete = source.producer_version.is_some()
        && source.generated_at.is_some()
        && source.freshness_ms.is_some();
    let provenance_empty = source.producer_version.is_none()
        && source.generated_at.is_none()
        && source.freshness_ms.is_none();
    source.id == expected_id
        && source.available == source.error_code.is_none()
        && source.scope == LATEST_SNAPSHOT_OUT_OF_RANGE_SCOPE
        && source.schema_version.is_none_or(|version| version > 0)
        && source.snapshot_version.is_none_or(|version| version > 0)
        && source
            .schema_version
            .is_none_or(|schema| source.snapshot_version == Some(schema))
        && (provenance_complete || provenance_empty)
        && (!provenance_complete
            || (source.schema_version.is_some() && source.snapshot_version.is_some()))
        && (!provenance_empty || (source.schema_version.is_none() && source.view.is_none()))
        && source
            .producer_version
            .as_deref()
            .is_none_or(is_valid_producer_version)
        && source
            .generated_at
            .as_deref()
            .is_none_or(is_valid_generated_at)
        && source
            .view
            .as_deref()
            .is_none_or(|value| knowledge && matches!(value, "activity" | "legacy-data"))
        && source
            .error_code
            .as_deref()
            .is_none_or(is_snapshot_error_code)
        && (!source.available
            || (source.schema_version == Some(EXPORT_SCHEMA_VERSION)
                && source.snapshot_version == Some(EXPORT_SCHEMA_VERSION)
                && provenance_complete
                && (!knowledge || source.view.is_some())))
}

#[cfg(any(target_os = "windows", test))]
fn projects_are_deterministic(projects: &[ExportGitProject]) -> bool {
    let mut previous_identity: Option<String> = None;
    for project in projects {
        let Some(path) = parse_safe_project_path(&project.path) else {
            return false;
        };
        if path.as_str() != project.path
            || previous_identity
                .as_deref()
                .is_some_and(|previous| previous.as_bytes() >= path.identity().as_bytes())
        {
            return false;
        }
        previous_identity = Some(path.identity().to_owned());
    }
    true
}

#[cfg(any(target_os = "windows", test))]
fn is_bounded_export_text(value: &str, max_bytes: usize, preserve_newlines: bool) -> bool {
    bounded_text(value, max_bytes, preserve_newlines) == value
}

#[cfg(any(target_os = "windows", test))]
fn is_bounded_metadata_text(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && value.chars().all(|character| !character.is_control())
}

#[cfg(any(target_os = "windows", test))]
fn is_valid_producer_version(value: &str) -> bool {
    is_bounded_metadata_text(value, 64)
        && value.is_ascii()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
}

#[cfg(any(target_os = "windows", test))]
fn is_valid_generated_at(value: &str) -> bool {
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
        && DateKey::parse(&value[..10]).is_ok()
        && value[11..13].parse::<u32>().is_ok_and(|hour| hour <= 23)
        && value[14..16]
            .parse::<u32>()
            .is_ok_and(|minute| minute <= 59)
        && value[17..19]
            .parse::<u32>()
            .is_ok_and(|second| second <= 59)
}

#[cfg(any(target_os = "windows", test))]
fn is_safe_error_code(value: &str) -> bool {
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

#[cfg(any(target_os = "windows", test))]
fn is_git_error_code(value: &str) -> bool {
    matches!(
        value,
        "no_safe_project_paths"
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

#[cfg(any(target_os = "windows", test))]
fn is_snapshot_error_code(value: &str) -> bool {
    matches!(
        value,
        "snapshot_unavailable"
            | "snapshot_invalid"
            | "snapshot_schema_unsupported"
            | "snapshot_payload_invalid"
            | "snapshot_changed_during_read"
    )
}

fn read_privacy_rules(conn: &Connection) -> PrivacyRules {
    let raw = db::get_setting(conn, "privacy_rules", "{}");
    if raw.len() > MAX_PRIVACY_JSON_BYTES {
        return privacy_fail_closed();
    }
    let rules = match serde_json::from_str::<PrivacyRules>(&raw) {
        Ok(rules) => rules,
        Err(_) => return privacy_fail_closed(),
    };
    if rules.excluded_processes.len() > MAX_PRIVACY_RULES
        || rules.excluded_title_patterns.len() > MAX_PRIVACY_RULES
        || rules.redact_title_patterns.len() > MAX_PRIVACY_RULES
        || rules
            .excluded_processes
            .iter()
            .any(|value| !bounded_rule_text(value, MAX_APP_BYTES))
        || rules
            .excluded_title_patterns
            .iter()
            .chain(rules.redact_title_patterns.iter())
            .any(|value| {
                !bounded_rule_text(value, MAX_REGEX_BYTES) || regex::Regex::new(value).is_err()
            })
    {
        return privacy_fail_closed();
    }
    rules
}

fn privacy_fail_closed() -> PrivacyRules {
    PrivacyRules {
        mask_all_titles: true,
        ..PrivacyRules::default()
    }
}

fn bounded_rule_text(value: &str, max_bytes: usize) -> bool {
    !value.is_empty() && value.len() <= max_bytes && !value.chars().any(char::is_control)
}

fn sanitized_session(
    session: Session,
    rules: &PrivacyRules,
) -> Result<Option<ExportSession>, String> {
    if session.id < 0
        || session.end_ts < session.start_ts
        || session.duration_ms < 0
        || session.duration_ms > session.end_ts.saturating_sub(session.start_ts)
    {
        return Err("export 활동 데이터가 올바르지 않습니다".into());
    }
    // Bound untrusted DB text before regex/privacy work so a malformed local
    // row cannot make export allocate proportional to an unbounded title.
    let app_input = bounded_text(&session.app, MAX_APP_BYTES, false);
    let title_input = bounded_text(&session.title, MAX_TITLE_BYTES, true);
    let Some((app, title)) = apply_privacy(rules, &app_input, &title_input) else {
        return Ok(None);
    };
    let app = bounded_text(&redact_obvious_secret(&app), MAX_APP_BYTES, false);
    let title = bounded_text(&redact_obvious_secret(&title), MAX_TITLE_BYTES, true);
    Ok(Some(ExportSession {
        id: session.id,
        app,
        title,
        start_ts_ms: session.start_ts,
        end_ts_ms: session.end_ts,
        duration_ms: session.duration_ms,
    }))
}

/// DB 세션 제목은 수집 시점의 설정만으로 모든 credential 형식을 알 수 없다.
/// 명백한 bearer/basic·known token prefix·key assignment·private-key marker는
/// export 경계에서 한 번 더 전체 제목을 `[redacted]`로 줄인다. 원문은 로그나
/// error에도 전달하지 않는다.
fn redact_obvious_secret(value: &str) -> String {
    let lower = value.to_ascii_lowercase();
    let has_assignment = [
        "password",
        "passwd",
        "secret",
        "token",
        "access_token",
        "refresh_token",
        "api_key",
        "apikey",
        "client_secret",
        "credential",
        "authorization",
    ]
    .iter()
    .any(|key| secret_assignment(&lower, key));
    let has_token_prefix = [
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
    ]
    .iter()
    .any(|marker| lower.contains(marker));
    let has_private_key = lower.contains("-----begin ") && lower.contains(" key-----");
    if has_assignment || has_token_prefix || has_private_key {
        "[redacted]".into()
    } else {
        value.to_string()
    }
}

fn secret_assignment(value: &str, key: &str) -> bool {
    let mut offset = 0usize;
    while let Some(found) = value[offset..].find(key) {
        let end = offset + found + key.len();
        let suffix = value[end..].trim_start_matches(|character: char| {
            character == '=' || character == ':' || character.is_ascii_whitespace()
        });
        if value[end..].chars().next().is_some_and(|character| {
            character == '=' || character == ':' || character.is_ascii_whitespace()
        }) && !suffix.is_empty()
        {
            return true;
        }
        offset = end;
        if offset >= value.len() {
            break;
        }
    }
    false
}

fn build_app_totals(sessions: &[ExportSession]) -> Vec<ExportAppTotal> {
    let mut totals = BTreeMap::<String, (i64, usize)>::new();
    for session in sessions {
        let entry = totals.entry(session.app.clone()).or_default();
        entry.0 = entry.0.saturating_add(session.duration_ms);
        entry.1 += 1;
    }
    let mut output = totals
        .into_iter()
        .map(|(app, (duration_ms, sessions))| ExportAppTotal {
            app,
            duration_ms,
            sessions,
        })
        .collect::<Vec<_>>();
    output.sort_by(|left, right| {
        right
            .duration_ms
            .cmp(&left.duration_ms)
            .then_with(|| left.app.as_bytes().cmp(right.app.as_bytes()))
    });
    output
}

#[derive(Debug, Default)]
struct GitExport {
    projects: Vec<ExportGitProject>,
    total_commits: u32,
    daily_commits: Vec<u32>,
    error_codes: Vec<String>,
}

fn collect_git_export(projects: &[String], range: &ValidatedRange) -> GitExport {
    let mut output = GitExport {
        daily_commits: vec![0; range.days.len()],
        ..GitExport::default()
    };
    for path in projects {
        let since = format!("--since=@{}", range.start_ms.div_euclid(1_000));
        let before = format!("--before=@{}", ceil_seconds(range.end_ms));
        // Keep every argument as an individual argv value. In particular,
        // paths and date bounds never pass through a shell or shell quoting.
        let args = [
            "--no-pager",
            "log",
            since.as_str(),
            before.as_str(),
            "--format=%ct",
            "--",
        ];
        let result = devbox_git::run_bounded(&args, path, GIT_TIMEOUT, MAX_GIT_OUTPUT_BYTES)
            .and_then(|stdout| {
                parse_git_timestamps(&stdout, range, &mut output.daily_commits)
                    .map_err(ToOwned::to_owned)
            });
        let (commits, error_code) = match result {
            Ok(commits) => (commits, None),
            Err(code) => {
                output.error_codes.push(code.clone());
                (0, Some(code))
            }
        };
        output.projects.push(ExportGitProject {
            path: path.clone(),
            commits,
            error_code,
        });
        output.total_commits = output.total_commits.saturating_add(commits);
    }
    output
        .error_codes
        .sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    output.error_codes.dedup();
    output
}

fn ceil_seconds(milliseconds: i64) -> i64 {
    let seconds = milliseconds.div_euclid(1_000);
    if milliseconds.rem_euclid(1_000) == 0 {
        seconds
    } else {
        seconds.saturating_add(1)
    }
}

fn parse_git_timestamps(
    stdout: &str,
    range: &ValidatedRange,
    daily_commits: &mut [u32],
) -> Result<u32, &'static str> {
    let mut total = 0u32;
    let mut increments = vec![0u32; daily_commits.len()];
    for line in stdout.lines().filter(|line| !line.is_empty()) {
        let seconds = line
            .trim()
            .parse::<i64>()
            .map_err(|_| "git_output_invalid")?;
        let timestamp = seconds.checked_mul(1_000).ok_or("git_output_invalid")?;
        // Git's --since/--before filters are second-granularity. The final
        // inclusion decision is therefore always made against the exact
        // requested millisecond half-open range here.
        if timestamp < range.start_ms || timestamp >= range.end_ms {
            continue;
        }
        total = total.saturating_add(1);
        let index = range.days.binary_search_by(|day| {
            if timestamp < day.start_ms {
                Ordering::Greater
            } else if timestamp >= day.end_ms {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        });
        if let Ok(index) = index {
            increments[index] = increments[index].saturating_add(1);
        } else {
            // The range and every boundary are validated as contiguous. A
            // timestamp in the range must be in exactly one bucket; rejecting
            // an impossible result is safer than silently mis-bucketing it.
            return Err("git_output_invalid");
        }
    }
    for (current, increment) in daily_commits.iter_mut().zip(increments) {
        *current = (*current).saturating_add(increment);
    }
    Ok(total)
}

fn export_git(git: GitExport) -> ExportGit {
    ExportGit {
        projects: git.projects,
        total_commits: git.total_commits,
        error_codes: git.error_codes,
    }
}

fn safe_project_path(value: &str) -> Option<String> {
    let path = parse_safe_project_path(value)?;
    if path.as_str().len() > MAX_PATH_BYTES || redact_obvious_secret(path.as_str()) != path.as_str()
    {
        return None;
    }
    Some(path.into_string())
}

fn bounded_text(value: &str, max_bytes: usize, preserve_newlines: bool) -> String {
    let mut output = String::with_capacity(value.len().min(max_bytes));
    let mut truncated = false;
    for character in value.chars() {
        let character = if character == '\0'
            || (character.is_control() && !(preserve_newlines && matches!(character, '\r' | '\n')))
        {
            ' '
        } else {
            character
        };
        if output.len().saturating_add(character.len_utf8()) > max_bytes {
            truncated = true;
            break;
        }
        output.push(character);
    }
    if truncated {
        // Keep the ellipsis inside the advertised byte bound, even when the
        // last source character is multibyte UTF-8.
        let ellipsis = '…';
        let reserve = ellipsis.len_utf8();
        while output.len().saturating_add(reserve) > max_bytes {
            output.pop();
        }
        if max_bytes >= reserve {
            output.push(ellipsis);
        }
    }
    output
}

fn csv_cell(value: impl AsRef<str>) -> String {
    let value = value.as_ref();
    if value
        .bytes()
        .any(|byte| matches!(byte, b',' | b'"' | b'\r' | b'\n'))
    {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn render_markdown(document: &ExportDocument) -> String {
    let mut out = String::new();
    out.push_str("# Life Log digest\n\n");
    out.push_str(&format!(
        "- Range: `{}` to `{}` ({} ≤ start < {})\n",
        document.range.start_date,
        document.range.end_date,
        document.range.start_ms,
        document.range.end_ms
    ));
    out.push_str(&format!(
        "- Timezone: {}\n",
        markdown_cell(&document.range.timezone)
    ));
    out.push_str(&format!("- Export schema: `{}`\n", document.schema_version));
    out.push_str("\n## Aggregation rules\n\n| Rule | Definition |\n| --- | --- |\n");
    for (name, rule) in [
        ("Session window", &document.rules.session_window),
        ("Session duration", &document.rules.session_duration),
        ("Daily buckets", &document.rules.daily_buckets),
        ("Privacy", &document.rules.privacy),
        ("App totals", &document.rules.app_totals),
        ("Git commits", &document.rules.git_commits),
        ("Snapshot scope", &document.rules.snapshot_scope),
    ] {
        push_md_row(&mut out, name, rule);
    }
    out.push('\n');

    out.push_str("## Summary\n\n| Metric | Value |\n| --- | ---: |\n");
    push_md_row(
        &mut out,
        "PC usage (ms)",
        &document.summary.pc_usage_ms.to_string(),
    );
    push_md_row(
        &mut out,
        "Sessions",
        &document.summary.session_count.to_string(),
    );
    push_md_row(
        &mut out,
        "Git commits",
        &document.summary.git.total_commits.to_string(),
    );
    if let Some(run) = &document.summary.run {
        push_md_row(
            &mut out,
            "Run Manager successful runs",
            &run.success.to_string(),
        );
        push_md_row(&mut out, "Run Manager failed runs", &run.failed.to_string());
        push_md_row(
            &mut out,
            "Run Manager active services",
            &run.active_service_count.to_string(),
        );
        push_md_row(
            &mut out,
            "Run Manager active uptime (ms)",
            &run.active_service_uptime_ms.to_string(),
        );
        push_md_row(
            &mut out,
            "Run Manager last run (ms)",
            &run.last_run_at_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".into()),
        );
    }
    if let Some(knowledge) = &document.summary.knowledge {
        push_md_row(
            &mut out,
            "Knowledge notes modified today",
            &knowledge.notes_modified_today.to_string(),
        );
        push_md_row(
            &mut out,
            "Knowledge last modified (ms)",
            &knowledge
                .last_modified_at_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".into()),
        );
        push_md_row(
            &mut out,
            "Knowledge identifiers truncated",
            if knowledge.identifiers_truncated {
                "true"
            } else {
                "false"
            },
        );
    }
    out.push('\n');

    out.push_str("## Daily digest\n\n| Date | Start (ms) | End (ms) | PC usage (ms) | Sessions | Git commits |\n| --- | ---: | ---: | ---: | ---: | ---: |\n");
    for day in &document.daily {
        out.push('|');
        for value in [
            day.date.clone(),
            day.start_ms.to_string(),
            day.end_ms.to_string(),
            day.pc_usage_ms.to_string(),
            day.session_count.to_string(),
            day.git_commits.to_string(),
        ] {
            out.push(' ');
            out.push_str(&markdown_cell(&value));
            out.push_str(" |");
        }
        out.push('\n');
    }
    out.push('\n');

    out.push_str("## Applications\n\n| App | Duration (ms) | Sessions |\n| --- | ---: | ---: |\n");
    for app in &document.summary.app_totals {
        out.push_str(&format!(
            "| {} | {} | {} |\n",
            markdown_cell(&app.app),
            app.duration_ms,
            app.sessions
        ));
    }
    out.push('\n');

    out.push_str(
        "## Git projects\n\n| Project path | Commits | Error code |\n| --- | ---: | --- |\n",
    );
    for project in &document.summary.git.projects {
        out.push_str(&format!(
            "| {} | {} | {} |\n",
            markdown_cell(&project.path),
            project.commits,
            markdown_cell(project.error_code.as_deref().unwrap_or("-")),
        ));
    }
    if !document.summary.git.error_codes.is_empty() {
        push_md_row(
            &mut out,
            "Git error codes",
            &document.summary.git.error_codes.join(", "),
        );
    }
    out.push('\n');

    out.push_str("## Sources\n\n| Source | Available | Schema | Snapshot | Producer | Generated at | Freshness (ms) | View | Scope | Error code |\n| --- | --- | ---: | ---: | --- | --- | ---: | --- | --- | --- |\n");
    for source in &document.sources {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            markdown_cell(&source.id),
            source.available,
            source
                .schema_version
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".into()),
            source
                .snapshot_version
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".into()),
            markdown_cell(source.producer_version.as_deref().unwrap_or("-")),
            markdown_cell(source.generated_at.as_deref().unwrap_or("-")),
            source
                .freshness_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".into()),
            markdown_cell(source.view.as_deref().unwrap_or("-")),
            markdown_cell(&source.scope),
            markdown_cell(source.error_code.as_deref().unwrap_or("-")),
        ));
    }
    out.push('\n');

    out.push_str("## Sessions\n\n| ID | App | Title | Start (ms) | End (ms) | Duration (ms) |\n| ---: | --- | --- | ---: | ---: | ---: |\n");
    for session in &document.sessions {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            session.id,
            markdown_cell(&session.app),
            markdown_cell(&session.title),
            session.start_ts_ms,
            session.end_ts_ms,
            session.duration_ms
        ));
    }
    out
}

fn push_md_row(out: &mut String, label: &str, value: &str) {
    out.push_str(&format!(
        "| {} | {} |\n",
        markdown_cell(label),
        markdown_cell(value)
    ));
}

fn markdown_cell(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace(['\r', '\n'], " ")
        .replace('`', "\\`")
}

fn render_csv(document: &ExportDocument) -> String {
    // 하나의 CSV에 서로 다른 행 종류를 담되, 고정 header와 record_type으로
    // spreadsheet/스크립트가 source별 행을 안정적으로 분리할 수 있게 한다.
    const COLUMN_COUNT: usize = 24;
    let mut out = EXPORT_CSV_HEADER.to_string();
    out.push_str("\r\n");
    let range_start = document.range.start_date.as_str();
    let range_end = document.range.end_date.as_str();
    let mut row = |values: Vec<String>| {
        debug_assert_eq!(values.len(), COLUMN_COUNT);
        out.push_str(
            &values
                .into_iter()
                .map(csv_cell)
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push_str("\r\n");
    };
    let mut summary_rows = vec![
        (
            "export_schema_version".to_string(),
            document.schema_version.to_string(),
        ),
        (
            "pc_usage_ms".to_string(),
            document.summary.pc_usage_ms.to_string(),
        ),
        (
            "session_count".to_string(),
            document.summary.session_count.to_string(),
        ),
        (
            "git_commits".to_string(),
            document.summary.git.total_commits.to_string(),
        ),
        (
            "rule_session_window".to_string(),
            document.rules.session_window.clone(),
        ),
        (
            "rule_session_duration".to_string(),
            document.rules.session_duration.clone(),
        ),
        (
            "rule_daily_buckets".to_string(),
            document.rules.daily_buckets.clone(),
        ),
        ("rule_privacy".to_string(), document.rules.privacy.clone()),
        (
            "rule_app_totals".to_string(),
            document.rules.app_totals.clone(),
        ),
        (
            "rule_git_commits".to_string(),
            document.rules.git_commits.clone(),
        ),
        (
            "rule_snapshot_scope".to_string(),
            document.rules.snapshot_scope.clone(),
        ),
    ];
    if let Some(run) = &document.summary.run {
        summary_rows.extend([
            ("run_success".to_string(), run.success.to_string()),
            ("run_failed".to_string(), run.failed.to_string()),
            (
                "run_active_service_count".to_string(),
                run.active_service_count.to_string(),
            ),
            (
                "run_active_service_uptime_ms".to_string(),
                run.active_service_uptime_ms.to_string(),
            ),
            (
                "run_last_run_at_ms".to_string(),
                run.last_run_at_ms
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
            ),
        ]);
    }
    if let Some(knowledge) = &document.summary.knowledge {
        summary_rows.extend([
            (
                "knowledge_notes_modified_today".to_string(),
                knowledge.notes_modified_today.to_string(),
            ),
            (
                "knowledge_last_modified_at_ms".to_string(),
                knowledge
                    .last_modified_at_ms
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
            ),
            (
                "knowledge_identifiers_truncated".to_string(),
                knowledge.identifiers_truncated.to_string(),
            ),
        ]);
    }
    for (metric, value) in summary_rows {
        row(vec![
            "summary".into(),
            "".into(),
            range_start.into(),
            range_end.into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            metric,
            value,
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
        ]);
    }
    for day in &document.daily {
        row(vec![
            "daily".into(),
            day.date.clone(),
            range_start.into(),
            range_end.into(),
            "".into(),
            "".into(),
            "".into(),
            day.start_ms.to_string(),
            day.end_ms.to_string(),
            day.pc_usage_ms.to_string(),
            "".into(),
            day.git_commits.to_string(),
            "session_count".into(),
            day.session_count.to_string(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
        ]);
    }
    for app in &document.summary.app_totals {
        row(vec![
            "app".into(),
            "".into(),
            range_start.into(),
            range_end.into(),
            "".into(),
            app.app.clone(),
            "".into(),
            "".into(),
            "".into(),
            app.duration_ms.to_string(),
            "".into(),
            "".into(),
            "sessions".into(),
            app.sessions.to_string(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
        ]);
    }
    for project in &document.summary.git.projects {
        row(vec![
            "git".into(),
            "".into(),
            range_start.into(),
            range_end.into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            project.path.clone(),
            project.commits.to_string(),
            "".into(),
            "".into(),
            "git".into(),
            if project.error_code.is_none() {
                "true".into()
            } else {
                "false".into()
            },
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "requested-range".into(),
            project.error_code.clone().unwrap_or_default(),
        ]);
    }
    for session in &document.sessions {
        row(vec![
            "session".into(),
            "".into(),
            range_start.into(),
            range_end.into(),
            session.id.to_string(),
            session.app.clone(),
            session.title.clone(),
            session.start_ts_ms.to_string(),
            session.end_ts_ms.to_string(),
            session.duration_ms.to_string(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
        ]);
    }
    for source in &document.sources {
        row(vec![
            "source".into(),
            "".into(),
            range_start.into(),
            range_end.into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            source.id.clone(),
            if source.available {
                "true".into()
            } else {
                "false".into()
            },
            source
                .schema_version
                .map(|value| value.to_string())
                .unwrap_or_default(),
            source
                .snapshot_version
                .map(|value| value.to_string())
                .unwrap_or_default(),
            source.producer_version.clone().unwrap_or_default(),
            source.generated_at.clone().unwrap_or_default(),
            source
                .freshness_ms
                .map(|value| value.to_string())
                .unwrap_or_default(),
            source.view.clone().unwrap_or_default(),
            source.scope.clone(),
            source.error_code.clone().unwrap_or_default(),
        ]);
    }
    out
}

#[derive(Debug)]
struct SourceResult<T> {
    metadata: SourceMetadata,
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "latest-only payload is validated but intentionally omitted from a range export"
        )
    )]
    digest: Option<T>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RunSnapshot {
    #[serde(default)]
    active_services: Vec<RunService>,
    runs: RunCounts,
    last_run_at_ms: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RunService {
    id: String,
    uptime_ms: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RunCounts {
    success: i64,
    failed: i64,
}

fn read_run_source() -> SourceResult<RunDigest> {
    read_run_source_in(&devbox_integration::integration_root())
}

fn read_run_source_in(root: &Path) -> SourceResult<RunDigest> {
    read_snapshot_source_in(
        root,
        "run-manager",
        LATEST_SNAPSHOT_OUT_OF_RANGE_SCOPE,
        |envelope| {
            let payload: RunSnapshot = serde_json::from_value(envelope.data.clone())
                .map_err(|_| "snapshot_payload_invalid".to_string())?;
            let mut service_ids = BTreeSet::new();
            if payload.active_services.len() > MAX_RUN_SERVICES
                || payload.runs.success < 0
                || payload.runs.failed < 0
                || payload.runs.success > MAX_RUN_COUNT
                || payload.runs.failed > MAX_RUN_COUNT
                || payload.last_run_at_ms.is_some_and(|value| value < 0)
                || payload.active_services.iter().any(|service| {
                    service.id.is_empty()
                        || service.id.len() > MAX_SERVICE_ID_BYTES
                        || service.id.chars().any(char::is_control)
                        || service.uptime_ms < 0
                        || service.uptime_ms > MAX_SERVICE_UPTIME_MS
                        || !service_ids.insert(service.id.as_str())
                })
            {
                return Err("snapshot_payload_invalid".into());
            }
            Ok(RunDigest {
                success: payload.runs.success,
                failed: payload.runs.failed,
                active_service_count: payload.active_services.len(),
                active_service_uptime_ms: sum_durations(
                    payload
                        .active_services
                        .iter()
                        .map(|service| service.uptime_ms),
                ),
                last_run_at_ms: payload.last_run_at_ms,
            })
        },
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct KnowledgeSnapshot {
    notes_modified_today: u64,
    last_modified_at_ms: Option<i64>,
    note_ids: Vec<String>,
    identifiers_truncated: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyKnowledgeSnapshot {
    notes_modified_today: u64,
    last_modified_at_ms: Option<i64>,
}

fn read_knowledge_source() -> SourceResult<KnowledgeDigest> {
    read_knowledge_source_in(&devbox_integration::integration_root())
}

fn read_knowledge_source_in(root: &Path) -> SourceResult<KnowledgeDigest> {
    let producer = "knowledge-base";
    let reference = match latest_snapshot_reference(root, producer) {
        Ok(Some(reference)) => reference,
        Ok(None) => {
            return unavailable_source(
                producer,
                LATEST_SNAPSHOT_OUT_OF_RANGE_SCOPE,
                "snapshot_unavailable",
            )
        }
        Err(selection) => {
            return unavailable_source_with_version(
                producer,
                LATEST_SNAPSHOT_OUT_OF_RANGE_SCOPE,
                selection.version,
                selection.code,
            )
        }
    };
    let initial_view = reference
        .views
        .iter()
        .find(|view| view.kind == "activity")
        .map(|view| view.kind.as_str());
    let mut metadata = source_metadata_from_reference(
        &reference,
        LATEST_SNAPSHOT_OUT_OF_RANGE_SCOPE,
        initial_view,
    );
    if reference.version != EXPORT_SCHEMA_VERSION {
        return invalid_source(metadata, "snapshot_schema_unsupported");
    }
    let envelope = match devbox_integration::read_snapshot_in(root, producer, reference.version) {
        Ok(Some(envelope)) => envelope,
        Ok(None) => return invalid_source(metadata, "snapshot_unavailable"),
        Err(_) => return invalid_source(metadata, "snapshot_invalid"),
    };
    if !same_snapshot_identity(&reference, &envelope) {
        return invalid_source(metadata, "snapshot_changed_during_read");
    }
    if !snapshot_payload_is_stable(root, producer, &reference, &envelope) {
        return invalid_source(metadata, "snapshot_changed_during_read");
    }
    if !snapshot_is_still_selected(root, producer, &reference) {
        return invalid_source(metadata, "snapshot_changed_during_read");
    }
    let views = match envelope.views() {
        Ok(views) => views,
        Err(_) => return invalid_source(metadata, "snapshot_invalid"),
    };
    let payload = if views.is_empty() {
        metadata.view = Some("legacy-data".into());
        serde_json::from_value::<LegacyKnowledgeSnapshot>(envelope.data)
            .map(|value| (value.notes_modified_today, value.last_modified_at_ms, false))
    } else {
        if views.len() != 1 || !views.contains_key("activity") {
            return invalid_source(metadata, "snapshot_schema_unsupported");
        }
        metadata.view = Some("activity".into());
        if reference.views.len() != 1 {
            return invalid_source(metadata, "snapshot_schema_unsupported");
        }
        let Some(view_ref) = reference.views.iter().find(|view| view.kind == "activity") else {
            return invalid_source(metadata, "snapshot_schema_unsupported");
        };
        let Some(view) = views.get("activity") else {
            return invalid_source(metadata, "snapshot_schema_unsupported");
        };
        if view.schema_version != 1
            || view.entries.len() != 1
            || view_ref.schema_version != view.schema_version
            || view_ref.entry_count != view.entries.len()
        {
            return invalid_source(metadata, "snapshot_schema_unsupported");
        }
        metadata.freshness_ms = Some(view_ref.freshness_ms);
        serde_json::from_value::<KnowledgeSnapshot>(view.entries[0].clone()).and_then(|value| {
            if value.note_ids.len() > MAX_NOTE_IDS
                || value.notes_modified_today > MAX_NOTES_MODIFIED
                || value
                    .last_modified_at_ms
                    .is_some_and(|timestamp| timestamp < 0)
            {
                return Err(serde_json::Error::io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "invalid activity bounds",
                )));
            }
            let mut ids = value.note_ids.clone();
            ids.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
            if ids.windows(2).any(|pair| pair[0] == pair[1])
                || ids.iter().any(|id| {
                    id.len() > MAX_NOTE_ID_BYTES
                        || id.chars().any(char::is_control)
                        || !valid_note_id(id)
                })
                || value.notes_modified_today < ids.len() as u64
                || (!value.identifiers_truncated && value.notes_modified_today != ids.len() as u64)
            {
                return Err(serde_json::Error::io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "invalid activity identifiers",
                )));
            }
            Ok((
                value.notes_modified_today,
                value.last_modified_at_ms,
                value.identifiers_truncated,
            ))
        })
    };
    let Ok((notes_modified_today, last_modified_at_ms, identifiers_truncated)) = payload else {
        return invalid_source(metadata, "snapshot_payload_invalid");
    };
    if notes_modified_today > MAX_NOTES_MODIFIED
        || last_modified_at_ms.is_some_and(|value| value < 0)
    {
        return invalid_source(metadata, "snapshot_payload_invalid");
    }
    SourceResult {
        metadata,
        digest: Some(KnowledgeDigest {
            notes_modified_today,
            last_modified_at_ms,
            identifiers_truncated,
        }),
    }
}

fn valid_note_id(value: &str) -> bool {
    let Some(number) = value.strip_prefix("note-") else {
        return false;
    };
    !number.is_empty()
        && !number.starts_with('0')
        && number.bytes().all(|byte| byte.is_ascii_digit())
        && number.parse::<u64>().is_ok_and(|value| value > 0)
}

fn sum_durations(values: impl Iterator<Item = i64>) -> i64 {
    values.fold(0, i64::saturating_add)
}

fn read_snapshot_source_in<T>(
    root: &Path,
    producer: &str,
    scope: &str,
    parse: impl FnOnce(&devbox_integration::Envelope) -> Result<T, String>,
) -> SourceResult<T> {
    let reference = match latest_snapshot_reference(root, producer) {
        Ok(Some(reference)) => reference,
        Ok(None) => return unavailable_source(producer, scope, "snapshot_unavailable"),
        Err(selection) => {
            return unavailable_source_with_version(
                producer,
                scope,
                selection.version,
                selection.code,
            )
        }
    };
    let metadata = source_metadata_from_reference(&reference, scope, None);
    if reference.version != EXPORT_SCHEMA_VERSION {
        return invalid_source(metadata, "snapshot_schema_unsupported");
    }
    let envelope = match devbox_integration::read_snapshot_in(root, producer, reference.version) {
        Ok(Some(envelope)) => envelope,
        Ok(None) => return invalid_source(metadata, "snapshot_unavailable"),
        Err(_) => return invalid_source(metadata, "snapshot_invalid"),
    };
    if !same_snapshot_identity(&reference, &envelope) {
        return invalid_source(metadata, "snapshot_changed_during_read");
    }
    if !snapshot_payload_is_stable(root, producer, &reference, &envelope) {
        return invalid_source(metadata, "snapshot_changed_during_read");
    }
    if !snapshot_is_still_selected(root, producer, &reference) {
        return invalid_source(metadata, "snapshot_changed_during_read");
    }
    match parse(&envelope) {
        Ok(digest) => SourceResult {
            metadata,
            digest: Some(digest),
        },
        Err(error_code) => invalid_source(metadata, &error_code),
    }
}

/// Pick one exact snapshot before reading its payload. Invalid newer versions
/// are not silently replaced by an older valid version: the result is either
/// that selected version or a stable unavailable/invalid status.
fn latest_snapshot_reference(
    root: &Path,
    producer: &str,
) -> Result<Option<devbox_integration::SnapshotRef>, SnapshotSelectionError> {
    let report = devbox_integration::discover_report_in(root);
    let newest_version = report
        .snapshots
        .iter()
        .filter(|snapshot| snapshot.producer == producer)
        .map(|snapshot| snapshot.version)
        .chain(
            report
                .issues
                .iter()
                .filter(|issue| issue.producer == producer)
                .filter_map(|issue| issue.version),
        )
        .max();
    let Some(newest_version) = newest_version else {
        return if report.root_error.is_some() {
            Err(SnapshotSelectionError {
                version: None,
                code: "snapshot_unavailable",
            })
        } else {
            Ok(None)
        };
    };
    if let Some(reference) = report
        .snapshots
        .into_iter()
        .find(|snapshot| snapshot.producer == producer && snapshot.version == newest_version)
    {
        return Ok(Some(reference));
    }
    Err(SnapshotSelectionError {
        version: Some(newest_version),
        code: "snapshot_invalid",
    })
}

#[derive(Debug)]
struct SnapshotSelectionError {
    version: Option<u32>,
    code: &'static str,
}

fn same_snapshot_identity(
    reference: &devbox_integration::SnapshotRef,
    envelope: &devbox_integration::Envelope,
) -> bool {
    reference.producer == envelope.producer
        && reference.version == envelope.schema_version
        && reference.producer_version == envelope.producer_version
        && reference.generated_at == envelope.generated_at
}

fn snapshot_is_still_selected(
    root: &Path,
    producer: &str,
    reference: &devbox_integration::SnapshotRef,
) -> bool {
    latest_snapshot_reference(root, producer)
        .ok()
        .flatten()
        .is_some_and(|current| {
            current.path == reference.path
                && current.producer == reference.producer
                && current.version == reference.version
                && current.producer_version == reference.producer_version
                && current.generated_at == reference.generated_at
        })
}

fn snapshot_payload_is_stable(
    root: &Path,
    producer: &str,
    reference: &devbox_integration::SnapshotRef,
    envelope: &devbox_integration::Envelope,
) -> bool {
    match devbox_integration::read_snapshot_in(root, producer, reference.version) {
        Ok(Some(current)) => {
            current.schema_version == envelope.schema_version
                && current.producer == envelope.producer
                && current.producer_version == envelope.producer_version
                && current.generated_at == envelope.generated_at
                && current.data == envelope.data
        }
        _ => false,
    }
}

fn source_metadata_from_reference(
    reference: &devbox_integration::SnapshotRef,
    scope: &str,
    view: Option<&str>,
) -> SourceMetadata {
    SourceMetadata {
        id: reference.producer.clone(),
        available: true,
        schema_version: Some(reference.version),
        snapshot_version: Some(reference.version),
        producer_version: Some(reference.producer_version.clone()),
        generated_at: Some(reference.generated_at.clone()),
        freshness_ms: Some(reference.freshness_ms),
        view: view.map(ToOwned::to_owned),
        scope: scope.into(),
        error_code: None,
    }
}

fn unavailable_source<T>(producer: &str, scope: &str, error_code: &str) -> SourceResult<T> {
    unavailable_source_with_version(producer, scope, None, error_code)
}

fn unavailable_source_with_version<T>(
    producer: &str,
    scope: &str,
    snapshot_version: Option<u32>,
    error_code: &str,
) -> SourceResult<T> {
    SourceResult {
        metadata: SourceMetadata {
            id: producer.into(),
            available: false,
            schema_version: None,
            snapshot_version,
            producer_version: None,
            generated_at: None,
            freshness_ms: None,
            view: None,
            scope: scope.into(),
            error_code: Some(error_code.into()),
        },
        digest: None,
    }
}

fn invalid_source<T>(mut metadata: SourceMetadata, error_code: &str) -> SourceResult<T> {
    metadata.available = false;
    metadata.error_code = Some(error_code.into());
    SourceResult {
        metadata,
        digest: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::db::migrate;
    use crate::core::models::ClosedSession;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_SOURCE_FIXTURE: AtomicU64 = AtomicU64::new(1);

    struct SourceFixture(PathBuf);

    impl SourceFixture {
        fn new() -> Self {
            let id = NEXT_SOURCE_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "devbox-life-log-export-{}-{id}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for SourceFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn database() -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        migrate(&connection).unwrap();
        connection
    }

    fn insert(connection: &Connection, app: &str, title: &str, start: i64, end: i64) {
        crate::core::db::insert_session(
            connection,
            &ClosedSession {
                app: app.into(),
                title: title.into(),
                start_ts: start,
                end_ts: end,
            },
        )
        .unwrap();
    }

    fn input(format: ExportFormat) -> ExportInput {
        ExportInput {
            start_date: "2024-01-01".into(),
            end_date: "2024-01-02".into(),
            timezone: "UTC".into(),
            day_start: 0,
            day_end: DAY_MS * 2,
            day_boundaries: vec![
                ExportDayBoundary {
                    date: "2024-01-01".into(),
                    start_ms: 0,
                    end_ms: DAY_MS,
                },
                ExportDayBoundary {
                    date: "2024-01-02".into(),
                    start_ms: DAY_MS,
                    end_ms: DAY_MS * 2,
                },
            ],
            format,
        }
    }

    fn valid_empty_sources() -> Vec<SourceMetadata> {
        vec![
            SourceMetadata {
                id: "life-log".into(),
                available: true,
                schema_version: Some(EXPORT_SCHEMA_VERSION),
                snapshot_version: None,
                producer_version: Some(env!("CARGO_PKG_VERSION").into()),
                generated_at: None,
                freshness_ms: None,
                view: None,
                scope: "requested-range".into(),
                error_code: None,
            },
            SourceMetadata {
                id: "git".into(),
                available: false,
                schema_version: None,
                snapshot_version: None,
                producer_version: None,
                generated_at: None,
                freshness_ms: None,
                view: None,
                scope: "requested-range".into(),
                error_code: Some("no_safe_project_paths".into()),
            },
            SourceMetadata {
                id: "run-manager".into(),
                available: false,
                schema_version: None,
                snapshot_version: None,
                producer_version: None,
                generated_at: None,
                freshness_ms: None,
                view: None,
                scope: LATEST_SNAPSHOT_OUT_OF_RANGE_SCOPE.into(),
                error_code: Some("snapshot_unavailable".into()),
            },
            SourceMetadata {
                id: "knowledge-base".into(),
                available: false,
                schema_version: None,
                snapshot_version: None,
                producer_version: None,
                generated_at: None,
                freshness_ms: None,
                view: None,
                scope: LATEST_SNAPSHOT_OUT_OF_RANGE_SCOPE.into(),
                error_code: Some("snapshot_unavailable".into()),
            },
        ]
    }

    #[test]
    fn validates_date_range_and_limits() {
        let mut value = input(ExportFormat::Json);
        value.start_date = "2024-02-30".into();
        assert!(validate_range(&value).is_err());
        value = input(ExportFormat::Json);
        value.day_end = DAY_MS * (MAX_EXPORT_DAYS as i64 + 1);
        assert!(validate_range(&value).is_err());
    }

    #[test]
    fn accepts_explicit_local_civil_boundaries_that_are_not_24_hours() {
        let value = ExportInput {
            start_date: "2024-11-03".into(),
            end_date: "2024-11-03".into(),
            timezone: "America/New_York".into(),
            day_start: 0,
            day_end: DAY_MS + 3_600_000,
            day_boundaries: vec![ExportDayBoundary {
                date: "2024-11-03".into(),
                start_ms: 0,
                end_ms: DAY_MS + 3_600_000,
            }],
            format: ExportFormat::Json,
        };
        let validated = validate_range(&value).unwrap();
        assert_eq!(validated.days.len(), 1);
        assert_eq!(
            validated.days[0].end_ms - validated.days[0].start_ms,
            DAY_MS + 3_600_000
        );
    }

    #[test]
    fn safe_project_path_rejects_relative_traversal_and_device_paths() {
        assert!(safe_project_path("relative/project").is_none());
        assert!(safe_project_path("C:\\projects\\..\\secret").is_none());
        assert!(safe_project_path("\\\\.\\PIPE\\secret").is_none());
        assert!(safe_project_path("C:\\projects\\token=do-not-export").is_none());
        assert_eq!(
            safe_project_path("C:\\projects\\devbox"),
            Some("C:\\projects\\devbox".into())
        );
        assert_eq!(
            safe_project_path("/home/user/devbox"),
            Some("/home/user/devbox".into())
        );
    }

    #[test]
    fn duplicate_windows_project_identity_uses_deterministic_display_path() {
        let connection = database();
        let prepared = prepare_document(
            &connection,
            &[
                "c:/Work/Devbox".into(),
                "C:\\Work\\Devbox".into(),
                "C:/Work/Devbox".into(),
            ],
            &input(ExportFormat::Json),
        )
        .unwrap();

        assert_eq!(prepared.safe_projects, vec!["C:/Work/Devbox"]);
    }

    #[test]
    fn export_boundary_redacts_obvious_credentials_in_titles() {
        assert_eq!(redact_obvious_secret("Bearer sk-live-value"), "[redacted]");
        assert_eq!(
            redact_obvious_secret("settings password=do-not-export"),
            "[redacted]"
        );
        assert_eq!(
            redact_obvious_secret("Authorization : bearer raw-secret"),
            "[redacted]"
        );
        assert_eq!(
            redact_obvious_secret("ordinary project window"),
            "ordinary project window"
        );
        assert_eq!(
            bounded_text("title\u{1b}[31m", MAX_TITLE_BYTES, true),
            "title [31m"
        );

        let rules = PrivacyRules::default();
        let session = sanitized_session(
            Session {
                id: 1,
                app: "token=do-not-export".into(),
                title: "ordinary title".into(),
                start_ts: 0,
                end_ts: 1,
                duration_ms: 1,
            },
            &rules,
        )
        .unwrap()
        .unwrap();
        assert_eq!(session.app, "[redacted]");
        assert!(!serde_json::to_string(&session)
            .unwrap()
            .contains("do-not-export"));
    }

    #[test]
    fn corrupt_session_numbers_fail_closed_instead_of_being_rewritten() {
        for (id, end_ts, duration_ms) in [(-1, 10, 10), (1, -1, 0), (1, 10, -1), (1, 10, 11)] {
            let connection = database();
            connection
                .execute(
                    "INSERT INTO sessions (id, app, title, start_ts, end_ts, duration_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![id, "app", "title", 0_i64, end_ts, duration_ms],
                )
                .unwrap();
            let error = prepare_document(&connection, &[], &input(ExportFormat::Json))
                .err()
                .unwrap();
            assert_eq!(error, "export 활동 데이터가 올바르지 않습니다");
            assert!(!error.contains("title"));
        }
    }

    #[test]
    fn git_project_errors_remain_attached_to_their_project_row() {
        let git = ExportGit {
            projects: vec![ExportGitProject {
                path: "/safe/project".into(),
                commits: 0,
                error_code: Some("git_timeout".into()),
            }],
            total_commits: 0,
            error_codes: vec!["git_timeout".into()],
        };
        let source = SourceMetadata {
            id: "git".into(),
            available: false,
            schema_version: None,
            snapshot_version: None,
            producer_version: None,
            generated_at: None,
            freshness_ms: None,
            view: None,
            scope: "requested-range".into(),
            error_code: Some("git_timeout".into()),
        };
        assert!(valid_git_source(&source, &git));
        assert_eq!(git.projects[0].error_code.as_deref(), Some("git_timeout"));

        let mut document = futures_not_required_build(&database(), &input(ExportFormat::Csv));
        document.summary.git = git;
        let csv = render(&document, ExportFormat::Csv).unwrap().content;
        let row = csv.lines().find(|line| line.starts_with("git,")).unwrap();
        let cells = row.split(',').collect::<Vec<_>>();
        assert_eq!(cells.len(), 24);
        assert_eq!(cells[10], "/safe/project");
        assert_eq!(cells[14], "git");
        assert_eq!(cells[15], "false");
        assert_eq!(cells[23], "git_timeout");
    }

    #[test]
    fn git_timestamps_use_exact_half_open_range_and_local_boundaries() {
        let value = input(ExportFormat::Json);
        let range = validate_range(&value).unwrap();
        let mut daily = vec![0; range.days.len()];
        let count = parse_git_timestamps("0\n86400\n172800\n", &range, &mut daily).unwrap();
        assert_eq!(count, 2);
        assert_eq!(daily, vec![1, 1]);

        let mut unchanged = vec![0; range.days.len()];
        assert_eq!(
            parse_git_timestamps("not-a-timestamp\n", &range, &mut unchanged),
            Err("git_output_invalid")
        );
        assert_eq!(unchanged, vec![0, 0]);
        assert_eq!(ceil_seconds(-1), 0);
    }

    #[test]
    fn privacy_rules_are_bounded_before_regex_compilation() {
        let connection = database();
        db::set_setting(
            &connection,
            "privacy_rules",
            &serde_json::json!({
                "redactTitlePatterns": ["x".repeat(MAX_REGEX_BYTES + 1)]
            })
            .to_string(),
        );
        assert!(read_privacy_rules(&connection).mask_all_titles);
    }

    #[test]
    fn csv_escapes_commas_quotes_and_newlines() {
        assert_eq!(csv_cell("plain"), "plain");
        assert_eq!(csv_cell("a,b"), "\"a,b\"");
        assert_eq!(csv_cell("a\"b"), "\"a\"\"b\"");
        assert_eq!(csv_cell("a\nb"), "\"a\nb\"");
    }

    #[test]
    fn export_rendering_is_deterministic_and_applies_privacy() {
        let connection = database();
        insert(&connection, "Code.exe", "secret patient 123", 0, 100);
        insert(&connection, "chrome.exe", "GitHub", 100, 250);
        db::set_setting(
            &connection,
            "privacy_rules",
            r#"{"redactTitlePatterns":["patient \\d+"]}"#,
        );
        let document = futures_not_required_build(&connection, &input(ExportFormat::Json));
        let json_a = render(&document, ExportFormat::Json).unwrap().content;
        let json_b = render(&document, ExportFormat::Json).unwrap().content;
        assert_eq!(json_a, json_b);
        assert!(json_a.contains("[redacted]"));
        assert!(!json_a.contains("patient 123"));
        assert_eq!(document.sessions.len(), 2);
    }

    // async git collection is deliberately not run in unit formatting tests. This
    // helper exercises the same bounded DB/privacy path with an empty git source.
    fn futures_not_required_build(connection: &Connection, input: &ExportInput) -> ExportDocument {
        let range = validate_range(input).unwrap();
        let rules = read_privacy_rules(connection);
        let rows = db::get_timeline_limited(
            connection,
            range.start_ms,
            range.end_ms,
            MAX_EXPORT_SESSIONS,
        )
        .unwrap();
        let sessions = rows
            .into_iter()
            .map(|row| sanitized_session(row, &rules))
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let daily = range
            .days
            .iter()
            .map(|day| {
                let day_sessions = sessions
                    .iter()
                    .filter(|session| {
                        session.start_ts_ms >= day.start_ms && session.start_ts_ms < day.end_ms
                    })
                    .collect::<Vec<_>>();
                DailyDigest {
                    date: day.date.as_string(),
                    start_ms: day.start_ms,
                    end_ms: day.end_ms,
                    pc_usage_ms: sum_durations(
                        day_sessions.iter().map(|session| session.duration_ms),
                    ),
                    session_count: day_sessions.len(),
                    git_commits: 0,
                }
            })
            .collect();
        ExportDocument {
            schema_version: EXPORT_SCHEMA_VERSION,
            range: ExportRange {
                start_date: range.start_date.as_string(),
                end_date: range.end_date.as_string(),
                timezone: range.timezone,
                start_ms: range.start_ms,
                end_ms: range.end_ms,
                day_boundaries: range
                    .days
                    .iter()
                    .map(|day| ExportDayBoundary {
                        date: day.date.as_string(),
                        start_ms: day.start_ms,
                        end_ms: day.end_ms,
                    })
                    .collect(),
            },
            rules: export_rules(),
            summary: ExportSummary {
                pc_usage_ms: sessions.iter().map(|session| session.duration_ms).sum(),
                session_count: sessions.len(),
                app_totals: build_app_totals(&sessions),
                git: ExportGit::default(),
                run: None,
                knowledge: None,
            },
            daily,
            sessions,
            sources: valid_empty_sources(),
        }
    }

    #[test]
    fn save_validation_rejects_cross_source_error_codes() {
        let connection = database();
        insert(&connection, "editor.exe", "source.rs", 0, 100);
        let document = futures_not_required_build(&connection, &input(ExportFormat::Json));
        assert!(validate_document(&document));

        let mut tampered = document.clone();
        tampered.sources[2].error_code = Some("git_timeout".into());
        assert!(!validate_document(&tampered));

        let mut tampered = document;
        tampered.sessions[0].duration_ms = tampered.sessions[0]
            .end_ts_ms
            .saturating_sub(tampered.sessions[0].start_ts_ms)
            .saturating_add(1);
        assert!(!validate_document(&tampered));
    }

    #[test]
    fn markdown_escapes_table_cells_and_csv_has_fixed_header() {
        let document = ExportDocument {
            schema_version: EXPORT_SCHEMA_VERSION,
            range: ExportRange {
                start_date: "2024-01-01".into(),
                end_date: "2024-01-01".into(),
                timezone: "UTC".into(),
                start_ms: 0,
                end_ms: DAY_MS,
                day_boundaries: vec![ExportDayBoundary {
                    date: "2024-01-01".into(),
                    start_ms: 0,
                    end_ms: DAY_MS,
                }],
            },
            rules: export_rules(),
            summary: ExportSummary {
                pc_usage_ms: 1,
                session_count: 1,
                app_totals: vec![ExportAppTotal {
                    app: r"a|b\`".into(),
                    duration_ms: 1,
                    sessions: 1,
                }],
                git: ExportGit::default(),
                run: Some(RunDigest {
                    success: 2,
                    failed: 1,
                    active_service_count: 1,
                    active_service_uptime_ms: 42,
                    last_run_at_ms: Some(9),
                }),
                knowledge: Some(KnowledgeDigest {
                    notes_modified_today: 3,
                    last_modified_at_ms: Some(8),
                    identifiers_truncated: true,
                }),
            },
            daily: vec![],
            sessions: vec![ExportSession {
                id: 1,
                app: "a".into(),
                title: "line\nvalue".into(),
                start_ts_ms: 0,
                end_ts_ms: 1,
                duration_ms: 1,
            }],
            sources: vec![],
        };
        let markdown = render(&document, ExportFormat::Markdown).unwrap().content;
        assert!(markdown.contains(r"a\|b\\\`"));
        assert!(!markdown.contains("line\nvalue"));
        let csv = render(&document, ExportFormat::Csv).unwrap().content;
        assert!(csv.starts_with("record_type,date,range_start_date"));
        assert!(csv.contains("\"line\nvalue\""));
        assert!(csv.contains("run_active_service_uptime_ms,42"));
        assert!(csv.contains("knowledge_identifiers_truncated,true"));
        assert!(csv.lines().next().unwrap().split(',').count() == 24);
        assert_eq!(
            render(&document, ExportFormat::Json).unwrap().origin,
            ExportOrigin::Native
        );
        let reparsed: ExportDocument =
            serde_json::from_str(&render(&document, ExportFormat::Json).unwrap().content).unwrap();
        assert_eq!(reparsed, document);
    }

    #[test]
    fn generated_output_obeys_size_limit() {
        let mut document = ExportDocument {
            schema_version: EXPORT_SCHEMA_VERSION,
            range: ExportRange {
                start_date: "2024-01-01".into(),
                end_date: "2024-01-01".into(),
                timezone: "UTC".into(),
                start_ms: 0,
                end_ms: DAY_MS,
                day_boundaries: vec![ExportDayBoundary {
                    date: "2024-01-01".into(),
                    start_ms: 0,
                    end_ms: DAY_MS,
                }],
            },
            rules: export_rules(),
            summary: ExportSummary {
                pc_usage_ms: 0,
                session_count: 0,
                app_totals: vec![],
                git: ExportGit::default(),
                run: None,
                knowledge: None,
            },
            daily: vec![],
            sessions: vec![],
            sources: vec![],
        };
        document.sessions.push(ExportSession {
            id: 1,
            app: "app".into(),
            title: "x".repeat(MAX_EXPORT_BYTES),
            start_ts_ms: 0,
            end_ts_ms: 1,
            duration_ms: 1,
        });
        assert!(render(&document, ExportFormat::Json).is_err());
    }

    #[test]
    fn fixture_source_metadata_follows_contract_order() {
        let sources = [
            SourceMetadata {
                id: "life-log".into(),
                available: true,
                schema_version: Some(1),
                snapshot_version: None,
                producer_version: Some("0.5.0".into()),
                generated_at: None,
                freshness_ms: None,
                view: None,
                scope: "requested-range".into(),
                error_code: None,
            },
            SourceMetadata {
                id: "git".into(),
                available: false,
                schema_version: None,
                snapshot_version: None,
                producer_version: None,
                generated_at: None,
                freshness_ms: None,
                view: None,
                scope: "requested-range".into(),
                error_code: Some("no_safe_project_paths".into()),
            },
            SourceMetadata {
                id: "run-manager".into(),
                available: true,
                schema_version: Some(1),
                snapshot_version: Some(1),
                producer_version: Some("0.5.0".into()),
                generated_at: Some("2024-01-01T00:00:00Z".into()),
                freshness_ms: Some(10),
                view: None,
                scope: LATEST_SNAPSHOT_OUT_OF_RANGE_SCOPE.into(),
                error_code: None,
            },
            SourceMetadata {
                id: "knowledge-base".into(),
                available: false,
                schema_version: None,
                snapshot_version: None,
                producer_version: None,
                generated_at: None,
                freshness_ms: None,
                view: None,
                scope: LATEST_SNAPSHOT_OUT_OF_RANGE_SCOPE.into(),
                error_code: Some("snapshot_unavailable".into()),
            },
        ];
        assert_eq!(
            sources
                .iter()
                .map(|source| source.id.as_str())
                .collect::<Vec<_>>(),
            ["life-log", "git", "run-manager", "knowledge-base"]
        );
        assert_eq!(
            sources[3].error_code.as_deref(),
            Some("snapshot_unavailable")
        );
    }

    #[test]
    fn run_manager_fixture_is_reduced_to_safe_digest() {
        let fixture = SourceFixture::new();
        let envelope = devbox_integration::Envelope::new(
            "run-manager",
            "0.5.0",
            serde_json::json!({
                "activeServices": [{ "id": "job-secret-name", "uptimeMs": 42 }],
                "runs": { "success": 3, "failed": 1 },
                "lastRunAtMs": 1_700_000_000_000_i64,
            }),
        );
        let directory = devbox_integration::snapshot_dir_in(fixture.path(), "run-manager", 1);
        devbox_integration::write_atomic(&envelope, &directory).unwrap();

        let result = read_run_source_in(fixture.path());
        assert_eq!(
            result.metadata.generated_at.as_deref(),
            Some(envelope.generated_at.as_str())
        );
        assert_eq!(result.metadata.snapshot_version, Some(1));
        assert!(result.metadata.freshness_ms.is_some());
        assert_eq!(
            result.digest,
            Some(RunDigest {
                success: 3,
                failed: 1,
                active_service_count: 1,
                active_service_uptime_ms: 42,
                last_run_at_ms: Some(1_700_000_000_000),
            })
        );
    }

    #[test]
    fn run_manager_duplicate_service_identity_is_rejected() {
        let fixture = SourceFixture::new();
        let envelope = devbox_integration::Envelope::new(
            "run-manager",
            "0.5.0",
            serde_json::json!({
                "activeServices": [
                    { "id": "same-service", "uptimeMs": 10 },
                    { "id": "same-service", "uptimeMs": 20 }
                ],
                "runs": { "success": 1, "failed": 0 },
                "lastRunAtMs": null,
            }),
        );
        let directory = devbox_integration::snapshot_dir_in(fixture.path(), "run-manager", 1);
        devbox_integration::write_atomic(&envelope, &directory).unwrap();

        let result = read_run_source_in(fixture.path());
        assert_eq!(result.digest, None);
        assert_eq!(
            result.metadata.error_code.as_deref(),
            Some("snapshot_payload_invalid")
        );
    }

    #[test]
    fn newest_snapshot_version_is_selected_without_falling_back_to_an_older_one() {
        let fixture = SourceFixture::new();
        let old = devbox_integration::Envelope::new(
            "run-manager",
            "0.5.0",
            serde_json::json!({
                "activeServices": [],
                "runs": { "success": 1, "failed": 0 },
                "lastRunAtMs": null,
            }),
        );
        devbox_integration::write_atomic(
            &old,
            &devbox_integration::snapshot_dir_in(fixture.path(), "run-manager", 1),
        )
        .unwrap();
        let mut newer = old.clone();
        newer.schema_version = 2;
        devbox_integration::write_atomic(
            &newer,
            &devbox_integration::snapshot_dir_in(fixture.path(), "run-manager", 2),
        )
        .unwrap();

        let result = read_run_source_in(fixture.path());
        assert_eq!(result.digest, None);
        assert_eq!(result.metadata.snapshot_version, Some(2));
        assert_eq!(
            result.metadata.error_code.as_deref(),
            Some("snapshot_schema_unsupported")
        );
    }

    #[test]
    fn corrupt_newer_snapshot_preserves_its_version_without_falling_back() {
        let fixture = SourceFixture::new();
        let old = devbox_integration::Envelope::new(
            "run-manager",
            "0.5.0",
            serde_json::json!({
                "activeServices": [],
                "runs": { "success": 1, "failed": 0 },
                "lastRunAtMs": null,
            }),
        );
        devbox_integration::write_atomic(
            &old,
            &devbox_integration::snapshot_dir_in(fixture.path(), "run-manager", 1),
        )
        .unwrap();
        let newer = devbox_integration::snapshot_dir_in(fixture.path(), "run-manager", 2);
        std::fs::create_dir_all(&newer).unwrap();
        std::fs::write(newer.join("summary.json"), b"not-json").unwrap();

        let result = read_run_source_in(fixture.path());
        assert_eq!(result.digest, None);
        assert_eq!(result.metadata.snapshot_version, Some(2));
        assert_eq!(
            result.metadata.error_code.as_deref(),
            Some("snapshot_invalid")
        );
    }

    #[test]
    fn knowledge_fixture_does_not_export_note_ids_and_rejects_bad_ids() {
        let fixture = SourceFixture::new();
        let views = devbox_integration::SnapshotViews::from([(
            "activity".to_string(),
            devbox_integration::SnapshotView {
                schema_version: 1,
                freshness_ms: 10,
                entries: vec![serde_json::json!({
                    "notesModifiedToday": 2,
                    "lastModifiedAtMs": 1_700_000_000_000_i64,
                    "noteIds": ["note-2", "note-7"],
                    "identifiersTruncated": false,
                })],
            },
        )]);
        let envelope = devbox_integration::Envelope::with_views("knowledge-base", "0.5.0", views);
        let directory = devbox_integration::snapshot_dir_in(fixture.path(), "knowledge-base", 1);
        devbox_integration::write_atomic(&envelope, &directory).unwrap();

        let result = read_knowledge_source_in(fixture.path());
        assert_eq!(
            result.digest,
            Some(KnowledgeDigest {
                notes_modified_today: 2,
                last_modified_at_ms: Some(1_700_000_000_000),
                identifiers_truncated: false,
            })
        );
        assert_eq!(result.metadata.view.as_deref(), Some("activity"));
        assert!(result
            .metadata
            .freshness_ms
            .is_some_and(|value| value >= 10));
        let encoded = serde_json::to_string(&result.digest).unwrap();
        assert!(!encoded.contains("note-2"));

        let bad = devbox_integration::Envelope::with_views(
            "knowledge-base",
            "0.5.0",
            devbox_integration::SnapshotViews::from([(
                "activity".to_string(),
                devbox_integration::SnapshotView {
                    schema_version: 1,
                    freshness_ms: 0,
                    entries: vec![serde_json::json!({
                        "notesModifiedToday": 1,
                        "lastModifiedAtMs": null,
                        "noteIds": ["note-secret/path"],
                        "identifiersTruncated": false,
                    })],
                },
            )]),
        );
        devbox_integration::write_atomic(
            &bad,
            &devbox_integration::snapshot_dir_in(fixture.path(), "knowledge-base", 1),
        )
        .unwrap();
        let rejected = read_knowledge_source_in(fixture.path());
        assert_eq!(
            rejected.metadata.error_code.as_deref(),
            Some("snapshot_payload_invalid")
        );
        assert!(!rejected
            .metadata
            .error_code
            .as_deref()
            .unwrap()
            .contains("secret/path"));
    }
}
