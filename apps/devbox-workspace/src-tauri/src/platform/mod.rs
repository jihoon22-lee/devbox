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

// API-owned request cleanup uses additional methods from this same implementation.
#[cfg(windows)]
#[allow(dead_code)]
#[path = "../../../../../crates/http-client-engine/src/commands/process_tree.rs"]
pub(crate) mod owned_process;

pub(crate) mod task_sources;

pub(crate) mod terminal_focus;

#[cfg(any(windows, test))]
pub(crate) mod runtime_bridge;
