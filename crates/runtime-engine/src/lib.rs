//! Native domain engine and product-host adapter; no standalone application.
mod applink;
pub mod cleanup;
pub(crate) mod commands;
pub mod component;
pub mod api;
pub mod api_control;
pub mod core;
pub mod integration;
pub(crate) mod lifecycle;
pub(crate) mod log_lens;
pub mod logs;
pub mod notifications;
pub mod platform;
pub mod scheduler;
pub mod storage;
mod task_control;
mod workspace_orchestration;
pub mod workspace_sources;
