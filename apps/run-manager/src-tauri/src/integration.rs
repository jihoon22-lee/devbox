//! devbox 공용 루트 integration snapshot producer (파일럿).
//!
//! 경로: `%LOCALAPPDATA%\devbox\integration\<producer-id>\v1\summary.json`
//! envelope 직렬화·원자 기록·경로 계산은 `crates/integration`(@devbox) 프리미티브를 쓴다.
//! 여기서는 producer별 데이터 내용만 담당한다.
//!
//! - secret·환경변수 값은 포함하지 않는다
//! - 기록 실패가 앱 동작을 막지 않는다 (오류만 로그)

use crate::storage::DatabaseState;
use serde::Serialize;

const PRODUCER_ID: &str = "run-manager";
const SNAPSHOT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotData {
    active_services: Vec<ServiceUptime>,
    runs: RunCounts,
    last_run_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceUptime {
    id: String,
    uptime_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunCounts {
    success: i64,
    failed: i64,
}

/// 주기적으로 snapshot을 쓴다 (상태 변화 추적보다 주기적 기록이 단순·충분 — [설계]).
pub fn spawn_snapshot_writer(database: std::sync::Arc<DatabaseState>) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(SNAPSHOT_INTERVAL).await;
            if let Err(error) = write_snapshot(&database) {
                eprintln!("run-manager integration snapshot 실패: {error}");
            }
        }
    });
}

/// snapshot을 즉시 쓴다 (테스트·종료 시 1회 호출 가능).
pub fn write_snapshot(database: &DatabaseState) -> Result<(), String> {
    let data = build_data(database)?;
    let envelope = devbox_integration::Envelope::new(
        PRODUCER_ID,
        env!("CARGO_PKG_VERSION"),
        serde_json::to_value(&data).map_err(|e| e.to_string())?,
    );
    let dir = devbox_integration::snapshot_dir(PRODUCER_ID, 1);
    devbox_integration::write_atomic(&envelope, &dir)
}

fn build_data(database: &DatabaseState) -> Result<SnapshotData, String> {
    let now = current_epoch_ms();
    let (success, failed) = database
        .run_counts_since(start_of_today_utc())
        .map_err(|e| e.to_string())?;
    let last_run_at_ms = database.last_run_at().map_err(|e| e.to_string())?;

    let mut active_services = Vec::new();
    if let Ok(running) = database.list_running_services() {
        for (job, instance) in running {
            let uptime_ms = instance
                .active_run_id
                .as_ref()
                .and_then(|run_id| database.get_run(run_id).ok().flatten())
                .and_then(|run| run.started_at.or(run.scheduled_at))
                .map(|started| (now - started).max(0))
                .unwrap_or(0);
            active_services.push(ServiceUptime {
                id: job.id.clone(),
                uptime_ms,
            });
        }
    }
    active_services.sort_by(|a, b| a.id.cmp(&b.id));

    Ok(SnapshotData {
        active_services,
        runs: RunCounts { success, failed },
        last_run_at_ms,
    })
}

fn start_of_today_utc() -> i64 {
    // epoch ms → UTC 날짜 시작 (365일 계산의 복잡함 없이 일자 경계만)
    let now = current_epoch_ms();
    now.div_euclid(86_400_000) * 86_400_000
}

fn current_epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_of_today_is_day_aligned() {
        let today = start_of_today_utc();
        assert_eq!(today % 86_400_000, 0);
    }
}
