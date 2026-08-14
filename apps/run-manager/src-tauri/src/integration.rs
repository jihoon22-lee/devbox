//! devbox 공용 루트 integration snapshot producer (파일럿).
//!
//! 경로: `%LOCALAPPDATA%\devbox\integration\<producer-id>\v1\summary.json`
//! (consumer는 카탈로그 앱 ID만 알면 된다 — §10.1)
//!
//! - 임시 파일 + rename으로 원자 기록
//! - secret·환경변수 값은 포함하지 않는다
//! - 기록 실패가 앱 동작을 막지 않는다 (오류만 로그)

use crate::storage::DatabaseState;
use serde::Serialize;

const PRODUCER_ID: &str = "run-manager";
const SNAPSHOT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct IntegrationEnvelope {
    schema_version: u32,
    producer: &'static str,
    producer_version: &'static str,
    generated_at: String,
    data: SnapshotData,
}

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
    let envelope = IntegrationEnvelope {
        schema_version: 1,
        producer: PRODUCER_ID,
        producer_version: env!("CARGO_PKG_VERSION"),
        generated_at: utc_now(),
        data,
    };
    let json = serde_json::to_string_pretty(&envelope).map_err(|e| e.to_string())?;

    let dir = snapshot_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let target = dir.join("summary.json");
    let tmp = dir.join("summary.json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &target).map_err(|e| e.to_string())?;
    Ok(())
}

/// `%LOCALAPPDATA%\devbox\integration\run-manager\v1`
fn snapshot_dir() -> std::path::PathBuf {
    let base = if cfg!(target_os = "windows") {
        std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into())
    } else {
        std::env::temp_dir().to_string_lossy().into_owned()
    };
    std::path::PathBuf::from(base)
        .join("devbox")
        .join("integration")
        .join(PRODUCER_ID)
        .join("v1")
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

fn utc_now() -> String {
    // ISO-8601 UTC (마이크로초 생략)
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let day_secs = secs.rem_euclid(86_400);
    let (h, m, s) = (day_secs / 3600, (day_secs % 3600) / 60, day_secs % 60);
    let (y, mo, d) = civil_from_days(days as i64);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_from_days_known_dates() {
        // 1970-01-01 → days 0
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        let secs: i64 = 1_783_000_000;
        let days = secs.div_euclid(86_400);
        let (y, m, _d) = civil_from_days(days);
        assert_eq!((y, m), (2026, 7), "2026-08-14 근처 ({y}-{m})");
    }

    #[test]
    fn start_of_today_is_day_aligned() {
        let today = start_of_today_utc();
        assert_eq!(today % 86_400_000, 0);
    }
}
