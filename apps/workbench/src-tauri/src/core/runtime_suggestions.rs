//! Read-only consumer for `wsl-desktop/runtime/v1`.
//!
//! The consumer never runs WSL/Docker commands and never opens Workbench's
//! profile store. It validates the complete versioned view before returning a
//! bounded, deterministic set of published host-port suggestions.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const PRODUCER: &str = "wsl-desktop";
const SNAPSHOT_VERSION: u32 = 1;
const VIEW_KIND: &str = "runtime";
const VIEW_VERSION: u32 = 1;
const SOURCE_LABEL: &str = "WSL Desktop runtime/v1";

pub const FRESH_MAX_MS: u64 = 2 * 60 * 1_000;
pub const EXPIRED_AFTER_MS: u64 = 15 * 60 * 1_000;

const MAX_DISTROS: usize = 64;
const MAX_DISTRO_NAME_BYTES: usize = 128;
const MAX_CONTAINERS_PER_DISTRO: usize = 256;
const MAX_CONTAINERS_TOTAL: usize = 512;
const MAX_CONTAINER_ID_BYTES: usize = 64;
const MAX_CONTAINER_NAME_BYTES: usize = 256;
const MAX_MAPPINGS_PER_CONTAINER: usize = 32;
const MAX_MAPPINGS_TOTAL: usize = 1_024;
const MAX_TERMINALS_PER_DISTRO: u16 = 256;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeSuggestionStatus {
    Fresh,
    Stale,
    Expired,
    Missing,
    Corrupt,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimePortSource {
    pub distro: String,
    pub container: String,
    pub container_state: String,
    pub target: u16,
    pub protocol: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimePortSuggestion {
    pub published: u16,
    pub sources: Vec<RuntimePortSource>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSuggestions {
    pub source: &'static str,
    pub status: RuntimeSuggestionStatus,
    pub producer_version: Option<String>,
    pub freshness_ms: Option<u32>,
    pub ports: Vec<RuntimePortSuggestion>,
}

impl RuntimeSuggestions {
    fn unavailable(status: RuntimeSuggestionStatus) -> Self {
        Self {
            source: SOURCE_LABEL,
            status,
            producer_version: None,
            freshness_ms: None,
            ports: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RuntimeDistroEntry {
    id: String,
    name: String,
    state: String,
    terminal_count: u16,
    docker_availability: DockerAvailability,
    containers: Vec<RuntimeContainer>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum DockerAvailability {
    Available,
    Missing,
    Error,
    NotQueried,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RuntimeContainer {
    id: String,
    name: String,
    state: String,
    port_mappings: Vec<PortMapping>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PortMapping {
    published: u16,
    target: u16,
    protocol: String,
}

/// Tauri command entry point. Missing and corrupt producers are normal,
/// distinguishable read states rather than raw IPC failures.
#[tauri::command]
pub fn wsl_runtime_suggestions() -> RuntimeSuggestions {
    read_runtime_suggestions_in(&devbox_integration::integration_root())
}

pub fn read_runtime_suggestions_in(root: &Path) -> RuntimeSuggestions {
    let discovery = devbox_integration::discover_report_in(root);
    if discovery.root_error.is_some() {
        return RuntimeSuggestions::unavailable(RuntimeSuggestionStatus::Corrupt);
    }
    let Some(reference) = discovery
        .snapshots
        .iter()
        .find(|snapshot| snapshot.producer == PRODUCER && snapshot.version == SNAPSHOT_VERSION)
    else {
        let corrupt = discovery.issues.iter().any(|issue| {
            issue.producer == PRODUCER
                && issue
                    .version
                    .is_none_or(|version| version == SNAPSHOT_VERSION)
        });
        return RuntimeSuggestions::unavailable(if corrupt {
            RuntimeSuggestionStatus::Corrupt
        } else {
            RuntimeSuggestionStatus::Missing
        });
    };
    let Some(view_reference) = reference.views.iter().find(|view| view.kind == VIEW_KIND) else {
        return RuntimeSuggestions::unavailable(RuntimeSuggestionStatus::Corrupt);
    };
    if view_reference.schema_version != VIEW_VERSION {
        return RuntimeSuggestions::unavailable(RuntimeSuggestionStatus::Corrupt);
    }

    let envelope = match devbox_integration::read_snapshot_in(root, PRODUCER, SNAPSHOT_VERSION) {
        Ok(Some(envelope)) => envelope,
        Ok(None) | Err(_) => {
            return RuntimeSuggestions::unavailable(RuntimeSuggestionStatus::Corrupt)
        }
    };
    let views = match envelope.views() {
        Ok(views) => views,
        Err(_) => return RuntimeSuggestions::unavailable(RuntimeSuggestionStatus::Corrupt),
    };
    let Some(view) = views.get(VIEW_KIND) else {
        return RuntimeSuggestions::unavailable(RuntimeSuggestionStatus::Corrupt);
    };
    if view.schema_version != VIEW_VERSION || view.entries.len() > MAX_DISTROS {
        return RuntimeSuggestions::unavailable(RuntimeSuggestionStatus::Corrupt);
    }

    let entries = view
        .entries
        .iter()
        .cloned()
        .map(serde_json::from_value::<RuntimeDistroEntry>)
        .collect::<Result<Vec<_>, _>>();
    let Ok(entries) = entries else {
        return RuntimeSuggestions::unavailable(RuntimeSuggestionStatus::Corrupt);
    };
    let ports = match validate_and_collect(entries) {
        Some(ports) => ports,
        None => return RuntimeSuggestions::unavailable(RuntimeSuggestionStatus::Corrupt),
    };

    let freshness = view_reference.freshness_ms;
    let status = if freshness <= FRESH_MAX_MS {
        RuntimeSuggestionStatus::Fresh
    } else if freshness <= EXPIRED_AFTER_MS {
        RuntimeSuggestionStatus::Stale
    } else {
        RuntimeSuggestionStatus::Expired
    };
    RuntimeSuggestions {
        source: SOURCE_LABEL,
        status,
        producer_version: Some(reference.producer_version.clone()),
        freshness_ms: Some(freshness.min(u64::from(u32::MAX)) as u32),
        ports,
    }
}

fn validate_and_collect(entries: Vec<RuntimeDistroEntry>) -> Option<Vec<RuntimePortSuggestion>> {
    let mut distro_ids = BTreeSet::new();
    let mut container_count = 0usize;
    let mut mapping_count = 0usize;
    let mut by_port: BTreeMap<u16, BTreeSet<RuntimePortSourceKey>> = BTreeMap::new();

    for entry in entries {
        if entry.id != entry.name
            || entry.state != "running"
            || entry.terminal_count > MAX_TERMINALS_PER_DISTRO
            || !safe_distro_name(&entry.id)
            || !distro_ids.insert(entry.id.clone())
            || entry.containers.len() > MAX_CONTAINERS_PER_DISTRO
            || (entry.docker_availability != DockerAvailability::Available
                && !entry.containers.is_empty())
        {
            return None;
        }
        container_count = container_count.checked_add(entry.containers.len())?;
        if container_count > MAX_CONTAINERS_TOTAL {
            return None;
        }

        let mut container_ids = BTreeSet::new();
        for container in entry.containers {
            if !safe_container_id(&container.id)
                || !container_ids.insert(container.id)
                || !safe_container_name(&container.name)
                || !matches!(
                    container.state.as_str(),
                    "created"
                        | "dead"
                        | "exited"
                        | "paused"
                        | "removing"
                        | "restarting"
                        | "running"
                        | "unknown"
                )
                || container.port_mappings.len() > MAX_MAPPINGS_PER_CONTAINER
            {
                return None;
            }
            mapping_count = mapping_count.checked_add(container.port_mappings.len())?;
            if mapping_count > MAX_MAPPINGS_TOTAL {
                return None;
            }
            for mapping in container.port_mappings {
                if mapping.published == 0
                    || mapping.target == 0
                    || !matches!(mapping.protocol.as_str(), "tcp" | "udp" | "sctp")
                {
                    return None;
                }
                // ProjectProfile.expected_ports is a TCP health/start check.
                // UDP/SCTP mappings remain valid snapshot data but cannot be
                // represented faithfully in that destination field.
                if mapping.protocol != "tcp" {
                    continue;
                }
                by_port
                    .entry(mapping.published)
                    .or_default()
                    .insert(RuntimePortSourceKey {
                        distro: entry.name.clone(),
                        container: container.name.clone(),
                        container_state: container.state.clone(),
                        target: mapping.target,
                        protocol: mapping.protocol,
                    });
            }
        }
    }

    Some(
        by_port
            .into_iter()
            .map(|(published, sources)| RuntimePortSuggestion {
                published,
                sources: sources.into_iter().map(Into::into).collect(),
            })
            .collect(),
    )
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RuntimePortSourceKey {
    distro: String,
    container: String,
    container_state: String,
    target: u16,
    protocol: String,
}

impl From<RuntimePortSourceKey> for RuntimePortSource {
    fn from(value: RuntimePortSourceKey) -> Self {
        Self {
            distro: value.distro,
            container: value.container,
            container_state: value.container_state,
            target: value.target,
            protocol: value.protocol,
        }
    }
}

fn safe_distro_name(value: &str) -> bool {
    value == value.trim()
        && !value.is_empty()
        && value.len() <= MAX_DISTRO_NAME_BYTES
        && !value.contains('/')
        && !matches!(value, "." | "..")
        && value.chars().all(|character| !character.is_control())
        && devbox_wsl::distro::validate_distro_name(value).is_ok()
}

fn safe_container_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_CONTAINER_ID_BYTES
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        && value == value.to_ascii_lowercase()
}

fn safe_container_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_CONTAINER_NAME_BYTES
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
        && !looks_sensitive(value)
}

fn looks_sensitive(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    lower.starts_with("bearer ")
        || lower.starts_with("basic ")
        || lower.starts_with("sk-")
        || lower.starts_with("ghp_")
        || lower.starts_with("github_pat_")
        || lower.contains("password")
        || lower.contains("secret")
        || lower.contains("token")
        || lower.contains("authorization")
        || lower.contains("cookie")
        || lower.contains("credential")
        || lower.contains("api_key")
        || lower.contains("apikey")
}

#[cfg(test)]
mod tests {
    use super::*;
    use devbox_integration::{Envelope, SnapshotView, SnapshotViews};
    use serde_json::{json, Value};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new(label: &str) -> Self {
            let id = NEXT_ROOT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "devbox-workbench-runtime-suggestions-{label}-{}-{id}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            Self(path)
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn container(id: &str, name: &str, state: &str, mappings: Value) -> Value {
        json!({
            "id": id,
            "name": name,
            "state": state,
            "portMappings": mappings,
        })
    }

    fn distro(id: &str, containers: Vec<Value>) -> Value {
        json!({
            "id": id,
            "name": id,
            "state": "running",
            "terminalCount": 1,
            "dockerAvailability": "available",
            "containers": containers,
        })
    }

    fn write_snapshot(root: &Path, freshness_ms: u64, entries: Vec<Value>) {
        let mut views = SnapshotViews::new();
        views.insert(
            VIEW_KIND.to_owned(),
            SnapshotView {
                schema_version: VIEW_VERSION,
                freshness_ms,
                entries,
            },
        );
        let envelope = Envelope::with_views(PRODUCER, "0.2.1", views);
        devbox_integration::write_atomic(
            &envelope,
            &devbox_integration::snapshot_dir_in(root, PRODUCER, SNAPSHOT_VERSION),
        )
        .unwrap();
    }

    #[test]
    fn distinguishes_fresh_stale_expired_and_missing_snapshots() {
        let missing = TestRoot::new("missing");
        assert_eq!(
            read_runtime_suggestions_in(&missing.0).status,
            RuntimeSuggestionStatus::Missing
        );

        let fresh = TestRoot::new("fresh");
        write_snapshot(&fresh.0, 0, vec![]);
        assert_eq!(
            read_runtime_suggestions_in(&fresh.0).status,
            RuntimeSuggestionStatus::Fresh
        );

        let stale = TestRoot::new("stale");
        write_snapshot(&stale.0, FRESH_MAX_MS + 10_000, vec![]);
        assert_eq!(
            read_runtime_suggestions_in(&stale.0).status,
            RuntimeSuggestionStatus::Stale
        );

        let expired = TestRoot::new("expired");
        write_snapshot(&expired.0, EXPIRED_AFTER_MS + 10_000, vec![]);
        assert_eq!(
            read_runtime_suggestions_in(&expired.0).status,
            RuntimeSuggestionStatus::Expired
        );
    }

    #[test]
    fn sorts_published_ports_and_deduplicates_their_sources() {
        let root = TestRoot::new("dedupe");
        let mapping_8080 = json!({ "published": 8080, "target": 80, "protocol": "tcp" });
        write_snapshot(
            &root.0,
            0,
            vec![
                distro(
                    "Ubuntu-Z",
                    vec![container(
                        "bbbb",
                        "web",
                        "running",
                        json!([
                            { "published": 9000, "target": 9000, "protocol": "tcp" },
                            mapping_8080.clone(),
                            mapping_8080.clone()
                        ]),
                    )],
                ),
                distro(
                    "Ubuntu-A",
                    vec![container("aaaa", "api", "exited", json!([mapping_8080]))],
                ),
            ],
        );

        let result = read_runtime_suggestions_in(&root.0);
        assert_eq!(result.status, RuntimeSuggestionStatus::Fresh);
        assert_eq!(
            result
                .ports
                .iter()
                .map(|port| port.published)
                .collect::<Vec<_>>(),
            vec![8080, 9000]
        );
        assert_eq!(result.ports[0].sources.len(), 2);
        assert_eq!(result.ports[0].sources[0].distro, "Ubuntu-A");
        assert_eq!(result.ports[0].sources[0].container_state, "exited");
        assert!(result.source.contains("runtime/v1"));
        assert_eq!(result.producer_version.as_deref(), Some("0.2.1"));
    }

    #[test]
    fn corrupt_or_future_payload_fails_closed_without_returning_raw_detail() {
        let root = TestRoot::new("future-field");
        let mut entry = distro(
            "Ubuntu",
            vec![container(
                "abcd",
                "api",
                "running",
                json!([{ "published": 8080, "target": 80, "protocol": "tcp" }]),
            )],
        );
        entry.as_object_mut().unwrap().insert(
            "rawDockerDetail".into(),
            Value::String("DO_NOT_RETURN_PRIVATE_DETAIL".into()),
        );
        write_snapshot(&root.0, 0, vec![entry]);

        let result = read_runtime_suggestions_in(&root.0);
        assert_eq!(result.status, RuntimeSuggestionStatus::Corrupt);
        assert!(result.ports.is_empty());
        assert!(result.producer_version.is_none());
        assert!(!format!("{result:?}").contains("DO_NOT_RETURN_PRIVATE_DETAIL"));
    }

    #[test]
    fn malformed_snapshot_is_corrupt_not_missing() {
        let root = TestRoot::new("malformed");
        let directory = devbox_integration::snapshot_dir_in(&root.0, PRODUCER, SNAPSHOT_VERSION);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("summary.json"), b"{not-json").unwrap();

        let result = read_runtime_suggestions_in(&root.0);
        assert_eq!(result.status, RuntimeSuggestionStatus::Corrupt);
        assert!(result.ports.is_empty());
    }

    #[test]
    fn invalid_port_or_container_contract_rejects_the_complete_view() {
        let root = TestRoot::new("invalid");
        write_snapshot(
            &root.0,
            0,
            vec![distro(
                "Ubuntu",
                vec![container(
                    "abcd",
                    "unsafe name",
                    "running",
                    json!([{ "published": 8080, "target": 80, "protocol": "tcp" }]),
                )],
            )],
        );
        assert_eq!(
            read_runtime_suggestions_in(&root.0).status,
            RuntimeSuggestionStatus::Corrupt
        );
    }

    #[test]
    fn omits_non_tcp_mappings_that_expected_ports_cannot_represent() {
        let root = TestRoot::new("non-tcp");
        write_snapshot(
            &root.0,
            0,
            vec![distro(
                "Ubuntu",
                vec![container(
                    "abcd",
                    "dns",
                    "running",
                    json!([
                        { "published": 5353, "target": 5353, "protocol": "udp" },
                        { "published": 8080, "target": 80, "protocol": "tcp" }
                    ]),
                )],
            )],
        );
        let result = read_runtime_suggestions_in(&root.0);
        assert_eq!(
            result
                .ports
                .iter()
                .map(|port| port.published)
                .collect::<Vec<_>>(),
            vec![8080]
        );
    }
}
