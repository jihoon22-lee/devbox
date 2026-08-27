//! Start Workspace 사전 점검의 순수 계약과 판정 로직.
//!
//! 이 모듈은 실제 파일·WSL·TCP probe를 수행하지 않는다. probe 결과를
//! bounded DTO로 바꾸는 책임만 가지며, command 계층은 이 계약을 이용해
//! read-only observation을 수집한다. 따라서 사전 점검 결과에는 경로·PID·
//! subprocess stderr 같은 원문이 들어가지 않는다.

use serde::Serialize;

/// Start Workspace가 직접 열어야 하는 devbox 앱과 capability.
/// 설치된 executable 경로는 결과에 절대 포함하지 않는다.
pub const REQUIRED_APP_SPECS: &[(&str, &str)] =
    &[("wsl-desktop", "path"), ("code-pad", "workspace")];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PreflightStatus {
    Pass,
    Warning,
    Failure,
    Unavailable,
}

impl PreflightStatus {
    pub fn is_blocking(self) -> bool {
        matches!(self, Self::Failure | Self::Unavailable)
    }

    pub fn is_non_blocking(self) -> bool {
        !self.is_blocking()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectoryProbe {
    Available,
    Missing,
    Unsafe,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortProbe {
    Free,
    Existing,
    Conflict,
    #[allow(dead_code)]
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceSnapshotProbe {
    /// The producer has not published a snapshot yet. This is distinguishable
    /// from malformed data and is a non-blocking warning because a later
    /// lifecycle step may start the configured service.
    Missing,
    /// The snapshot was read and every configured dependency is active.
    AllRunning,
    /// The snapshot was read, but one or more dependencies are not active.
    SomeNotRunning,
    /// The producer snapshot exists but cannot be trusted.
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ResourceState {
    Available,
    Existing,
    WorkbenchStarted,
    NotRunning,
    Missing,
    Conflict,
    Unsafe,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceProvenance {
    /// Stable kind, never a user-controlled path or subprocess output.
    pub kind: String,
    /// Stable app/capability/port slot identifier. Service slots are indexed
    /// (`service-1`, …) rather than reflecting arbitrary service metadata.
    pub id: String,
    pub state: ResourceState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightItem {
    /// Stable UI key. It is intentionally not a free-form label from a probe.
    pub key: String,
    pub status: PreflightStatus,
    /// Fixed, user-facing text. It never includes path, credential or stderr.
    pub detail: String,
    pub resources: Vec<ResourceProvenance>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePreflight {
    pub profile_id: String,
    /// Warnings are reviewable but do not prevent the explicit continue action.
    pub ready: bool,
    pub items: Vec<PreflightItem>,
}

impl WorkspacePreflight {
    pub fn new(profile_id: impl Into<String>, items: Vec<PreflightItem>) -> Self {
        let ready = items.iter().all(|item| item.status.is_non_blocking());
        Self {
            profile_id: profile_id.into(),
            ready,
            items,
        }
    }

    pub fn resources(&self) -> impl Iterator<Item = &ResourceProvenance> {
        self.items.iter().flat_map(|item| item.resources.iter())
    }
}

fn item(
    key: &str,
    status: PreflightStatus,
    detail: &str,
    resources: Vec<ResourceProvenance>,
) -> PreflightItem {
    PreflightItem {
        key: key.to_string(),
        status,
        detail: detail.to_string(),
        resources,
    }
}

fn resource(kind: &str, id: impl Into<String>, state: ResourceState) -> ResourceProvenance {
    ResourceProvenance {
        kind: kind.to_string(),
        id: id.into(),
        state,
    }
}

/// 판정 가능한 required app/capability 목록을 결과 DTO로 바꾼다.
///
/// App identity alone is insufficient: a catalog entry may be installed but
/// still lack the exact handoff capability required by Start Workspace.
pub fn assess_required_apps(installed_capabilities: &[(&str, &str)]) -> PreflightItem {
    let resources = REQUIRED_APP_SPECS
        .iter()
        .map(|(app_id, capability)| {
            let installed =
                installed_capabilities
                    .iter()
                    .any(|(candidate_id, candidate_capability)| {
                        candidate_id == app_id && candidate_capability == capability
                    });
            resource(
                "app",
                format!("{app_id}:{capability}"),
                if installed {
                    ResourceState::Available
                } else {
                    ResourceState::Missing
                },
            )
        })
        .collect::<Vec<_>>();
    let missing = resources
        .iter()
        .filter(|entry| entry.state == ResourceState::Missing)
        .count();
    if missing == 0 {
        item(
            "required-apps",
            PreflightStatus::Pass,
            "필수 devbox 앱을 사용할 수 있습니다",
            resources,
        )
    } else {
        item(
            "required-apps",
            PreflightStatus::Failure,
            "필수 devbox 앱이 없습니다. Devbox Manager에서 설치하세요",
            resources,
        )
    }
}

pub fn assess_distro(configured: bool, probe: DirectoryProbe) -> PreflightItem {
    if !configured {
        return item(
            "wsl-distro",
            PreflightStatus::Pass,
            "WSL distro가 설정되지 않았습니다",
            Vec::new(),
        );
    }

    let (status, detail, state) = match probe {
        DirectoryProbe::Available => (
            PreflightStatus::Pass,
            "설정한 WSL distro를 사용할 수 있습니다",
            ResourceState::Available,
        ),
        DirectoryProbe::Missing => (
            PreflightStatus::Failure,
            "설정한 WSL distro를 찾을 수 없습니다",
            ResourceState::Missing,
        ),
        DirectoryProbe::Unsafe => (
            PreflightStatus::Failure,
            "WSL distro 설정을 안전하게 확인할 수 없습니다",
            ResourceState::Unsafe,
        ),
        DirectoryProbe::Unavailable => (
            PreflightStatus::Unavailable,
            "WSL distro 상태를 확인할 수 없습니다",
            ResourceState::Unavailable,
        ),
    };
    item(
        "wsl-distro",
        status,
        detail,
        vec![resource("distro", "wsl-distro", state)],
    )
}

pub fn assess_working_directories(probes: &[DirectoryProbe]) -> PreflightItem {
    if probes.is_empty() {
        return item(
            "working-directory",
            PreflightStatus::Failure,
            "Workspace 경로가 설정되지 않았습니다",
            vec![resource("directory", "workspace", ResourceState::Missing)],
        );
    }

    let resources = probes
        .iter()
        .enumerate()
        .map(|(index, probe)| {
            let state = match probe {
                DirectoryProbe::Available => ResourceState::Available,
                DirectoryProbe::Missing => ResourceState::Missing,
                DirectoryProbe::Unsafe => ResourceState::Unsafe,
                DirectoryProbe::Unavailable => ResourceState::Unavailable,
            };
            resource("directory", format!("workspace-{}", index + 1), state)
        })
        .collect::<Vec<_>>();
    let status = if probes
        .iter()
        .any(|probe| matches!(probe, DirectoryProbe::Unsafe | DirectoryProbe::Missing))
    {
        PreflightStatus::Failure
    } else if probes
        .iter()
        .any(|probe| matches!(probe, DirectoryProbe::Unavailable))
    {
        PreflightStatus::Unavailable
    } else {
        PreflightStatus::Pass
    };
    let detail = match status {
        PreflightStatus::Pass => "Workspace working directory를 사용할 수 있습니다",
        PreflightStatus::Failure => "Workspace working directory를 사용할 수 없습니다",
        PreflightStatus::Unavailable => "Workspace working directory 상태를 확인할 수 없습니다",
        PreflightStatus::Warning => unreachable!("directory probe has no warning state"),
    };
    item("working-directory", status, detail, resources)
}

pub fn assess_ports(probes: &[PortProbe]) -> PreflightItem {
    if probes.is_empty() {
        return item(
            "ports",
            PreflightStatus::Pass,
            "확인할 예상 port가 없습니다",
            Vec::new(),
        );
    }

    let resources = probes
        .iter()
        .enumerate()
        .map(|(index, probe)| {
            let state = match probe {
                PortProbe::Free => ResourceState::Available,
                PortProbe::Existing => ResourceState::Existing,
                PortProbe::Conflict => ResourceState::Conflict,
                PortProbe::Unavailable => ResourceState::Unavailable,
            };
            resource("tcp-port", format!("port-{}", index + 1), state)
        })
        .collect::<Vec<_>>();
    let status = if probes
        .iter()
        .any(|probe| matches!(probe, PortProbe::Conflict))
    {
        PreflightStatus::Failure
    } else if probes
        .iter()
        .any(|probe| matches!(probe, PortProbe::Unavailable))
    {
        PreflightStatus::Unavailable
    } else if probes
        .iter()
        .any(|probe| matches!(probe, PortProbe::Existing))
    {
        PreflightStatus::Warning
    } else {
        PreflightStatus::Pass
    };
    let detail = match status {
        PreflightStatus::Pass => "예상 TCP port를 사용할 수 있습니다",
        PreflightStatus::Warning => "이미 사용 중인 예상 port가 있습니다",
        PreflightStatus::Failure => "예상 TCP port 충돌이 있습니다",
        PreflightStatus::Unavailable => "예상 TCP port 상태를 확인할 수 없습니다",
    };
    item("ports", status, detail, resources)
}

/// Build service provenance with one running bit per configured service. The
/// aggregate `ServiceSnapshotProbe` controls the user-visible status, while
/// the per-entry bits preserve which dependencies were already running.
pub fn assess_service_dependencies_with_running(
    running: &[bool],
    probe: ServiceSnapshotProbe,
) -> PreflightItem {
    if running.is_empty() {
        return item(
            "service-dependencies",
            PreflightStatus::Pass,
            "확인할 Run Manager service dependency가 없습니다",
            Vec::new(),
        );
    }

    let resources = running
        .iter()
        .enumerate()
        .map(|(index, is_running)| {
            let state = match probe {
                ServiceSnapshotProbe::Unavailable => ResourceState::Unavailable,
                _ if *is_running => ResourceState::Existing,
                _ => ResourceState::NotRunning,
            };
            resource("service", format!("service-{}", index + 1), state)
        })
        .collect::<Vec<_>>();
    let (status, detail) = match probe {
        ServiceSnapshotProbe::AllRunning => (
            PreflightStatus::Pass,
            "필요한 Run Manager service가 실행 중입니다",
        ),
        ServiceSnapshotProbe::SomeNotRunning => (
            PreflightStatus::Warning,
            "실행되지 않은 Run Manager service dependency가 있습니다",
        ),
        ServiceSnapshotProbe::Missing => (
            PreflightStatus::Warning,
            "Run Manager service 상태 snapshot이 없습니다",
        ),
        ServiceSnapshotProbe::Unavailable => (
            PreflightStatus::Unavailable,
            "Run Manager service dependency를 확인할 수 없습니다",
        ),
    };
    item("service-dependencies", status, detail, resources)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_apps_fail_closed_without_reflecting_executable_paths() {
        let item = assess_required_apps(&[("wsl-desktop", "path")]);
        assert_eq!(item.status, PreflightStatus::Failure);
        assert_eq!(item.resources[0].state, ResourceState::Available);
        assert_eq!(item.resources[1].state, ResourceState::Missing);
        let json = serde_json::to_string(&item).unwrap();
        assert!(!json.contains(".exe"));
        assert!(!json.contains("C:"));
    }

    #[test]
    fn required_apps_pass_only_when_each_capability_is_installed() {
        let item = assess_required_apps(&[("wsl-desktop", "path"), ("code-pad", "workspace")]);
        assert_eq!(item.status, PreflightStatus::Pass);
        assert!(item
            .resources
            .iter()
            .all(|resource| resource.state == ResourceState::Available));
    }

    #[test]
    fn required_apps_reject_the_right_apps_with_the_wrong_capabilities() {
        let item = assess_required_apps(&[("wsl-desktop", "workspace"), ("code-pad", "path")]);
        assert_eq!(item.status, PreflightStatus::Failure);
        assert!(item
            .resources
            .iter()
            .all(|resource| resource.state == ResourceState::Missing));
    }

    #[test]
    fn directory_states_are_blocking_except_for_complete_available_set() {
        assert!(assess_working_directories(&[DirectoryProbe::Available])
            .status
            .is_non_blocking());
        assert_eq!(
            assess_working_directories(&[DirectoryProbe::Missing]).status,
            PreflightStatus::Failure
        );
        assert_eq!(
            assess_working_directories(&[DirectoryProbe::Unsafe]).status,
            PreflightStatus::Failure
        );
        assert_eq!(
            assess_working_directories(&[DirectoryProbe::Unavailable]).status,
            PreflightStatus::Unavailable
        );
    }

    #[test]
    fn existing_port_is_warning_but_unowned_conflict_blocks() {
        assert_eq!(
            assess_ports(&[PortProbe::Existing]).status,
            PreflightStatus::Warning
        );
        assert_eq!(
            assess_ports(&[PortProbe::Conflict]).status,
            PreflightStatus::Failure
        );
        assert_eq!(
            assess_ports(&[PortProbe::Unavailable]).status,
            PreflightStatus::Unavailable
        );
        let resources = assess_ports(&[PortProbe::Existing]).resources;
        assert_eq!(resources[0].state, ResourceState::Existing);
    }

    #[test]
    fn service_snapshot_states_preserve_existing_resource_provenance() {
        let running = assess_service_dependencies_with_running(
            &[true, true],
            ServiceSnapshotProbe::AllRunning,
        );
        assert_eq!(running.status, PreflightStatus::Pass);
        assert!(running
            .resources
            .iter()
            .all(|resource| resource.state == ResourceState::Existing));

        let missing = assess_service_dependencies_with_running(
            &[false, false],
            ServiceSnapshotProbe::SomeNotRunning,
        );
        assert_eq!(missing.status, PreflightStatus::Warning);
        assert!(missing
            .resources
            .iter()
            .all(|resource| resource.state == ResourceState::NotRunning));

        let mixed = assess_service_dependencies_with_running(
            &[true, false],
            ServiceSnapshotProbe::SomeNotRunning,
        );
        assert_eq!(mixed.status, PreflightStatus::Warning);
        assert_eq!(mixed.resources[0].state, ResourceState::Existing);
        assert_eq!(mixed.resources[1].state, ResourceState::NotRunning);

        assert_eq!(
            assess_service_dependencies_with_running(&[false], ServiceSnapshotProbe::Unavailable)
                .status,
            PreflightStatus::Unavailable
        );
    }

    #[test]
    fn workspace_ready_allows_warnings_but_not_failures() {
        let warning =
            WorkspacePreflight::new("profile-1", vec![assess_ports(&[PortProbe::Existing])]);
        assert!(warning.ready);

        let failure =
            WorkspacePreflight::new("profile-1", vec![assess_ports(&[PortProbe::Conflict])]);
        assert!(!failure.ready);
    }

    #[test]
    fn resource_provenance_serializes_only_stable_state_names() {
        let item = PreflightItem {
            key: "processes".into(),
            status: PreflightStatus::Pass,
            detail: "프로세스를 시작했습니다".into(),
            resources: vec![resource(
                "process",
                "code-pad",
                ResourceState::WorkbenchStarted,
            )],
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("workbenchStarted"));
        assert!(json.contains("code-pad"));
        assert!(!json.contains("pid"));
        assert!(!json.contains("TOP_SECRET"));
    }
}
