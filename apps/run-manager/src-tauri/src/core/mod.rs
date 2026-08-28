pub mod cron;
pub mod imports;
pub mod log_search;
pub mod models;
pub mod policies;
pub mod retention;
// Log rotation and tailing are filesystem-backed I/O, so the implementation
// lives at the app module level rather than in the pure core namespace. Keep
// this re-export for callers that already use `core::logs` while the API is
// being integrated with the command layer.
pub use crate::logs;
pub mod shell;
