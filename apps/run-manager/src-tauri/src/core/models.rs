use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::net::IpAddr;

pub const DEFAULT_SERVICE_START_GRACE_MS: i64 = 10_000;

/// The kinds supported by the shared database table. Phase 1 command handlers
/// deliberately accept only `Job`; the service variant is reserved for Phase 2.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum JobKind {
    Job,
    Service,
}

impl JobKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Job => "job",
            Self::Service => "service",
        }
    }
}

impl fmt::Display for JobKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str((*self).as_str())
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TargetKind {
    Windows,
    Wsl,
}

impl TargetKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Wsl => "wsl",
        }
    }
}

impl fmt::Display for TargetKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str((*self).as_str())
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum OverlapPolicy {
    #[default]
    Skip,
    Queue,
    KillPrevious,
}

impl OverlapPolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Skip => "skip",
            Self::Queue => "queue",
            Self::KillPrevious => "kill-previous",
        }
    }
}

impl fmt::Display for OverlapPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str((*self).as_str())
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RestartPolicy {
    #[default]
    Never,
    OnFailure,
    Always,
}

impl RestartPolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Never => "never",
            Self::OnFailure => "on-failure",
            Self::Always => "always",
        }
    }
}

impl fmt::Display for RestartPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str((*self).as_str())
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RunStatus {
    Queued,
    Starting,
    Running,
    Stopping,
    Succeeded,
    Failed,
    Cancelled,
    Skipped,
}

/// Bounded, read-only history query.  The UI may combine any of these fields;
/// storage applies them in one parameterized query so a history search never
/// becomes a path, command, or SQL execution boundary.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunHistoryFilter {
    pub job_id: Option<String>,
    /// `job` or `service`; `None` means both kinds.
    pub kind: Option<JobKind>,
    pub status: Option<RunStatus>,
    pub start_at: Option<i64>,
    pub end_at: Option<i64>,
    pub min_duration_ms: Option<i64>,
    pub max_duration_ms: Option<i64>,
    pub limit: Option<u32>,
}

impl RunHistoryFilter {
    pub const MAX_ID_BYTES: usize = 128;
    pub const MAX_DURATION_MS: i64 = 30 * 24 * 60 * 60 * 1_000;
    pub const MAX_LIMIT: u32 = 500;

    /// Validate untrusted query values before they reach SQLite.  Duration is
    /// measured only for runs with a start; queued/skipped rows do not match a
    /// duration bound.  `now` is used for a still-running row by storage.
    pub fn validate(&self) -> Result<(), ModelError> {
        if let Some(job_id) = &self.job_id {
            validate_history_text("job_id", job_id, Self::MAX_ID_BYTES)?;
        }
        if let (Some(start_at), Some(end_at)) = (self.start_at, self.end_at) {
            if end_at <= start_at {
                return Err(ModelError::InvalidHistoryRange);
            }
        }
        if self.start_at.is_some_and(|value| value < 0)
            || self.end_at.is_some_and(|value| value < 0)
        {
            return Err(ModelError::InvalidHistoryRange);
        }
        if self
            .min_duration_ms
            .is_some_and(|value| !(0..=Self::MAX_DURATION_MS).contains(&value))
            || self
                .max_duration_ms
                .is_some_and(|value| !(0..=Self::MAX_DURATION_MS).contains(&value))
        {
            return Err(ModelError::InvalidHistoryDuration);
        }
        if let (Some(minimum), Some(maximum)) = (self.min_duration_ms, self.max_duration_ms) {
            if maximum < minimum {
                return Err(ModelError::InvalidHistoryDuration);
            }
        }
        if self
            .limit
            .is_some_and(|value| value == 0 || value > Self::MAX_LIMIT)
        {
            return Err(ModelError::InvalidHistoryRange);
        }
        Ok(())
    }
}

fn validate_history_text(
    field: &'static str,
    value: &str,
    max_bytes: usize,
) -> Result<(), ModelError> {
    if value.is_empty() || value.len() > max_bytes || value.chars().any(char::is_control) {
        return Err(ModelError::InvalidHistoryField(field));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ServiceInstanceState {
    Stopped,
    Starting,
    Running,
    Stopping,
    RetryWaiting,
}

impl ServiceInstanceState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Stopping => "stopping",
            Self::RetryWaiting => "retry_waiting",
        }
    }
}

impl fmt::Display for ServiceInstanceState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str((*self).as_str())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ServiceInstance {
    pub job_id: String,
    pub generation: i64,
    pub active_run_id: Option<String>,
    pub state: ServiceInstanceState,
    pub owner_instance_id: Option<String>,
    pub attempt_token: Option<String>,
    pub next_retry_at: Option<i64>,
    pub consecutive_failures: i64,
    pub updated_at: i64,
}

/// Redacted read DTO for the service-instance UI boundary. Ownership tokens
/// and internal identity never round-trip to the frontend.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ServiceInstanceView {
    pub job_id: String,
    pub generation: i64,
    pub state: ServiceInstanceState,
    pub consecutive_failures: i64,
    pub next_retry_at: Option<i64>,
}

impl ServiceInstanceView {
    pub fn from_instance(instance: &ServiceInstance) -> Self {
        Self {
            job_id: instance.job_id.clone(),
            generation: instance.generation,
            state: instance.state,
            consecutive_failures: instance.consecutive_failures,
            next_retry_at: instance.next_retry_at,
        }
    }
}

impl RunStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Stopping => "stopping",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Skipped => "skipped",
        }
    }
}

impl fmt::Display for RunStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str((*self).as_str())
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JobInput {
    pub name: String,
    pub command: String,
    pub cwd: Option<String>,
    pub target_kind: TargetKind,
    pub target_distro: Option<String>,
    /// Plaintext values are accepted only at the command boundary. Commands
    /// consume this field immediately and pass ciphertext to storage; the
    /// database and read DTOs never contain this map.
    #[serde(default)]
    pub environment: EnvironmentUpdate,
    pub cron_expr: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub overlap_policy: OverlapPolicy,
    #[serde(default)]
    pub catch_up: bool,
}

impl JobInput {
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.name.trim().is_empty() {
            return Err(ModelError::EmptyField("name"));
        }
        if self.command.is_empty() {
            return Err(ModelError::EmptyField("command"));
        }
        if self.cron_expr.trim().is_empty() {
            return Err(ModelError::EmptyField("cron_expr"));
        }
        for (field, value) in [
            ("name", self.name.as_str()),
            ("command", self.command.as_str()),
            ("cron_expr", self.cron_expr.as_str()),
        ] {
            if value.contains('\0') {
                return Err(ModelError::NulByte(field));
            }
        }
        if let Some(cwd) = &self.cwd {
            if cwd.contains('\0') {
                return Err(ModelError::NulByte("cwd"));
            }
        }
        if let Some(distro) = &self.target_distro {
            if distro.contains('\0') {
                return Err(ModelError::NulByte("target_distro"));
            }
        }
        crate::core::cron::validate_cron(&self.cron_expr)
            .map_err(|error| ModelError::InvalidCron(error.to_string()))?;
        self.environment.validate()?;
        match self.target_kind {
            TargetKind::Windows if self.target_distro.is_some() => {
                Err(ModelError::UnexpectedTargetDistro)
            }
            TargetKind::Wsl if self.target_distro.as_deref().is_none_or(str::is_empty) => {
                Err(ModelError::MissingTargetDistro)
            }
            _ => Ok(()),
        }
    }
}

/// The only environment operation exposed by the create/update command DTO.
/// `keep` preserves an existing ciphertext, `clear` explicitly removes it,
/// and `replace` supplies a complete new map for immediate encryption.
#[derive(Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum EnvironmentUpdate {
    #[serde(rename = "keep")]
    #[default]
    Keep,
    #[serde(rename = "replace")]
    Replace { values: BTreeMap<String, String> },
    #[serde(rename = "clear")]
    Clear,
}

impl fmt::Debug for EnvironmentUpdate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Keep => formatter.write_str("Keep"),
            Self::Clear => formatter.write_str("Clear"),
            Self::Replace { .. } => formatter.write_str("Replace { values: <redacted> }"),
        }
    }
}

impl EnvironmentUpdate {
    pub fn validate(&self) -> Result<(), ModelError> {
        if let Self::Replace { values } = self {
            crate::core::shell::validate_environment(values)
                .map_err(|error| ModelError::InvalidEnvironment(error.to_string()))?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ServiceInput {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub cwd: Option<String>,
    pub target_kind: TargetKind,
    #[serde(default)]
    pub target_distro: Option<String>,
    #[serde(default)]
    pub environment: EnvironmentUpdate,
    #[serde(default)]
    pub restart_policy: RestartPolicy,
    #[serde(default)]
    pub auto_start: bool,
    #[serde(default)]
    pub health_tcp_address: Option<String>,
    #[serde(default)]
    pub health_tcp_port: Option<u16>,
}

impl ServiceInput {
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.name.trim().is_empty() {
            return Err(ModelError::EmptyField("name"));
        }
        if self.command.trim().is_empty() {
            return Err(ModelError::EmptyField("command"));
        }
        for (field, value) in [
            ("name", self.name.as_str()),
            ("command", self.command.as_str()),
        ] {
            if value.contains('\0') {
                return Err(ModelError::NulByte(field));
            }
        }
        if let Some(cwd) = &self.cwd {
            if cwd.contains('\0') {
                return Err(ModelError::NulByte("cwd"));
            }
        }
        if let Some(distro) = &self.target_distro {
            if distro.contains('\0') {
                return Err(ModelError::NulByte("target_distro"));
            }
        }
        match self.target_kind {
            TargetKind::Windows if self.target_distro.is_some() => {
                return Err(ModelError::UnexpectedTargetDistro);
            }
            TargetKind::Wsl
                if self
                    .target_distro
                    .as_deref()
                    .is_none_or(|distro| distro.trim().is_empty()) =>
            {
                return Err(ModelError::MissingTargetDistro);
            }
            _ => {}
        }

        match (&self.health_tcp_address, self.health_tcp_port) {
            (None, None) => {}
            (Some(address), Some(port)) => {
                if address.trim().is_empty() {
                    return Err(ModelError::EmptyField("health_tcp_address"));
                }
                if address.contains('\0') {
                    return Err(ModelError::NulByte("health_tcp_address"));
                }
                if port == 0 {
                    return Err(ModelError::InvalidServicePort);
                }
                if !address.eq_ignore_ascii_case("localhost")
                    && address
                        .parse::<IpAddr>()
                        .map_or(true, |ip| !ip.is_loopback())
                {
                    return Err(ModelError::NonLocalServiceAddress);
                }
            }
            (Some(_), None) => return Err(ModelError::MissingServicePort),
            (None, Some(_)) => return Err(ModelError::MissingServiceAddress),
        }
        self.environment.validate()?;
        Ok(())
    }
}

/// Storage receives only the result of the protector. This type is not
/// deserializable and therefore cannot be supplied by a frontend caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvironmentCiphertextUpdate {
    Keep,
    Replace(Vec<u8>),
    Clear,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub kind: JobKind,
    pub name: String,
    pub command: String,
    pub cwd: Option<String>,
    pub target_kind: TargetKind,
    pub target_distro: Option<String>,
    /// Only presence is exposed to the UI; ciphertext bytes never round-trip
    /// through a read DTO.
    pub env_configured: bool,
    pub cron_expr: Option<String>,
    pub enabled: bool,
    pub overlap_policy: OverlapPolicy,
    pub catch_up: bool,
    pub last_evaluated_at: Option<i64>,
    pub next_queue_sequence: i64,
    pub restart_policy: Option<RestartPolicy>,
    pub auto_start: Option<bool>,
    pub health_tcp_address: Option<String>,
    pub health_tcp_port: Option<u16>,
    pub health_start_grace_ms: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: String,
    pub job_id: String,
    pub scheduled_at: Option<i64>,
    pub occurrence_wall_key: Option<String>,
    pub queue_sequence: i64,
    pub blocked_by_run_id: Option<String>,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub exit_code: Option<i32>,
    pub status: RunStatus,
    pub owner_instance_id: Option<String>,
    pub attempt_token: Option<String>,
    pub error_message: Option<String>,
    pub target_pid: Option<i64>,
    pub target_process_created_at: Option<i64>,
    pub target_pgid: Option<i64>,
    pub target_sid: Option<i64>,
    pub process_marker: Option<String>,
    pub log_dir: Option<String>,
    pub logs_deleted_at: Option<i64>,
    pub created_at: i64,
}

/// Stable, redacted read DTO for Tauri/UI boundaries. Process identity,
/// ownership tokens, raw adapter/storage text, and filesystem paths remain
/// inside the scheduler/storage layers. `logs_available` is only a boolean;
/// `tail_log` resolves the app-owned directory from the run id server-side.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunView {
    pub id: String,
    pub job_id: String,
    pub scheduled_at: Option<i64>,
    pub occurrence_wall_key: Option<String>,
    pub queue_sequence: i64,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub exit_code: Option<i32>,
    pub status: RunStatus,
    pub logs_available: bool,
    pub failure_code: Option<String>,
    pub created_at: i64,
}

impl RunView {
    pub fn from_run(run: &Run) -> Self {
        Self {
            id: run.id.clone(),
            job_id: run.job_id.clone(),
            scheduled_at: run.scheduled_at,
            occurrence_wall_key: run.occurrence_wall_key.clone(),
            queue_sequence: run.queue_sequence,
            started_at: run.started_at,
            ended_at: run.ended_at,
            exit_code: run.exit_code,
            status: run.status,
            logs_available: run.log_dir.is_some(),
            failure_code: (run.status == RunStatus::Failed)
                .then(|| public_failure_code(run.error_message.as_deref()))
                .flatten(),
            created_at: run.created_at,
        }
    }
}

fn public_failure_code(value: Option<&str>) -> Option<String> {
    let code = match value? {
        "spawn-failed"
        | "nonzero-exit"
        | "log-write-failed"
        | "environment-unavailable"
        | "termination-timeout"
        | "wsl-unavailable"
        | "workspace-task-source-changed"
        | "workspace-task-configuration-invalid"
        | "process-crashed"
        | "storage-failed" => value?,
        _ => return None,
    };
    Some(code.to_string())
}

/// Identity and app-owned log reference returned by a platform spawn. This
/// is persisted only after the same owner/attempt CAS that moves a run to
/// `running`; an empty identity is valid only for a failed pre-spawn path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunExecutionMetadata {
    pub log_dir: Option<String>,
    pub target_pid: Option<i64>,
    pub target_process_created_at: Option<i64>,
    pub target_pgid: Option<i64>,
    pub target_sid: Option<i64>,
    pub process_marker: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClaimResult {
    pub inserted: bool,
    pub run: Run,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NotificationOutboxItem {
    pub id: String,
    pub kind: String,
    pub job_id: Option<String>,
    pub run_id: Option<String>,
    pub error_code: String,
    pub idempotency_key: String,
    pub created_at: i64,
    pub delivered_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewNotification {
    pub job_id: Option<String>,
    pub run_id: Option<String>,
    pub error_code: String,
    pub idempotency_key: String,
    pub created_at: i64,
}

impl NewNotification {
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.error_code.trim().is_empty() {
            return Err(ModelError::EmptyField("error_code"));
        }
        if self.idempotency_key.trim().is_empty() {
            return Err(ModelError::EmptyField("idempotency_key"));
        }
        if self.error_code.contains('\0') || self.idempotency_key.contains('\0') {
            return Err(ModelError::NulByte("notification"));
        }
        Ok(())
    }
}

/// Storage-facing boundary for the platform environment protector. The
/// Windows implementation uses DPAPI CurrentUser; non-Windows builds return a
/// stable unavailable error rather than silently persisting plaintext.
pub trait EnvironmentProtector: Send + Sync {
    fn is_available(&self) -> bool {
        true
    }

    fn encrypt(
        &self,
        environment: &BTreeMap<String, String>,
    ) -> Result<Vec<u8>, EnvironmentProtectionError>;

    fn decrypt(
        &self,
        ciphertext: &[u8],
    ) -> Result<BTreeMap<String, String>, EnvironmentProtectionError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvironmentProtectionError {
    Unavailable,
    InvalidInput,
    InvalidCiphertext,
    CryptoFailure,
}

impl fmt::Display for EnvironmentProtectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Unavailable => "environment protection is unavailable on this platform",
            Self::InvalidInput => "environment values are invalid",
            Self::InvalidCiphertext => "stored environment data is invalid",
            Self::CryptoFailure => "environment protection failed",
        })
    }
}

impl std::error::Error for EnvironmentProtectionError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelError {
    EmptyField(&'static str),
    NulByte(&'static str),
    InvalidCron(String),
    InvalidEnvironment(String),
    UnexpectedTargetDistro,
    MissingTargetDistro,
    MissingServiceAddress,
    MissingServicePort,
    InvalidServicePort,
    NonLocalServiceAddress,
    InvalidHistoryField(&'static str),
    InvalidHistoryRange,
    InvalidHistoryDuration,
}

impl fmt::Display for ModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyField(field) => write!(formatter, "{field} must not be empty"),
            Self::NulByte(field) => write!(formatter, "{field} contains a NUL byte"),
            Self::InvalidCron(error) => {
                write!(formatter, "cron_expr: invalid cron expression ({error})")
            }
            Self::InvalidEnvironment(error) => write!(formatter, "environment: {error}"),
            Self::UnexpectedTargetDistro => {
                formatter.write_str("target_distro is only valid for WSL jobs")
            }
            Self::MissingTargetDistro => {
                formatter.write_str("target_distro is required for WSL jobs")
            }
            Self::MissingServiceAddress => {
                formatter.write_str("health_tcp_address is required when health_tcp_port is set")
            }
            Self::MissingServicePort => {
                formatter.write_str("health_tcp_port is required when health_tcp_address is set")
            }
            Self::InvalidServicePort => {
                formatter.write_str("health_tcp_port must be between 1 and 65535")
            }
            Self::NonLocalServiceAddress => {
                formatter.write_str("health_tcp_address must be localhost or a loopback IP")
            }
            Self::InvalidHistoryField(field) => {
                write!(formatter, "history filter field {field} is invalid")
            }
            Self::InvalidHistoryRange => {
                formatter.write_str("history filter end_at must be after start_at")
            }
            Self::InvalidHistoryDuration => {
                formatter.write_str("history duration filter is invalid")
            }
        }
    }
}

impl std::error::Error for ModelError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(target_kind: TargetKind, target_distro: Option<&str>) -> JobInput {
        JobInput {
            name: "backup".to_string(),
            command: "echo backup".to_string(),
            cwd: None,
            target_kind,
            target_distro: target_distro.map(str::to_string),
            environment: EnvironmentUpdate::Keep,
            cron_expr: "0 * * * *".to_string(),
            enabled: false,
            overlap_policy: OverlapPolicy::default(),
            catch_up: false,
        }
    }

    #[test]
    fn target_distro_matches_target_kind() {
        assert!(input(TargetKind::Windows, Some("Ubuntu"))
            .validate()
            .is_err());
        assert!(input(TargetKind::Wsl, None).validate().is_err());
        assert!(input(TargetKind::Wsl, Some("Ubuntu")).validate().is_ok());
        assert!(input(TargetKind::Windows, None).validate().is_ok());
    }

    #[test]
    fn command_boundary_rejects_nul_bytes() {
        let mut value = input(TargetKind::Windows, None);
        value.command.push('\0');
        assert_eq!(value.validate(), Err(ModelError::NulByte("command")));
    }

    #[test]
    fn history_filter_rejects_unbounded_or_inverted_ranges() {
        assert_eq!(
            RunHistoryFilter {
                job_id: Some(String::new()),
                ..RunHistoryFilter::default()
            }
            .validate(),
            Err(ModelError::InvalidHistoryField("job_id"))
        );
        assert_eq!(
            RunHistoryFilter {
                start_at: Some(2),
                end_at: Some(2),
                ..RunHistoryFilter::default()
            }
            .validate(),
            Err(ModelError::InvalidHistoryRange)
        );
        assert_eq!(
            RunHistoryFilter {
                min_duration_ms: Some(10),
                max_duration_ms: Some(9),
                ..RunHistoryFilter::default()
            }
            .validate(),
            Err(ModelError::InvalidHistoryDuration)
        );
        assert_eq!(
            RunHistoryFilter {
                min_duration_ms: Some(RunHistoryFilter::MAX_DURATION_MS + 1),
                ..RunHistoryFilter::default()
            }
            .validate(),
            Err(ModelError::InvalidHistoryDuration)
        );
        for filter in [
            RunHistoryFilter {
                start_at: Some(-1),
                ..RunHistoryFilter::default()
            },
            RunHistoryFilter {
                limit: Some(0),
                ..RunHistoryFilter::default()
            },
            RunHistoryFilter {
                limit: Some(RunHistoryFilter::MAX_LIMIT + 1),
                ..RunHistoryFilter::default()
            },
        ] {
            assert_eq!(filter.validate(), Err(ModelError::InvalidHistoryRange));
        }
    }

    #[test]
    fn run_view_redacts_identity_paths_and_raw_errors() {
        let run = Run {
            id: "run".to_string(),
            job_id: "job".to_string(),
            scheduled_at: None,
            occurrence_wall_key: None,
            queue_sequence: 1,
            blocked_by_run_id: Some("blocked".to_string()),
            started_at: Some(1),
            ended_at: None,
            exit_code: None,
            status: RunStatus::Failed,
            owner_instance_id: Some("owner-secret".to_string()),
            attempt_token: Some("token-secret".to_string()),
            error_message: Some("spawn-failed".to_string()),
            target_pid: Some(42),
            target_process_created_at: Some(7),
            target_pgid: Some(42),
            target_sid: Some(42),
            process_marker: Some("marker-secret".to_string()),
            log_dir: Some("logs/runs/run".to_string()),
            logs_deleted_at: None,
            created_at: 1,
        };
        let view = RunView::from_run(&run);
        let json = serde_json::to_string(&view).unwrap();
        assert!(json.contains("logsAvailable"));
        assert!(json.contains("spawn-failed"));
        for secret in ["owner-secret", "token-secret", "marker-secret", "logs/runs"] {
            assert!(!json.contains(secret));
        }

        let cancelled = Run {
            status: RunStatus::Cancelled,
            error_message: Some("manual-stop".to_string()),
            ..run
        };
        assert_eq!(RunView::from_run(&cancelled).failure_code, None);
    }

    #[test]
    fn save_validation_rejects_invalid_cron_expressions() {
        let mut value = input(TargetKind::Windows, None);
        value.cron_expr = "61 * * * *".to_string();
        assert!(matches!(value.validate(), Err(ModelError::InvalidCron(_))));
    }

    #[test]
    fn service_instance_state_uses_durable_snake_case_values() {
        let state = ServiceInstanceState::RetryWaiting;
        assert_eq!(state.to_string(), "retry_waiting");
        assert_eq!(serde_json::to_string(&state).unwrap(), "\"retry_waiting\"");
        assert_eq!(
            serde_json::from_str::<ServiceInstanceState>("\"retry_waiting\"").unwrap(),
            state
        );
    }

    #[test]
    fn environment_update_wire_shape_is_explicit_and_debug_is_redacted() {
        let update = serde_json::from_value::<EnvironmentUpdate>(serde_json::json!({
            "action": "replace",
            "values": { "TOKEN": "secret" }
        }))
        .unwrap();
        assert!(!format!("{update:?}").contains("secret"));
        assert_eq!(
            serde_json::from_value::<EnvironmentUpdate>(serde_json::json!({
                "action": "clear"
            }))
            .unwrap(),
            EnvironmentUpdate::Clear
        );
    }

    fn service_input() -> ServiceInput {
        ServiceInput {
            name: "web".to_string(),
            command: "node server.js".to_string(),
            cwd: None,
            target_kind: TargetKind::Windows,
            target_distro: None,
            environment: EnvironmentUpdate::Keep,
            restart_policy: RestartPolicy::OnFailure,
            auto_start: true,
            health_tcp_address: Some("127.0.0.1".to_string()),
            health_tcp_port: Some(8080),
        }
    }

    #[test]
    fn service_input_uses_explicit_restart_policies() {
        let input = service_input();
        assert!(input.validate().is_ok());
        assert_eq!(
            serde_json::to_string(&RestartPolicy::OnFailure).unwrap(),
            "\"on-failure\""
        );
        let defaulted: ServiceInput = serde_json::from_str(
            r#"{
                "name":"web",
                "command":"node server.js",
                "cwd":null,
                "targetKind":"windows",
                "targetDistro":null,
                "healthTcpAddress":null,
                "healthTcpPort":null
            }"#,
        )
        .unwrap();
        assert_eq!(defaulted.restart_policy, RestartPolicy::Never);
        assert!(!defaulted.auto_start);
    }

    #[test]
    fn service_input_rejects_non_local_or_partial_health_endpoints() {
        let mut input = service_input();
        input.health_tcp_address = Some("8.8.8.8".to_string());
        assert_eq!(input.validate(), Err(ModelError::NonLocalServiceAddress));

        let mut input = service_input();
        input.health_tcp_port = None;
        assert_eq!(input.validate(), Err(ModelError::MissingServicePort));

        let mut input = service_input();
        input.health_tcp_address = None;
        assert_eq!(input.validate(), Err(ModelError::MissingServiceAddress));
    }

    #[test]
    fn service_input_rejects_invalid_port_and_shell_like_address() {
        let mut input = service_input();
        input.health_tcp_port = Some(0);
        assert_eq!(input.validate(), Err(ModelError::InvalidServicePort));

        let mut input = service_input();
        input.health_tcp_address = Some("127.0.0.1;curl evil".to_string());
        assert_eq!(input.validate(), Err(ModelError::NonLocalServiceAddress));
    }
}
