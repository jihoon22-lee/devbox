use crate::commands::tracking::AppState;
use crate::core::aggregate::collect_git;
use crate::core::attribution::{attribute_sessions, Attribution, ProjectMatch};
use crate::core::db;
use crate::core::models::{DayPoint, DaySummary, RangeSummary};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

const KNOWLEDGE_PRODUCER: &str = "knowledge-base";
const KNOWLEDGE_SNAPSHOT_VERSION: u32 = 1;
const KNOWLEDGE_ACTIVITY_KIND: &str = "activity";
const KNOWLEDGE_ACTIVITY_SCHEMA_VERSION: u32 = 1;
const MAX_KNOWLEDGE_NOTE_IDS: usize = 512;

/// Life Log UI에 필요한 Knowledge 활동 집계만 노출한다.
/// payload의 불투명 note ID는 검증과 중복 방지에만 쓰고 frontend로 전달하지 않는다.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeActivity {
    pub notes_modified_today: u64,
    pub last_modified_at_ms: Option<i64>,
    pub identified_notes: usize,
    pub identifiers_complete: bool,
    pub legacy_snapshot: bool,
}

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
    pub freshness_state: String,
    pub scope: String,
    pub error_code: Option<String>,
    pub explanation: String,
    pub error: Option<String>,
    pub knowledge_activity: Option<KnowledgeActivity>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectProbe {
    pub path: String,
    pub target: String,
    pub repository: bool,
    pub error_code: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct KnowledgeActivityPayload {
    notes_modified_today: u64,
    last_modified_at_ms: Option<i64>,
    note_ids: Vec<String>,
    identifiers_truncated: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyKnowledgeActivityPayload {
    notes_modified_today: u64,
    last_modified_at_ms: Option<i64>,
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
    let operation = state.digest_operations.begin()?;
    let cancellation = operation.cancellation();
    let conn = state
        .db
        .lock()
        .map_err(|_| "Life Log DB를 잠글 수 없습니다".to_string())?;
    let sessions = db::get_timeline_limited_with_cancel(
        &conn,
        day_start,
        day_end,
        crate::core::export::MAX_EXPORT_SESSIONS + 1,
        Arc::clone(&cancellation),
    )
    .map_err(|_| {
        if operation.is_cancelled() {
            "digest_cancelled".to_string()
        } else {
            "Life Log 활동 데이터를 읽을 수 없습니다".to_string()
        }
    })?;
    if sessions.len() > crate::core::export::MAX_EXPORT_SESSIONS {
        return Err("Life Log 활동 데이터가 제한을 초과했습니다".into());
    }
    let raw_projects = db::get_setting_bounded(
        &conn,
        "projects",
        "",
        crate::core::export::MAX_PROJECT_SETTING_BYTES,
    )
    .unwrap_or_default();
    let projects = crate::core::export::parse_project_setting(&raw_projects).unwrap_or_default();
    drop(conn);
    if operation.is_cancelled() {
        return Err("digest_cancelled".into());
    }

    let profiles: Vec<ProjectMatch> = projects
        .iter()
        .filter_map(|p| {
            let safe = devbox_filesystem::parse_safe_project_path(p)?;
            let basename = p
                .trim_end_matches(['/', '\\'])
                .rsplit(['/', '\\'])
                .next()
                .filter(|b| !b.is_empty())?;
            Some(ProjectMatch {
                project_id: safe.as_str().to_owned(),
                basenames: vec![basename.to_string()],
            })
        })
        .collect();

    let rows: Vec<(String, String, i64)> = sessions
        .into_iter()
        .map(|s| (s.app, s.title, s.duration_ms))
        .collect();
    if operation.is_cancelled() {
        return Err("digest_cancelled".into());
    }
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
    let root = devbox_integration::integration_root();
    let mut statuses = source_statuses_in(devbox_integration::discover_report_in(&root), &root);
    // The Life Log producer snapshot is a project-summary view for other
    // apps, not the source used by this screen's local digest. Replace that
    // self-snapshot row with an explicit live-local source so the UI cannot
    // show two ambiguous Life Log entries.
    statuses.retain(|status| status.producer != "life-log");
    statuses.push(SourceStatus {
        producer: "life-log".into(),
        available: true,
        schema_version: Some(1),
        producer_version: Some(env!("CARGO_PKG_VERSION").into()),
        generated_at: None,
        freshness_ms: Some(0),
        freshness_state: "fresh".into(),
        scope: "live-local".into(),
        error_code: None,
        explanation: crate::core::source_explanation::explanation_for_source(
            "life-log",
            "live-local",
        )
        .into(),
        error: None,
        knowledge_activity: None,
    });
    statuses.sort_by(|a, b| {
        (&a.producer, a.schema_version, !a.available).cmp(&(
            &b.producer,
            b.schema_version,
            !b.available,
        ))
    });
    statuses
}

fn source_statuses_in(
    report: devbox_integration::DiscoveryReport,
    root: &Path,
) -> Vec<SourceStatus> {
    let mut statuses = Vec::with_capacity(
        report.snapshots.len() + report.issues.len() + usize::from(report.root_error.is_some()),
    );
    statuses.extend(
        report
            .snapshots
            .into_iter()
            .map(|snapshot| source_status_from_snapshot(snapshot, root)),
    );
    statuses.extend(report.issues.into_iter().map(|issue| {
        let producer = issue.producer;
        let error_code = crate::core::source_explanation::error_code(Some(&issue.error));
        let explanation =
            crate::core::source_explanation::explanation_for_source(&producer, "unavailable")
                .to_owned();
        let error = issue.error;
        SourceStatus {
            producer,
            available: false,
            schema_version: issue.version,
            producer_version: None,
            generated_at: None,
            freshness_ms: None,
            freshness_state: "error".into(),
            scope: "unavailable".into(),
            error_code,
            explanation,
            error: Some(error),
            knowledge_activity: None,
        }
    }));
    if let Some(error) = report.root_error {
        statuses.push(SourceStatus {
            producer: "integration-root".into(),
            available: false,
            schema_version: None,
            producer_version: None,
            generated_at: None,
            freshness_ms: None,
            freshness_state: "error".into(),
            scope: "unavailable".into(),
            error_code: crate::core::source_explanation::error_code(Some(&error)),
            explanation: crate::core::source_explanation::explanation_for_source(
                "integration-root",
                "unavailable",
            )
            .into(),
            error: Some(error),
            knowledge_activity: None,
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

fn source_status_from_snapshot(
    snapshot: devbox_integration::SnapshotRef,
    root: &Path,
) -> SourceStatus {
    let freshness_ms = Some(snapshot.freshness_ms.min(i64::MAX as u64) as i64);
    let scope = crate::core::source_explanation::scope_for_source(&snapshot.producer).to_string();
    let base = SourceStatus {
        producer: snapshot.producer.clone(),
        available: true,
        schema_version: Some(snapshot.version),
        producer_version: Some(snapshot.producer_version.clone()),
        generated_at: Some(snapshot.generated_at.clone()),
        freshness_ms,
        freshness_state: crate::core::source_explanation::freshness_state(
            true,
            Some(snapshot.freshness_ms),
            false,
        )
        .into(),
        scope: scope.clone(),
        error_code: None,
        explanation: crate::core::source_explanation::explanation_for_source(
            &snapshot.producer,
            &scope,
        )
        .into(),
        error: None,
        knowledge_activity: None,
    };
    if snapshot.producer != KNOWLEDGE_PRODUCER {
        return base;
    }
    if snapshot.version != KNOWLEDGE_SNAPSHOT_VERSION {
        return unavailable_source(
            base,
            "Knowledge activity snapshot schema를 지원하지 않습니다",
        );
    }

    match read_knowledge_activity(root, &snapshot) {
        Ok((activity, activity_freshness_ms)) => SourceStatus {
            freshness_ms: Some(activity_freshness_ms.min(i64::MAX as u64) as i64),
            freshness_state: crate::core::source_explanation::freshness_state(
                true,
                Some(activity_freshness_ms),
                false,
            )
            .into(),
            knowledge_activity: Some(activity),
            ..base
        },
        Err(error) => unavailable_source(base, &error),
    }
}

fn unavailable_source(mut status: SourceStatus, error: &str) -> SourceStatus {
    status.available = false;
    status.error = Some(error.to_owned());
    status.freshness_state = "error".into();
    status.error_code = crate::core::source_explanation::error_code(Some(error));
    status.knowledge_activity = None;
    status
}

fn read_knowledge_activity(
    root: &Path,
    snapshot: &devbox_integration::SnapshotRef,
) -> Result<(KnowledgeActivity, u64), String> {
    let envelope =
        devbox_integration::read_snapshot_in(root, KNOWLEDGE_PRODUCER, KNOWLEDGE_SNAPSHOT_VERSION)?
            .ok_or_else(|| "Knowledge activity snapshot을 읽을 수 없습니다".to_string())?;
    let has_views = envelope
        .data
        .as_object()
        .is_some_and(|data| data.contains_key("views"));
    if !has_views {
        let legacy: LegacyKnowledgeActivityPayload = serde_json::from_value(envelope.data)
            .map_err(|_| "Knowledge activity payload가 올바르지 않습니다")?;
        validate_last_modified(legacy.notes_modified_today, legacy.last_modified_at_ms)?;
        return Ok((
            KnowledgeActivity {
                notes_modified_today: legacy.notes_modified_today,
                last_modified_at_ms: legacy.last_modified_at_ms,
                identified_notes: 0,
                identifiers_complete: legacy.notes_modified_today == 0,
                legacy_snapshot: true,
            },
            snapshot.freshness_ms,
        ));
    }

    let mut views = envelope.views()?;
    let view = views
        .remove(KNOWLEDGE_ACTIVITY_KIND)
        .ok_or_else(|| "Knowledge activity view가 없습니다".to_string())?;
    if view.schema_version != KNOWLEDGE_ACTIVITY_SCHEMA_VERSION {
        return Err("Knowledge activity view schema를 지원하지 않습니다".into());
    }
    if view.entries.len() != 1 {
        return Err("Knowledge activity entry 수가 올바르지 않습니다".into());
    }
    let entry = view
        .entries
        .into_iter()
        .next()
        .ok_or_else(|| "Knowledge activity entry 수가 올바르지 않습니다".to_string())?;
    let payload: KnowledgeActivityPayload = serde_json::from_value(entry)
        .map_err(|_| "Knowledge activity payload가 올바르지 않습니다")?;
    validate_knowledge_payload(&payload)?;

    let freshness_ms = snapshot
        .views
        .iter()
        .find(|reference| reference.kind == KNOWLEDGE_ACTIVITY_KIND)
        .map(|reference| reference.freshness_ms)
        .ok_or_else(|| "Knowledge activity view metadata가 없습니다".to_string())?;
    Ok((
        KnowledgeActivity {
            notes_modified_today: payload.notes_modified_today,
            last_modified_at_ms: payload.last_modified_at_ms,
            identified_notes: payload.note_ids.len(),
            identifiers_complete: !payload.identifiers_truncated,
            legacy_snapshot: false,
        },
        freshness_ms,
    ))
}

fn validate_knowledge_payload(payload: &KnowledgeActivityPayload) -> Result<(), String> {
    validate_last_modified(payload.notes_modified_today, payload.last_modified_at_ms)?;
    if payload.note_ids.len() > MAX_KNOWLEDGE_NOTE_IDS {
        return Err("Knowledge activity 식별자 수가 올바르지 않습니다".into());
    }
    let identified = payload.note_ids.len() as u64;
    let valid_count = if payload.identifiers_truncated {
        payload.notes_modified_today > identified
    } else {
        payload.notes_modified_today == identified
    };
    if !valid_count {
        return Err("Knowledge activity 식별자 수가 올바르지 않습니다".into());
    }

    let mut unique = BTreeSet::new();
    for note_id in &payload.note_ids {
        if !valid_note_id(note_id) || !unique.insert(note_id) {
            return Err("Knowledge activity 식별자가 올바르지 않습니다".into());
        }
    }
    Ok(())
}

fn validate_last_modified(
    notes_modified_today: u64,
    last_modified_at_ms: Option<i64>,
) -> Result<(), String> {
    if last_modified_at_ms.is_some_and(|timestamp| timestamp < 0)
        || (notes_modified_today > 0 && last_modified_at_ms.is_none())
    {
        return Err("Knowledge activity 최근 수정 시각이 올바르지 않습니다".into());
    }
    Ok(())
}

fn valid_note_id(note_id: &str) -> bool {
    let Some(number) = note_id.strip_prefix("note-") else {
        return false;
    };
    !number.is_empty()
        && !number.starts_with('0')
        && number.bytes().all(|byte| byte.is_ascii_digit())
        && number.parse::<u64>().is_ok_and(|value| value > 0)
}

/// 프로젝트 경로 목록 설정 (줄바꿈 구분).
#[tauri::command]
pub fn set_projects(
    state: tauri::State<'_, Arc<AppState>>,
    paths: Vec<String>,
) -> Result<Vec<String>, String> {
    let paths = crate::core::export::normalize_project_settings(&paths)?;
    let conn = state
        .db
        .lock()
        .map_err(|_| "project_settings_unavailable".to_string())?;
    db::set_setting(&conn, "projects", &paths.join("\n"));
    drop(conn);
    crate::integration::request_snapshot_write(state.inner().clone());
    Ok(paths)
}

/// Explicit connection check from Settings. Merely displaying or saving a
/// WSL target never starts its distro; this action may start it through
/// `wsl.exe`, so the frontend labels it accordingly.
#[tauri::command]
pub async fn probe_project(path: String) -> Result<ProjectProbe, String> {
    let normalized = crate::core::export::normalize_project_settings(&[path])?
        .into_iter()
        .next()
        .ok_or_else(|| "project_path_invalid".to_string())?;
    let target = devbox_git::GitTarget::from_project_path(&normalized)?;
    let target_label = if matches!(&target, devbox_git::GitTarget::Wsl { .. }) {
        "wsl"
    } else {
        "windows"
    }
    .to_string();
    let result = tokio::task::spawn_blocking(move || {
        let output = devbox_git::run_bounded_target(
            &["--no-pager", "rev-parse", "--is-inside-work-tree"],
            &target,
            Duration::from_secs(2),
            128,
        );
        match output {
            Ok(output) if output.trim() == "true" => (true, None),
            Ok(_) => (false, Some("git_output_invalid".to_string())),
            Err(code) => (false, Some(code)),
        }
    })
    .await
    .map_err(|_| "project_probe_failed".to_string())?;
    Ok(ProjectProbe {
        path: normalized,
        target: target_label,
        repository: result.0,
        error_code: result.1,
    })
}

#[tauri::command]
pub fn get_projects(state: tauri::State<'_, Arc<AppState>>) -> Vec<String> {
    let raw = db::get_setting_bounded(
        &state.db.lock().unwrap(),
        "projects",
        "",
        crate::core::export::MAX_PROJECT_SETTING_BYTES,
    )
    .unwrap_or_default();
    crate::core::export::parse_project_setting(&raw).unwrap_or_default()
}

fn saved_projects(state: &tauri::State<'_, Arc<AppState>>) -> Vec<String> {
    let raw = db::get_setting_bounded(
        &state.db.lock().unwrap(),
        "projects",
        "",
        crate::core::export::MAX_PROJECT_SETTING_BYTES,
    )
    .unwrap_or_default();
    crate::core::export::parse_project_setting(&raw).unwrap_or_default()
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
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

    struct FixtureRoot(PathBuf);

    impl FixtureRoot {
        fn new() -> Self {
            let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "devbox-life-log-knowledge-{}-{sequence}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for FixtureRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn publish(root: &Path, envelope: &devbox_integration::Envelope) {
        let dir =
            devbox_integration::snapshot_dir_in(root, &envelope.producer, envelope.schema_version);
        devbox_integration::write_atomic(envelope, &dir).unwrap();
    }

    fn activity_envelope(
        entry: serde_json::Value,
        schema_version: u32,
        freshness_ms: u64,
    ) -> devbox_integration::Envelope {
        let views = devbox_integration::SnapshotViews::from([(
            KNOWLEDGE_ACTIVITY_KIND.to_owned(),
            devbox_integration::SnapshotView {
                schema_version,
                freshness_ms,
                entries: vec![entry],
            },
        )]);
        devbox_integration::Envelope::with_views(KNOWLEDGE_PRODUCER, "0.5.0", views)
    }

    fn discover(root: &Path) -> Vec<SourceStatus> {
        source_statuses_in(devbox_integration::discover_report_in(root), root)
    }

    #[test]
    fn source_statuses_preserve_valid_and_isolated_error_rows() {
        let root = FixtureRoot::new();
        let report = devbox_integration::DiscoveryReport {
            snapshots: vec![devbox_integration::SnapshotRef {
                producer: "run-manager".into(),
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

        let statuses = source_statuses_in(report, root.path());
        assert_eq!(statuses.len(), 2);
        assert_eq!(statuses[0].producer, "run-manager");
        assert!(statuses[0].available);
        assert_eq!(statuses[0].freshness_ms, Some(1_234));
        assert_eq!(statuses[1].producer, "run-manager");
        assert!(!statuses[1].available);
        assert_eq!(statuses[1].schema_version, Some(2));
    }

    #[test]
    fn consumes_activity_view_and_uses_view_freshness() {
        let root = FixtureRoot::new();
        publish(
            root.path(),
            &activity_envelope(
                serde_json::json!({
                    "notesModifiedToday": 2,
                    "lastModifiedAtMs": 1_800_000_000_000_i64,
                    "noteIds": ["note-2", "note-7"],
                    "identifiersTruncated": false,
                }),
                KNOWLEDGE_ACTIVITY_SCHEMA_VERSION,
                5_000,
            ),
        );

        let statuses = discover(root.path());

        assert_eq!(statuses.len(), 1);
        let status = &statuses[0];
        assert!(status.available);
        assert!(status
            .freshness_ms
            .is_some_and(|freshness| freshness >= 5_000));
        assert_eq!(
            status.knowledge_activity,
            Some(KnowledgeActivity {
                notes_modified_today: 2,
                last_modified_at_ms: Some(1_800_000_000_000),
                identified_notes: 2,
                identifiers_complete: true,
                legacy_snapshot: false,
            })
        );
    }

    #[test]
    fn consumes_legacy_flat_snapshot_during_rolling_upgrade() {
        let root = FixtureRoot::new();
        publish(
            root.path(),
            &devbox_integration::Envelope::new(
                KNOWLEDGE_PRODUCER,
                "0.4.1",
                serde_json::json!({
                    "notesModifiedToday": 3,
                    "lastModifiedAtMs": 1_800_000_000_000_i64,
                }),
            ),
        );

        let statuses = discover(root.path());

        assert_eq!(statuses.len(), 1);
        assert!(statuses[0].available);
        assert_eq!(
            statuses[0].knowledge_activity,
            Some(KnowledgeActivity {
                notes_modified_today: 3,
                last_modified_at_ms: Some(1_800_000_000_000),
                identified_notes: 0,
                identifiers_complete: false,
                legacy_snapshot: true,
            })
        );
    }

    #[test]
    fn rejects_unsupported_view_schema_but_preserves_diagnostics() {
        let root = FixtureRoot::new();
        publish(
            root.path(),
            &activity_envelope(
                serde_json::json!({
                    "notesModifiedToday": 0,
                    "lastModifiedAtMs": null,
                    "noteIds": [],
                    "identifiersTruncated": false,
                }),
                2,
                2_000,
            ),
        );

        let statuses = discover(root.path());

        assert_eq!(statuses.len(), 1);
        assert!(!statuses[0].available);
        assert_eq!(
            statuses[0].error.as_deref(),
            Some("Knowledge activity view schema를 지원하지 않습니다")
        );
        assert_eq!(statuses[0].producer_version.as_deref(), Some("0.5.0"));
        assert!(statuses[0].generated_at.is_some());
        assert!(statuses[0].freshness_ms.is_some());
        assert!(statuses[0].knowledge_activity.is_none());
    }

    #[test]
    fn rejects_unsafe_or_duplicate_identifiers_without_echoing_them() {
        for note_ids in [
            serde_json::json!(["note-secret/path"]),
            serde_json::json!(["note-1", "note-1"]),
        ] {
            let root = FixtureRoot::new();
            let count = note_ids.as_array().unwrap().len();
            publish(
                root.path(),
                &activity_envelope(
                    serde_json::json!({
                        "notesModifiedToday": count,
                        "lastModifiedAtMs": 1_800_000_000_000_i64,
                        "noteIds": note_ids,
                        "identifiersTruncated": false,
                    }),
                    KNOWLEDGE_ACTIVITY_SCHEMA_VERSION,
                    0,
                ),
            );

            let statuses = discover(root.path());

            assert_eq!(statuses.len(), 1);
            let error = statuses[0].error.as_deref().unwrap();
            assert_eq!(error, "Knowledge activity 식별자가 올바르지 않습니다");
            assert!(!error.contains("secret/path"));
        }
    }

    #[test]
    fn corrupt_knowledge_snapshot_does_not_hide_other_sources() {
        let root = FixtureRoot::new();
        publish(
            root.path(),
            &devbox_integration::Envelope::new(
                "run-manager",
                "0.5.0",
                serde_json::json!({ "runs": 4 }),
            ),
        );
        let knowledge_dir = devbox_integration::snapshot_dir_in(
            root.path(),
            KNOWLEDGE_PRODUCER,
            KNOWLEDGE_SNAPSHOT_VERSION,
        );
        std::fs::create_dir_all(&knowledge_dir).unwrap();
        std::fs::write(knowledge_dir.join("summary.json"), "{ malformed").unwrap();

        let statuses = discover(root.path());

        assert_eq!(statuses.len(), 2);
        let knowledge = statuses
            .iter()
            .find(|status| status.producer == KNOWLEDGE_PRODUCER)
            .unwrap();
        let run_manager = statuses
            .iter()
            .find(|status| status.producer == "run-manager")
            .unwrap();
        assert!(!knowledge.available);
        assert!(run_manager.available);
    }

    #[test]
    fn source_statuses_show_root_failure_without_inventing_a_producer() {
        let root = FixtureRoot::new();
        let statuses = source_statuses_in(
            devbox_integration::DiscoveryReport {
                root_error: Some("integration root를 읽을 수 없습니다".into()),
                ..Default::default()
            },
            root.path(),
        );
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].producer, "integration-root");
        assert!(!statuses[0].available);
        assert_eq!(statuses[0].schema_version, None);
    }

    #[test]
    fn source_statuses_are_empty_when_nothing_has_been_published() {
        let root = FixtureRoot::new();
        assert!(source_statuses_in(Default::default(), root.path()).is_empty());
    }
}
