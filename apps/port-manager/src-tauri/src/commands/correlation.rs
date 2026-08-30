//! Read-only listener correlation and revalidated navigation actions.
//!
//! Run Manager and Workbench remain the sources of truth. Port Manager reads
//! their independent strict named views, labels each match by confidence, and
//! issues only bounded opaque action keys. Every action re-collects listeners
//! and re-reads the source snapshots before launching another app.

use super::ports::{collect_ports, PortRow};
use crate::core::listeners::{ListenerIdentity, ListenerSource};
use devbox_applink::{
    CreateHandoff, HandoffDescriptor, HandoffError, HandoffPublication, HandoffStore,
    LogSourceStream, OpenRequest, OpenTarget,
};
use devbox_integration::{PortBindingEntry, PortBindingProcess, PortBindingTargetKind};
use serde::Serialize;
use std::path::Path;
use std::sync::{Mutex, MutexGuard, OnceLock, TryLockError};

const RUN_MANAGER: &str = "run-manager";
const WORKBENCH: &str = "workbench";
const PORT_MANAGER: &str = "port-manager";
const SNAPSHOT_SCHEMA_VERSION: u32 = 1;
const MAX_SNAPSHOT_AGE_MS: u64 = 180_000;
const MAX_CLOCK_SKEW_MS: u64 = 30_000;
const MAX_CORRELATIONS_PER_ROW: usize = 64;
const MAX_TOTAL_CORRELATIONS: usize = 4_096;
const LOG_LENS_CAPABILITY: &str = "handoff:log-source/v1";
static LOG_DISPATCH_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CorrelationConfidence {
    Verified,
    Declared,
    Expected,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct PortCorrelation {
    pub source_app: String,
    pub target_kind: String,
    pub target_id: String,
    pub label: String,
    pub confidence: CorrelationConfidence,
    pub action_key: String,
    pub logs_available: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SnapshotSourceState {
    Available,
    Missing,
    Invalid,
    Stale,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct SnapshotSourceStatus {
    pub producer: String,
    pub state: SnapshotSourceState,
    pub freshness_ms: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ObservedPortRow {
    #[serde(flatten)]
    pub row: PortRow,
    pub correlations: Vec<PortCorrelation>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PortObservationSnapshot {
    pub rows: Vec<ObservedPortRow>,
    pub sources: Vec<SnapshotSourceStatus>,
    pub correlations_truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct LogLensDispatch {
    pub handoff_id: String,
}

#[derive(Debug, Clone)]
struct BindingSource {
    producer: &'static str,
    entries: Vec<PortBindingEntry>,
}

#[derive(Debug, Clone)]
struct ResolvedCorrelation {
    public: PortCorrelation,
    run_id: Option<String>,
}

struct CorrelationResult {
    rows: Vec<(PortRow, Vec<ResolvedCorrelation>)>,
    sources: Vec<SnapshotSourceStatus>,
    truncated: bool,
}

#[tauri::command]
pub async fn list_port_observations() -> Result<PortObservationSnapshot, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let rows = collect_ports().map_err(|error| error.to_string())?;
        Ok(observe_rows_in(
            &devbox_integration::integration_root(),
            rows,
            now_ms(),
        ))
    })
    .await
    .map_err(|_| "listener 정보를 가져오지 못했습니다.".to_string())?
}

#[tauri::command]
pub async fn open_port_owner(action_key: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let resolved = resolve_current_action(&action_key)?;
        let target = match resolved.public.target_kind.as_str() {
            "task" if resolved.public.source_app == RUN_MANAGER => OpenTarget::Task {
                id: resolved.public.target_id,
            },
            "profile" if resolved.public.source_app == WORKBENCH => OpenTarget::Profile {
                id: resolved.public.target_id,
            },
            _ => return Err("port action is unavailable".to_string()),
        };
        devbox_launch::launch_open(
            &resolved.public.source_app,
            &OpenRequest {
                target,
                from: Some(PORT_MANAGER.into()),
            },
        )
        .map(|_| ())
        .map_err(|_| "port action launch failed".to_string())
    })
    .await
    .map_err(|_| "port action is unavailable".to_string())?
}

#[tauri::command]
pub async fn open_port_log(
    action_key: String,
    stream: LogSourceStream,
) -> Result<LogLensDispatch, String> {
    tauri::async_runtime::spawn_blocking(move || dispatch_log(action_key, stream))
        .await
        .map_err(|_| "log source handoff is unavailable".to_string())?
}

fn resolve_current_action(action_key: &str) -> Result<ResolvedCorrelation, String> {
    if !valid_action_key(action_key) {
        return Err("port action is unavailable".into());
    }
    let rows = collect_ports().map_err(|_| "port action is unavailable".to_string())?;
    let observed = correlate_rows_in(&devbox_integration::integration_root(), rows, now_ms());
    observed
        .rows
        .into_iter()
        .flat_map(|(_, correlations)| correlations)
        .find(|correlation| correlation.public.action_key == action_key)
        .ok_or_else(|| "port action is stale".to_string())
}

fn dispatch_log(action_key: String, stream: LogSourceStream) -> Result<LogLensDispatch, String> {
    let _guard = dispatch_lock()?;
    let resolved = resolve_current_action(&action_key)?;
    if resolved.public.source_app != RUN_MANAGER || !resolved.public.logs_available {
        return Err("log source is unavailable".into());
    }
    let run_id = resolved
        .run_id
        .ok_or_else(|| "log source is unavailable".to_string())?;
    if !devbox_launch::installed_targets(LOG_LENS_CAPABILITY)
        .iter()
        .any(|target| target.id == devbox_applink::LOG_SOURCE_TARGET_APP)
    {
        return Err("log lens is unavailable".into());
    }
    let payload = devbox_applink::run_log_source_payload(&run_id, stream)
        .map_err(|_| "log source is unavailable".to_string())?;
    let now = now_ms();
    if now == 0 {
        return Err("log source handoff is unavailable".into());
    }
    let store = handoff_store();
    let publication = store
        .create_with_publication(
            CreateHandoff {
                kind: devbox_applink::LOG_SOURCE_HANDOFF_KIND.into(),
                source_app: PORT_MANAGER.into(),
                target_app: Some(devbox_applink::LOG_SOURCE_TARGET_APP.into()),
                payload,
            },
            now,
        )
        .map_err(|_| "log source handoff is unavailable".to_string())?;
    let request = OpenRequest {
        target: HandoffDescriptor {
            id: publication.descriptor.id.clone(),
            kind: publication.descriptor.kind.clone(),
        }
        .into(),
        from: Some(PORT_MANAGER.into()),
    };
    match devbox_launch::launch_open(devbox_applink::LOG_SOURCE_TARGET_APP, &request) {
        Ok(_) => Ok(LogLensDispatch {
            handoff_id: publication.descriptor.id,
        }),
        Err(_) => {
            cleanup_publication(&store, &publication)?;
            Err("log lens launch failed".into())
        }
    }
}

fn dispatch_lock() -> Result<MutexGuard<'static, ()>, String> {
    match LOG_DISPATCH_LOCK.get_or_init(|| Mutex::new(())).try_lock() {
        Ok(guard) => Ok(guard),
        Err(TryLockError::Poisoned(poisoned)) => Ok(poisoned.into_inner()),
        Err(TryLockError::WouldBlock) => Err("log source handoff is busy".into()),
    }
}

fn cleanup_publication(
    store: &HandoffStore,
    publication: &HandoffPublication,
) -> Result<(), String> {
    match store.remove_pending(publication) {
        Ok(()) | Err(HandoffError::Missing) => Ok(()),
        Err(_) => Err("log source handoff cleanup failed".into()),
    }
}

fn handoff_store() -> HandoffStore {
    HandoffStore::new(devbox_applink::handoff_root_in(
        &devbox_integration::common_root(),
    ))
}

fn valid_action_key(value: &str) -> bool {
    value.len() == "port-action-".len() + 64
        && value.starts_with("port-action-")
        && value["port-action-".len()..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn observe_rows_in(root: &Path, rows: Vec<PortRow>, now: u64) -> PortObservationSnapshot {
    let correlated = correlate_rows_in(root, rows, now);
    PortObservationSnapshot {
        rows: correlated
            .rows
            .into_iter()
            .map(|(row, correlations)| ObservedPortRow {
                row,
                correlations: correlations
                    .into_iter()
                    .map(|correlation| correlation.public)
                    .collect(),
            })
            .collect(),
        sources: correlated.sources,
        correlations_truncated: correlated.truncated,
    }
}

fn correlate_rows_in(root: &Path, rows: Vec<PortRow>, now: u64) -> CorrelationResult {
    let (run, run_status) = read_source(root, RUN_MANAGER, now);
    let (workbench, workbench_status) = read_source(root, WORKBENCH, now);
    let sources = [run, workbench].into_iter().flatten().collect::<Vec<_>>();
    let mut remaining = MAX_TOTAL_CORRELATIONS;
    let mut truncated = false;
    let mut correlated = Vec::with_capacity(rows.len());
    for row in rows {
        let mut correlations = sources
            .iter()
            .flat_map(|source| correlations_for_row(&row, source))
            .collect::<Vec<_>>();
        correlations
            .sort_by(|left, right| correlation_sort_key(left).cmp(&correlation_sort_key(right)));
        let limit = MAX_CORRELATIONS_PER_ROW.min(remaining);
        if correlations.len() > limit {
            correlations.truncate(limit);
            truncated = true;
        }
        remaining = remaining.saturating_sub(correlations.len());
        correlated.push((row, correlations));
    }
    CorrelationResult {
        rows: correlated,
        sources: vec![run_status, workbench_status],
        truncated,
    }
}

fn read_source(
    root: &Path,
    producer: &'static str,
    now: u64,
) -> (Option<BindingSource>, SnapshotSourceStatus) {
    let missing = || SnapshotSourceStatus {
        producer: producer.into(),
        state: SnapshotSourceState::Missing,
        freshness_ms: None,
    };
    let envelope = match devbox_integration::read_named_view_snapshot_in(
        root,
        producer,
        SNAPSHOT_SCHEMA_VERSION,
        devbox_integration::PORT_BINDINGS_VIEW_KIND,
    ) {
        Ok(Some(envelope)) => envelope,
        Ok(None) => return (None, missing()),
        Err(_) => {
            return (
                None,
                SnapshotSourceStatus {
                    producer: producer.into(),
                    state: SnapshotSourceState::Invalid,
                    freshness_ms: None,
                },
            )
        }
    };
    let generated = match devbox_integration::generated_at_epoch_ms(&envelope.generated_at) {
        Ok(generated) => generated,
        Err(_) => {
            return (
                None,
                SnapshotSourceStatus {
                    producer: producer.into(),
                    state: SnapshotSourceState::Invalid,
                    freshness_ms: None,
                },
            )
        }
    };
    let entries = match devbox_integration::port_bindings_from_envelope(&envelope) {
        Ok(entries) => entries,
        Err(_) => {
            return (
                None,
                SnapshotSourceStatus {
                    producer: producer.into(),
                    state: SnapshotSourceState::Invalid,
                    freshness_ms: None,
                },
            )
        }
    };
    let freshness = now.saturating_sub(generated);
    if generated > now.saturating_add(MAX_CLOCK_SKEW_MS) {
        return (
            None,
            SnapshotSourceStatus {
                producer: producer.into(),
                state: SnapshotSourceState::Invalid,
                freshness_ms: None,
            },
        );
    }
    let bounded_freshness = u32::try_from(freshness).unwrap_or(u32::MAX);
    if freshness > MAX_SNAPSHOT_AGE_MS {
        return (
            None,
            SnapshotSourceStatus {
                producer: producer.into(),
                state: SnapshotSourceState::Stale,
                freshness_ms: Some(bounded_freshness),
            },
        );
    }
    (
        Some(BindingSource { producer, entries }),
        SnapshotSourceStatus {
            producer: producer.into(),
            state: SnapshotSourceState::Available,
            freshness_ms: Some(bounded_freshness),
        },
    )
}

fn correlations_for_row(row: &PortRow, source: &BindingSource) -> Vec<ResolvedCorrelation> {
    if row.port.port == 0
        || !row.port.proto.to_ascii_uppercase().starts_with("TCP")
        || !row.endpoint().is_listener()
    {
        return Vec::new();
    }
    source
        .entries
        .iter()
        .filter_map(|entry| match entry {
            PortBindingEntry::RunService {
                id,
                label,
                address,
                port,
                target_kind,
                target_distro,
                run_id,
                logs_available,
                process,
                ..
            } if *port == row.port.port
                && listener_address_matches(row, address)
                && run_source_matches(row, *target_kind, target_distro.as_deref()) =>
            {
                let confidence = if verified_windows_process(row, process.as_ref()) {
                    CorrelationConfidence::Verified
                } else {
                    CorrelationConfidence::Declared
                };
                resolved_correlation(
                    row,
                    source,
                    "task",
                    id,
                    label,
                    confidence,
                    *logs_available,
                    run_id.clone(),
                )
            }
            PortBindingEntry::WorkbenchProfile { id, label, port } if *port == row.port.port => {
                resolved_correlation(
                    row,
                    source,
                    "profile",
                    id,
                    label,
                    CorrelationConfidence::Expected,
                    false,
                    None,
                )
            }
            _ => None,
        })
        .collect()
}

/// A configured Run service is a declaration for one loopback endpoint, not
/// every listener that happens to reuse the same port. `localhost` is the
/// only intentionally dual-stack declaration; concrete IPv4/IPv6 addresses
/// must match exactly.
fn listener_address_matches(row: &PortRow, expected: &str) -> bool {
    let Ok(actual) = row.port.local_addr.parse::<std::net::SocketAddr>() else {
        return false;
    };
    if expected.eq_ignore_ascii_case("localhost") {
        return actual.ip().is_loopback();
    }
    expected
        .parse::<std::net::IpAddr>()
        .is_ok_and(|expected| actual.ip() == expected)
}

fn run_source_matches(
    row: &PortRow,
    target: PortBindingTargetKind,
    target_distro: Option<&str>,
) -> bool {
    match (target, row.source) {
        (PortBindingTargetKind::Windows, ListenerSource::Windows) => true,
        (PortBindingTargetKind::Wsl, ListenerSource::Wsl) => {
            target_distro.is_some_and(|expected| {
                row.wsl_distro
                    .as_deref()
                    .is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
            })
        }
        _ => false,
    }
}

fn verified_windows_process(row: &PortRow, expected: Option<&PortBindingProcess>) -> bool {
    let (Some(expected), Some(ListenerIdentity::Windows { pid, start_time })) =
        (expected, row.identity.as_ref())
    else {
        return false;
    };
    let Some(created_at_ms) = windows_filetime_epoch_ms(start_time) else {
        return false;
    };
    *pid == expected.pid && created_at_ms == expected.created_at_ms
}

fn windows_filetime_epoch_ms(value: &str) -> Option<u64> {
    const WINDOWS_TO_UNIX_EPOCH_100NS: u64 = 11_644_473_600 * 10_000_000;
    value
        .parse::<u64>()
        .ok()?
        .checked_sub(WINDOWS_TO_UNIX_EPOCH_100NS)
        .map(|ticks| ticks / 10_000)
}

#[allow(clippy::too_many_arguments)]
fn resolved_correlation(
    row: &PortRow,
    source: &BindingSource,
    target_kind: &str,
    target_id: &str,
    label: &str,
    confidence: CorrelationConfidence,
    logs_available: bool,
    run_id: Option<String>,
) -> Option<ResolvedCorrelation> {
    let canonical = serde_json::to_string(&serde_json::json!({
        "producer": source.producer,
        "endpoint": row.endpoint(),
        "identity": row.identity,
        "targetKind": target_kind,
        "targetId": target_id,
        "runId": run_id,
    }))
    .ok()?;
    let action_key = devbox_integration::opaque_identity("port-action", &canonical).ok()?;
    Some(ResolvedCorrelation {
        public: PortCorrelation {
            source_app: source.producer.into(),
            target_kind: target_kind.into(),
            target_id: target_id.into(),
            label: label.into(),
            confidence,
            action_key,
            logs_available,
        },
        run_id,
    })
}

fn correlation_sort_key(correlation: &ResolvedCorrelation) -> (u8, &str, &str, &str) {
    let confidence = match correlation.public.confidence {
        CorrelationConfidence::Verified => 0,
        CorrelationConfidence::Declared => 1,
        CorrelationConfidence::Expected => 2,
    };
    (
        confidence,
        &correlation.public.source_app,
        &correlation.public.target_kind,
        &correlation.public.target_id,
    )
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use devbox_process::PortInfo;

    fn row(source: ListenerSource, identity: Option<ListenerIdentity>) -> PortRow {
        PortRow {
            port: PortInfo {
                proto: "TCP".into(),
                local_addr: "127.0.0.1:8080".into(),
                port: 8080,
                state: "LISTENING".into(),
                pid: Some(42),
            },
            process_name: Some("api.exe".into()),
            source,
            command_line: None,
            executable_path: None,
            process_start_time: None,
            wsl_distro: (source == ListenerSource::Wsl).then(|| "Ubuntu".into()),
            wsl_start_tick: None,
            container_engine: None,
            container_id: None,
            container_name: None,
            identity,
        }
    }

    fn filetime(epoch_ms: u64) -> String {
        const OFFSET: u64 = 11_644_473_600 * 10_000_000;
        (OFFSET + epoch_ms * 10_000).to_string()
    }

    fn write(root: &Path, producer: &str, entries: Vec<PortBindingEntry>) {
        let envelope =
            devbox_integration::port_bindings_envelope(producer, "0.5.1", entries).unwrap();
        devbox_integration::write_named_view_snapshot_atomic(
            &envelope,
            root,
            devbox_integration::PORT_BINDINGS_VIEW_KIND,
        )
        .unwrap();
    }

    #[test]
    fn exact_windows_identity_is_verified_and_mismatch_is_only_declared() {
        let root = tempfile::tempdir().unwrap();
        let created_at_ms = now_ms();
        write(
            root.path(),
            RUN_MANAGER,
            vec![PortBindingEntry::RunService {
                id: "service-1".into(),
                label: "API".into(),
                address: "127.0.0.1".into(),
                port: 8080,
                target_kind: PortBindingTargetKind::Windows,
                target_distro: None,
                run_id: Some("run-1".into()),
                logs_available: true,
                process: Some(PortBindingProcess {
                    pid: 42,
                    created_at_ms,
                }),
            }],
        );
        let exact = row(
            ListenerSource::Windows,
            Some(ListenerIdentity::Windows {
                pid: 42,
                start_time: filetime(created_at_ms),
            }),
        );
        let mismatch = row(
            ListenerSource::Windows,
            Some(ListenerIdentity::Windows {
                pid: 43,
                start_time: filetime(created_at_ms),
            }),
        );
        let snapshot = observe_rows_in(root.path(), vec![exact, mismatch], now_ms());
        assert_eq!(
            snapshot.rows[0].correlations[0].confidence,
            CorrelationConfidence::Verified
        );
        assert_eq!(
            snapshot.rows[1].correlations[0].confidence,
            CorrelationConfidence::Declared
        );
        assert!(valid_action_key(
            &snapshot.rows[0].correlations[0].action_key
        ));
    }

    #[test]
    fn run_declaration_requires_the_configured_loopback_address() {
        let source = BindingSource {
            producer: RUN_MANAGER,
            entries: vec![PortBindingEntry::RunService {
                id: "service-1".into(),
                label: "API".into(),
                address: "127.0.0.1".into(),
                port: 8080,
                target_kind: PortBindingTargetKind::Windows,
                target_distro: None,
                run_id: None,
                logs_available: false,
                process: None,
            }],
        };
        let exact = row(ListenerSource::Windows, None);
        assert_eq!(correlations_for_row(&exact, &source).len(), 1);

        let mut wildcard = exact.clone();
        wildcard.port.local_addr = "0.0.0.0:8080".into();
        assert!(correlations_for_row(&wildcard, &source).is_empty());

        let mut localhost = source.clone();
        let PortBindingEntry::RunService { address, .. } = &mut localhost.entries[0] else {
            panic!("run service entry");
        };
        *address = "localhost".into();
        let mut ipv6 = exact;
        ipv6.port.local_addr = "[::1]:8080".into();
        assert_eq!(correlations_for_row(&ipv6, &localhost).len(), 1);
    }

    #[test]
    fn producers_are_isolated_and_expected_never_becomes_ownership() {
        let root = tempfile::tempdir().unwrap();
        write(
            root.path(),
            WORKBENCH,
            vec![PortBindingEntry::WorkbenchProfile {
                id: "profile-1".into(),
                label: "Frontend".into(),
                port: 8080,
            }],
        );
        let run_path = devbox_integration::named_view_snapshot_path_in(
            root.path(),
            RUN_MANAGER,
            1,
            devbox_integration::PORT_BINDINGS_VIEW_KIND,
        )
        .unwrap();
        std::fs::create_dir_all(run_path.parent().unwrap()).unwrap();
        std::fs::write(run_path, b"{broken").unwrap();

        let snapshot = observe_rows_in(
            root.path(),
            vec![row(ListenerSource::Windows, None)],
            now_ms(),
        );
        assert_eq!(snapshot.sources[0].state, SnapshotSourceState::Invalid);
        assert_eq!(snapshot.sources[1].state, SnapshotSourceState::Available);
        assert_eq!(snapshot.rows[0].correlations.len(), 1);
        assert_eq!(
            snapshot.rows[0].correlations[0].confidence,
            CorrelationConfidence::Expected
        );
        assert!(!snapshot.rows[0].correlations[0].logs_available);
    }

    #[test]
    fn observation_bounds_same_port_correlations_and_reports_truncation() {
        let root = tempfile::tempdir().unwrap();
        let entries = (0..=MAX_CORRELATIONS_PER_ROW)
            .map(|index| PortBindingEntry::WorkbenchProfile {
                id: format!("profile-{index:03}"),
                label: format!("Profile {index:03}"),
                port: 8080,
            })
            .collect();
        write(root.path(), WORKBENCH, entries);

        let snapshot = observe_rows_in(
            root.path(),
            vec![row(ListenerSource::Windows, None)],
            now_ms(),
        );
        assert_eq!(
            snapshot.rows[0].correlations.len(),
            MAX_CORRELATIONS_PER_ROW
        );
        assert!(snapshot.correlations_truncated);
        assert_eq!(snapshot.rows[0].correlations[0].target_id, "profile-000");
    }

    #[test]
    fn action_key_changes_with_listener_identity_and_is_stable_across_heartbeat() {
        let source = BindingSource {
            producer: RUN_MANAGER,
            entries: vec![],
        };
        let first = row(
            ListenerSource::Windows,
            Some(ListenerIdentity::Windows {
                pid: 42,
                start_time: filetime(1_000),
            }),
        );
        let mut moved = first.clone();
        moved.port.local_addr = "0.0.0.0:8080".into();
        let one = resolved_correlation(
            &first,
            &source,
            "task",
            "service-1",
            "API",
            CorrelationConfidence::Declared,
            false,
            None,
        )
        .unwrap();
        let two = resolved_correlation(
            &moved,
            &source,
            "task",
            "service-1",
            "API",
            CorrelationConfidence::Declared,
            false,
            None,
        )
        .unwrap();
        assert_ne!(one.public.action_key, two.public.action_key);

        let same_after_heartbeat = resolved_correlation(
            &first,
            &source,
            "task",
            "service-1",
            "API",
            CorrelationConfidence::Declared,
            false,
            None,
        )
        .unwrap();
        assert_eq!(
            one.public.action_key,
            same_after_heartbeat.public.action_key
        );
    }

    #[test]
    fn filetime_conversion_matches_run_manager_epoch_identity() {
        let epoch = 1_725_000_000_123;
        assert_eq!(windows_filetime_epoch_ms(&filetime(epoch)), Some(epoch));
        assert_eq!(windows_filetime_epoch_ms("0"), None);
        assert_eq!(windows_filetime_epoch_ms("not-a-number"), None);
    }
}
