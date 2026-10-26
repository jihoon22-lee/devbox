//! Installation authority is internal and bound to one update and one quit round.
use serde::{Deserialize, Serialize};
#[path = "runner_files.rs"]
pub mod files;
#[path = "runner_recovery.rs"]
pub mod recovery;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Binding {
    pub operation: u64,
    pub update_generation: u64,
    pub quit_generation: u64,
    pub target_version: String,
    pub fixture_install: bool,
}
impl Binding {
    pub fn valid(&self) -> bool {
        self.operation != 0
            && self.update_generation != 0
            && self.quit_generation != 0
            && self.target_version.len() <= 128
            && semver::Version::parse(&self.target_version).is_ok()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessIdentity {
    pub pid: u32,
    pub started: u64,
    pub session: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RunnerError {
    #[error("invalidBinding")]
    InvalidBinding,
    #[error("identity")]
    Identity,
    #[error("storage")]
    Storage,
    #[error("spawn")]
    Spawn,
    #[error("timeout")]
    Timeout,
    #[error("cancelled")]
    Cancelled,
    #[error("installer")]
    Installer,
    #[error("relaunch")]
    Relaunch,
    #[error("unknown")]
    Unknown,
}
pub type Result<T> = std::result::Result<T, RunnerError>;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Prepared,
    Armed,
    Committed,
    Exited,
    Installing,
    Finished,
    Disarmed,
}
pub struct Gate {
    binding: Binding,
    parent: ProcessIdentity,
    phase: Phase,
}
impl Gate {
    pub fn new(binding: Binding, parent: ProcessIdentity) -> Result<Self> {
        if !binding.valid() || parent.pid == 0 || parent.started == 0 {
            return Err(RunnerError::InvalidBinding);
        }
        Ok(Self {
            binding,
            parent,
            phase: Phase::Prepared,
        })
    }
    pub fn arm(&mut self, binding: &Binding, parent: ProcessIdentity) -> Result<()> {
        if self.phase != Phase::Prepared || binding != &self.binding || parent != self.parent {
            return Err(RunnerError::Identity);
        }
        self.phase = Phase::Armed;
        Ok(())
    }
    pub fn commit(&mut self) -> Result<()> {
        if self.phase != Phase::Armed {
            return Err(RunnerError::InvalidBinding);
        }
        self.phase = Phase::Committed;
        Ok(())
    }
    pub fn exited(&mut self, parent: ProcessIdentity) -> Result<()> {
        if self.phase != Phase::Committed || parent != self.parent {
            return Err(RunnerError::Identity);
        }
        self.phase = Phase::Exited;
        Ok(())
    }
    pub fn take_install(&mut self) -> Result<()> {
        if self.phase != Phase::Exited {
            return Err(RunnerError::InvalidBinding);
        }
        self.phase = Phase::Installing;
        Ok(())
    }
    pub fn finish(&mut self) -> Result<()> {
        if self.phase != Phase::Installing {
            return Err(RunnerError::InvalidBinding);
        }
        self.phase = Phase::Finished;
        Ok(())
    }
    pub fn disarm(&mut self) {
        if !matches!(self.phase, Phase::Installing | Phase::Finished) {
            self.phase = Phase::Disarmed;
        }
    }
}

#[cfg(test)]
#[path = "runner_state_tests.rs"]
mod tests;
