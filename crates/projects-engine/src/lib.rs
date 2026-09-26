//! Native domain engine and product-host adapter; no standalone application.
mod applink;
// Native Rust services remain available without any Tauri command registration.
pub mod commands;
pub mod component;
pub mod core;
mod integration;
mod platform;

pub mod api;
