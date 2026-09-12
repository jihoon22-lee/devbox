//! OS-independent process and port utilities shared by the desktop apps.
//!
//! The crate contains data types and parsers that can be tested on WSL. It does
//! not execute `netstat`, invoke Windows APIs, or own a process-tree lifetime.

pub mod models;
pub mod netstat;
pub mod process;

pub use models::PortInfo;
pub use netstat::{extract_port, parse_netstat_output};
pub use process::{is_process_alive, lookup_process, ProcessId, ProcessSnapshot, ProcessSummary};
