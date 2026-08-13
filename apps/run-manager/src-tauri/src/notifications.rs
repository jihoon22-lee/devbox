use crate::core::models::{NewNotification, NotificationOutboxItem};
use crate::storage::DatabaseState;
use std::fmt;
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

pub const DEDUPLICATION_WINDOW_MILLIS: i64 = 5 * 60 * 1_000;
pub const DELIVERED_RETENTION_MILLIS: i64 = 30 * 24 * 60 * 60 * 1_000;
const PENDING_DRAIN_LIMIT: u32 = 100;
const MAX_JOB_NAME_CHARS: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureCode {
    SpawnFailed,
    NonzeroExit,
    LogWriteFailed,
    EnvironmentUnavailable,
    TerminationTimeout,
    WslUnavailable,
    ProcessCrashed,
}

impl FailureCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SpawnFailed => "spawn_failed",
            Self::NonzeroExit => "nonzero_exit",
            Self::LogWriteFailed => "log_write_failed",
            Self::EnvironmentUnavailable => "environment_unavailable",
            Self::TerminationTimeout => "termination_timeout",
            Self::WslUnavailable => "wsl_unavailable",
            Self::ProcessCrashed => "process_crashed",
        }
    }

    const fn user_label(self) -> &'static str {
        match self {
            Self::SpawnFailed => "프로세스를 시작하지 못했습니다",
            Self::NonzeroExit => "명령이 오류 코드로 종료되었습니다",
            Self::LogWriteFailed => "실행 로그를 저장하지 못했습니다",
            Self::EnvironmentUnavailable => "보호된 환경변수를 불러오지 못했습니다",
            Self::TerminationTimeout => "프로세스 종료 제한 시간을 초과했습니다",
            Self::WslUnavailable => "WSL 실행 환경을 사용할 수 없습니다",
            Self::ProcessCrashed => "프로세스가 예기치 않게 종료되었습니다",
        }
    }
}

impl TryFrom<&str> for FailureCode {
    type Error = UnknownFailureCode;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "spawn_failed" => Ok(Self::SpawnFailed),
            "nonzero_exit" => Ok(Self::NonzeroExit),
            "log_write_failed" => Ok(Self::LogWriteFailed),
            "environment_unavailable" => Ok(Self::EnvironmentUnavailable),
            "termination_timeout" => Ok(Self::TerminationTimeout),
            "wsl_unavailable" => Ok(Self::WslUnavailable),
            "process_crashed" => Ok(Self::ProcessCrashed),
            _ => Err(UnknownFailureCode),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnknownFailureCode;

impl fmt::Display for UnknownFailureCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown fixed run failure code")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureEvent {
    pub job_id: String,
    pub run_id: String,
    pub code: FailureCode,
    pub occurred_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryOutcome {
    Delivered,
    Queued,
    Suppressed,
}

trait NotificationSink {
    fn send(&self, title: &str, body: &str) -> Result<(), String>;
}

struct TauriNotificationSink<'a>(&'a AppHandle);

impl NotificationSink for TauriNotificationSink<'_> {
    fn send(&self, title: &str, body: &str) -> Result<(), String> {
        self.0
            .notification()
            .builder()
            .title(title)
            .body(body)
            .show()
            .map_err(|error| error.to_string())
    }
}

/// Persist a sanitized failure event before attempting the native toast. A
/// notification API failure never rolls back the run or scheduler state.
pub fn notify_run_failure(
    app: &AppHandle,
    database: &DatabaseState,
    event: FailureEvent,
) -> Result<DeliveryOutcome, String> {
    notify_run_failure_with(database, event, &TauriNotificationSink(app))
}

fn notify_run_failure_with(
    database: &DatabaseState,
    event: FailureEvent,
    sink: &impl NotificationSink,
) -> Result<DeliveryOutcome, String> {
    let notification = NewNotification {
        job_id: Some(event.job_id.clone()),
        run_id: Some(event.run_id.clone()),
        error_code: event.code.as_str().to_string(),
        idempotency_key: format!(
            "run-failed:{}:{}:{}",
            event.job_id,
            event.run_id,
            event.code.as_str()
        ),
        created_at: event.occurred_at,
    };
    let Some(item) = database
        .enqueue_notification_if_not_recent(
            notification,
            event
                .occurred_at
                .saturating_sub(DEDUPLICATION_WINDOW_MILLIS),
        )
        .map_err(|error| error.to_string())?
    else {
        return Ok(DeliveryOutcome::Suppressed);
    };
    deliver_item(database, &item, sink, event.occurred_at)
}

/// Retry pending durable events and prune only old delivered metadata. Pending
/// events are never removed by retention.
pub fn drain_pending(app: &AppHandle, database: &DatabaseState) -> Result<usize, String> {
    let now = crate::storage::current_epoch_millis();
    database
        .delete_delivered_notifications_before(now.saturating_sub(DELIVERED_RETENTION_MILLIS))
        .map_err(|error| error.to_string())?;
    drain_pending_with(database, &TauriNotificationSink(app), now)
}

fn drain_pending_with(
    database: &DatabaseState,
    sink: &impl NotificationSink,
    delivered_at: i64,
) -> Result<usize, String> {
    let items = database
        .list_pending_notifications(PENDING_DRAIN_LIMIT)
        .map_err(|error| error.to_string())?;
    let mut delivered = 0;
    for item in items {
        if deliver_item(database, &item, sink, delivered_at)? == DeliveryOutcome::Delivered {
            delivered += 1;
        }
    }
    Ok(delivered)
}

fn deliver_item(
    database: &DatabaseState,
    item: &NotificationOutboxItem,
    sink: &impl NotificationSink,
    delivered_at: i64,
) -> Result<DeliveryOutcome, String> {
    let job_name = item
        .job_id
        .as_deref()
        .and_then(|job_id| database.get_job(job_id).ok().flatten())
        .map(|job| sanitize_job_name(&job.name))
        .unwrap_or_else(|| "알 수 없는 작업".to_string());
    let detail = FailureCode::try_from(item.error_code.as_str())
        .map(FailureCode::user_label)
        .unwrap_or("작업 실행 중 오류가 발생했습니다");
    let body = format!("‘{job_name}’ — {detail}");
    if sink.send("Run Manager 작업 실패", &body).is_err() {
        return Ok(DeliveryOutcome::Queued);
    }
    database
        .mark_notification_delivered(&item.id, delivered_at)
        .map_err(|error| error.to_string())?;
    Ok(DeliveryOutcome::Delivered)
}

fn sanitize_job_name(value: &str) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut sanitized = compact.chars().take(MAX_JOB_NAME_CHARS).collect::<String>();
    if compact.chars().count() > MAX_JOB_NAME_CHARS {
        sanitized.push('…');
    }
    if sanitized.is_empty() {
        "이름 없는 작업".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::{EnvironmentUpdate, JobInput, OverlapPolicy, TargetKind};
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingSink {
        fail: bool,
        bodies: Mutex<Vec<String>>,
    }

    impl NotificationSink for RecordingSink {
        fn send(&self, _title: &str, body: &str) -> Result<(), String> {
            self.bodies.lock().unwrap().push(body.to_string());
            if self.fail {
                Err("notification unavailable".to_string())
            } else {
                Ok(())
            }
        }
    }

    fn input(name: &str) -> JobInput {
        JobInput {
            name: name.to_string(),
            command: "echo ok".to_string(),
            cwd: None,
            environment: EnvironmentUpdate::Keep,
            cron_expr: "0 * * * *".to_string(),
            enabled: true,
            overlap_policy: OverlapPolicy::Skip,
            catch_up: true,
            target_kind: TargetKind::Windows,
            target_distro: None,
        }
    }

    #[test]
    fn five_minute_window_suppresses_repeated_job_toasts() {
        let database = DatabaseState::open_in_memory().unwrap();
        let job = database
            .create_job_at(input(" nightly\nbackup "), 1)
            .unwrap();
        let first_run = database.create_manual_run_at(&job.id, 2).unwrap();
        let second_run = database.create_manual_run_at(&job.id, 3).unwrap();
        let sink = RecordingSink::default();
        assert_eq!(
            notify_run_failure_with(
                &database,
                FailureEvent {
                    job_id: job.id.clone(),
                    run_id: first_run.id,
                    code: FailureCode::SpawnFailed,
                    occurred_at: 1_000,
                },
                &sink,
            )
            .unwrap(),
            DeliveryOutcome::Delivered
        );
        assert_eq!(
            notify_run_failure_with(
                &database,
                FailureEvent {
                    job_id: job.id,
                    run_id: second_run.id,
                    code: FailureCode::NonzeroExit,
                    occurred_at: 1_000 + DEDUPLICATION_WINDOW_MILLIS - 1,
                },
                &sink,
            )
            .unwrap(),
            DeliveryOutcome::Suppressed
        );
        assert_eq!(
            sink.bodies.lock().unwrap().as_slice(),
            ["‘nightly backup’ — 프로세스를 시작하지 못했습니다"]
        );
    }

    #[test]
    fn failed_native_delivery_remains_pending_and_drain_retries() {
        let database = DatabaseState::open_in_memory().unwrap();
        let job = database.create_job_at(input("build"), 1).unwrap();
        let run = database.create_manual_run_at(&job.id, 2).unwrap();
        assert_eq!(
            notify_run_failure_with(
                &database,
                FailureEvent {
                    job_id: job.id,
                    run_id: run.id,
                    code: FailureCode::LogWriteFailed,
                    occurred_at: 3,
                },
                &RecordingSink {
                    fail: true,
                    ..Default::default()
                },
            )
            .unwrap(),
            DeliveryOutcome::Queued
        );
        assert_eq!(database.list_pending_notifications(10).unwrap().len(), 1);
        assert_eq!(
            drain_pending_with(&database, &RecordingSink::default(), 4).unwrap(),
            1
        );
        assert!(database.list_pending_notifications(10).unwrap().is_empty());
    }

    #[test]
    fn job_names_are_compacted_and_unicode_truncated() {
        assert_eq!(sanitize_job_name("  a\n\tb  "), "a b");
        let long = "가".repeat(MAX_JOB_NAME_CHARS + 1);
        assert_eq!(
            sanitize_job_name(&long).chars().count(),
            MAX_JOB_NAME_CHARS + 1
        );
        assert!(sanitize_job_name(&long).ends_with('…'));
    }
}
