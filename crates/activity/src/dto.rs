//! RPC parameter and result types (no daemon dependency).
use crate::model::event::Observation;
use crate::model::privacy::{InvalidRule, PrivacyRules};
use devbox_protocol::domain_error;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// `activity.ingest` events per call at most.
pub const MAX_EVENTS: usize = 1000;
/// `runId`: 1..=64 bytes of ASCII letters, digits, `-` and `_`.
pub const MAX_RUN_ID_BYTES: usize = 64;
/// Stored app names and titles are cut to these UTF-8 byte lengths (v0.9.0 limits).
pub const MAX_APP_BYTES: usize = 256;
pub const MAX_TITLE_BYTES: usize = 4 * 1024;
/// `activity.range` spans at most this many days (inclusive).
pub const MAX_RANGE_DAYS: i64 = 31;
/// `activity.export` spans at most this many days (inclusive).
pub const MAX_EXPORT_DAYS: i64 = 366;
/// `activity.day` titles at most.
pub const MAX_DAY_TITLES: usize = 50;

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct NoParams {}

/// The app's offline-buffer batch: `upto` is its last observation `seq`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IngestBatch {
    pub run_id: String,
    pub upto: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct Ingest {
    /// `null` outside the app's batch path: session ids are then random.
    pub batch: Option<IngestBatch>,
    /// At most 1,000.
    pub events: Vec<Observation>,
    /// With `batch`: the run's cumulative count of observations its offline
    /// buffer dropped (resends repeat it; the daemon adds only the increase).
    /// Without: dropped since the previous call.
    pub dropped: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct Ingested {
    /// Events applied; `0` for a batch at or below the run's last `upto`.
    pub accepted: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum StorageState {
    Ok,
    StorageFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum StorageProblem {
    DiskFull,
    Database,
}

/// Storage health. The error text itself stays in the daemon log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivityStatus {
    pub rev: String,
    pub state: StorageState,
    /// Closed sessions waiting to be written.
    pub pending: u32,
    /// Lost before storage: retry-queue overflow plus the app's offline drops.
    pub dropped: u64,
    pub last_error: Option<StorageProblem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RulesView {
    pub rev: String,
    pub rules: PrivacyRules,
    pub idle_threshold_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetRules {
    pub rules: PrivacyRules,
    /// `60_000..=86_400_000`.
    pub idle_threshold_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct RulesRev {
    pub rev: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct DayQuery {
    /// Local date `YYYY-MM-DD`.
    pub date: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppMs {
    pub app: String,
    pub ms: i64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectMs {
    pub project_id: String,
    pub ms: i64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TitleMs {
    pub app: String,
    pub title: String,
    pub ms: i64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HourMs {
    /// Local hour `0..=23`.
    pub hour: u32,
    pub ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DayActivity {
    pub rev: String,
    pub date: String,
    pub total_ms: i64,
    pub apps: Vec<AppMs>,
    pub projects: Vec<ProjectMs>,
    /// Top 50 non-empty titles.
    pub titles: Vec<TitleMs>,
    /// 24 local hours.
    pub hours: Vec<HourMs>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct RangeQuery {
    /// Inclusive local dates, at most 31 days.
    pub from: String,
    pub to: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DayTotal {
    pub date: String,
    pub total_ms: i64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RangeActivity {
    pub rev: String,
    pub days: Vec<DayTotal>,
    pub apps: Vec<AppMs>,
    pub projects: Vec<ProjectMs>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Csv,
    Json,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ExportQuery {
    /// Inclusive local dates, at most 366 days.
    pub from: String,
    pub to: String,
    pub format: ExportFormat,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct Exported {
    pub path: String,
}

/// Stored sessions or the rules changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ActivityChanged {
    pub rev: String,
}

// `config_unavailable`: `config.toml` is a symlink, not a regular file or not TOML.
domain_error! {
    pub enum ActivityError {
        InvalidRules { problems: Vec<InvalidRule> } = "activity.invalid_rules",
        InvalidRange = "activity.invalid_range",
        InvalidInput = "activity.invalid_input",
        ConfigUnavailable = "activity.config_unavailable",
    }
}
