//! Native-only Windows execution transport; never exported to the renderer registry.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use zeroize::Zeroize;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunWindows {
    pub run_id: String,
    pub attempt_id: String,
    pub cwd: String,
    pub command: String,
    pub env: BTreeMap<String, String>,
    pub ports: Vec<u16>,
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
pub const POWERSHELL_BOOTSTRAP: &str = r#"$ErrorActionPreference='Stop'; $utf8=New-Object System.Text.UTF8Encoding($false); [Console]::InputEncoding=$utf8; [Console]::OutputEncoding=$utf8; $OutputEncoding=$utf8; try { $r=[Console]::In.ReadToEnd() | ConvertFrom-Json; foreach($p in $r.env.PSObject.Properties) { [Environment]::SetEnvironmentVariable($p.Name,[string]$p.Value,'Process') }; Set-Location -LiteralPath $r.cwd; $global:LASTEXITCODE=0; & ([ScriptBlock]::Create($r.command)); $ok=$?; $code=$global:LASTEXITCODE; if($code -ne 0) { exit $code }; if(!$ok) { exit 1 }; exit 0 } catch { [Console]::Error.WriteLine('Windows execution failed'); exit 1 }"#;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparedWindows {
    pub interop: String,
    pub request: RunWindows,
    pub injected: Vec<String>,
}

pub const MAX_WINDOWS_REQUEST_BYTES: usize = 8 * 1024 * 1024;
