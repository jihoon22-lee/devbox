//! Native-only Windows execution transport; never exported to the renderer registry.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use zeroize::Zeroize;
mod observation;
pub use observation::*;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunWindows {
    pub run_id: String,
    pub attempt_id: String,
    pub cwd: String,
    pub command: String,
    pub env: BTreeMap<String, String>,
    pub ports: Vec<u16>,
    pub observation: ObservationControl,
}
impl RunWindows {
    pub fn tracked(&self) -> bool {
        valid_windows_identity(&self.run_id)
            && valid_windows_identity(&self.attempt_id)
            && self.observation.valid()
    }
}
impl Drop for RunWindows {
    fn drop(&mut self) {
        for value in self.env.values_mut() {
            value.zeroize();
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunIdentity {
    pub run_id: String,
    pub attempt_id: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WindowsEvent {
    pub run_id: String,
    pub attempt_id: String,
    pub event: WindowsOutput,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum WindowsOutput {
    Stdout { data: Vec<u8> },
    Stderr { data: Vec<u8> },
    Finished { code: i32 },
    Failed,
    Ready,
    Heartbeat,
}
// Both adapters use a fixed bootstrap. All user input (including secret env) is stdin JSON.
// The child emits UTF-8 so the Linux wrapper's byte redactor sees the original secret bytes.
pub const POWERSHELL_BOOTSTRAP: &str = concat!(
    include_str!("windows/native.ps1"),
    "\n",
    include_str!("windows/bootstrap.ps1")
);
pub const POWERSHELL_OBSERVATION_PROBE: &str = concat!(
    include_str!("windows/native.ps1"),
    "\n",
    include_str!("windows/probe.ps1")
);

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparedWindows {
    pub interop: String,
    pub request: RunWindows,
    pub injected: Vec<String>,
}

pub const MAX_WINDOWS_REQUEST_BYTES: usize = 8 * 1024 * 1024;

#[cfg(test)]
mod tests;
