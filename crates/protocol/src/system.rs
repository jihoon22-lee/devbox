//! 데몬 자체에 대한 메서드. 도메인 crate가 아니라 프로토콜에 둔다.
use crate::method::Exporter;
use crate::{domain_error, method};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, ts_rs::TS)]
pub struct PingParams {}

#[derive(Debug, Deserialize, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct Pong {
    pub daemon_id: String,
    pub version: String,
    pub uptime_ms: i64,
}

#[derive(Debug, Deserialize, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct DaemonInfo {
    pub version: String,
    pub instance: String,
    pub pid: u32,
    pub started_ms: i64,
    pub data_dir: String,
    pub config_path: String,
}

domain_error! {
    pub enum SystemError {
        Unavailable = "system.unavailable",
        EnvRefreshFailed = "system.env_refresh_failed",
    }
}

method!(pub Ping = "system.ping", query, PingParams => Pong, SystemError);
method!(pub Info = "system.info", query, PingParams => DaemonInfo, SystemError);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Ok,
    Warn,
    Fail,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
pub struct Check {
    pub id: String,
    pub status: Status,
    pub message: String,
    pub fix: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
pub struct DoctorReport {
    pub checks: Vec<Check>,
}
method!(pub Doctor = "system.doctor", query, PingParams => DoctorReport, SystemError);

#[derive(Debug, Deserialize, Serialize, ts_rs::TS)]
pub struct RefreshEnvResult {
    pub restarting: bool,
}
method!(pub RefreshEnv = "system.refresh_env", mutation, PingParams => RefreshEnvResult, SystemError);

pub fn export<X: Exporter>(x: &mut X) {
    x.method::<Ping>();
    x.method::<Info>();
    x.method::<Doctor>();
    x.method::<RefreshEnv>();
}
