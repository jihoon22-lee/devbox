//! Pure capability modeling for the read-only Dev Setup audit.
//!
//! Native probes are intentionally kept in the command layer. This module
//! receives only coarse facts and turns them into stable states, evidence
//! codes, and a non-mutating review plan. No path, process output, registry
//! value, environment value, or command line can enter the public contract.

use super::related_tools::DetectionSource;

pub const DEV_SETUP_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallState {
    Present,
    // The current executable probe deliberately cannot prove absence. Keep
    // this state for a future explicit package-inventory source without
    // weakening the backend-only `Unknown` contract in the meantime.
    #[allow(dead_code)]
    Absent,
    Unknown,
}

impl InstallState {
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Absent => "absent",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvailabilityState {
    Available,
    Unavailable,
    Unknown,
}

impl AvailabilityState {
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unavailable => "unavailable",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendState {
    Running,
    Stopped,
    Present,
    Absent,
    Unknown,
}

impl BackendState {
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::Present => "present",
            Self::Absent => "absent",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub enum DockerCliProbe {
    Confirmed(DetectionSource),
    NotFound,
    Unrecognized,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub enum WslBackendProbe {
    Running,
    Stopped,
    Present,
    Absent,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilityEvidence {
    pub source: &'static str,
    pub result: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerCapability {
    pub desktop_install: InstallState,
    pub desktop_launch: AvailabilityState,
    pub windows_cli: AvailabilityState,
    pub wsl_backend: BackendState,
    pub evidence: Vec<CapabilityEvidence>,
}

/// Absence from PATH and reviewed standard locations is not proof that Docker
/// Desktop is uninstalled: Docker can be installed in a custom location while
/// its WSL backend remains registered. Therefore `NotFound` is intentionally
/// modeled as an unknown install state but an unavailable Manager launch.
pub fn model_docker_capability(
    desktop: DetectionSource,
    cli: DockerCliProbe,
    backend: WslBackendProbe,
) -> DockerCapability {
    let (desktop_install, desktop_launch, desktop_result) = match desktop {
        DetectionSource::Path => (InstallState::Present, AvailabilityState::Available, "path"),
        DetectionSource::KnownLocation => (
            InstallState::Present,
            AvailabilityState::Available,
            "known-location",
        ),
        DetectionSource::NotFound => (
            InstallState::Unknown,
            AvailabilityState::Unavailable,
            "not-observed",
        ),
        DetectionSource::Unavailable => (
            InstallState::Unknown,
            AvailabilityState::Unknown,
            "unavailable",
        ),
    };

    let (windows_cli, cli_result) = match cli {
        DockerCliProbe::Confirmed(DetectionSource::Path) => (AvailabilityState::Available, "path"),
        DockerCliProbe::Confirmed(DetectionSource::KnownLocation) => {
            (AvailabilityState::Available, "known-location")
        }
        DockerCliProbe::Confirmed(_) | DockerCliProbe::Unavailable => {
            (AvailabilityState::Unknown, "unavailable")
        }
        DockerCliProbe::NotFound => (AvailabilityState::Unavailable, "not-observed"),
        DockerCliProbe::Unrecognized => (AvailabilityState::Unknown, "unrecognized"),
    };

    let (wsl_backend, registration_result, runtime_result) = match backend {
        WslBackendProbe::Running => (BackendState::Running, "registered", "running"),
        WslBackendProbe::Stopped => (BackendState::Stopped, "registered", "stopped"),
        WslBackendProbe::Present => (BackendState::Present, "registered", "unavailable"),
        WslBackendProbe::Absent => (BackendState::Absent, "not-registered", "not-observed"),
        WslBackendProbe::Unavailable => (BackendState::Unknown, "unavailable", "unavailable"),
    };

    DockerCapability {
        desktop_install,
        desktop_launch,
        windows_cli,
        wsl_backend,
        evidence: vec![
            CapabilityEvidence {
                source: "desktop-executable",
                result: desktop_result,
            },
            CapabilityEvidence {
                source: "windows-cli",
                result: cli_result,
            },
            CapabilityEvidence {
                source: "wsl-registration",
                result: registration_result,
            },
            CapabilityEvidence {
                source: "wsl-runtime",
                result: runtime_result,
            },
        ],
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanStatus {
    Satisfied,
    Review,
    Unknown,
}

impl PlanStatus {
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Satisfied => "satisfied",
            Self::Review => "review",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanAction {
    None,
    ReviewInstall,
    VerifyInstallation,
    ReviewLaunchPath,
    ReviewCli,
    StartBackend,
    ReviewBackend,
    ReviewWinget,
}

impl PlanAction {
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ReviewInstall => "review-install",
            Self::VerifyInstallation => "verify-installation",
            Self::ReviewLaunchPath => "review-launch-path",
            Self::ReviewCli => "review-cli",
            Self::StartBackend => "start-backend",
            Self::ReviewBackend => "review-backend",
            Self::ReviewWinget => "review-winget",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanItem {
    pub capability_id: &'static str,
    pub status: PlanStatus,
    pub action: PlanAction,
}

pub fn build_dev_setup_plan(docker: &DockerCapability, winget: AvailabilityState) -> Vec<PlanItem> {
    let desktop_install = match docker.desktop_install {
        InstallState::Present => PlanItem {
            capability_id: "docker-desktop-install",
            status: PlanStatus::Satisfied,
            action: PlanAction::None,
        },
        InstallState::Absent => PlanItem {
            capability_id: "docker-desktop-install",
            status: PlanStatus::Review,
            action: PlanAction::ReviewInstall,
        },
        InstallState::Unknown => PlanItem {
            capability_id: "docker-desktop-install",
            status: PlanStatus::Unknown,
            action: PlanAction::VerifyInstallation,
        },
    };
    let desktop_launch = match docker.desktop_launch {
        AvailabilityState::Available => PlanItem {
            capability_id: "docker-desktop-launch",
            status: PlanStatus::Satisfied,
            action: PlanAction::None,
        },
        AvailabilityState::Unavailable => PlanItem {
            capability_id: "docker-desktop-launch",
            status: PlanStatus::Review,
            action: PlanAction::ReviewLaunchPath,
        },
        AvailabilityState::Unknown => PlanItem {
            capability_id: "docker-desktop-launch",
            status: PlanStatus::Unknown,
            action: PlanAction::VerifyInstallation,
        },
    };
    let windows_cli = match docker.windows_cli {
        AvailabilityState::Available => PlanItem {
            capability_id: "docker-windows-cli",
            status: PlanStatus::Satisfied,
            action: PlanAction::None,
        },
        AvailabilityState::Unavailable => PlanItem {
            capability_id: "docker-windows-cli",
            status: PlanStatus::Review,
            action: PlanAction::ReviewCli,
        },
        AvailabilityState::Unknown => PlanItem {
            capability_id: "docker-windows-cli",
            status: PlanStatus::Unknown,
            action: PlanAction::VerifyInstallation,
        },
    };
    let wsl_backend = match docker.wsl_backend {
        BackendState::Running => PlanItem {
            capability_id: "docker-wsl-backend",
            status: PlanStatus::Satisfied,
            action: PlanAction::None,
        },
        BackendState::Stopped | BackendState::Present => PlanItem {
            capability_id: "docker-wsl-backend",
            status: PlanStatus::Review,
            action: PlanAction::StartBackend,
        },
        BackendState::Absent => PlanItem {
            capability_id: "docker-wsl-backend",
            status: PlanStatus::Review,
            action: PlanAction::ReviewBackend,
        },
        BackendState::Unknown => PlanItem {
            capability_id: "docker-wsl-backend",
            status: PlanStatus::Unknown,
            action: PlanAction::VerifyInstallation,
        },
    };
    let winget = match winget {
        AvailabilityState::Available => PlanItem {
            capability_id: "winget",
            status: PlanStatus::Satisfied,
            action: PlanAction::None,
        },
        AvailabilityState::Unavailable => PlanItem {
            capability_id: "winget",
            status: PlanStatus::Review,
            action: PlanAction::ReviewWinget,
        },
        AvailabilityState::Unknown => PlanItem {
            capability_id: "winget",
            status: PlanStatus::Unknown,
            action: PlanAction::VerifyInstallation,
        },
    };

    vec![
        desktop_install,
        desktop_launch,
        windows_cli,
        wsl_backend,
        winget,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_only_does_not_claim_desktop_absent_or_offer_install() {
        let capability = model_docker_capability(
            DetectionSource::NotFound,
            DockerCliProbe::NotFound,
            WslBackendProbe::Running,
        );
        assert_eq!(capability.desktop_install, InstallState::Unknown);
        assert_eq!(capability.desktop_launch, AvailabilityState::Unavailable);
        assert_eq!(capability.windows_cli, AvailabilityState::Unavailable);
        assert_eq!(capability.wsl_backend, BackendState::Running);

        let plan = build_dev_setup_plan(&capability, AvailabilityState::Available);
        assert_eq!(plan[0].status, PlanStatus::Unknown);
        assert_eq!(plan[0].action, PlanAction::VerifyInstallation);
        assert!(!plan
            .iter()
            .any(|item| item.action == PlanAction::ReviewInstall));
    }

    #[test]
    fn reviewed_desktop_and_cli_are_available_with_coarse_evidence() {
        let capability = model_docker_capability(
            DetectionSource::KnownLocation,
            DockerCliProbe::Confirmed(DetectionSource::Path),
            WslBackendProbe::Stopped,
        );
        assert_eq!(capability.desktop_install, InstallState::Present);
        assert_eq!(capability.desktop_launch, AvailabilityState::Available);
        assert_eq!(capability.windows_cli, AvailabilityState::Available);
        assert_eq!(capability.wsl_backend, BackendState::Stopped);
        assert_eq!(
            capability.evidence,
            vec![
                CapabilityEvidence {
                    source: "desktop-executable",
                    result: "known-location",
                },
                CapabilityEvidence {
                    source: "windows-cli",
                    result: "path",
                },
                CapabilityEvidence {
                    source: "wsl-registration",
                    result: "registered",
                },
                CapabilityEvidence {
                    source: "wsl-runtime",
                    result: "stopped",
                },
            ]
        );
    }

    #[test]
    fn incompatible_docker_shim_remains_unknown() {
        let capability = model_docker_capability(
            DetectionSource::Unavailable,
            DockerCliProbe::Unrecognized,
            WslBackendProbe::Unavailable,
        );
        assert_eq!(capability.windows_cli, AvailabilityState::Unknown);
        assert_eq!(capability.evidence[1].result, "unrecognized");
        assert_eq!(capability.wsl_backend, BackendState::Unknown);
    }

    #[test]
    fn explicit_absence_is_the_only_state_that_can_plan_an_install_review() {
        let mut capability = model_docker_capability(
            DetectionSource::NotFound,
            DockerCliProbe::NotFound,
            WslBackendProbe::Absent,
        );
        capability.desktop_install = InstallState::Absent;
        let plan = build_dev_setup_plan(&capability, AvailabilityState::Unavailable);
        assert_eq!(plan[0].status, PlanStatus::Review);
        assert_eq!(plan[0].action, PlanAction::ReviewInstall);
    }
}
