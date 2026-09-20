pub mod dev_setup;
pub mod diagnostics;
pub mod doctor;
pub mod local_quality;
// This legacy module also contains the standalone install/batch commands. The
// embedded host consumes its read-only registry/diagnostic helpers; the remaining
// Tauri entrypoints are deliberately not registered in Control Center.
#[cfg_attr(not(feature = "standalone"), allow(dead_code))]
pub mod manager;
pub mod related_tools;
