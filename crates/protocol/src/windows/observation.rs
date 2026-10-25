//! Internal receipt and read-only observation contract. No renderer-supplied proof.
use super::RunIdentity;
use serde::{Deserialize, Serialize};

pub const OBSERVATION_SCHEMA: u32 = 1;
pub const OBSERVATION_JOB_PREFIX: &str = "Local\\Devbox.Observe.";
pub const OBSERVATION_GRANT_TIMEOUT_MS: u64 = 15_000;
pub const MAX_WINDOWS_PROBE_BYTES: usize = 1024 * 1024;
pub const MAX_WINDOWS_PROBE_PATHS: usize = 1024;
pub const MAX_WINDOWS_PROBE_PATH_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObservationControl {
    pub schema: u32,
    pub launch_id: String,
    pub nonce: String,
    pub job_name: String,
    pub assigned_path: String,
    pub grant_path: String,
    pub closed_path: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObservationAssigned {
    pub schema: u32,
    pub run_id: String,
    pub launch_id: String,
    pub attempt_id: String,
    pub nonce: String,
    pub job_name: String,
    pub pid: u32,
    /// Decimal Windows FILETIME ticks, preserving the exact process start identity.
    pub creation_time: String,
    pub sid: String,
    pub session_id: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "camelCase", deny_unknown_fields)]
pub enum ObservationState {
    Active { count: u32 },
    Empty,
    NotFound,
    Unavailable,
    IdentityMismatch,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PathObservation {
    Safe,
    Unsafe,
    Unavailable,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum WindowsProbe {
    Observation { assigned: ObservationAssigned },
    Paths { paths: Vec<String> },
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum WindowsProbeResult {
    Observation { state: ObservationState },
    Paths { state: PathObservation },
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WindowsProbeRequest {
    pub probe_id: String,
    pub probe: WindowsProbe,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WindowsProbeReply {
    pub probe_id: String,
    pub result: WindowsProbeResult,
}
pub fn valid_windows_identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
}
fn valid_job(nonce: &str, job: &str) -> bool {
    nonce.len() == 32
        && nonce
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && job == format!("{OBSERVATION_JOB_PREFIX}{nonce}")
}
pub fn valid_windows_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    let drive = bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1..3] == *b":\\";
    let unc =
        path.starts_with("\\\\") && path[2..].split('\\').filter(|p| !p.is_empty()).count() >= 2;
    (drive || unc)
        && path.len() <= 16_384
        && !path.chars().any(char::is_control)
        && !path.contains(['/', '*', '?', '"', '<', '>', '|'])
        && !path[if drive { 2 } else { 0 }..].contains(':')
        && !path.split('\\').any(|part| part == "." || part == "..")
}
impl ObservationControl {
    pub fn valid(&self) -> bool {
        let parent = |s: &str| s.rsplit_once('\\').map(|(p, _)| p.to_owned());
        self.schema == OBSERVATION_SCHEMA
            && valid_windows_identity(&self.launch_id)
            && valid_job(&self.nonce, &self.job_name)
            && [&self.assigned_path, &self.grant_path, &self.closed_path]
                .iter()
                .all(|s| valid_windows_path(s))
            && self.assigned_path != self.grant_path
            && self.assigned_path != self.closed_path
            && self.grant_path != self.closed_path
            && parent(&self.assigned_path) == parent(&self.grant_path)
            && parent(&self.assigned_path) == parent(&self.closed_path)
    }
    pub fn accepts(&self, run: &RunIdentity, assigned: &ObservationAssigned) -> bool {
        self.valid()
            && assigned.valid()
            && assigned.schema == self.schema
            && assigned.run_id == run.run_id
            && assigned.attempt_id == run.attempt_id
            && assigned.launch_id == self.launch_id
            && assigned.nonce == self.nonce
            && assigned.job_name == self.job_name
    }
}
impl ObservationAssigned {
    pub fn valid(&self) -> bool {
        self.schema == OBSERVATION_SCHEMA
            && [&self.run_id, &self.launch_id, &self.attempt_id]
                .iter()
                .all(|s| valid_windows_identity(s))
            && valid_job(&self.nonce, &self.job_name)
            && self.pid > 0
            && self
                .creation_time
                .parse::<u64>()
                .is_ok_and(|ticks| ticks > 0)
            && self.sid.starts_with("S-1-")
            && self.sid.len() <= 184
            && self
                .sid
                .bytes()
                .all(|b| b.is_ascii_digit() || b"S-".contains(&b))
    }
}
impl WindowsProbe {
    pub fn valid(&self) -> bool {
        match self {
            Self::Observation { assigned } => assigned.valid(),
            Self::Paths { paths } => {
                !paths.is_empty()
                    && paths.len() <= MAX_WINDOWS_PROBE_PATHS
                    && paths.iter().map(String::len).sum::<usize>() <= MAX_WINDOWS_PROBE_PATH_BYTES
                    && paths.iter().all(|p| valid_windows_path(p))
            }
        }
    }
    pub fn unavailable(&self) -> WindowsProbeResult {
        match self {
            Self::Observation { .. } => WindowsProbeResult::Observation {
                state: ObservationState::Unavailable,
            },
            Self::Paths { .. } => WindowsProbeResult::Paths {
                state: PathObservation::Unavailable,
            },
        }
    }
}
impl WindowsProbeResult {
    pub fn valid_for(&self, request: &WindowsProbe) -> bool {
        match (self, request) {
            (
                Self::Observation {
                    state: ObservationState::Active { count },
                },
                WindowsProbe::Observation { .. },
            ) => *count > 0,
            (Self::Observation { .. }, WindowsProbe::Observation { .. })
            | (Self::Paths { .. }, WindowsProbe::Paths { .. }) => true,
            _ => false,
        }
    }
}
