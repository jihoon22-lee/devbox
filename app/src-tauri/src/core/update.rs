//! Native-owned update discovery/downloads. Installation belongs to the quit barrier phase.
mod http;
mod release;
mod service;
mod storage;
#[cfg(windows)]
pub use service::Service;
#[cfg(any(test, devbox_update_fixture))]
pub mod fixture;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
pub enum UpdateError {
    #[error("network")]
    Network,
    #[error("invalidRelease")]
    InvalidRelease,
    #[error("size")]
    Size,
    #[error("digest")]
    Digest,
    #[error("timeout")]
    Timeout,
    #[error("storage")]
    Storage,
    #[error("cancelled")]
    Cancelled,
    #[error("staleOperation")]
    StaleOperation,
    #[error("nativeActive")]
    NativeActive,
    #[error("nativeUnknown")]
    NativeUnknown,
    #[error("quitRejected")]
    QuitRejected,
    #[error("runner")]
    Runner,
    #[error("installFailed")]
    InstallFailed,
    #[error("relaunchFailed")]
    RelaunchFailed,
    #[error("recoveryRequired")]
    RecoveryRequired,
}
pub type R<T> = Result<T, UpdateError>;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Idle,
    Checking,
    Available,
    Downloading,
    Ready,
    // Reserved for the separate all-window installation/quit phase.
    #[allow(dead_code)]
    Installing,
    Failed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    None,
    Check,
    Download,
    Cancel,
    Install,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Disabled {
    Channel,
    DevelopmentVersion,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub enabled: bool,
    pub disabled_reason: Option<Disabled>,
    pub phase: Phase,
    pub generation: u64,
    pub revision: u64,
    pub current_version: String,
    pub available_version: Option<String>,
    pub received_bytes: u64,
    pub total_bytes: Option<u64>,
    pub error: Option<UpdateError>,
    pub next_action: Action,
}

pub(crate) mod runner;
#[cfg(windows)]
pub(crate) use storage::{Identity, SealedSource};

#[cfg(windows)]
pub use service::install::{CommitFence, InstallSummary};
