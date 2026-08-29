//! Deterministic local daily/weekly digest with existing monthly support.
//!
//! The digest deliberately builds on the `life-log/export/v1` producer.  That
//! keeps the date-boundary, privacy, Git, and integration-snapshot rules in one
//! place while giving the UI a small summary suitable for a period view.  The
//! current Run Manager and Knowledge snapshots are provenance only: they are
//! not range-keyed history and therefore never become period totals here.

use crate::core::export::{
    self, ExportAppTotal, ExportDayBoundary, ExportFormat, ExportGit, ExportInput, ExportRange,
    ExportSession, SourceMetadata,
};
use devbox_filesystem::parse_safe_project_path;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::hash::{BuildHasher, RandomState};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const DIGEST_SCHEMA_VERSION: u32 = 1;
pub const MAX_DIGEST_DAYS: usize = export::MAX_EXPORT_DAYS;
pub const MAX_DIGEST_APPS: usize = 2_048;
pub const MAX_DIGEST_BYTES: usize = export::MAX_EXPORT_BYTES;
const MAX_HEADLINE_BYTES: usize = 512;

const DIGEST_MARKDOWN_HEADER: &str = "# Life Log local digest\n\n";

/// A period is inclusive at the civil-date level.  The supplied range still
/// uses the export contract's exclusive `endMs` and authoritative boundaries.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DigestPeriod {
    Day,
    Week,
    Month,
}

impl DigestPeriod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
        }
    }

    fn valid_day_count(self, count: usize) -> bool {
        match self {
            Self::Day => count == 1,
            Self::Week => count == 7,
            Self::Month => (28..=31).contains(&count),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DigestFilter {
    /// Exact sanitized application name.  `None` means all applications.
    pub app: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DigestInput {
    pub start_date: String,
    pub end_date: String,
    pub timezone: String,
    pub day_start: i64,
    pub day_end: i64,
    pub day_boundaries: Vec<ExportDayBoundary>,
    pub period: DigestPeriod,
    #[serde(default)]
    pub filter: DigestFilter,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DigestRules {
    pub session_window: String,
    pub session_duration: String,
    pub daily_buckets: String,
    pub app_filter: String,
    pub app_totals: String,
    pub git_commits: String,
    pub snapshot_scope: String,
    pub privacy: String,
    pub external_processing: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DigestDay {
    pub date: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub pc_usage_ms: i64,
    pub session_count: usize,
    pub git_commits: u32,
    pub top_app: Option<String>,
    pub has_activity: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DigestSummary {
    pub pc_usage_ms: i64,
    pub session_count: usize,
    pub active_days: usize,
    pub total_days: usize,
    pub average_daily_usage_ms: i64,
    pub top_app: Option<String>,
    pub git_commits: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DigestDocument {
    pub schema_version: u32,
    pub period: DigestPeriod,
    pub range: ExportRange,
    pub filter: DigestFilter,
    pub rules: DigestRules,
    pub headline: String,
    pub summary: DigestSummary,
    pub daily: Vec<DigestDay>,
    pub app_totals: Vec<ExportAppTotal>,
    pub git: ExportGit,
    pub sources: Vec<SourceMetadata>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DigestOrigin {
    Native,
    BrowserPreview,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DigestResponse {
    pub origin: DigestOrigin,
    pub document: DigestDocument,
    pub markdown: String,
    /// Native responses carry a server-owned immutable save handle. Browser
    /// previews deliberately leave it absent because they have no native
    /// storage boundary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
}

pub const DIGEST_HANDLE_TTL: Duration = Duration::from_secs(120);
const MAX_DIGEST_HANDLES: usize = 8;
const DIGEST_HANDLE_BYTES: usize = 32;

struct StoredDigest {
    created_at: Instant,
    response: DigestResponse,
}

/// Short-lived immutable native save artifacts. The UI never sends the
/// digest input back for saving; it sends only this opaque handle, so the
/// bytes shown on screen and the bytes passed to `atomic_write` are identical
/// even if the database or Git repository changes after rendering.
pub struct DigestHandleStore {
    entries: Mutex<HashMap<String, StoredDigest>>,
    sequence: AtomicU64,
    entropy: RandomState,
}

impl Default for DigestHandleStore {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            sequence: AtomicU64::new(0),
            entropy: RandomState::new(),
        }
    }
}

impl DigestHandleStore {
    pub fn issue(&self, mut response: DigestResponse) -> Result<DigestResponse, String> {
        if !validate_response(&response) {
            return Err("digest_output_invalid".into());
        }
        let handle = self.new_handle();
        response.handle = Some(handle.clone());
        if !validate_response(&response) {
            return Err("digest_output_invalid".into());
        }
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| "digest 저장 핸들을 잠글 수 없습니다".to_string())?;
        self.prune_expired(&mut entries);
        if entries.len() >= MAX_DIGEST_HANDLES {
            if let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, value)| value.created_at)
                .map(|(key, _)| key.clone())
            {
                entries.remove(&oldest);
            }
        }
        entries.insert(
            handle,
            StoredDigest {
                created_at: Instant::now(),
                response: response.clone(),
            },
        );
        Ok(response)
    }

    pub fn get(&self, handle: &str) -> Result<DigestResponse, String> {
        if !valid_handle(handle) {
            return Err("digest_handle_expired".into());
        }
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| "digest 저장 핸들을 잠글 수 없습니다".to_string())?;
        self.prune_expired(&mut entries);
        entries
            .get(handle)
            .map(|stored| stored.response.clone())
            .ok_or_else(|| "digest_handle_expired".into())
    }

    fn prune_expired(&self, entries: &mut HashMap<String, StoredDigest>) {
        entries.retain(|_, stored| stored.created_at.elapsed() <= DIGEST_HANDLE_TTL);
    }

    fn new_handle(&self) -> String {
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_nanos() as u64)
            .unwrap_or_default();
        let entropy = self.entropy.hash_one((now, sequence, std::process::id()));
        format!("{entropy:016x}{sequence:016x}")
    }
}

fn valid_handle(handle: &str) -> bool {
    handle.len() == DIGEST_HANDLE_BYTES && handle.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Validate digest fields and delegate the shared range/boundary contract to
/// the export producer so the two features cannot drift on DST, date, or
/// privacy semantics.  This runs before any DB or Git work.
pub fn validate_input(input: &DigestInput) -> Result<(), String> {
    if !input.period.valid_day_count(input.day_boundaries.len())
        || input.day_boundaries.len() > MAX_DIGEST_DAYS
    {
        return Err("digest 기간이 올바르지 않습니다".into());
    }
    match input.period {
        DigestPeriod::Day if input.start_date != input.end_date => {
            return Err("digest 일 범위가 올바르지 않습니다".into());
        }
        DigestPeriod::Week if !is_monday(&input.start_date) => {
            return Err("digest 주 범위가 올바르지 않습니다".into());
        }
        DigestPeriod::Month
            if !input.start_date.ends_with("-01")
                || date_prefix(&input.start_date) != date_prefix(&input.end_date)
                || !export::is_month_end_date(&input.end_date) =>
        {
            return Err("digest 월 범위가 올바르지 않습니다".into());
        }
        _ => {}
    }
    if let Some(app) = &input.filter.app {
        if app.is_empty()
            || app.len() > 256
            || app.chars().any(char::is_control)
            || contains_secret_marker(app)
        {
            return Err("digest 필터가 올바르지 않습니다".into());
        }
    }
    export::validate_range_input(&export_input(input))
        .map_err(|_| "digest 기간이 올바르지 않습니다".to_string())?;
    Ok(())
}

fn date_prefix(value: &str) -> Option<&str> {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| !matches!(index, 4 | 7) && !byte.is_ascii_digit())
    {
        return None;
    }
    Some(&value[..7])
}

/// Return whether a valid-looking YYYY-MM-DD key starts on Monday. The shared
/// export validator performs the authoritative calendar validation afterwards;
/// malformed values are simply treated as not Monday here.
fn is_monday(value: &str) -> bool {
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
    let Ok(year) = value[0..4].parse::<i32>() else {
        return false;
    };
    let Ok(month) = value[5..7].parse::<usize>() else {
        return false;
    };
    let Ok(day) = value[8..10].parse::<i32>() else {
        return false;
    };
    if !(1..=12).contains(&month) || day < 1 {
        return false;
    }
    // Sakamoto's Gregorian weekday algorithm: Sunday = 0, Monday = 1.
    const OFFSETS: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let adjusted_year = if month < 3 { year - 1 } else { year };
    (adjusted_year + adjusted_year / 4 - adjusted_year / 100
        + adjusted_year / 400
        + OFFSETS[month - 1]
        + day)
        .rem_euclid(7)
        == 1
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
        "client_secret",
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

fn export_input(input: &DigestInput) -> ExportInput {
    ExportInput {
        start_date: input.start_date.clone(),
        end_date: input.end_date.clone(),
        timezone: input.timezone.clone(),
        day_start: input.day_start,
        day_end: input.day_end,
        day_boundaries: input.day_boundaries.clone(),
        format: ExportFormat::Json,
    }
}

/// Prepare bounded DB rows and validated local snapshots before the command
/// releases its DB mutex.  Git is still performed by `build_response` after
/// the lock is released.
#[cfg(test)]
pub fn prepare(
    conn: &Connection,
    projects: &[String],
    input: &DigestInput,
) -> Result<export::PreparedExport, String> {
    prepare_with_cancel(conn, projects, input, Arc::new(AtomicBool::new(false)))
}

pub fn prepare_with_cancel(
    conn: &Connection,
    projects: &[String],
    input: &DigestInput,
    cancellation: Arc<AtomicBool>,
) -> Result<export::PreparedExport, String> {
    validate_input(input)?;
    export::prepare_document_with_cancel(conn, projects, &export_input(input), cancellation)
}

/// Build a small digest from the already bounded export snapshot.  No network,
/// LLM, filesystem, or external process is introduced here; those boundaries
/// are inherited from `core::export`.
#[cfg(test)]
pub async fn build_response(
    prepared: export::PreparedExport,
    input: &DigestInput,
) -> Result<DigestResponse, String> {
    build_response_with_cancel(prepared, input, Arc::new(AtomicBool::new(false))).await
}

pub async fn build_response_with_cancel(
    prepared: export::PreparedExport,
    input: &DigestInput,
    cancellation: Arc<AtomicBool>,
) -> Result<DigestResponse, String> {
    validate_input(input)?;
    if cancellation.load(Ordering::Acquire) {
        return Err("digest_cancelled".into());
    }
    let export_document =
        export::build_document_with_cancel(prepared, Arc::clone(&cancellation)).await?;
    check_cancelled(&cancellation)?;
    if !range_matches_input(&export_document.range, input) {
        return Err("digest 기간이 올바르지 않습니다".into());
    }
    let selected_sessions = export_document
        .sessions
        .iter()
        .filter(|session| {
            input
                .filter
                .app
                .as_deref()
                .is_none_or(|app| app == session.app)
        })
        .collect::<Vec<_>>();
    check_cancelled(&cancellation)?;
    let app_totals = build_app_totals_with_cancel(&selected_sessions, Some(&cancellation))?;

    let mut daily = Vec::with_capacity(export_document.range.day_boundaries.len());
    for (boundary, exported_day) in export_document
        .range
        .day_boundaries
        .iter()
        .zip(export_document.daily.iter())
    {
        check_cancelled(&cancellation)?;
        let day_sessions = selected_sessions
            .iter()
            .filter(|session| {
                session.start_ts_ms >= boundary.start_ms && session.start_ts_ms < boundary.end_ms
            })
            .copied()
            .collect::<Vec<_>>();
        check_cancelled(&cancellation)?;
        let pc_usage_ms = sum_durations(day_sessions.iter().map(|s| s.duration_ms))?;
        let session_count = day_sessions.len();
        daily.push(DigestDay {
            date: boundary.date.clone(),
            start_ms: boundary.start_ms,
            end_ms: boundary.end_ms,
            pc_usage_ms,
            session_count,
            // Git is independent of the application filter.  A future
            // project filter must add per-project daily provenance before
            // it is allowed to change this value.
            git_commits: exported_day.git_commits,
            top_app: top_app(&day_sessions),
            has_activity: pc_usage_ms > 0 || session_count > 0 || exported_day.git_commits > 0,
        });
    }

    let pc_usage_ms = sum_durations(selected_sessions.iter().map(|session| session.duration_ms))?;
    let session_count = selected_sessions.len();
    let total_days = daily.len();
    let active_days = daily.iter().filter(|day| day.has_activity).count();
    let summary = DigestSummary {
        pc_usage_ms,
        session_count,
        active_days,
        total_days,
        average_daily_usage_ms: if total_days == 0 {
            0
        } else {
            pc_usage_ms / total_days as i64
        },
        top_app: app_totals.first().map(|app| app.app.clone()),
        git_commits: export_document.summary.git.total_commits,
    };
    let filter_description = input
        .filter
        .app
        .as_deref()
        .map(|app| format!("exact sanitized app `{app}` only"))
        .unwrap_or_else(|| "all sanitized applications".into());
    let rules = digest_rules(filter_description);
    let headline = digest_headline(
        input.period,
        active_days,
        total_days,
        session_count,
        summary.git_commits,
    );
    let document = DigestDocument {
        schema_version: DIGEST_SCHEMA_VERSION,
        period: input.period,
        range: export_document.range,
        filter: input.filter.clone(),
        rules,
        headline,
        summary,
        daily,
        app_totals,
        git: export_document.summary.git,
        sources: export_document.sources,
    };
    let markdown = render_markdown(&document);
    let byte_length = markdown.len();
    if byte_length > MAX_DIGEST_BYTES
        || serde_json::to_vec(&document)
            .map_err(|_| "digest 결과를 만들 수 없습니다".to_string())?
            .len()
            > MAX_DIGEST_BYTES
    {
        return Err("digest 결과가 크기 제한을 초과했습니다".into());
    }
    let response = DigestResponse {
        origin: DigestOrigin::Native,
        document,
        markdown,
        handle: None,
    };
    check_cancelled(&cancellation)?;
    validate_response(&response)
        .then_some(response)
        .ok_or_else(|| "digest 결과를 검증하지 못했습니다".into())
}

fn check_cancelled(cancellation: &AtomicBool) -> Result<(), String> {
    if cancellation.load(Ordering::Acquire) {
        Err("digest_cancelled".into())
    } else {
        Ok(())
    }
}

fn range_matches_input(range: &ExportRange, input: &DigestInput) -> bool {
    range.start_date == input.start_date
        && range.end_date == input.end_date
        && range.timezone == input.timezone.trim()
        && range.start_ms == input.day_start
        && range.end_ms == input.day_end
        && range.day_boundaries == input.day_boundaries
}

fn digest_rules(app_filter: String) -> DigestRules {
    DigestRules {
        session_window: "start_ts_ms >= range.startMs && start_ts_ms < range.endMs".into(),
        session_duration: "stored durationMs is retained; sessions are assigned by start timestamp and are not clipped".into(),
        daily_buckets: "the supplied local civil-day boundaries are authoritative; no fixed 24-hour arithmetic is used".into(),
        app_filter,
        app_totals: "sanitized sessions are grouped by app; duration descending then app byte order".into(),
        git_commits: "read-only bounded git counts come from the requested range and remain independent of the app filter".into(),
        snapshot_scope: "Run Manager and Knowledge latest snapshots are provenance only because they are not range-keyed history".into(),
        privacy: "Life Log privacy rules and obvious credential markers are reapplied before aggregation".into(),
        external_processing: "rule-based local aggregation only; no cloud/local LLM, network, telemetry, or external activity transfer".into(),
    }
}

fn digest_headline(
    period: DigestPeriod,
    active_days: usize,
    total_days: usize,
    session_count: usize,
    git_commits: u32,
) -> String {
    format!(
        "{} local digest: {} of {} days active, {} sessions, {} Git commits",
        period.as_str(),
        active_days,
        total_days,
        session_count,
        git_commits
    )
}

#[cfg(test)]
fn build_app_totals(sessions: &[&ExportSession]) -> Result<Vec<ExportAppTotal>, String> {
    build_app_totals_with_cancel(sessions, None)
}

fn build_app_totals_with_cancel(
    sessions: &[&ExportSession],
    cancellation: Option<&AtomicBool>,
) -> Result<Vec<ExportAppTotal>, String> {
    if let Some(cancellation) = cancellation {
        check_cancelled(cancellation)?;
    }
    let mut totals = BTreeMap::<String, (i64, usize)>::new();
    for (index, session) in sessions.iter().enumerate() {
        if index % 1_024 == 0 {
            if let Some(cancellation) = cancellation {
                check_cancelled(cancellation)?;
            }
        }
        let entry = totals.entry(session.app.clone()).or_default();
        entry.0 = entry
            .0
            .checked_add(session.duration_ms)
            .ok_or_else(|| "digest duration overflow".to_string())?;
        entry.1 = entry
            .1
            .checked_add(1)
            .ok_or_else(|| "digest session count overflow".to_string())?;
    }
    if totals.len() > MAX_DIGEST_APPS {
        return Err("digest 앱 수 제한을 초과했습니다".into());
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
    Ok(output)
}

fn top_app(sessions: &[&ExportSession]) -> Option<String> {
    let mut totals = BTreeMap::<String, i64>::new();
    for session in sessions {
        let value = totals.entry(session.app.clone()).or_default();
        *value = value.checked_add(session.duration_ms)?;
    }
    totals
        .into_iter()
        .max_by(|(left_app, left_duration), (right_app, right_duration)| {
            left_duration
                .cmp(right_duration)
                .then_with(|| right_app.as_bytes().cmp(left_app.as_bytes()))
        })
        .map(|(app, _)| app)
}

fn sum_durations(mut values: impl Iterator<Item = i64>) -> Result<i64, String> {
    values.try_fold(0_i64, |total, value| {
        total
            .checked_add(value)
            .ok_or_else(|| "digest duration overflow".to_string())
    })
}

fn markdown_cell(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace(['\r', '\n'], " ")
        .replace('`', "\\`")
}

fn markdown_row(output: &mut String, label: &str, value: &str) {
    output.push_str("| ");
    output.push_str(&markdown_cell(label));
    output.push_str(" | ");
    output.push_str(&markdown_cell(value));
    output.push_str(" |\n");
}

fn render_markdown(document: &DigestDocument) -> String {
    let mut output = String::from(DIGEST_MARKDOWN_HEADER);
    output.push_str("- Period: `");
    output.push_str(document.period.as_str());
    output.push_str("`\n- Range: `");
    output.push_str(&document.range.start_date);
    output.push_str("` to `");
    output.push_str(&document.range.end_date);
    output.push_str("` (date keys inclusive; end timestamp exclusive)\n- Timezone: `");
    output.push_str(&markdown_cell(&document.range.timezone));
    output.push_str("`\n- Filter: ");
    output.push_str(&markdown_cell(
        document.filter.app.as_deref().unwrap_or("all apps"),
    ));
    output.push_str("\n- ");
    output.push_str(&markdown_cell(&document.headline));
    output.push_str("\n\n");

    output.push_str("## Summary\n\n| Metric | Value |\n| --- | ---: |\n");
    markdown_row(
        &mut output,
        "PC usage (ms)",
        &document.summary.pc_usage_ms.to_string(),
    );
    markdown_row(
        &mut output,
        "Sessions",
        &document.summary.session_count.to_string(),
    );
    markdown_row(
        &mut output,
        "Active days",
        &format!(
            "{} / {}",
            document.summary.active_days, document.summary.total_days
        ),
    );
    markdown_row(
        &mut output,
        "Average daily usage (ms)",
        &document.summary.average_daily_usage_ms.to_string(),
    );
    markdown_row(
        &mut output,
        "Git commits",
        &document.summary.git_commits.to_string(),
    );
    markdown_row(
        &mut output,
        "Top app",
        document.summary.top_app.as_deref().unwrap_or("-"),
    );
    if document.summary.session_count == 0 && document.summary.git_commits == 0 {
        output.push_str("\nNo activity was recorded in the selected period or filter.\n");
    }
    output.push('\n');

    output.push_str("## Daily digest\n\n| Date | PC usage (ms) | Sessions | Git commits | Top app |\n| --- | ---: | ---: | ---: | --- |\n");
    for day in &document.daily {
        output.push_str("| ");
        output.push_str(&markdown_cell(&day.date));
        output.push_str(" | ");
        output.push_str(&day.pc_usage_ms.to_string());
        output.push_str(" | ");
        output.push_str(&day.session_count.to_string());
        output.push_str(" | ");
        output.push_str(&day.git_commits.to_string());
        output.push_str(" | ");
        output.push_str(&markdown_cell(day.top_app.as_deref().unwrap_or("-")));
        output.push_str(" |\n");
    }
    output.push('\n');

    output
        .push_str("## Applications\n\n| App | Duration (ms) | Sessions |\n| --- | ---: | ---: |\n");
    if document.app_totals.is_empty() {
        output.push_str("| - | 0 | 0 |\n");
    } else {
        for app in &document.app_totals {
            output.push_str("| ");
            output.push_str(&markdown_cell(&app.app));
            output.push_str(" | ");
            output.push_str(&app.duration_ms.to_string());
            output.push_str(" | ");
            output.push_str(&app.sessions.to_string());
            output.push_str(" |\n");
        }
    }
    output.push('\n');

    output
        .push_str("## Git projects\n\n| Project | Commits | Error code |\n| --- | ---: | --- |\n");
    if document.git.projects.is_empty() {
        output.push_str("| - | 0 | no_safe_project_paths |\n");
    } else {
        for project in &document.git.projects {
            output.push_str("| ");
            output.push_str(&markdown_cell(&project.path));
            output.push_str(" | ");
            output.push_str(&project.commits.to_string());
            output.push_str(" | ");
            output.push_str(&markdown_cell(project.error_code.as_deref().unwrap_or("-")));
            output.push_str(" |\n");
        }
    }
    output.push('\n');

    output.push_str(
        "## Sources\n\n| Source | Available | Scope | Error code |\n| --- | --- | --- | --- |\n",
    );
    for source in &document.sources {
        output.push_str("| ");
        output.push_str(&markdown_cell(&source.id));
        output.push_str(" | ");
        output.push_str(if source.available { "true" } else { "false" });
        output.push_str(" | ");
        output.push_str(&markdown_cell(&source.scope));
        output.push_str(" | ");
        output.push_str(&markdown_cell(source.error_code.as_deref().unwrap_or("-")));
        output.push_str(" |\n");
    }
    output.push('\n');

    output.push_str("## Rules\n\n| Rule | Definition |\n| --- | --- |\n");
    for (name, rule) in [
        ("Session window", &document.rules.session_window),
        ("Session duration", &document.rules.session_duration),
        ("Daily buckets", &document.rules.daily_buckets),
        ("App filter", &document.rules.app_filter),
        ("App totals", &document.rules.app_totals),
        ("Git commits", &document.rules.git_commits),
        ("Snapshot scope", &document.rules.snapshot_scope),
        ("Privacy", &document.rules.privacy),
        ("External processing", &document.rules.external_processing),
    ] {
        markdown_row(&mut output, name, rule);
    }
    output
}

/// Save boundary validation.  This is intentionally independent of the
/// renderer and rejects malformed/stale in-memory responses without exposing
/// their contents in an error.
pub fn validate_response(response: &DigestResponse) -> bool {
    if serde_json::to_vec(response).map_or(true, |bytes| bytes.len() > MAX_DIGEST_BYTES)
        || response.origin != DigestOrigin::Native
        || response.document.schema_version != DIGEST_SCHEMA_VERSION
        || response
            .handle
            .as_deref()
            .is_some_and(|handle| !valid_handle(handle))
        || response.markdown.len() > MAX_DIGEST_BYTES
        || !response.markdown.starts_with(DIGEST_MARKDOWN_HEADER)
        || response.document.daily.len() != response.document.range.day_boundaries.len()
        || !bounded_metadata(&response.document.headline, MAX_HEADLINE_BYTES)
        || !response.markdown.contains("## Rules\n\n")
    {
        return false;
    }
    let expected_ids = ["life-log", "git", "run-manager", "knowledge-base"];
    if response.document.sources.len() != expected_ids.len()
        || response
            .document
            .sources
            .iter()
            .zip(expected_ids)
            .any(|(source, expected)| source.id != expected)
    {
        return false;
    }
    let input = DigestInput {
        start_date: response.document.range.start_date.clone(),
        end_date: response.document.range.end_date.clone(),
        timezone: response.document.range.timezone.clone(),
        day_start: response.document.range.start_ms,
        day_end: response.document.range.end_ms,
        day_boundaries: response.document.range.day_boundaries.clone(),
        period: response.document.period,
        filter: response.document.filter.clone(),
    };
    if validate_input(&input).is_err() {
        return false;
    }
    let expected_filter = input
        .filter
        .app
        .as_deref()
        .map(|app| format!("exact sanitized app `{app}` only"))
        .unwrap_or_else(|| "all sanitized applications".into());
    if response.document.rules != digest_rules(expected_filter)
        || response.document.headline
            != digest_headline(
                response.document.period,
                response.document.summary.active_days,
                response.document.summary.total_days,
                response.document.summary.session_count,
                response.document.summary.git_commits,
            )
        || response.document.git.total_commits != response.document.summary.git_commits
        || response.document.summary.average_daily_usage_ms
            != if response.document.summary.total_days == 0 {
                0
            } else {
                response.document.summary.pc_usage_ms / response.document.summary.total_days as i64
            }
    {
        return false;
    }
    if response.document.summary.pc_usage_ms < 0
        || response.document.summary.session_count > export::MAX_EXPORT_SESSIONS
        || response.document.summary.active_days > response.document.summary.total_days
        || response.document.summary.total_days != response.document.daily.len()
        || response.document.summary.average_daily_usage_ms < 0
        || response
            .document
            .daily
            .iter()
            .enumerate()
            .any(|(index, day)| {
                let Some(boundary) = response.document.range.day_boundaries.get(index) else {
                    return true;
                };
                day.date != boundary.date
                    || day.start_ms != boundary.start_ms
                    || day.end_ms != boundary.end_ms
                    || day.has_activity
                        != (day.pc_usage_ms > 0 || day.session_count > 0 || day.git_commits > 0)
                    || day.pc_usage_ms < 0
                    || day.session_count > export::MAX_EXPORT_SESSIONS
                    || day.top_app.as_deref().is_some_and(|app| {
                        !bounded_metadata(app, 256)
                            || contains_secret_marker(app)
                            || response
                                .document
                                .filter
                                .app
                                .as_deref()
                                .is_some_and(|filter| filter != app)
                    })
                    || (day.session_count == 0 && day.top_app.is_some())
                    || (day.session_count > 0
                        && day.top_app.as_deref().is_none_or(|app| {
                            !response
                                .document
                                .app_totals
                                .iter()
                                .any(|total| total.app == app)
                        }))
            })
    {
        return false;
    }
    if response.document.app_totals.len() > MAX_DIGEST_APPS
        || response.document.app_totals.windows(2).any(|pair| {
            pair[0].duration_ms < pair[1].duration_ms
                || (pair[0].duration_ms == pair[1].duration_ms
                    && pair[0].app.as_bytes() >= pair[1].app.as_bytes())
        })
    {
        return false;
    }
    for app in &response.document.app_totals {
        if !bounded_metadata(&app.app, 256)
            || contains_secret_marker(&app.app)
            || response
                .document
                .filter
                .app
                .as_deref()
                .is_some_and(|filter| filter != app.app)
            || app.duration_ms < 0
            || app.sessions == 0
            || app.sessions > export::MAX_EXPORT_SESSIONS
        {
            return false;
        }
    }
    let expected_session_count = response
        .document
        .daily
        .iter()
        .fold(0usize, |sum, day| sum.saturating_add(day.session_count));
    let expected_pc_usage =
        sum_durations(response.document.daily.iter().map(|day| day.pc_usage_ms))
            .ok()
            .unwrap_or(i64::MIN);
    let app_session_count = response
        .document
        .app_totals
        .iter()
        .fold(0usize, |sum, app| sum.saturating_add(app.sessions));
    let app_pc_usage = sum_durations(
        response
            .document
            .app_totals
            .iter()
            .map(|app| app.duration_ms),
    )
    .ok()
    .unwrap_or(i64::MIN);
    let expected_active_days = response
        .document
        .daily
        .iter()
        .filter(|day| day.has_activity)
        .count();
    let mut expected_git_errors = response
        .document
        .git
        .projects
        .iter()
        .filter_map(|project| project.error_code.as_deref())
        .filter(|code| safe_source_error(code))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    expected_git_errors.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    expected_git_errors.dedup();
    let mut project_identities = Vec::new();
    let projects_valid = response.document.git.projects.len() <= export::MAX_EXPORT_PROJECTS
        && response.document.git.projects.iter().all(|project| {
            let Some(path) = parse_safe_project_path(&project.path) else {
                return false;
            };
            if path.as_str() != project.path
                || contains_secret_marker(&project.path)
                || project
                    .error_code
                    .as_deref()
                    .is_some_and(|code| !safe_source_error(code) || project.commits != 0)
            {
                return false;
            }
            if project_identities
                .iter()
                .any(|identity: &String| identity == path.identity())
            {
                return false;
            }
            if project_identities
                .last()
                .is_some_and(|previous: &String| previous.as_bytes() >= path.identity().as_bytes())
            {
                return false;
            }
            project_identities.push(path.identity().to_owned());
            true
        });
    let git_daily_total = response
        .document
        .daily
        .iter()
        .fold(0u32, |sum, day| sum.saturating_add(day.git_commits));
    response.document.summary.session_count == expected_session_count
        && response.document.summary.session_count == app_session_count
        && response.document.summary.pc_usage_ms == expected_pc_usage
        && response.document.summary.pc_usage_ms == app_pc_usage
        && response.document.summary.total_days == response.document.daily.len()
        && response.document.summary.active_days == expected_active_days
        && response.document.summary.top_app
            == response
                .document
                .app_totals
                .first()
                .map(|app| app.app.clone())
        && response.document.git.total_commits
            == response
                .document
                .git
                .projects
                .iter()
                .fold(0u32, |sum, project| sum.saturating_add(project.commits))
        && response.document.git.total_commits == git_daily_total
        && response.document.git.error_codes == expected_git_errors
        && projects_valid
        && response
            .document
            .period
            .valid_day_count(response.document.daily.len())
        && response
            .document
            .sources
            .iter()
            .enumerate()
            .all(|(index, source)| {
                valid_source(source, expected_ids[index], &response.document.git)
            })
        && response.markdown == render_markdown(&response.document)
}

fn valid_source(source: &SourceMetadata, expected_id: &str, git: &ExportGit) -> bool {
    if source.id != expected_id
        || source.scope
            != if expected_id == "run-manager" || expected_id == "knowledge-base" {
                "latest-snapshot-out-of-range"
            } else {
                "requested-range"
            }
        || !bounded_metadata(&source.scope, 64)
        || source.error_code.as_deref().is_some_and(|code| {
            if expected_id == "run-manager" || expected_id == "knowledge-base" {
                !safe_snapshot_error(code)
            } else {
                !safe_source_error(code)
            }
        })
        || source
            .producer_version
            .as_deref()
            .is_some_and(|value| !bounded_metadata(value, 64))
        || source
            .producer_version
            .as_deref()
            .is_some_and(|value| !valid_producer_version(value))
        || source
            .generated_at
            .as_deref()
            .is_some_and(|value| !bounded_metadata(value, 32))
        || source
            .generated_at
            .as_deref()
            .is_some_and(|value| !valid_generated_at(value))
        || source
            .freshness_ms
            .is_some_and(|value| value > export::MAX_PROVENANCE_FRESHNESS_MS)
    {
        return false;
    }
    match expected_id {
        "life-log" => {
            source.available
                && source.error_code.is_none()
                && source.schema_version == Some(export::EXPORT_SCHEMA_VERSION)
                && source.snapshot_version.is_none()
                && source.producer_version.as_deref() == Some(env!("CARGO_PKG_VERSION"))
                && source.generated_at.is_none()
                && source.freshness_ms.is_none()
                && source.view.is_none()
        }
        "git" => {
            let expected_error = if git.projects.is_empty() {
                Some("no_safe_project_paths")
            } else {
                git.error_codes.first().map(String::as_str)
            };
            source.available == expected_error.is_none()
                && source.error_code.as_deref() == expected_error
                && source.schema_version.is_none()
                && source.snapshot_version.is_none()
                && source.producer_version.is_none()
                && source.generated_at.is_none()
                && source.freshness_ms.is_none()
                && source.view.is_none()
                && git.projects.iter().all(|project| {
                    parse_safe_project_path(&project.path)
                        .is_some_and(|path| path.as_str() == project.path)
                        && !contains_secret_marker(&project.path)
                })
        }
        "run-manager" | "knowledge-base" => {
            let provenance_complete = source.producer_version.is_some()
                && source.generated_at.is_some()
                && source.freshness_ms.is_some();
            let provenance_empty = source.producer_version.is_none()
                && source.generated_at.is_none()
                && source.freshness_ms.is_none();
            source.available == source.error_code.is_none()
                && source.schema_version.is_none_or(|version| version > 0)
                && source.snapshot_version.is_none_or(|version| version > 0)
                && source
                    .schema_version
                    .is_none_or(|version| source.snapshot_version == Some(version))
                && (provenance_complete || provenance_empty)
                && source.view.as_deref().is_none_or(|view| {
                    expected_id == "knowledge-base" && matches!(view, "activity" | "legacy-data")
                })
                && (!source.available
                    || (provenance_complete
                        && source.schema_version == Some(export::EXPORT_SCHEMA_VERSION)
                        && source.snapshot_version == Some(export::EXPORT_SCHEMA_VERSION)
                        && (expected_id != "knowledge-base"
                            || source
                                .view
                                .as_deref()
                                .is_some_and(|view| matches!(view, "activity" | "legacy-data")))))
        }
        _ => false,
    }
}

fn safe_snapshot_error(value: &str) -> bool {
    matches!(
        value,
        "snapshot_unavailable"
            | "snapshot_invalid"
            | "snapshot_schema_unsupported"
            | "snapshot_payload_invalid"
            | "snapshot_changed_during_read"
            | "snapshot_stale"
    )
}

fn valid_producer_version(value: &str) -> bool {
    if !value.is_ascii() || value.len() > 64 {
        return false;
    }
    let (without_build, build) = match value.split_once('+') {
        Some((core, build)) if !build.is_empty() => (core, Some(build)),
        Some(_) => return false,
        None => (value, None),
    };
    let (core, prerelease) = match without_build.split_once('-') {
        Some((core, prerelease)) if !prerelease.is_empty() => (core, Some(prerelease)),
        Some(_) => return false,
        None => (without_build, None),
    };
    let segments = core.split('.').collect::<Vec<_>>();
    if segments.len() != 3
        || segments.iter().any(|segment| {
            segment.is_empty()
                || !segment.bytes().all(|byte| byte.is_ascii_digit())
                || (segment.len() > 1 && segment.starts_with('0'))
        })
    {
        return false;
    }
    prerelease.is_none_or(|suffix| valid_semver_suffix(suffix, false))
        && build.is_none_or(|suffix| valid_semver_suffix(suffix, true))
}

fn valid_semver_suffix(value: &str, numeric_leading_zero_allowed: bool) -> bool {
    value.split('.').all(|segment| {
        !segment.is_empty()
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            && (numeric_leading_zero_allowed
                || !segment.bytes().all(|byte| byte.is_ascii_digit())
                || segment.len() == 1
                || !segment.starts_with('0'))
    })
}

fn valid_generated_at(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
        || bytes.iter().enumerate().any(|(index, byte)| {
            !matches!(index, 4 | 7 | 10 | 13 | 16 | 19) && !byte.is_ascii_digit()
        })
    {
        return false;
    }
    let number =
        |start: usize, end: usize| -> u32 { value[start..end].parse::<u32>().unwrap_or(u32::MAX) };
    let (year, month, day) = (number(0, 4), number(5, 7), number(8, 10));
    let (hour, minute, second) = (number(11, 13), number(14, 16), number(17, 19));
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    year > 0 && day > 0 && day <= days && hour <= 23 && minute <= 59 && second <= 59
}

fn bounded_metadata(value: &str, max_bytes: usize) -> bool {
    !value.is_empty() && value.len() <= max_bytes && !value.chars().any(char::is_control)
}

fn safe_source_error(value: &str) -> bool {
    crate::core::error_codes::is_source(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::db;
    use crate::core::models::ClosedSession;
    use std::sync::atomic::AtomicBool;

    fn input(period: DigestPeriod, days: usize) -> DigestInput {
        let boundaries = (0..days)
            .map(|index| ExportDayBoundary {
                date: format!("2024-01-{:02}", index + 1),
                start_ms: index as i64 * 86_400_000,
                end_ms: (index as i64 + 1) * 86_400_000,
            })
            .collect::<Vec<_>>();
        DigestInput {
            start_date: "2024-01-01".into(),
            end_date: format!("2024-01-{:02}", days),
            timezone: "UTC".into(),
            day_start: 0,
            day_end: days as i64 * 86_400_000,
            day_boundaries: boundaries,
            period,
            filter: DigestFilter::default(),
        }
    }

    fn calendar_month_input(year: i32, month: u32, days: usize) -> DigestInput {
        let boundaries = (1..=days)
            .map(|day| ExportDayBoundary {
                date: format!("{year:04}-{month:02}-{day:02}"),
                start_ms: (day as i64 - 1) * 86_400_000,
                end_ms: day as i64 * 86_400_000,
            })
            .collect::<Vec<_>>();
        DigestInput {
            start_date: format!("{year:04}-{month:02}-01"),
            end_date: format!("{year:04}-{month:02}-{days:02}"),
            timezone: "UTC".into(),
            day_start: 0,
            day_end: days as i64 * 86_400_000,
            day_boundaries: boundaries,
            period: DigestPeriod::Month,
            filter: DigestFilter::default(),
        }
    }

    #[test]
    fn validates_daily_weekly_and_existing_monthly_shapes() {
        assert!(validate_input(&input(DigestPeriod::Day, 1)).is_ok());
        let mut day_with_two_dates = input(DigestPeriod::Day, 1);
        day_with_two_dates.end_date = "2024-01-02".into();
        assert!(validate_input(&day_with_two_dates).is_err());
        assert!(validate_input(&input(DigestPeriod::Day, 2)).is_err());

        assert!(validate_input(&input(DigestPeriod::Week, 7)).is_ok());
        assert!(validate_input(&input(DigestPeriod::Week, 6)).is_err());
        let mut week_starting_on_sunday = input(DigestPeriod::Week, 7);
        week_starting_on_sunday.start_date = "2024-01-07".into();
        assert_eq!(
            validate_input(&week_starting_on_sunday).unwrap_err(),
            "digest 주 범위가 올바르지 않습니다"
        );
        assert!(validate_input(&input(DigestPeriod::Month, 31)).is_ok());
        assert!(validate_input(&input(DigestPeriod::Month, 7)).is_err());

        assert!(validate_input(&calendar_month_input(2023, 2, 28)).is_ok());
        assert!(validate_input(&calendar_month_input(2024, 2, 29)).is_ok());
        assert!(validate_input(&calendar_month_input(2024, 2, 28)).is_err());

        let mut invalid_date = input(DigestPeriod::Week, 7);
        invalid_date.start_date = "2024-02-30".into();
        assert_eq!(
            validate_input(&invalid_date).unwrap_err(),
            "digest 주 범위가 올바르지 않습니다"
        );
    }

    #[test]
    fn keeps_authoritative_dst_day_width_for_daily_digest() {
        let mut short_day = input(DigestPeriod::Day, 1);
        short_day.day_end = 23 * 60 * 60 * 1_000;
        short_day.day_boundaries[0].end_ms = short_day.day_end;
        assert!(validate_input(&short_day).is_ok());

        let mut long_day = input(DigestPeriod::Day, 1);
        long_day.day_end = 25 * 60 * 60 * 1_000;
        long_day.day_boundaries[0].end_ms = long_day.day_end;
        assert!(validate_input(&long_day).is_ok());
    }

    #[test]
    fn rejects_secret_or_control_filter_without_echoing_it() {
        for app in [
            "Authorization: bearer secret-value".to_string(),
            "C:\\private\\token".to_string(),
            "bad\u{0000}app".to_string(),
        ] {
            let mut digest_input = input(DigestPeriod::Week, 7);
            digest_input.filter.app = Some(app.clone());
            let error = validate_input(&digest_input).unwrap_err();
            assert_eq!(error, "digest 필터가 올바르지 않습니다");
            assert!(!error.contains(&app));
        }
    }

    #[test]
    fn app_aggregation_is_deterministic_and_empty_filter_is_supported() {
        let code = ExportSession {
            id: 1,
            app: "Code.exe".into(),
            title: "safe".into(),
            start_ts_ms: 0,
            end_ts_ms: 100,
            duration_ms: 100,
        };
        let browser = ExportSession {
            id: 2,
            app: "Browser.exe".into(),
            title: "safe".into(),
            start_ts_ms: 86_400_000,
            end_ts_ms: 86_400_200,
            duration_ms: 200,
        };
        let sessions = vec![&code, &browser];
        let totals = build_app_totals(&sessions).unwrap();
        assert_eq!(totals[0].app, "Browser.exe");
        assert_eq!(totals[1].app, "Code.exe");
        let tie = [
            ExportSession {
                id: 3,
                app: "z-app".into(),
                title: "safe".into(),
                start_ts_ms: 0,
                end_ts_ms: 100,
                duration_ms: 100,
            },
            ExportSession {
                id: 4,
                app: "a-app".into(),
                title: "safe".into(),
                start_ts_ms: 0,
                end_ts_ms: 100,
                duration_ms: 100,
            },
        ];
        assert_eq!(
            top_app(&tie.iter().collect::<Vec<_>>()).as_deref(),
            Some("a-app")
        );
        assert_eq!(build_app_totals(&[]).unwrap(), Vec::<ExportAppTotal>::new());
        assert_eq!(top_app(&[]), None);

        let cancellation = AtomicBool::new(true);
        assert_eq!(
            build_app_totals_with_cancel(&sessions, Some(&cancellation)).unwrap_err(),
            "digest_cancelled"
        );
    }

    #[test]
    fn native_response_reuses_bounded_export_and_passes_save_validation() {
        let connection = Connection::open_in_memory().unwrap();
        db::migrate(&connection).unwrap();
        db::insert_session(
            &connection,
            &ClosedSession {
                app: "Code.exe".into(),
                title: "ordinary title".into(),
                start_ts: 0,
                end_ts: 100,
            },
        )
        .unwrap();
        let digest_input = input(DigestPeriod::Week, 7);
        let prepared = prepare(&connection, &[], &digest_input).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let response = runtime
            .block_on(build_response(prepared, &digest_input))
            .unwrap();
        assert_eq!(response.document.summary.session_count, 1);
        assert_eq!(response.document.summary.active_days, 1);
        assert!(validate_response(&response));
        assert!(!response.markdown.contains("ordinary title"));
        assert!(response
            .markdown
            .contains("date keys inclusive; end timestamp exclusive"));
    }

    #[test]
    fn app_filter_is_applied_after_privacy_sanitization() {
        let connection = Connection::open_in_memory().unwrap();
        db::migrate(&connection).unwrap();
        db::insert_session(
            &connection,
            &ClosedSession {
                app: "Code.exe".into(),
                title: "code window".into(),
                start_ts: 0,
                end_ts: 100,
            },
        )
        .unwrap();
        db::insert_session(
            &connection,
            &ClosedSession {
                app: "Browser.exe".into(),
                title: "browser window".into(),
                start_ts: 100,
                end_ts: 300,
            },
        )
        .unwrap();
        let mut digest_input = input(DigestPeriod::Week, 7);
        digest_input.filter.app = Some("Code.exe".into());
        let prepared = prepare(&connection, &[], &digest_input).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let response = runtime
            .block_on(build_response(prepared, &digest_input))
            .unwrap();

        assert_eq!(response.document.summary.session_count, 1);
        assert_eq!(response.document.summary.pc_usage_ms, 100);
        assert_eq!(response.document.app_totals.len(), 1);
        assert_eq!(response.document.app_totals[0].app, "Code.exe");
        assert_eq!(response.document.daily[0].session_count, 1);
        assert!(validate_response(&response));
    }

    #[test]
    fn response_validation_rejects_changed_markdown_or_range_boundaries() {
        let connection = Connection::open_in_memory().unwrap();
        db::migrate(&connection).unwrap();
        let digest_input = input(DigestPeriod::Week, 7);
        let prepared = prepare(&connection, &[], &digest_input).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let response = runtime
            .block_on(build_response(prepared, &digest_input))
            .unwrap();

        let mut changed_markdown = response.clone();
        changed_markdown.markdown.push(' ');
        assert!(!validate_response(&changed_markdown));

        let mut changed_boundary = response;
        changed_boundary.document.range.day_boundaries[1].start_ms += 1;
        assert!(!validate_response(&changed_boundary));
    }

    #[test]
    fn response_validation_rejects_untrusted_snapshot_view() {
        let connection = Connection::open_in_memory().unwrap();
        db::migrate(&connection).unwrap();
        let digest_input = input(DigestPeriod::Week, 7);
        let prepared = prepare(&connection, &[], &digest_input).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let mut response = runtime
            .block_on(build_response(prepared, &digest_input))
            .unwrap();
        response.document.sources[2].view = Some("raw-local-path".into());
        assert!(!validate_response(&response));
    }

    #[test]
    fn save_handle_is_opaque_immutable_and_bounded_by_ttl_store() {
        let connection = Connection::open_in_memory().unwrap();
        db::migrate(&connection).unwrap();
        let digest_input = input(DigestPeriod::Week, 7);
        let prepared = prepare(&connection, &[], &digest_input).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let response = runtime
            .block_on(build_response(prepared, &digest_input))
            .unwrap();
        let store = DigestHandleStore::default();
        let issued = store.issue(response).unwrap();
        let handle = issued.handle.clone().unwrap();
        assert_eq!(handle.len(), 32);
        assert!(handle.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(store.get(&handle).unwrap(), issued);

        let mut tampered = store.get(&handle).unwrap();
        tampered.markdown.push('x');
        assert_ne!(tampered, store.get(&handle).unwrap());
        assert_eq!(
            store.get("not-a-handle").unwrap_err(),
            "digest_handle_expired"
        );
    }

    #[test]
    fn cancellation_is_rejected_before_native_db_or_git_work() {
        let connection = Connection::open_in_memory().unwrap();
        db::migrate(&connection).unwrap();
        let cancellation = Arc::new(AtomicBool::new(true));
        assert_eq!(
            prepare_with_cancel(
                &connection,
                &[],
                &input(DigestPeriod::Week, 7),
                cancellation
            )
            .err()
            .unwrap(),
            "digest_cancelled"
        );
    }
}
