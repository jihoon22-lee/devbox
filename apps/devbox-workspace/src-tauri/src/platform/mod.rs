pub(crate) mod definition_files;
pub mod definition_write;
pub mod git_files;
pub mod git_trust;
pub mod git_worktree;
pub mod project_files;
pub mod project_probe;
pub(crate) mod storage_paths;
pub mod windows_path;

pub mod wsl_distro;

pub mod wsl_helper;

#[cfg(windows)]
pub mod wsl_files;
pub mod wsl_project;

pub(crate) mod source_git;
pub(crate) mod terminal_launch;

// Windows adapters remain app-platform code under CONVENTIONS §4.
#[path = "../../../../devbox-api-studio/src-tauri/src/platform/browser_profile.rs"]
pub(crate) mod browser_profile;
// The shared adapter also exposes API Studio's no-callback wrapper.
#[cfg(windows)]
#[allow(dead_code)]
#[path = "../../../../devbox-api-studio/src-tauri/src/platform/browser_snapshot.rs"]
pub(crate) mod browser_snapshot;
#[path = "../../../../devbox-api-studio/src-tauri/src/platform/owned_copy.rs"]
pub(crate) mod owned_copy;
// API-owned request cleanup uses additional methods from this same implementation.
#[cfg(windows)]
#[allow(dead_code)]
#[path = "../../../../api-playground/src-tauri/src/commands/process_tree.rs"]
pub(crate) mod owned_process;

pub(crate) mod task_sources;
