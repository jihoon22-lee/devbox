use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use ts_rs::TS;

#[derive(Debug, Default, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ObserveQuery {}
#[derive(Debug, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessesQuery {
    #[serde(default)]
    pub project_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PortInfo {
    pub port: u16,
    pub proto: String,
    pub addr: String,
    pub pid: Option<u32>,
    pub process: Option<String>,
    // Identity sampled around the owning socket scan; absent when not verified.
    pub start_time_ms: Option<u64>,
    pub run_id: Option<String>,
    pub container_id: Option<String>,
    pub project_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub uid: u32,
    pub cwd: Option<String>,
    pub start_time_ms: u64,
    pub cpu_percent: f64,
    pub rss_bytes: u64,
    pub unit: Option<String>,
    pub run_id: Option<String>,
    pub project_id: Option<String>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum KillSignal {
    Term,
    Kill,
}
#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessExpectation {
    pub start_time_ms: u64,
}
#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KillProcess {
    pub pid: u32,
    pub signal: KillSignal,
    pub expect: ProcessExpectation,
}
#[derive(Debug, Serialize, Deserialize, TS)]
pub struct KillResult {
    pub pid: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PressureLine {
    pub avg10: f64,
    pub avg60: f64,
    pub avg300: f64,
    pub total_us: u64,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
pub struct Pressure {
    pub some: Option<PressureLine>,
    pub full: Option<PressureLine>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FilesystemInfo {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub used_bytes: u64,
}
#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub memory_total_bytes: u64,
    pub memory_available_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
    pub cpu_pressure: Option<Pressure>,
    pub memory_pressure: Option<Pressure>,
    pub io_pressure: Option<Pressure>,
    pub home_filesystem: Option<FilesystemInfo>,
}
/// Composition input: observe never reads another domain's database.
#[derive(Debug, Clone)]
pub struct ProjectRoot {
    pub id: String,
    pub path: PathBuf,
}
#[derive(Debug, Clone)]
pub struct UnitRun {
    pub unit: String,
    pub run_id: String,
    pub project_id: String,
}
