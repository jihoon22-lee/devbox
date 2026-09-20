use devbox_integration::{DiscoveryReport, SnapshotIssue, SnapshotRef};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

pub const LOCAL_QUALITY_SCHEMA_VERSION: u32 = 1;
pub const MAX_INSTALLATION_APPS: usize = 64;
pub const MAX_INTEGRATION_SNAPSHOTS: usize = 64;
pub const MAX_INTEGRATION_ISSUES: usize = 64;
pub const MAX_VIEWS_PER_SNAPSHOT: usize = 16;
pub const MAX_LOCAL_QUALITY_BYTES: usize = 256 * 1024;
const MAX_JAVASCRIPT_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogObservation {
    pub revision: u64,
    pub managed_app_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryRecordObservation {
    pub app_id: String,
    pub version: String,
    pub mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryObservation {
    pub revision: u64,
    pub records: Vec<RegistryRecordObservation>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LocalQualityStatus {
    Healthy,
    Attention,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SourceState {
    Ready,
    Unavailable,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum InstallationAppState {
    Installed,
    NotInstalled,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum IntegrationIssueKind {
    Invalid,
    Unreadable,
    Unsafe,
    LimitExceeded,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalQualitySnapshot {
    pub schema_version: u32,
    pub observed_at_ms: u64,
    pub mode: &'static str,
    pub status: LocalQualityStatus,
    pub installation: InstallationHealth,
    pub integration: IntegrationHealth,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstallationHealth {
    pub catalog_state: SourceState,
    pub registry_state: SourceState,
    pub catalog_revision: Option<u64>,
    pub registry_revision: Option<u64>,
    pub managed_app_count: usize,
    pub installed_app_count: Option<usize>,
    pub apps: Vec<InstallationAppHealth>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstallationAppHealth {
    pub app_id: String,
    pub state: InstallationAppState,
    pub version: Option<String>,
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationHealth {
    pub root_state: SourceState,
    pub root_issue: Option<IntegrationIssueKind>,
    pub snapshot_count: usize,
    pub issue_count: usize,
    pub snapshots: Vec<IntegrationSnapshotHealth>,
    pub issues: Vec<IntegrationIssueHealth>,
    pub snapshots_truncated: bool,
    pub issues_truncated: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationSnapshotHealth {
    pub producer: String,
    pub schema_version: u32,
    pub producer_version: String,
    pub freshness_ms: u64,
    pub views: Vec<IntegrationViewHealth>,
    pub views_truncated: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationViewHealth {
    pub kind: String,
    pub schema_version: u32,
    pub freshness_ms: u64,
    pub entry_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationIssueHealth {
    pub producer: String,
    pub schema_version: Option<u32>,
    pub kind: IntegrationIssueKind,
}

pub fn build_local_quality_snapshot(
    observed_at_ms: u64,
    catalog: Option<CatalogObservation>,
    registry: Option<RegistryObservation>,
    discovery: DiscoveryReport,
) -> LocalQualitySnapshot {
    let installation = build_installation_health(catalog, registry);
    let integration = build_integration_health(discovery);
    let status = if installation.catalog_state == SourceState::Ready
        && installation.registry_state == SourceState::Ready
        && !installation.truncated
        && integration.root_state == SourceState::Ready
        && integration.issue_count == 0
        && !integration.snapshots_truncated
        && !integration.issues_truncated
        && integration
            .snapshots
            .iter()
            .all(|snapshot| !snapshot.views_truncated)
    {
        LocalQualityStatus::Healthy
    } else {
        LocalQualityStatus::Attention
    };
    LocalQualitySnapshot {
        schema_version: LOCAL_QUALITY_SCHEMA_VERSION,
        observed_at_ms,
        mode: "local-only",
        status,
        installation,
        integration,
    }
}

fn build_installation_health(
    catalog: Option<CatalogObservation>,
    registry: Option<RegistryObservation>,
) -> InstallationHealth {
    let Some(catalog) = catalog else {
        return unavailable_installation_health();
    };
    let mut all_catalog_ids = HashSet::with_capacity(catalog.managed_app_ids.len());
    if catalog.revision == 0
        || catalog.revision > MAX_JAVASCRIPT_SAFE_INTEGER
        || catalog.managed_app_ids.is_empty()
        || catalog
            .managed_app_ids
            .iter()
            .any(|app_id| !valid_kebab_id(app_id) || !all_catalog_ids.insert(app_id.as_str()))
    {
        return unavailable_installation_health();
    }
    let managed_app_count = catalog.managed_app_ids.len();
    let truncated = managed_app_count > MAX_INSTALLATION_APPS;
    let unique_catalog_ids = catalog
        .managed_app_ids
        .iter()
        .take(MAX_INSTALLATION_APPS)
        .cloned()
        .collect::<HashSet<_>>();
    let registry_is_consistent = registry.as_ref().is_some_and(|observation| {
        let mut seen = HashSet::new();
        observation.revision > 0
            && observation.revision <= MAX_JAVASCRIPT_SAFE_INTEGER
            && observation.records.iter().all(|record| {
                unique_catalog_ids.contains(&record.app_id)
                    && seen.insert(record.app_id.as_str())
                    && matches!(record.mode.as_str(), "portable" | "installer")
                    && record.version.len() <= 64
                    && semver::Version::parse(&record.version).is_ok()
            })
    });
    let usable_registry = registry.filter(|_| registry_is_consistent && !truncated);
    let records = usable_registry
        .as_ref()
        .map(|observation| {
            observation
                .records
                .iter()
                .map(|record| (record.app_id.as_str(), record))
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();
    let apps = catalog
        .managed_app_ids
        .into_iter()
        .take(MAX_INSTALLATION_APPS)
        .map(|app_id| {
            let record = records.get(app_id.as_str());
            let state = if usable_registry.is_none() {
                InstallationAppState::Unknown
            } else if record.is_some() {
                InstallationAppState::Installed
            } else {
                InstallationAppState::NotInstalled
            };
            InstallationAppHealth {
                app_id,
                state,
                version: record.map(|record| record.version.clone()),
                mode: record.map(|record| record.mode.clone()),
            }
        })
        .collect();
    InstallationHealth {
        catalog_state: SourceState::Ready,
        registry_state: if usable_registry.is_some() {
            SourceState::Ready
        } else {
            SourceState::Unavailable
        },
        catalog_revision: Some(catalog.revision),
        registry_revision: usable_registry
            .as_ref()
            .map(|observation| observation.revision),
        managed_app_count,
        installed_app_count: usable_registry
            .as_ref()
            .map(|observation| observation.records.len()),
        apps,
        truncated,
    }
}

fn unavailable_installation_health() -> InstallationHealth {
    InstallationHealth {
        catalog_state: SourceState::Unavailable,
        registry_state: SourceState::Unavailable,
        catalog_revision: None,
        registry_revision: None,
        managed_app_count: 0,
        installed_app_count: None,
        apps: Vec::new(),
        truncated: false,
    }
}

fn valid_kebab_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("--")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn build_integration_health(discovery: DiscoveryReport) -> IntegrationHealth {
    let root_issue = discovery.root_error.as_deref().map(classify_issue);
    let snapshot_count = discovery.snapshots.len();
    let issue_count = discovery.issues.len();
    let snapshots_truncated = snapshot_count > MAX_INTEGRATION_SNAPSHOTS;
    let issues_truncated = issue_count > MAX_INTEGRATION_ISSUES;
    let snapshots = discovery
        .snapshots
        .into_iter()
        .take(MAX_INTEGRATION_SNAPSHOTS)
        .map(snapshot_health)
        .collect();
    let issues = discovery
        .issues
        .into_iter()
        .take(MAX_INTEGRATION_ISSUES)
        .map(issue_health)
        .collect();
    IntegrationHealth {
        root_state: if root_issue.is_some() {
            SourceState::Unavailable
        } else {
            SourceState::Ready
        },
        root_issue,
        snapshot_count,
        issue_count,
        snapshots,
        issues,
        snapshots_truncated,
        issues_truncated,
    }
}

fn snapshot_health(snapshot: SnapshotRef) -> IntegrationSnapshotHealth {
    let views_truncated = snapshot.views.len() > MAX_VIEWS_PER_SNAPSHOT;
    let views = snapshot
        .views
        .into_iter()
        .take(MAX_VIEWS_PER_SNAPSHOT)
        .map(|view| IntegrationViewHealth {
            kind: view.kind,
            schema_version: view.schema_version,
            freshness_ms: view.freshness_ms.min(MAX_JAVASCRIPT_SAFE_INTEGER),
            entry_count: view.entry_count,
        })
        .collect();
    IntegrationSnapshotHealth {
        producer: snapshot.producer,
        schema_version: snapshot.version,
        producer_version: snapshot.producer_version,
        freshness_ms: snapshot.freshness_ms.min(MAX_JAVASCRIPT_SAFE_INTEGER),
        views,
        views_truncated,
    }
}

fn issue_health(issue: SnapshotIssue) -> IntegrationIssueHealth {
    IntegrationIssueHealth {
        producer: issue.producer,
        schema_version: issue.version,
        kind: classify_issue(&issue.error),
    }
}

fn classify_issue(error: &str) -> IntegrationIssueKind {
    if error.contains("크기 제한") || error.contains("항목 제한") || error.contains("중첩 제한")
    {
        IntegrationIssueKind::LimitExceeded
    } else if error.contains("안전하지")
        || error.contains("안전하게")
        || error.contains("symbolic link")
        || error.contains("reparse point")
        || error.contains("읽는 동안 변경")
        || error.contains("민감 정보")
    {
        IntegrationIssueKind::Unsafe
    } else if error.contains("읽을 수 없") || error.contains("확인할 수 없") {
        IntegrationIssueKind::Unreadable
    } else {
        IntegrationIssueKind::Invalid
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use devbox_integration::{SnapshotIssue, SnapshotRef, SnapshotViewRef};
    use std::path::PathBuf;

    fn catalog() -> CatalogObservation {
        CatalogObservation {
            revision: 17,
            managed_app_ids: vec!["code-pad".into(), "life-log".into()],
        }
    }

    fn registry() -> RegistryObservation {
        RegistryObservation {
            revision: 8,
            records: vec![RegistryRecordObservation {
                app_id: "code-pad".into(),
                version: "0.5.0".into(),
                mode: "portable".into(),
            }],
        }
    }

    #[test]
    fn builds_a_path_free_local_only_snapshot() {
        let report = DiscoveryReport {
            snapshots: vec![SnapshotRef {
                producer: "life-log".into(),
                version: 1,
                producer_version: "0.5.0".into(),
                generated_at: "2026-08-31T00:00:00Z".into(),
                path: PathBuf::from("C:\\Users\\private\\integration\\summary.json"),
                freshness_ms: 1200,
                views: vec![SnapshotViewRef {
                    kind: "daily-activity".into(),
                    schema_version: 1,
                    freshness_ms: 1200,
                    entry_count: 7,
                }],
            }],
            issues: Vec::new(),
            root_error: None,
        };
        let snapshot =
            build_local_quality_snapshot(1_000, Some(catalog()), Some(registry()), report);
        assert_eq!(snapshot.status, LocalQualityStatus::Healthy);
        assert_eq!(snapshot.installation.installed_app_count, Some(1));
        assert_eq!(snapshot.integration.snapshots[0].producer, "life-log");
        let encoded = serde_json::to_string(&snapshot).unwrap();
        assert!(!encoded.contains("Users"));
        assert!(!encoded.contains("private"));
        assert!(!encoded.contains("summary.json"));
        assert!(encoded.len() <= MAX_LOCAL_QUALITY_BYTES);
    }

    #[test]
    fn unavailable_registry_never_claims_apps_are_not_installed() {
        let snapshot =
            build_local_quality_snapshot(1_000, Some(catalog()), None, DiscoveryReport::default());
        assert_eq!(snapshot.status, LocalQualityStatus::Attention);
        assert_eq!(
            snapshot.installation.registry_state,
            SourceState::Unavailable
        );
        assert_eq!(snapshot.installation.installed_app_count, None);
        assert!(snapshot
            .installation
            .apps
            .iter()
            .all(|app| app.state == InstallationAppState::Unknown));
    }

    #[test]
    fn inconsistent_registry_fails_closed_without_reflecting_records() {
        let snapshot = build_local_quality_snapshot(
            1_000,
            Some(catalog()),
            Some(RegistryObservation {
                revision: 8,
                records: vec![RegistryRecordObservation {
                    app_id: "unknown-app".into(),
                    version: "TOP_SECRET_PATH".into(),
                    mode: "portable".into(),
                }],
            }),
            DiscoveryReport::default(),
        );
        assert_eq!(
            snapshot.installation.registry_state,
            SourceState::Unavailable
        );
        let encoded = serde_json::to_string(&snapshot).unwrap();
        assert!(!encoded.contains("unknown-app"));
        assert!(!encoded.contains("TOP_SECRET_PATH"));
    }

    #[test]
    fn empty_managed_catalog_fails_closed() {
        let snapshot = build_local_quality_snapshot(
            1_000,
            Some(CatalogObservation {
                revision: 17,
                managed_app_ids: Vec::new(),
            }),
            Some(RegistryObservation {
                revision: 8,
                records: Vec::new(),
            }),
            DiscoveryReport::default(),
        );
        assert_eq!(snapshot.status, LocalQualityStatus::Attention);
        assert_eq!(
            snapshot.installation.catalog_state,
            SourceState::Unavailable
        );
        assert_eq!(
            snapshot.installation.registry_state,
            SourceState::Unavailable
        );
    }

    #[test]
    fn invalid_known_app_version_is_not_reflected() {
        let secret = "C:\\Users\\private\\registry-entry";
        let snapshot = build_local_quality_snapshot(
            1_000,
            Some(catalog()),
            Some(RegistryObservation {
                revision: 8,
                records: vec![RegistryRecordObservation {
                    app_id: "code-pad".into(),
                    version: secret.into(),
                    mode: "portable".into(),
                }],
            }),
            DiscoveryReport::default(),
        );
        assert_eq!(
            snapshot.installation.registry_state,
            SourceState::Unavailable
        );
        assert!(snapshot
            .installation
            .apps
            .iter()
            .all(|app| app.state == InstallationAppState::Unknown));
        assert!(!serde_json::to_string(&snapshot).unwrap().contains(secret));
    }

    #[test]
    fn integration_errors_are_classified_without_echoing_native_text() {
        let secret = "C:\\Users\\private\\secret.json";
        let snapshot = build_local_quality_snapshot(
            1_000,
            Some(catalog()),
            Some(registry()),
            DiscoveryReport {
                snapshots: Vec::new(),
                issues: vec![SnapshotIssue {
                    producer: "life-log".into(),
                    version: Some(1),
                    error: format!("snapshot 파일을 읽을 수 없습니다: {secret}"),
                }],
                root_error: None,
            },
        );
        assert_eq!(
            snapshot.integration.issues[0].kind,
            IntegrationIssueKind::Unreadable
        );
        assert!(!serde_json::to_string(&snapshot).unwrap().contains(secret));
    }

    #[test]
    fn integration_root_error_is_coarse_and_does_not_echo_native_text() {
        let secret = "C:\\Users\\private\\integration";
        let snapshot = build_local_quality_snapshot(
            1_000,
            Some(catalog()),
            Some(registry()),
            DiscoveryReport {
                snapshots: Vec::new(),
                issues: Vec::new(),
                root_error: Some(format!(
                    "integration root를 안전하게 읽을 수 없습니다: {secret}"
                )),
            },
        );
        assert_eq!(snapshot.status, LocalQualityStatus::Attention);
        assert_eq!(snapshot.integration.root_state, SourceState::Unavailable);
        assert_eq!(
            snapshot.integration.root_issue,
            Some(IntegrationIssueKind::Unsafe)
        );
        assert!(!serde_json::to_string(&snapshot).unwrap().contains(secret));
    }

    #[test]
    fn output_collections_are_strictly_bounded_and_report_truncation() {
        let managed_app_ids = (0..MAX_INSTALLATION_APPS + 1)
            .map(|index| format!("a{index:02}-{}", "a".repeat(60)))
            .collect::<Vec<_>>();
        let snapshots = (0..MAX_INTEGRATION_SNAPSHOTS + 1)
            .map(|index| SnapshotRef {
                producer: format!("p{index:02}-{}", "p".repeat(60)),
                version: 1,
                producer_version: format!("1.0.0+{}", "b".repeat(58)),
                generated_at: "2026-08-31T00:00:00Z".into(),
                path: PathBuf::from(format!("/private/{index}")),
                freshness_ms: 0,
                views: (0..MAX_VIEWS_PER_SNAPSHOT + 1)
                    .map(|view| SnapshotViewRef {
                        kind: format!("v{view:02}-{}", "v".repeat(60)),
                        schema_version: 1,
                        freshness_ms: 0,
                        entry_count: 0,
                    })
                    .collect(),
            })
            .collect();
        let issues = (0..MAX_INTEGRATION_ISSUES + 1)
            .map(|index| SnapshotIssue {
                producer: format!("i{index:02}-{}", "i".repeat(60)),
                version: Some(1),
                error: "snapshot 형식이 올바르지 않습니다".into(),
            })
            .collect();
        let snapshot = build_local_quality_snapshot(
            1_000,
            Some(CatalogObservation {
                revision: 17,
                managed_app_ids,
            }),
            None,
            DiscoveryReport {
                snapshots,
                issues,
                root_error: None,
            },
        );
        assert_eq!(snapshot.installation.apps.len(), MAX_INSTALLATION_APPS);
        assert!(snapshot.installation.truncated);
        assert_eq!(
            snapshot.integration.snapshots.len(),
            MAX_INTEGRATION_SNAPSHOTS
        );
        assert_eq!(snapshot.integration.issues.len(), MAX_INTEGRATION_ISSUES);
        assert!(snapshot.integration.snapshots_truncated);
        assert!(snapshot.integration.issues_truncated);
        assert!(snapshot.integration.snapshots[0].views_truncated);
        assert_eq!(
            snapshot.integration.snapshots[0].views.len(),
            MAX_VIEWS_PER_SNAPSHOT
        );
        assert!(serde_json::to_vec(&snapshot).unwrap().len() <= MAX_LOCAL_QUALITY_BYTES);
    }
}
