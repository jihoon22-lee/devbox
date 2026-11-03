//! Internal receipt and read-only observation contract. No renderer-supplied proof.
use super::RunIdentity;
use serde::{Deserialize, Serialize};

pub const OBSERVATION_SCHEMA: u32 = 1;
pub const OBSERVATION_JOB_PREFIX: &str = "Local\\Devbox.Observe.";
pub const OBSERVATION_GRANT_TIMEOUT_MS: u64 = 15_000;
/// The bootstrap runs no command unless its job holder is ready within this time.
pub const OBSERVATION_HOLDER_READY_TIMEOUT_MS: u64 = 15_000;
/// The bootstrap's own start (interop, PowerShell, Add-Type): what the daemon allowed it before
/// the job holder existed.
pub const OBSERVATION_BOOTSTRAP_START_MS: u64 = 20_000;
/// How long the daemon and the app's native-run tracker wait for a bootstrap's `assigned`
/// receipt. It is written only once the holder is ready, so the whole holder wait comes on top
/// of the start (M-1).
pub const OBSERVATION_ASSIGNMENT_DEADLINE_MS: u64 =
    OBSERVATION_BOOTSTRAP_START_MS + OBSERVATION_HOLDER_READY_TIMEOUT_MS;
pub const OBSERVATION_HOLDER_POLL_MS: u64 = 500;
/// A holder whose job never had a process leaves without a receipt after this time.
pub const OBSERVATION_HOLDER_IDLE_MS: u64 = 60_000;
/// The bootstrap's exit status when the holder was not ready. No command ran.
pub const OBSERVER_UNAVAILABLE_EXIT: i32 = 75;
pub const MAX_WINDOWS_PROBE_BYTES: usize = 1024 * 1024;
pub const MAX_WINDOWS_PROBE_PATHS: usize = 1024;
pub const MAX_WINDOWS_PROBE_PATH_BYTES: usize = 512 * 1024;
pub const MAX_WINDOWS_PROBE_PORTS: usize = 64;

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
    /// The job holder's ready marker: its [`HolderIdentity`], published once its handle keeps
    /// the job name. An rc.1 control has neither holder field (absent, decoded as `None`); a
    /// present but empty or malformed one is invalid (see [`ControlShape`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holder_ready_path: Option<String>,
    /// The holder's `{"nonce":…}` receipt: the job emptied after a process was assigned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emptied_path: Option<String>,
}
/// How a stored control is judged. Only a `Holder` control can start a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlShape {
    /// Created with the job holder's ready marker and emptied receipt.
    Holder,
    /// Stored by rc.1 before the holder existed: neither holder field. No holder ever ran for
    /// it, so it keeps rc.1's rule (`Empty | NotFound` closes) and can never start a command.
    Legacy,
}
/// The job holder's own process identity, published in its ready marker. PID with creation
/// time names one process in every session; SID and session tie it to the bootstrap.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HolderIdentity {
    pub nonce: String,
    pub pid: u32,
    /// Decimal Windows FILETIME ticks, as in [`ObservationAssigned::creation_time`].
    pub creation_time: String,
    pub sid: String,
    pub session_id: u32,
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
/// Listener ownership for every required port. Only `Owned` can authorize readiness.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ListenerObservation {
    Owned,
    /// Some required port has no listener owned by the assigned job, including unrelated ones.
    NotOwned,
    Empty,
    NotFound,
    IdentityMismatch,
    Unavailable,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum WindowsProbe {
    Observation {
        assigned: ObservationAssigned,
    },
    Paths {
        paths: Vec<String>,
    },
    Listeners {
        assigned: ObservationAssigned,
        ports: Vec<u16>,
    },
    /// Holder liveness by process identity only: no job lookup and no session gate. Answered
    /// with an observation state: gone is `NotFound`, alive is `Active`, unknown is
    /// `Unavailable` and another SID is `IdentityMismatch`.
    Holder {
        holder: HolderIdentity,
        /// The job was judged `IdentityMismatch`. Then only an observer outside the holder's
        /// session answers, so only a session-caused mismatch can be resolved.
        #[serde(rename = "crossSession")]
        cross_session: bool,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum WindowsProbeResult {
    Observation { state: ObservationState },
    Paths { state: PathObservation },
    Listeners { state: ListenerObservation },
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
fn valid_nonce(nonce: &str) -> bool {
    nonce.len() == 32
        && nonce
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn valid_job(nonce: &str, job: &str) -> bool {
    valid_nonce(nonce) && job == format!("{OBSERVATION_JOB_PREFIX}{nonce}")
}
fn valid_sid(sid: &str) -> bool {
    sid.starts_with("S-1-")
        && sid.len() <= 184
        && sid
            .bytes()
            .all(|b| b.is_ascii_digit() || b"S-".contains(&b))
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
    /// The only way a new attempt gets a control: every path is derived from the private
    /// control directory and the nonce, so the holder's two paths are always present.
    pub fn new(launch_id: String, nonce: String, directory: &str) -> Self {
        let path = |suffix: &str| format!("{directory}\\{nonce}.{suffix}");
        Self {
            schema: OBSERVATION_SCHEMA,
            launch_id,
            job_name: format!("{OBSERVATION_JOB_PREFIX}{nonce}"),
            assigned_path: path("assigned.json"),
            grant_path: path("grant.json"),
            closed_path: path("closed.json"),
            holder_ready_path: Some(path("holder-ready")),
            emptied_path: Some(path("emptied.json")),
            nonce,
        }
    }
    /// `Holder` with both holder paths, `Legacy` only when both are absent (an rc.1 row).
    /// Anything else, including one absent field or a present but empty or malformed one, is
    /// `None`: it fails closed and is never observed.
    pub fn shape(&self) -> Option<ControlShape> {
        let (shape, holder) = match (&self.holder_ready_path, &self.emptied_path) {
            (Some(ready), Some(emptied)) => (ControlShape::Holder, vec![ready, emptied]),
            (None, None) => (ControlShape::Legacy, Vec::new()),
            _ => return None,
        };
        let parent = |s: &str| s.rsplit_once('\\').map(|(p, _)| p.to_owned());
        let mut paths = vec![&self.assigned_path, &self.grant_path, &self.closed_path];
        paths.extend(holder);
        (self.schema == OBSERVATION_SCHEMA
            && valid_windows_identity(&self.launch_id)
            && valid_job(&self.nonce, &self.job_name)
            && paths.iter().all(|s| valid_windows_path(s))
            // Distinct files in one private control directory, the holder's two included.
            && paths
                .iter()
                .enumerate()
                .all(|(i, a)| paths[i + 1..].iter().all(|b| a != b))
            && paths
                .iter()
                .all(|s| parent(s) == parent(&self.assigned_path)))
        .then_some(shape)
    }
    /// Only a holder control may start a command or receive a grant.
    pub fn valid(&self) -> bool {
        self.shape() == Some(ControlShape::Holder)
    }
    pub fn accepts(&self, run: &RunIdentity, assigned: &ObservationAssigned) -> bool {
        self.valid() && self.matches(run, assigned)
    }
    /// A stored attempt of either shape is still observed, each under its own closing rule.
    pub fn observes(
        &self,
        run: &RunIdentity,
        assigned: &ObservationAssigned,
    ) -> Option<ControlShape> {
        self.shape().filter(|_| self.matches(run, assigned))
    }
    fn matches(&self, run: &RunIdentity, assigned: &ObservationAssigned) -> bool {
        assigned.valid()
            && assigned.schema == self.schema
            && assigned.run_id == run.run_id
            && assigned.attempt_id == run.attempt_id
            && assigned.launch_id == self.launch_id
            && assigned.nonce == self.nonce
            && assigned.job_name == self.job_name
    }
}
impl HolderIdentity {
    pub fn valid(&self) -> bool {
        valid_nonce(&self.nonce)
            && self.pid > 0
            && self
                .creation_time
                .parse::<u64>()
                .is_ok_and(|ticks| ticks > 0)
            && valid_sid(&self.sid)
    }
    /// Only the holder this attempt's bootstrap started: the same nonce, user and session.
    pub fn belongs_to(&self, assigned: &ObservationAssigned) -> bool {
        self.valid()
            && self.nonce == assigned.nonce
            && self.sid == assigned.sid
            && self.session_id == assigned.session_id
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
            && valid_sid(&self.sid)
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
            // The raw length is capped, so duplicate ports cannot hide an oversize request.
            Self::Listeners { assigned, ports } => {
                assigned.valid()
                    && !ports.is_empty()
                    && ports.len() <= MAX_WINDOWS_PROBE_PORTS
                    && !ports.contains(&0)
            }
            Self::Holder { holder, .. } => holder.valid(),
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
            Self::Listeners { .. } => WindowsProbeResult::Listeners {
                state: ListenerObservation::Unavailable,
            },
            Self::Holder { .. } => WindowsProbeResult::Observation {
                state: ObservationState::Unavailable,
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
                WindowsProbe::Observation { .. } | WindowsProbe::Holder { .. },
            ) => *count > 0,
            (
                Self::Observation { .. },
                WindowsProbe::Observation { .. } | WindowsProbe::Holder { .. },
            )
            | (Self::Paths { .. }, WindowsProbe::Paths { .. })
            | (Self::Listeners { .. }, WindowsProbe::Listeners { .. }) => true,
            _ => false,
        }
    }
}
