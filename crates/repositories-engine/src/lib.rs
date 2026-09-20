//! Native domain engine and product-host adapter; no standalone application.
#[cfg(feature = "desktop")]
mod applink;
pub mod commands;
pub mod component;
mod core;
mod integration;
mod runtime;
