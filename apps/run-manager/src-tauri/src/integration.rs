//! devbox 공용 루트 integration snapshot producer (파일럿).
//!
//! 경로: `%LOCALAPPDATA%\devbox\integration\<producer-id>\v<n>\summary.json`
//! 및 independently versioned named view sidecar
//! (`<view-kind>.json`; Run Manager는 `jobs-services.json`)를 사용한다.
//! envelope 직렬화·원자 기록·경로 계산은 `crates/integration`(@devbox) 프리미티브를 쓴다.
//! 여기서는 producer별 데이터 내용만 담당한다.
//!
//! - secret·환경변수 값은 포함하지 않는다
//! - 기록 실패가 앱 동작을 막지 않는다 (오류만 로그)

use crate::core::models::JobKind;
use crate::core::workspace_task_control::WorkspaceTaskControlReceipt;
use crate::core::workspace_tasks::WorkspaceTaskState;
use crate::storage::{DatabaseState, LauncherDefinition};
use devbox_applink::contains_sensitive_value;
use devbox_integration::{Envelope, SnapshotView, SnapshotViews};
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::Path;

const PRODUCER_ID: &str = "run-manager";
/// The original flat status file remains a separate compatibility protocol.
pub const LEGACY_SNAPSHOT_SCHEMA_VERSION: u32 = 1;
/// The named sidecar uses the same producer/version identity while keeping
/// `summary.json` byte-compatible for old flat consumers.
pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;
pub const JOBS_SERVICES_VIEW_KIND: &str = "jobs-services";
pub const JOBS_SERVICES_VIEW_SCHEMA_VERSION: u32 = 1;
pub const WORKSPACE_TASKS_VIEW_KIND: &str = "workspace-tasks";
pub const WORKSPACE_TASKS_VIEW_SCHEMA_VERSION: u32 = 1;
pub const TASK_CONTROL_RECEIPTS_VIEW_KIND: &str = "task-control-receipts";
pub const TASK_CONTROL_RECEIPTS_VIEW_SCHEMA_VERSION: u32 = 1;
const SNAPSHOT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);
/// Keep the producer and Launcher source bound in lockstep. A complete view is
/// rejected instead of silently hiding definitions when the local database is
/// larger than this bound.
pub const MAX_JOBS_SERVICES: usize = 2_048;
pub const MAX_ENTRY_ID_BYTES: usize = 128;
pub const MAX_ENTRY_LABEL_BYTES: usize = 256;
pub const MAX_ENTRY_DETAIL_BYTES: usize = 512;

const SNAPSHOT_ERROR: &str = "Run Manager snapshot을 만들 수 없습니다";

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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LauncherTaskEntry {
    id: String,
    label: String,
    detail: String,
    target_app: &'static str,
    target_kind: &'static str,
    payload_version: u32,
    payload: LauncherTaskPayload,
}

#[derive(Debug, Clone, Serialize)]
struct LauncherTaskPayload {
    id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceTaskSnapshotEntry {
    id: String,
    label: String,
    revision: String,
    task_kind: String,
    trusted: bool,
    shell_trusted: bool,
    available: bool,
    has_dependencies: bool,
    operation_active: bool,
}

/// 주기적으로 snapshot을 쓴다 (상태 변화 추적보다 주기적 기록이 단순·충분 — [설계]).
pub fn spawn_snapshot_writer(database: std::sync::Arc<DatabaseState>) {
    tauri::async_runtime::spawn(async move {
        loop {
            if let Err(error) = write_snapshot(&database) {
                eprintln!("run-manager integration snapshot 실패: {error}");
            }
            tokio::time::sleep(SNAPSHOT_INTERVAL).await;
        }
    });
}

/// snapshot을 즉시 쓴다 (테스트·종료 시 1회 호출 가능).
pub fn write_snapshot(database: &DatabaseState) -> Result<(), String> {
    write_snapshot_in(&devbox_integration::integration_root(), database)
}

fn write_snapshot_in(root: &Path, database: &DatabaseState) -> Result<(), String> {
    // Build and publish the compatibility status independently. If the new
    // jobs/services projection is over-bound or otherwise fails, existing
    // status consumers still receive a fresh flat v1 snapshot and a previously
    // published Launcher snapshot is left intact by write_atomic.
    let status = build_data(database)?;
    let legacy = Envelope::new(
        PRODUCER_ID,
        env!("CARGO_PKG_VERSION"),
        serde_json::to_value(&status).map_err(|_| SNAPSHOT_ERROR.to_string())?,
    );
    let legacy_dir =
        devbox_integration::snapshot_dir_in(root, PRODUCER_ID, LEGACY_SNAPSHOT_SCHEMA_VERSION);
    devbox_integration::write_atomic(&legacy, &legacy_dir)?;

    let definitions = database
        .list_launcher_definitions(MAX_JOBS_SERVICES + 1)
        .map_err(|_| SNAPSHOT_ERROR.to_string())?;
    let entries = build_launcher_entries(definitions)?;
    let multi_view = build_launcher_envelope(entries)?;
    devbox_integration::write_named_view_snapshot_atomic(
        &multi_view,
        root,
        JOBS_SERVICES_VIEW_KIND,
    )?;
    write_workspace_tasks_in(root, database)?;
    write_task_control_receipts_in(root, database)
}

pub fn write_workspace_tasks(database: &DatabaseState) -> Result<(), String> {
    write_workspace_tasks_in(&devbox_integration::integration_root(), database)
}

fn write_workspace_tasks_in(root: &Path, database: &DatabaseState) -> Result<(), String> {
    let workspace_tasks = database
        .list_workspace_task_states()
        .map_err(|_| SNAPSHOT_ERROR.to_owned())?;
    let active_roots = database
        .list_active_workspace_task_operation_roots()
        .map_err(|_| SNAPSHOT_ERROR.to_owned())?;
    let workspace_view = build_workspace_task_envelope(workspace_tasks, &active_roots)?;
    devbox_integration::write_named_view_snapshot_atomic(
        &workspace_view,
        root,
        WORKSPACE_TASKS_VIEW_KIND,
    )
}

pub fn write_task_control_receipts(database: &DatabaseState) -> Result<(), String> {
    write_task_control_receipts_in(&devbox_integration::integration_root(), database)
}

fn write_task_control_receipts_in(root: &Path, database: &DatabaseState) -> Result<(), String> {
    let receipts = database
        .list_workspace_task_control_receipts(100)
        .map_err(|_| SNAPSHOT_ERROR.to_owned())?;
    let envelope = build_task_control_receipts_envelope(receipts)?;
    devbox_integration::write_named_view_snapshot_atomic(
        &envelope,
        root,
        TASK_CONTROL_RECEIPTS_VIEW_KIND,
    )
}

#[cfg(test)]
fn build_envelope(database: &DatabaseState) -> Result<Envelope, String> {
    let definitions = database
        .list_launcher_definitions(MAX_JOBS_SERVICES + 1)
        .map_err(|_| SNAPSHOT_ERROR.to_string())?;
    let entries = build_launcher_entries(definitions)?;
    build_launcher_envelope(entries)
}

fn build_launcher_envelope(entries: Vec<serde_json::Value>) -> Result<Envelope, String> {
    let mut views = SnapshotViews::new();
    views.insert(
        JOBS_SERVICES_VIEW_KIND.to_owned(),
        SnapshotView {
            schema_version: JOBS_SERVICES_VIEW_SCHEMA_VERSION,
            freshness_ms: 0,
            entries,
        },
    );
    Ok(Envelope::with_views(
        PRODUCER_ID,
        env!("CARGO_PKG_VERSION"),
        views,
    ))
}

fn build_workspace_task_envelope(
    states: Vec<WorkspaceTaskState>,
    active_roots: &std::collections::HashSet<String>,
) -> Result<Envelope, String> {
    if states.len() > crate::core::workspace_tasks::MAX_TASKS {
        return Err(SNAPSHOT_ERROR.to_owned());
    }
    let mut ids = BTreeSet::new();
    let mut entries = Vec::with_capacity(states.len());
    for state in states {
        if !valid_entry_id(&state.job_id)
            || !ids.insert(state.job_id.clone())
            || state.revision.len() != 64
            || !state
                .revision
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(SNAPSHOT_ERROR.to_owned());
        }
        let label = if valid_public_text(state.label.trim(), MAX_ENTRY_LABEL_BYTES)
            && !contains_sensitive_value(state.label.trim())
            && !looks_like_path(state.label.trim())
        {
            state.label.trim().to_owned()
        } else {
            "Workspace task".to_owned()
        };
        let operation_active = active_roots.contains(&state.job_id);
        entries.push(
            serde_json::to_value(WorkspaceTaskSnapshotEntry {
                id: state.job_id,
                label,
                revision: state.revision,
                task_kind: state.task_kind.as_str().to_owned(),
                trusted: state.trusted,
                shell_trusted: state.shell_trusted,
                available: state.available,
                has_dependencies: !state.depends_on.is_empty(),
                operation_active,
            })
            .map_err(|_| SNAPSHOT_ERROR.to_owned())?,
        );
    }
    entries.sort_by(|left, right| {
        left.get("id")
            .and_then(serde_json::Value::as_str)
            .cmp(&right.get("id").and_then(serde_json::Value::as_str))
    });
    let mut views = SnapshotViews::new();
    views.insert(
        WORKSPACE_TASKS_VIEW_KIND.to_owned(),
        SnapshotView {
            schema_version: WORKSPACE_TASKS_VIEW_SCHEMA_VERSION,
            freshness_ms: 0,
            entries,
        },
    );
    Ok(Envelope::with_views(
        PRODUCER_ID,
        env!("CARGO_PKG_VERSION"),
        views,
    ))
}

fn build_task_control_receipts_envelope(
    receipts: Vec<WorkspaceTaskControlReceipt>,
) -> Result<Envelope, String> {
    if receipts.len() > 100 {
        return Err(SNAPSHOT_ERROR.to_owned());
    }
    let entries = receipts
        .into_iter()
        .map(|receipt| serde_json::to_value(receipt).map_err(|_| SNAPSHOT_ERROR.to_owned()))
        .collect::<Result<Vec<_>, _>>()?;
    let mut views = SnapshotViews::new();
    views.insert(
        TASK_CONTROL_RECEIPTS_VIEW_KIND.to_owned(),
        SnapshotView {
            schema_version: TASK_CONTROL_RECEIPTS_VIEW_SCHEMA_VERSION,
            freshness_ms: 0,
            entries,
        },
    );
    Ok(Envelope::with_views(
        PRODUCER_ID,
        env!("CARGO_PKG_VERSION"),
        views,
    ))
}

fn build_data(database: &DatabaseState) -> Result<SnapshotData, String> {
    let now = current_epoch_ms();
    let (success, failed) = database
        .run_counts_since(start_of_today_utc())
        .map_err(|error| error.to_string())?;
    let last_run_at_ms = database.last_run_at().map_err(|error| error.to_string())?;

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

fn build_launcher_entries(
    definitions: Vec<LauncherDefinition>,
) -> Result<Vec<serde_json::Value>, String> {
    if definitions.len() > MAX_JOBS_SERVICES {
        return Err(SNAPSHOT_ERROR.into());
    }

    let mut ids = BTreeSet::new();
    let mut entries = Vec::with_capacity(definitions.len());
    for definition in definitions {
        if !valid_entry_id(&definition.id) || !ids.insert(definition.id.clone()) {
            return Err(SNAPSHOT_ERROR.into());
        }
        let label = public_label(&definition);
        let detail = match definition.kind {
            JobKind::Job => "Run Manager · job",
            JobKind::Service => "Run Manager · service",
        }
        .to_owned();
        debug_assert!(detail.len() <= MAX_ENTRY_DETAIL_BYTES);
        let entry = LauncherTaskEntry {
            id: definition.id.clone(),
            label,
            detail,
            target_app: PRODUCER_ID,
            target_kind: "task",
            payload_version: 1,
            payload: LauncherTaskPayload { id: definition.id },
        };
        entries.push(serde_json::to_value(entry).map_err(|_| SNAPSHOT_ERROR.to_string())?);
    }
    entries.sort_by(|left, right| {
        left.get("id")
            .and_then(serde_json::Value::as_str)
            .cmp(&right.get("id").and_then(serde_json::Value::as_str))
    });
    Ok(entries)
}

fn public_label(definition: &LauncherDefinition) -> String {
    let candidate = definition.name.trim();
    if valid_public_text(candidate, MAX_ENTRY_LABEL_BYTES)
        && !contains_sensitive_value(candidate)
        && !looks_like_path(candidate)
    {
        return candidate.to_owned();
    }
    match definition.kind {
        JobKind::Job => "Run Manager 작업".to_owned(),
        JobKind::Service => "Run Manager 서비스".to_owned(),
    }
}

/// Job names are user-controlled labels, not a channel for exporting local
/// filesystem locations. Keep ordinary names such as `Build/API` readable,
/// but replace unambiguous absolute, UNC, and file-URI forms with a fallback.
fn looks_like_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    value.starts_with('/')
        || value.starts_with("//")
        || value.starts_with("\\\\")
        || value.starts_with("~/")
        || value.starts_with("~\\")
        || value.starts_with("./")
        || value.starts_with(".\\")
        || value.starts_with("../")
        || value.starts_with("..\\")
        || value
            .get(..7)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file://"))
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\'))
}

fn valid_entry_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ENTRY_ID_BYTES
        && !contains_sensitive_value(value)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn valid_public_text(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && value.trim() == value
        && !value.chars().any(char::is_control)
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
    use crate::core::models::{
        EnvironmentCiphertextUpdate, EnvironmentUpdate, JobInput, OverlapPolicy, RestartPolicy,
        ServiceInput, TargetKind,
    };
    use crate::core::workspace_task_control::WorkspaceTaskControlReceiptStatus;
    use crate::core::workspace_tasks::{WorkspaceTaskDependsOrder, WorkspaceTaskKind};
    use crate::storage::DatabaseState;
    use devbox_applink::{TaskControlAction, TASK_CONTROL_SCHEMA_VERSION};
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct LegacyConsumerStatus {
        active_services: Vec<LegacyConsumerService>,
        runs: LegacyConsumerRuns,
        last_run_at_ms: Option<i64>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct LegacyConsumerService {
        id: String,
        uptime_ms: i64,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct LegacyConsumerRuns {
        success: i64,
        failed: i64,
    }

    #[test]
    fn start_of_today_is_day_aligned() {
        let today = start_of_today_utc();
        assert_eq!(today % 86_400_000, 0);
    }

    fn database() -> DatabaseState {
        DatabaseState::open_in_memory().expect("in-memory database")
    }

    fn job_input(name: &str) -> JobInput {
        JobInput {
            name: name.into(),
            command: "echo local-only-command".into(),
            cwd: Some("C:\\private\\workspace".into()),
            target_kind: TargetKind::Windows,
            target_distro: None,
            environment: EnvironmentUpdate::Keep,
            cron_expr: "* * * * *".into(),
            enabled: false,
            overlap_policy: OverlapPolicy::Skip,
            catch_up: false,
        }
    }

    fn service_input(name: &str) -> ServiceInput {
        ServiceInput {
            name: name.into(),
            command: "echo local-only-service".into(),
            cwd: Some("C:\\private\\service".into()),
            target_kind: TargetKind::Windows,
            target_distro: None,
            environment: EnvironmentUpdate::Keep,
            restart_policy: RestartPolicy::Never,
            auto_start: false,
            health_tcp_address: None,
            health_tcp_port: None,
        }
    }

    fn sample_definition(id: &str, kind: JobKind, name: &str) -> LauncherDefinition {
        LauncherDefinition {
            id: id.into(),
            kind,
            name: name.into(),
        }
    }

    fn workspace_task_state() -> WorkspaceTaskState {
        WorkspaceTaskState {
            job_id: "workspace-task-1".to_owned(),
            source_id: "source-private".to_owned(),
            label: "Build workspace".to_owned(),
            task_kind: WorkspaceTaskKind::Process,
            source_root: "/home/private/project".to_owned(),
            revision: "a".repeat(64),
            target_kind: TargetKind::Windows,
            target_distro: None,
            environment_keys: vec!["PRIVATE_TOKEN".to_owned()],
            applied_override: Some("private override".to_owned()),
            depends_on: vec!["Prepare".to_owned()],
            depends_order: WorkspaceTaskDependsOrder::Parallel,
            has_problem_matcher: true,
            trusted: true,
            shell_trusted: false,
            available: true,
        }
    }

    #[test]
    fn publishes_all_jobs_services_in_named_sidecar() {
        let database = database();
        database
            .create_job_with_id_and_ciphertext_at(
                "job-2".into(),
                job_input("Build task"),
                None,
                1_000,
            )
            .unwrap();
        database
            .create_service_with_id_and_ciphertext_at(
                "service-1".into(),
                service_input("API service"),
                EnvironmentCiphertextUpdate::Clear,
                1_000,
            )
            .unwrap();

        assert_eq!(
            database
                .list_launcher_definitions(MAX_JOBS_SERVICES + 1)
                .unwrap(),
            vec![
                LauncherDefinition {
                    id: "job-2".into(),
                    kind: JobKind::Job,
                    name: "Build task".into(),
                },
                LauncherDefinition {
                    id: "service-1".into(),
                    kind: JobKind::Service,
                    name: "API service".into(),
                },
            ]
        );
        let envelope = build_envelope(&database).unwrap();
        assert_eq!(envelope.schema_version, SNAPSHOT_SCHEMA_VERSION);
        let views = envelope.views().unwrap();
        assert_eq!(
            views.keys().collect::<Vec<_>>(),
            vec![JOBS_SERVICES_VIEW_KIND]
        );
        assert_eq!(views[JOBS_SERVICES_VIEW_KIND].entries.len(), 2);
        assert_eq!(views[JOBS_SERVICES_VIEW_KIND].entries[0]["id"], "job-2");
        assert_eq!(views[JOBS_SERVICES_VIEW_KIND].entries[1]["id"], "service-1");
        for entry in &views[JOBS_SERVICES_VIEW_KIND].entries {
            assert_eq!(entry["targetApp"], PRODUCER_ID);
            assert_eq!(entry["targetKind"], "task");
            assert_eq!(entry["payloadVersion"], 1);
            assert_eq!(entry["payload"]["id"], entry["id"]);
        }
    }

    #[test]
    fn legacy_status_snapshot_stays_flat_and_named_sidecar_is_multi_view() {
        let database = database();
        let root = tempfile::tempdir().unwrap();

        write_snapshot_in(root.path(), &database).unwrap();

        let legacy = devbox_integration::read_snapshot_in(
            root.path(),
            PRODUCER_ID,
            LEGACY_SNAPSHOT_SCHEMA_VERSION,
        )
        .unwrap()
        .unwrap();
        assert_eq!(legacy.schema_version, LEGACY_SNAPSHOT_SCHEMA_VERSION);
        assert!(legacy.data.get("views").is_none());
        let parsed: LegacyConsumerStatus = serde_json::from_value(legacy.data.clone()).unwrap();
        assert!(parsed.active_services.is_empty());
        assert_eq!(parsed.runs.success, 0);
        assert_eq!(parsed.runs.failed, 0);
        assert_eq!(parsed.last_run_at_ms, None);
        assert_eq!(
            legacy
                .data
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            vec!["activeServices", "lastRunAtMs", "runs"]
        );

        let multi_view = devbox_integration::read_named_view_snapshot_in(
            root.path(),
            PRODUCER_ID,
            SNAPSHOT_SCHEMA_VERSION,
            JOBS_SERVICES_VIEW_KIND,
        )
        .unwrap()
        .unwrap();
        assert_eq!(multi_view.schema_version, SNAPSHOT_SCHEMA_VERSION);
        assert_eq!(multi_view.producer, PRODUCER_ID);
        assert!(multi_view
            .views()
            .unwrap()
            .contains_key(JOBS_SERVICES_VIEW_KIND));

        for view_kind in [WORKSPACE_TASKS_VIEW_KIND, TASK_CONTROL_RECEIPTS_VIEW_KIND] {
            let sidecar = devbox_integration::read_named_view_snapshot_in(
                root.path(),
                PRODUCER_ID,
                SNAPSHOT_SCHEMA_VERSION,
                view_kind,
            )
            .unwrap()
            .unwrap();
            assert!(sidecar.views().unwrap().contains_key(view_kind));
        }
    }

    #[test]
    fn workspace_task_projection_exposes_only_safe_control_metadata() {
        let active_roots = std::collections::HashSet::from(["workspace-task-1".to_owned()]);
        let envelope =
            build_workspace_task_envelope(vec![workspace_task_state()], &active_roots).unwrap();
        let entries = &envelope.views().unwrap()[WORKSPACE_TASKS_VIEW_KIND].entries;
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry["id"], "workspace-task-1");
        assert_eq!(entry["hasDependencies"], true);
        assert_eq!(entry["operationActive"], true);
        assert_eq!(
            entry
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            vec![
                "available",
                "hasDependencies",
                "id",
                "label",
                "operationActive",
                "revision",
                "shellTrusted",
                "taskKind",
                "trusted"
            ]
        );
        let serialized = serde_json::to_string(entry).unwrap();
        for private in [
            "/home/private/project",
            "PRIVATE_TOKEN",
            "private override",
            "source-private",
            "Prepare",
            "problemMatcher",
        ] {
            assert!(!serialized.contains(private), "leaked {private}");
        }
    }

    #[test]
    fn task_control_receipt_projection_omits_revision_and_execution_inputs() {
        let receipt = WorkspaceTaskControlReceipt {
            schema_version: TASK_CONTROL_SCHEMA_VERSION,
            request_id: "b".repeat(32),
            task_id: "workspace-task-1".to_owned(),
            action: TaskControlAction::Start,
            status: WorkspaceTaskControlReceiptStatus::Started,
            operation_id: Some("550e8400-e29b-41d4-a716-446655440000".to_owned()),
            failure_code: None,
            created_at: 1,
            updated_at: 2,
        };
        let envelope = build_task_control_receipts_envelope(vec![receipt]).unwrap();
        let entries = &envelope.views().unwrap()[TASK_CONTROL_RECEIPTS_VIEW_KIND].entries;
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(
            entry
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            vec![
                "action",
                "createdAt",
                "failureCode",
                "operationId",
                "requestId",
                "schemaVersion",
                "status",
                "taskId",
                "updatedAt"
            ]
        );
        let serialized = serde_json::to_string(entry).unwrap();
        for private_key in [
            "expectedRevision",
            "sourceRoot",
            "command",
            "environment",
            "cwd",
        ] {
            assert!(!serialized.contains(private_key));
        }
    }

    #[test]
    fn legacy_status_reports_running_service_and_legacy_metrics() {
        let database = database();
        database
            .create_service_with_id_and_ciphertext_at(
                "service-running".into(),
                service_input("Running service"),
                EnvironmentCiphertextUpdate::Clear,
                1_000,
            )
            .unwrap();
        let instance = database
            .claim_service_start("service-running", "owner-1", "attempt-1", 2_000)
            .unwrap()
            .unwrap();
        let run = database
            .create_service_run_at("service-running", 2_000)
            .unwrap();
        assert!(database
            .mark_service_running(
                "service-running",
                instance.generation,
                "owner-1",
                "attempt-1",
                &run.id,
                2_000,
            )
            .unwrap());

        let status = build_data(&database).unwrap();
        assert_eq!(status.active_services.len(), 1);
        assert_eq!(status.active_services[0].id, "service-running");
        assert_eq!(status.runs.success, 0);
        assert_eq!(status.runs.failed, 0);
        assert_eq!(status.last_run_at_ms, Some(2_000));
        let serialized = serde_json::to_value(status).unwrap();
        assert!(serialized.get("activeServices").is_some());
        assert!(serialized.get("runs").is_some());
        assert!(serialized.get("lastRunAtMs").is_some());
        let parsed: LegacyConsumerStatus = serde_json::from_value(serialized).unwrap();
        assert_eq!(parsed.active_services[0].id, "service-running");
        assert!(parsed.active_services[0].uptime_ms >= 0);
    }

    #[test]
    fn launcher_entries_never_copy_command_cwd_environment_or_credentials() {
        let unsafe_name = "Bearer raw-secret-for-a-command";
        let database = database();
        database
            .create_job_with_id_and_ciphertext_at(
                "job-private".into(),
                job_input(unsafe_name),
                Some(b"raw-private-environment".to_vec()),
                1_000,
            )
            .unwrap();
        let views = build_envelope(&database).unwrap().views().unwrap();
        let entries = &views[JOBS_SERVICES_VIEW_KIND].entries;
        let serialized = serde_json::to_string(&entries).unwrap();
        assert!(!serialized.contains(unsafe_name));
        assert!(!serialized.contains("echo local-only-command"));
        assert!(!serialized.contains("C:\\\\private\\\\workspace"));
        assert!(!serialized.contains("environment"));
        assert!(!serialized.contains("credential"));
        assert!(!serialized.contains("raw-private-environment"));
        assert_eq!(entries[0]["label"], "Run Manager 작업");
        assert!(entries[0].get("command").is_none());
        assert!(entries[0].get("cwd").is_none());
        assert!(entries[0].get("envConfigured").is_none());
        assert_eq!(
            entries[0]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            vec![
                "detail",
                "id",
                "label",
                "payload",
                "payloadVersion",
                "targetApp",
                "targetKind"
            ]
        );
        assert_eq!(
            entries[0]["payload"]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            vec!["id"]
        );
    }

    #[test]
    fn launcher_entries_reject_unsafe_or_oversized_ids() {
        for id in ["../secret", "api_key=raw-secret"] {
            assert!(
                build_launcher_entries(vec![sample_definition(id, JobKind::Job, "safe")]).is_err()
            );
        }
        let oversized_id = "x".repeat(MAX_ENTRY_ID_BYTES + 1);
        assert!(build_launcher_entries(vec![sample_definition(
            &oversized_id,
            JobKind::Job,
            "safe"
        )],)
        .is_err());
    }

    #[test]
    fn launcher_entries_reject_duplicate_ids_and_bound_total_items() {
        let duplicate = build_launcher_entries(vec![
            sample_definition("same-id", JobKind::Job, "job"),
            sample_definition("same-id", JobKind::Service, "service"),
        ]);
        assert!(duplicate.is_err());

        let jobs = (0..=MAX_JOBS_SERVICES)
            .map(|index| sample_definition(&format!("job-{index}"), JobKind::Job, "bounded"))
            .collect();
        assert!(build_launcher_entries(jobs).is_err());
    }

    #[test]
    fn launcher_projection_reads_one_over_bound_and_fails_closed() {
        let database = database();
        let inputs = (0..=MAX_JOBS_SERVICES)
            .map(|index| job_input(&format!("bounded-{index}")))
            .collect();
        let (created, skipped) = database
            .create_import_jobs_at_with_cancel(inputs, 1_000, || Ok(()))
            .unwrap();
        assert_eq!(created, (MAX_JOBS_SERVICES + 1) as u32);
        assert_eq!(skipped, 0);

        let definitions = database
            .list_launcher_definitions(MAX_JOBS_SERVICES + 1)
            .unwrap();
        assert_eq!(definitions.len(), MAX_JOBS_SERVICES + 1);
        assert!(build_launcher_entries(definitions).is_err());
    }

    #[test]
    fn launcher_snapshot_overflow_keeps_last_good_after_v1_succeeds() {
        let root = tempfile::tempdir().unwrap();
        let good_database = database();
        write_snapshot_in(root.path(), &good_database).unwrap();
        let previous_launcher_snapshot = devbox_integration::read_named_view_snapshot_in(
            root.path(),
            PRODUCER_ID,
            SNAPSHOT_SCHEMA_VERSION,
            JOBS_SERVICES_VIEW_KIND,
        )
        .unwrap()
        .unwrap();

        let oversized_database = database();
        let inputs = (0..=MAX_JOBS_SERVICES)
            .map(|index| job_input(&format!("overflow-{index}")))
            .collect();
        let (created, skipped) = oversized_database
            .create_import_jobs_at_with_cancel(inputs, 1_000, || Ok(()))
            .unwrap();
        assert_eq!(created, (MAX_JOBS_SERVICES + 1) as u32);
        assert_eq!(skipped, 0);

        assert!(write_snapshot_in(root.path(), &oversized_database).is_err());

        let legacy = devbox_integration::read_snapshot_in(
            root.path(),
            PRODUCER_ID,
            LEGACY_SNAPSHOT_SCHEMA_VERSION,
        )
        .unwrap()
        .unwrap();
        assert_eq!(legacy.schema_version, LEGACY_SNAPSHOT_SCHEMA_VERSION);
        assert!(legacy.data.get("views").is_none());

        let current_launcher_snapshot = devbox_integration::read_named_view_snapshot_in(
            root.path(),
            PRODUCER_ID,
            SNAPSHOT_SCHEMA_VERSION,
            JOBS_SERVICES_VIEW_KIND,
        )
        .unwrap()
        .unwrap();
        assert_eq!(current_launcher_snapshot, previous_launcher_snapshot);
    }

    #[test]
    fn launcher_entries_use_bounded_fallback_labels_for_malformed_names() {
        let long_name = "x".repeat(MAX_ENTRY_LABEL_BYTES + 1);
        let entries = build_launcher_entries(vec![
            sample_definition("job-safe", JobKind::Job, &long_name),
            sample_definition("service-safe", JobKind::Service, "safe service"),
        ])
        .unwrap();
        assert_eq!(entries[0]["label"], "Run Manager 작업");
        assert!(entries[0]["label"].as_str().unwrap().len() <= MAX_ENTRY_LABEL_BYTES);
        assert_eq!(entries[0]["detail"], "Run Manager · job");
        assert_eq!(entries[1]["detail"], "Run Manager · service");
    }

    #[test]
    fn launcher_entries_replace_absolute_and_uri_path_labels_but_keep_slash_names() {
        let path_names = [
            r"C:\private\workspace",
            "/home/private/workspace",
            r"\\server\share\workspace",
            "file:///private/workspace",
            "~/private/workspace",
            r"~\private\workspace",
            "./private/workspace",
            r".\private\workspace",
            "../private/workspace",
            r"..\private\workspace",
        ];
        for (index, name) in path_names.into_iter().enumerate() {
            let entries = build_launcher_entries(vec![sample_definition(
                &format!("path-{index}"),
                JobKind::Job,
                name,
            )])
            .unwrap();
            assert_eq!(entries[0]["label"], "Run Manager 작업", "{name}");
        }

        let entries = build_launcher_entries(vec![sample_definition(
            "slash-name",
            JobKind::Job,
            "Build/API",
        )])
        .unwrap();
        assert_eq!(entries[0]["label"], "Build/API");
    }
}
