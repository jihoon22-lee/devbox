//! WSL Desktop runtime snapshot의 순수 모델·정규화·파서.
//!
//! 이 모듈은 프로세스를 실행하거나 파일을 쓰지 않는다. `runtime_snapshot` 모듈이
//! 고정된 WSL/Docker argv로 수집한 출력을 이 모듈에 넘기면, 여기서 공개 snapshot에
//! 넣을 수 있는 값만 검증·정규화한다. 원문 status/ports/image/command/environment와
//! terminal session identity는 이 경계를 통과하지 않는다.

use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;
pub const RUNTIME_VIEW_SCHEMA_VERSION: u32 = 1;

pub const MAX_DISTROS: usize = 64;
pub const MAX_DISTRO_NAME_BYTES: usize = 128;
pub const MAX_CONTAINERS_PER_DISTRO: usize = 256;
pub const MAX_CONTAINERS_TOTAL: usize = 512;
pub const MAX_CONTAINER_ID_BYTES: usize = 64;
pub const MAX_CONTAINER_NAME_BYTES: usize = 256;
pub const MAX_PORT_MAPPINGS_PER_CONTAINER: usize = 32;
pub const MAX_PORT_MAPPINGS_TOTAL: usize = 1_024;
pub const MAX_TERMINALS_PER_DISTRO: usize = 256;
pub const MAX_DOCKER_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
#[allow(dead_code)]
pub const MAX_OUTPUT_LINE_BYTES: usize = 16 * 1024;
pub const DASHBOARD_STALE_AFTER_MS: u64 = 30_000;

pub const RUNNING_STATE: &str = "running";

const ERR_DISTRO_LIST: &str = "실행 중인 WSL distro 목록 형식이 올바르지 않습니다";
const ERR_DOCKER_LIST: &str = "Docker 컨테이너 목록 형식이 올바르지 않습니다";
const ERR_RUNTIME_BOUNDS: &str = "WSL runtime snapshot 범위를 초과했습니다";
const ERR_RUNTIME_PRIVACY: &str = "WSL runtime snapshot에 안전하지 않은 값이 있습니다";

/// Snapshot에 공개하는 distro 하나. WSL에는 별도의 사용자-facing numeric id가
/// 없으므로 `id`는 검증된 등록 이름과 동일한 안정적인 key다.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDistroEntry {
    pub id: String,
    pub name: String,
    pub state: String,
    pub terminal_count: u16,
    pub docker_availability: DockerAvailability,
    pub containers: Vec<RuntimeContainer>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeContainer {
    pub id: String,
    pub name: String,
    pub state: String,
    pub port_mappings: Vec<PortMapping>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PortMapping {
    pub published: u16,
    pub target: u16,
    pub protocol: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DockerAvailability {
    Available,
    Missing,
    Error,
    NotQueried,
}

/// One complete dashboard view. Resource, Docker and terminal state are captured together so
/// the UI never combines a new resource reading with an older distro/session list.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DashboardDistro {
    pub name: String,
    pub version: u32,
    pub default: bool,
    pub state: String,
    pub terminal_count: u16,
    pub docker_availability: DockerAvailability,
    pub containers: Vec<crate::core::models::ContainerInfo>,
    pub resource: Option<crate::core::resources::ResourceSummary>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DashboardSnapshot {
    pub revision: u64,
    pub captured_at_ms: u64,
    pub stale_after_ms: u64,
    pub distros: Vec<DashboardDistro>,
}

/// A single distro's Docker result supplied by the process runner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerResult {
    pub availability: DockerAvailability,
    pub containers: Vec<RuntimeContainer>,
}

/// Strip the local dashboard's Docker detail fields down to the public runtime contract.
/// Resource/dashboard IPC may retain the detail needed by its disclosure UI, while the
/// integration snapshot must never copy image/status/host-path-like raw values.
pub fn sanitize_dashboard_containers(
    containers: &[crate::core::models::ContainerInfo],
) -> Result<Vec<RuntimeContainer>, &'static str> {
    if containers.len() > MAX_CONTAINERS_PER_DISTRO {
        return Err(ERR_RUNTIME_BOUNDS);
    }
    let mut ids = BTreeSet::new();
    let mut sanitized = Vec::with_capacity(containers.len());
    for container in containers {
        let id = normalize_container_id(&container.id).ok_or(ERR_DOCKER_LIST)?;
        if !ids.insert(id.clone()) {
            return Err(ERR_DOCKER_LIST);
        }
        let name = normalize_container_name(&container.name).ok_or(ERR_RUNTIME_PRIVACY)?;
        let state =
            normalize_container_state(container.status.split_whitespace().next().unwrap_or(""));
        let port_mappings = parse_port_mappings(&container.ports)?;
        sanitized.push(RuntimeContainer {
            id,
            name,
            state,
            port_mappings,
        });
    }
    sanitized.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(sanitized)
}

/// Parse the output of `wsl.exe --list --running --quiet`.
///
/// The parser accepts the optional default-distro marker for compatibility with WSL builds
/// that still emit it, trims only presentation whitespace, rejects unsafe/oversized names,
/// and returns a stable sorted unique list. Any structural or bound violation fails closed.
#[allow(dead_code)]
pub fn parse_running_distros(input: &str) -> Result<Vec<String>, &'static str> {
    if input.len() > MAX_DOCKER_OUTPUT_BYTES {
        return Err(ERR_RUNTIME_BOUNDS);
    }

    let mut names = BTreeSet::new();
    let mut lines = 0usize;

    for raw_line in input.lines() {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if line.len() > MAX_OUTPUT_LINE_BYTES {
            return Err(ERR_RUNTIME_BOUNDS);
        }
        let line = line.trim().trim_start_matches('\u{feff}').trim();
        if line.is_empty() {
            continue;
        }
        lines = lines.saturating_add(1);
        if lines > MAX_DISTROS {
            return Err(ERR_RUNTIME_BOUNDS);
        }

        let name = line.strip_prefix('*').unwrap_or(line).trim();
        if name.is_empty() || name.len() > MAX_DISTRO_NAME_BYTES {
            return Err(ERR_RUNTIME_BOUNDS);
        }
        if !is_safe_distro_name(name) {
            return Err(ERR_DISTRO_LIST);
        }
        if !names.insert(name.to_owned()) {
            // Duplicate rows can indicate a changed/ambiguous command contract. Do not
            // silently publish a snapshot whose terminal count could be assigned twice.
            return Err(ERR_DISTRO_LIST);
        }
    }

    if names.len() > MAX_DISTROS {
        return Err(ERR_RUNTIME_BOUNDS);
    }
    Ok(names.into_iter().collect())
}

/// Parse the bounded four-field Docker format used by the producer:
/// `{{.ID}}\t{{.Names}}\t{{.State}}\t{{.Ports}}`.
///
/// Only validated IDs/names, an enum-like state and normalized published mappings survive.
/// Exposed-only ports and malformed individual port tokens are omitted; a malformed row or
/// duplicate container ID aborts the complete collection so callers can preserve last-good.
#[allow(dead_code)]
pub fn parse_docker_snapshot(input: &str) -> Result<Vec<RuntimeContainer>, &'static str> {
    if input.len() > MAX_DOCKER_OUTPUT_BYTES {
        return Err(ERR_RUNTIME_BOUNDS);
    }

    let mut containers = Vec::new();
    let mut ids = BTreeSet::new();
    for raw_line in input.lines() {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if line.is_empty() {
            continue;
        }
        if line.len() > MAX_OUTPUT_LINE_BYTES {
            return Err(ERR_RUNTIME_BOUNDS);
        }
        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.len() != 4 {
            return Err(ERR_DOCKER_LIST);
        }

        let id = normalize_container_id(fields[0]).ok_or(ERR_DOCKER_LIST)?;
        let name = normalize_container_name(fields[1]).ok_or(ERR_RUNTIME_PRIVACY)?;
        if !ids.insert(id.clone()) {
            return Err(ERR_DOCKER_LIST);
        }
        let state = normalize_container_state(fields[2]);
        let port_mappings = parse_port_mappings(fields[3])?;
        containers.push(RuntimeContainer {
            id,
            name,
            state,
            port_mappings,
        });

        if containers.len() > MAX_CONTAINERS_PER_DISTRO {
            return Err(ERR_RUNTIME_BOUNDS);
        }
    }

    containers.sort_by(|left, right| {
        left.id
            .cmp(&right.id)
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(containers)
}

/// Combine bounded per-distro results and terminal counts into deterministic view entries.
pub fn build_entries(
    distros: &[String],
    terminal_counts: &BTreeMap<String, usize>,
    docker_results: &BTreeMap<String, DockerResult>,
) -> Result<Vec<RuntimeDistroEntry>, &'static str> {
    if distros.len() > MAX_DISTROS {
        return Err(ERR_RUNTIME_BOUNDS);
    }

    let mut entries = Vec::with_capacity(distros.len());
    let mut total_containers = 0usize;
    let mut total_mappings = 0usize;
    for name in distros {
        let normalized_name = name.trim();
        if !is_safe_distro_name(normalized_name) {
            return Err(ERR_DISTRO_LIST);
        }

        let terminal_count = *terminal_counts.get(normalized_name).unwrap_or(&0);
        if terminal_count > MAX_TERMINALS_PER_DISTRO {
            return Err(ERR_RUNTIME_BOUNDS);
        }
        let docker = docker_results
            .get(normalized_name)
            .cloned()
            .unwrap_or(DockerResult {
                availability: DockerAvailability::NotQueried,
                containers: Vec::new(),
            });
        if !matches!(&docker.availability, DockerAvailability::Available)
            && !docker.containers.is_empty()
        {
            return Err(ERR_DOCKER_LIST);
        }
        if docker.containers.len() > MAX_CONTAINERS_PER_DISTRO {
            return Err(ERR_RUNTIME_BOUNDS);
        }
        total_containers = total_containers.saturating_add(docker.containers.len());
        if total_containers > MAX_CONTAINERS_TOTAL {
            return Err(ERR_RUNTIME_BOUNDS);
        }
        for container in &docker.containers {
            if container.port_mappings.len() > MAX_PORT_MAPPINGS_PER_CONTAINER {
                return Err(ERR_RUNTIME_BOUNDS);
            }
            total_mappings = total_mappings.saturating_add(container.port_mappings.len());
            if total_mappings > MAX_PORT_MAPPINGS_TOTAL {
                return Err(ERR_RUNTIME_BOUNDS);
            }
        }

        let mut containers = docker.containers;
        containers.sort_by(|left, right| {
            left.id
                .cmp(&right.id)
                .then_with(|| left.name.cmp(&right.name))
        });
        for container in &mut containers {
            container.port_mappings.sort_by(|left, right| {
                left.published
                    .cmp(&right.published)
                    .then_with(|| left.target.cmp(&right.target))
                    .then_with(|| left.protocol.cmp(&right.protocol))
            });
        }

        entries.push(RuntimeDistroEntry {
            id: normalized_name.to_owned(),
            name: normalized_name.to_owned(),
            state: RUNNING_STATE.to_owned(),
            terminal_count: terminal_count as u16,
            docker_availability: docker.availability,
            containers,
        });
    }

    // `parse_running_distros` already sorts, but keep the invariant at this public boundary
    // so fixture callers cannot accidentally create nondeterministic snapshots.
    entries.sort_by(|left, right| left.id.cmp(&right.id));
    validate_entries(&entries)?;
    Ok(entries)
}

/// Validate an entry vector immediately before serialization as an additional defense for
/// callers that construct fixtures or future producer code without using `build_entries`.
pub fn validate_entries(entries: &[RuntimeDistroEntry]) -> Result<(), &'static str> {
    if entries.len() > MAX_DISTROS {
        return Err(ERR_RUNTIME_BOUNDS);
    }
    let mut distro_ids = BTreeSet::new();
    let mut total_containers = 0usize;
    let mut total_mappings = 0usize;
    for entry in entries {
        if entry.id != entry.name
            || entry.state != RUNNING_STATE
            || entry.id.trim() != entry.id
            || !is_safe_distro_name(&entry.id)
            || !distro_ids.insert(entry.id.clone())
        {
            return Err(ERR_DISTRO_LIST);
        }
        if usize::from(entry.terminal_count) > MAX_TERMINALS_PER_DISTRO {
            return Err(ERR_RUNTIME_BOUNDS);
        }
        if entry.containers.len() > MAX_CONTAINERS_PER_DISTRO {
            return Err(ERR_RUNTIME_BOUNDS);
        }
        if !matches!(&entry.docker_availability, DockerAvailability::Available)
            && !entry.containers.is_empty()
        {
            return Err(ERR_DOCKER_LIST);
        }
        total_containers = total_containers.saturating_add(entry.containers.len());
        if total_containers > MAX_CONTAINERS_TOTAL {
            return Err(ERR_RUNTIME_BOUNDS);
        }

        let mut container_ids = BTreeSet::new();
        for container in &entry.containers {
            let Some(normalized_id) = normalize_container_id(&container.id) else {
                return Err(ERR_DOCKER_LIST);
            };
            if normalized_id != container.id
                || normalize_container_name(&container.name).as_deref()
                    != Some(container.name.as_str())
                || !container_ids.insert(container.id.clone())
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
            {
                return Err(ERR_RUNTIME_PRIVACY);
            }
            if container.port_mappings.len() > MAX_PORT_MAPPINGS_PER_CONTAINER {
                return Err(ERR_RUNTIME_BOUNDS);
            }
            total_mappings = total_mappings.saturating_add(container.port_mappings.len());
            if total_mappings > MAX_PORT_MAPPINGS_TOTAL {
                return Err(ERR_RUNTIME_BOUNDS);
            }
            for mapping in &container.port_mappings {
                if mapping.published == 0
                    || mapping.target == 0
                    || !matches!(mapping.protocol.as_str(), "tcp" | "udp" | "sctp")
                {
                    return Err(ERR_DOCKER_LIST);
                }
            }
        }
    }
    Ok(())
}

fn normalize_container_id(raw: &str) -> Option<String> {
    if raw.is_empty()
        || raw.len() > MAX_CONTAINER_ID_BYTES
        || !raw.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    Some(raw.to_ascii_lowercase())
}

fn normalize_container_name(raw: &str) -> Option<String> {
    if raw.is_empty()
        || raw.len() > MAX_CONTAINER_NAME_BYTES
        || !raw
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
        || !raw.as_bytes()[0].is_ascii_alphanumeric()
        || looks_like_sensitive_value(raw)
    {
        return None;
    }
    Some(raw.to_owned())
}

fn looks_like_sensitive_value(value: &str) -> bool {
    let trimmed = value.trim();
    let lower = trimmed.to_ascii_lowercase();
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

pub(crate) fn is_safe_distro_name(name: &str) -> bool {
    name == name.trim()
        && !name.is_empty()
        && name.len() <= MAX_DISTRO_NAME_BYTES
        && !name.contains('/')
        && !matches!(name, "." | "..")
        && devbox_wsl::distro::validate_distro_name(name).is_ok()
        && name.chars().all(|character| !character.is_control())
}

fn normalize_container_state(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "created" => "created",
        "dead" => "dead",
        "exited" => "exited",
        "paused" => "paused",
        "removing" => "removing",
        "restarting" => "restarting",
        "running" => "running",
        _ => "unknown",
    }
    .to_owned()
}

fn parse_port_mappings(raw: &str) -> Result<Vec<PortMapping>, &'static str> {
    let mut mappings = BTreeSet::new();
    for token in raw.split(',') {
        let token = token.trim();
        if token.is_empty() || !token.contains("->") {
            // `80/tcp` is an exposed-only port and has no published host port.
            continue;
        }
        let Some((published_raw, target_raw)) = token.split_once("->") else {
            continue;
        };
        let Some((target_raw, protocol_raw)) = target_raw.rsplit_once('/') else {
            continue;
        };
        let Some(published) = parse_single_port(
            published_raw
                .rsplit_once(':')
                .map_or(published_raw, |(_, port)| port),
        ) else {
            continue;
        };
        let Some(target) = parse_single_port(target_raw) else {
            continue;
        };
        let protocol = protocol_raw.trim().to_ascii_lowercase();
        if !matches!(protocol.as_str(), "tcp" | "udp" | "sctp") {
            continue;
        }
        mappings.insert((published, target, protocol));
        if mappings.len() > MAX_PORT_MAPPINGS_PER_CONTAINER {
            return Err(ERR_RUNTIME_BOUNDS);
        }
    }

    Ok(mappings
        .into_iter()
        .map(|(published, target, protocol)| PortMapping {
            published,
            target,
            protocol,
        })
        .collect())
}

fn parse_single_port(raw: &str) -> Option<u16> {
    let raw = raw.trim();
    if raw.is_empty() || raw.contains('-') {
        return None;
    }
    let port = raw.parse::<u16>().ok()?;
    (port > 0).then_some(port)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docker_result(input: &str) -> DockerResult {
        DockerResult {
            availability: DockerAvailability::Available,
            containers: parse_docker_snapshot(input).unwrap(),
        }
    }

    #[test]
    fn running_distro_parser_is_sorted_bounded_and_deduplicates_nothing() {
        let parsed = parse_running_distros("* Ubuntu 24.04\r\nDebian-12\n").unwrap();
        assert_eq!(parsed, vec!["Debian-12", "Ubuntu 24.04"]);
        assert_eq!(parse_running_distros("").unwrap(), Vec::<String>::new());
        assert_eq!(parse_running_distros("* Ubuntu\n").unwrap(), vec!["Ubuntu"]);
    }

    #[test]
    fn running_distro_parser_rejects_duplicate_or_unsafe_names_without_echoing_input() {
        for input in [
            "Ubuntu\nUbuntu\n",
            "a;b\n",
            "../runtime\n",
            &"x".repeat(MAX_DISTRO_NAME_BYTES + 1),
        ] {
            let error = parse_running_distros(input).unwrap_err();
            assert!(matches!(error, ERR_DISTRO_LIST | ERR_RUNTIME_BOUNDS));
            assert!(!error.contains(input.trim()));
        }
        assert_eq!(
            parse_running_distros(&" \n".repeat(MAX_DOCKER_OUTPUT_BYTES / 2 + 1)).unwrap_err(),
            ERR_RUNTIME_BOUNDS
        );
    }

    #[test]
    fn docker_parser_normalizes_state_and_deduplicates_ipv4_ipv6_bindings() {
        let containers = parse_docker_snapshot(
            "ABCDEF012345\tapi-service\trunning\t0.0.0.0:8080->80/tcp, :::8080->80/tcp, 80/tcp\n",
        )
        .unwrap();
        assert_eq!(containers.len(), 1);
        assert_eq!(containers[0].id, "abcdef012345");
        assert_eq!(containers[0].state, "running");
        assert_eq!(
            containers[0].port_mappings,
            vec![PortMapping {
                published: 8080,
                target: 80,
                protocol: "tcp".into(),
            }]
        );
    }

    #[test]
    fn docker_parser_uses_unknown_for_unrecognized_state_and_skips_invalid_ports() {
        let containers = parse_docker_snapshot(
            "abc123\tworker\tfuture-state\tbad->bad/tcp, 65536/tcp, 9090/tcp, 127.0.0.1:9090->90/icmp\n",
        )
        .unwrap();
        assert_eq!(containers[0].state, "unknown");
        assert!(containers[0].port_mappings.is_empty());
    }

    #[test]
    fn docker_parser_rejects_malformed_rows_and_sensitive_or_path_names() {
        for input in [
            "abc123\tapi\trunning\n",
            "abc123\t../secret\trunning\t8080->80/tcp\n",
            "abc123\tBearer secret\trunning\t8080->80/tcp\n",
            "abc123\tAPIKEY\trunning\t8080->80/tcp\n",
            "zzzz\tapi\trunning\t8080->80/tcp\n",
        ] {
            let error = parse_docker_snapshot(input).unwrap_err();
            assert!(matches!(error, ERR_DOCKER_LIST | ERR_RUNTIME_PRIVACY));
            assert!(!error.contains(input.trim()));
        }
    }

    #[test]
    fn docker_parser_keeps_empty_output_and_sorts_containers() {
        assert!(parse_docker_snapshot("\r\n").unwrap().is_empty());
        let containers =
            parse_docker_snapshot("bbb\tz\texited\t\naaa\ta\tcreated\t8000->80/tcp\n").unwrap();
        assert_eq!(
            containers
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["aaa", "bbb"]
        );
    }

    #[test]
    fn build_entries_is_deterministic_and_marks_missing_or_unqueried() {
        let distros = vec!["Ubuntu".to_owned(), "Debian".to_owned()];
        let counts = BTreeMap::from([("Ubuntu".to_owned(), 2usize)]);
        let docker = BTreeMap::from([(
            "Ubuntu".to_owned(),
            docker_result("abc123\tapi\trunning\t8080->80/tcp\n"),
        )]);
        let entries = build_entries(&distros, &counts, &docker).unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["Debian", "Ubuntu"]
        );
        assert_eq!(
            entries[0].docker_availability,
            DockerAvailability::NotQueried
        );
        assert_eq!(entries[1].terminal_count, 2);
        assert_eq!(entries[1].containers[0].port_mappings[0].published, 8080);
    }

    #[test]
    fn bounds_fail_closed_instead_of_truncating() {
        let distros = vec!["Ubuntu".to_owned()];
        let counts = BTreeMap::from([("Ubuntu".to_owned(), MAX_TERMINALS_PER_DISTRO + 1)]);
        let docker = BTreeMap::new();
        assert_eq!(
            build_entries(&distros, &counts, &docker).unwrap_err(),
            ERR_RUNTIME_BOUNDS
        );

        let oversized = format!(
            "abc123\tapi\trunning\t{}\n",
            "x".repeat(MAX_OUTPUT_LINE_BYTES)
        );
        assert_eq!(
            parse_docker_snapshot(&oversized).unwrap_err(),
            ERR_RUNTIME_BOUNDS
        );
    }

    #[test]
    fn build_entries_canonicalizes_distro_whitespace() {
        let distros = vec![" Ubuntu ".to_owned()];
        let entries = build_entries(&distros, &BTreeMap::new(), &BTreeMap::new()).unwrap();
        assert_eq!(entries[0].id, "Ubuntu");
        assert_eq!(entries[0].name, "Ubuntu");
    }

    #[test]
    fn unavailable_docker_cannot_publish_container_data() {
        let containers = parse_docker_snapshot("abc123\tapi\trunning\t\n").unwrap();
        let docker = BTreeMap::from([(
            "Ubuntu".to_owned(),
            DockerResult {
                availability: DockerAvailability::Error,
                containers,
            },
        )]);
        assert_eq!(
            build_entries(&["Ubuntu".to_owned()], &BTreeMap::new(), &docker).unwrap_err(),
            ERR_DOCKER_LIST
        );
    }
}
