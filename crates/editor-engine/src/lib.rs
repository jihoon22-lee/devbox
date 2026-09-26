//! Native domain engine and product-host adapter; no standalone application.
#[cfg(feature = "desktop")]
pub mod applink;
pub mod commands;
#[cfg(feature = "desktop")]
pub mod component;
pub mod core;
pub mod lsp;
#[cfg(feature = "desktop")]
pub mod watcher;

#[cfg(feature = "desktop")]
pub mod api;
