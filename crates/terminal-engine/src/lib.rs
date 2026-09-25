//! Native domain engine and product-host adapter; no standalone application.
pub(crate) mod commands;
pub mod component;
pub mod api;
pub mod core;
mod integration;
mod runtime_snapshot;
#[cfg(feature = "desktop")]
mod terminal_owner;
