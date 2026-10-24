//! Pure activity logic ported from v0.9.0 `activity-engine` (`core/{sessionizer,
//! idle,privacy,attribution}.rs`, behavior unchanged) plus the wire observation.
//! Builds without the `daemon` feature so the Windows app reuses it.
pub mod attribution;
pub mod event;
pub mod idle;
pub mod privacy;
pub mod sessionizer;

use serde::Serialize;

/// 닫힌 세션 (sessionizer가 배출) — v0.9.0 `core/models.rs`
#[derive(Debug, Clone, Serialize)]
pub struct ClosedSession {
    pub app: String,
    pub title: String,
    pub start_ts: i64,
    pub end_ts: i64,
}
