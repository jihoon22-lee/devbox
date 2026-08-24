use crate::commands::tracking::AppState;
use crate::core::aggregate::collect_git;
use crate::core::attribution::{attribute_sessions, Attribution, ProjectMatch};
use crate::core::db;
use crate::core::models::{DayPoint, DaySummary, RangeSummary};
use serde::Serialize;
use std::sync::Arc;

/// integration snapshot source 상태 (UI 표시용).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceStatus {
    pub producer: String,
    pub available: bool,
    pub schema_version: Option<u32>,
    pub producer_version: Option<String>,
    pub generated_at: Option<String>,
    /// 마지막 갱신 이후 경과 (ms). snapshot이 없으면 None.
    pub freshness_ms: Option<i64>,
    pub error: Option<String>,
}

/// 기간 내 활동을 프로젝트로 귀속한다. 미귀속은 별도로 표시한다.
///
/// 프로젝트 identity는 설정된 git 프로젝트 경로의 basename을 쓴다. Workbench의
/// ProjectProfile(§10.2)이 생기면 canonical key로 대체한다.
#[tauri::command]
pub fn project_attribution(
    state: tauri::State<'_, Arc<AppState>>,
    day_start: i64,
    day_end: i64,
) -> Result<AttributionResult, String> {
    let conn = state.db.lock().unwrap();
    let sessions = db::get_timeline(&conn, day_start, day_end).map_err(|e| e.to_string())?;
    let projects = db::get_setting(&conn, "projects", "")
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    drop(conn);

    let profiles: Vec<ProjectMatch> = projects
        .iter()
        .filter_map(|p| {
            let basename = p
                .trim_end_matches(['/', '\\'])
                .rsplit(['/', '\\'])
                .next()
                .filter(|b| !b.is_empty())?;
            Some(ProjectMatch {
                project_id: p.clone(),
                basenames: vec![basename.to_string()],
            })
        })
        .collect();

    let rows: Vec<(String, String, i64)> = sessions
        .into_iter()
        .map(|s| (s.app, s.title, s.duration_ms))
        .collect();
    let (attributed, unattributed) = attribute_sessions(&rows, &profiles);

    Ok(AttributionResult {
        attributed,
        unattributed,
        profile_count: profiles.len(),
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttributionResult {
    pub attributed: Vec<Attribution>,
    pub unattributed: Attribution,
    pub profile_count: usize,
}

/// 공용 integration root에서 자동 발견한 모든 source 상태를 반환한다.
#[tauri::command]
pub fn integration_sources() -> Vec<SourceStatus> {
    source_statuses(devbox_integration::discover_report())
}

fn source_statuses(report: devbox_integration::DiscoveryReport) -> Vec<SourceStatus> {
    let mut statuses = Vec::with_capacity(
        report.snapshots.len() + report.issues.len() + usize::from(report.root_error.is_some()),
    );
    statuses.extend(report.snapshots.into_iter().map(|snapshot| SourceStatus {
        producer: snapshot.producer,
        available: true,
        schema_version: Some(snapshot.version),
        producer_version: Some(snapshot.producer_version),
        generated_at: Some(snapshot.generated_at),
        freshness_ms: Some(snapshot.freshness_ms.min(i64::MAX as u64) as i64),
        error: None,
    }));
    statuses.extend(report.issues.into_iter().map(|issue| SourceStatus {
        producer: issue.producer,
        available: false,
        schema_version: issue.version,
        producer_version: None,
        generated_at: None,
        freshness_ms: None,
        error: Some(issue.error),
    }));
    if let Some(error) = report.root_error {
        statuses.push(SourceStatus {
            producer: "integration-root".into(),
            available: false,
            schema_version: None,
            producer_version: None,
            generated_at: None,
            freshness_ms: None,
            error: Some(error),
        });
    }
    statuses.sort_by(|a, b| {
        (&a.producer, a.schema_version, !a.available).cmp(&(
            &b.producer,
            b.schema_version,
            !b.available,
        ))
    });
    statuses
}

/// 프로젝트 경로 목록 설정 (줄바꿈 구분).
#[tauri::command]
pub fn set_projects(
    state: tauri::State<'_, Arc<AppState>>,
    paths: Vec<String>,
) -> Result<(), String> {
    db::set_setting(&state.db.lock().unwrap(), "projects", &paths.join("\n"));
    Ok(())
}

#[tauri::command]
pub fn get_projects(state: tauri::State<'_, Arc<AppState>>) -> Vec<String> {
    db::get_setting(&state.db.lock().unwrap(), "projects", "")
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn saved_projects(state: &tauri::State<'_, Arc<AppState>>) -> Vec<String> {
    db::get_setting(&state.db.lock().unwrap(), "projects", "")
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// 하루 요약. 내부 활동 DB(동기) + git(비동기)을 합친다.
#[tauri::command]
pub async fn get_day(
    state: tauri::State<'_, Arc<AppState>>,
    date: String,
    day_start: i64,
    day_end: i64,
) -> Result<DaySummary, String> {
    // 잠금은 await 전에 해제한다 (MutexGuard는 Send가 아님)
    let (pc_usage_ms, app_totals) = {
        let conn = state.db.lock().unwrap();
        (
            db::pc_usage(&conn, day_start, day_end),
            db::get_app_stats(&conn, day_start, day_end).unwrap_or_default(),
        )
    };
    let projects = saved_projects(&state);
    let git = collect_git(&projects, day_start, day_end).await;

    Ok(DaySummary {
        date,
        pc_usage_ms,
        app_totals,
        git,
    })
}

/// 기간(주/월) 요약. 일별 사용량 + 합계 + git을 한 번에 조회한다.
#[tauri::command]
pub async fn get_range(
    state: tauri::State<'_, Arc<AppState>>,
    label: String,
    day_start: i64,
    day_end: i64,
) -> Result<RangeSummary, String> {
    let (pc_usage_ms, app_totals, daily) = {
        let conn = state.db.lock().unwrap();
        let app_totals = db::get_app_stats(&conn, day_start, day_end).unwrap_or_default();
        let daily = db::get_daily_usage(&conn, day_start, day_end)
            .into_iter()
            .map(|(day_ms, pc_usage_ms)| DayPoint {
                day_ms,
                pc_usage_ms,
            })
            .collect();
        (db::pc_usage(&conn, day_start, day_end), app_totals, daily)
    };
    let projects = saved_projects(&state);
    let git = collect_git(&projects, day_start, day_end).await;

    Ok(RangeSummary {
        label,
        pc_usage_ms,
        app_totals,
        git,
        daily,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_statuses_preserve_valid_and_isolated_error_rows() {
        let report = devbox_integration::DiscoveryReport {
            snapshots: vec![devbox_integration::SnapshotRef {
                producer: "knowledge-base".into(),
                version: 1,
                producer_version: "0.5.0".into(),
                generated_at: "2026-08-25T12:00:00Z".into(),
                path: std::path::PathBuf::from("unused-by-ui.json"),
                freshness_ms: 1_234,
                views: vec![],
            }],
            issues: vec![devbox_integration::SnapshotIssue {
                producer: "run-manager".into(),
                version: Some(2),
                error: "snapshot JSON 형식이 올바르지 않습니다".into(),
            }],
            root_error: None,
        };

        let statuses = source_statuses(report);
        assert_eq!(statuses.len(), 2);
        assert_eq!(statuses[0].producer, "knowledge-base");
        assert!(statuses[0].available);
        assert_eq!(statuses[0].freshness_ms, Some(1_234));
        assert_eq!(statuses[1].producer, "run-manager");
        assert!(!statuses[1].available);
        assert_eq!(statuses[1].schema_version, Some(2));
    }

    #[test]
    fn source_statuses_show_root_failure_without_inventing_a_producer() {
        let statuses = source_statuses(devbox_integration::DiscoveryReport {
            root_error: Some("integration root를 읽을 수 없습니다".into()),
            ..Default::default()
        });
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].producer, "integration-root");
        assert!(!statuses[0].available);
        assert_eq!(statuses[0].schema_version, None);
    }

    #[test]
    fn source_statuses_are_empty_when_nothing_has_been_published() {
        assert!(source_statuses(Default::default()).is_empty());
    }
}
