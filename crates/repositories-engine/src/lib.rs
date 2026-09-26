//! Native domain engine and product-host adapter; no standalone application.
#[cfg(feature = "desktop")]
mod applink;
pub mod commands;
pub mod component;
pub(crate) mod core;
mod integration;
mod runtime;

pub mod api;
