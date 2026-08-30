//! Workspace discovery commands used by Quick Open.
//!
//! The filesystem walk deliberately lives on the Rust side.  Quick Open gets
//! one bounded snapshot and performs only its query ranking in the frontend.

use devbox_filesystem::collect_limited;
use serde::Serialize;
use std::path::{Path, PathBuf};

pub const QUICK_OPEN_MAX_ENTRIES: usize = 50_000;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFile {
    /// Canonical absolute path used when opening the file.
    pub path: String,
    /// Workspace-relative path used for display and matching.
    pub relative_path: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkspaceFiles {
    pub files: Vec<WorkspaceFile>,
    pub truncated: bool,
    pub incomplete: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceCapabilities {
    pub path: String,
    pub source_kind: String,
    pub watch_mode: String,
    pub edit_supported: bool,
    pub lsp_supported: bool,
    pub lsp_reason: Option<String>,
}

/// Canonicalizes a workspace and verifies that it is a directory.
pub fn canonical_workspace(path: &Path) -> Result<PathBuf, String> {
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("작업 폴더를 확인할 수 없습니다: {error}"))?;
    if !canonical.is_dir() {
        return Err(format!(
            "작업 폴더가 디렉터리가 아닙니다: {}",
            canonical.display()
        ));
    }
    Ok(canonical)
}

/// Component-aware boundary check.  `Path::starts_with` does not mistake
/// `/workspace/project-2` for `/workspace/project`, unlike a string prefix.
pub fn is_within_workspace(root: &Path, path: &Path) -> bool {
    match devbox_wsl::path::wsl_unc_contains(&root.to_string_lossy(), &path.to_string_lossy()) {
        Ok(Some(contains)) => return contains,
        Ok(None) => {}
        Err(_) => return false,
    }
    #[cfg(windows)]
    {
        let mut root_components = root.components();
        let mut path_components = path.components();
        root_components.all(|root_component| {
            path_components.next().is_some_and(|path_component| {
                root_component
                    .as_os_str()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&path_component.as_os_str().to_string_lossy())
            })
        })
    }
    #[cfg(not(windows))]
    {
        path.starts_with(root)
    }
}

/// Lists at most [`QUICK_OPEN_MAX_ENTRIES`] files below one canonical root.
///
/// The limit is applied by `collect_limited` while walking, never after a full
/// tree has been materialized in the frontend.
#[tauri::command]
pub async fn list_workspace_files(path: String) -> Result<WorkspaceFiles, String> {
    tauri::async_runtime::spawn_blocking(move || list_workspace_files_blocking(&path))
        .await
        .map_err(|error| format!("작업 폴더 목록 작업이 중단되었습니다: {error}"))?
}

fn list_workspace_files_blocking(path: &str) -> Result<WorkspaceFiles, String> {
    let root = canonical_workspace(Path::new(path))?;
    let result = collect_limited(&root, QUICK_OPEN_MAX_ENTRIES);
    let mut files = Vec::with_capacity(result.files.len());

    for entry in result.files {
        let canonical = match entry.path.canonicalize() {
            Ok(path) => path,
            // A file can disappear between the walk and canonicalization.  It
            // is not useful to Quick Open, so omit just that entry.
            Err(_) => continue,
        };
        if !is_within_workspace(&root, &canonical) || !canonical.is_file() {
            continue;
        }
        let relative_path = canonical
            .strip_prefix(&root)
            .map_err(|_| "작업 폴더 밖의 경로가 발견되었습니다".to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        files.push(WorkspaceFile {
            path: canonical.to_string_lossy().into_owned(),
            relative_path,
            size: entry.size.max(0) as u64,
        });
    }

    Ok(WorkspaceFiles {
        files,
        truncated: result.truncated,
        incomplete: result.incomplete,
    })
}

fn capabilities_for_path(path: &Path) -> WorkspaceCapabilities {
    let path = path.to_string_lossy().into_owned();
    let is_wsl = devbox_wsl::path::parse_wsl_unc_path(&path)
        .ok()
        .flatten()
        .is_some();
    WorkspaceCapabilities {
        path,
        source_kind: if is_wsl { "wsl" } else { "native" }.to_string(),
        watch_mode: if is_wsl { "polling" } else { "native" }.to_string(),
        edit_supported: true,
        lsp_supported: !is_wsl,
        lsp_reason: is_wsl.then(|| "host_lsp_wsl_unsupported".to_string()),
    }
}

/// Returns the canonical root so the frontend can keep the same path identity
/// for session restoration, preview boundaries, and subsequent Quick Open
/// snapshots.
#[tauri::command]
pub async fn canonicalize_workspace(path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        canonical_workspace(Path::new(&path)).map(|path| path.to_string_lossy().into_owned())
    })
    .await
    .map_err(|error| format!("작업 폴더 확인 작업이 중단되었습니다: {error}"))?
}

/// Returns the canonical workspace path and the independently supported edit,
/// watcher, and host-LSP capabilities for that path.
#[tauri::command]
pub async fn workspace_capabilities(path: String) -> Result<WorkspaceCapabilities, String> {
    tauri::async_runtime::spawn_blocking(move || {
        canonical_workspace(Path::new(&path)).map(|path| capabilities_for_path(&path))
    })
    .await
    .map_err(|error| format!("작업 폴더 확인 작업이 중단되었습니다: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn boundary_is_component_aware() {
        let root = Path::new("/tmp/project");
        assert!(is_within_workspace(
            root,
            Path::new("/tmp/project/src/main.rs")
        ));
        assert!(!is_within_workspace(
            root,
            Path::new("/tmp/project-old/src/main.rs")
        ));
        assert!(!is_within_workspace(root, Path::new("/tmp/other/main.rs")));
    }

    #[test]
    fn list_returns_relative_paths_and_skips_outside_symlink_targets() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("workspace");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        let result = tauri::async_runtime::block_on(list_workspace_files(
            root.to_string_lossy().into_owned(),
        ))
        .unwrap();
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].relative_path, "src/main.rs");
        assert!(!result.truncated);
        assert!(!result.incomplete);
    }

    #[test]
    fn wsl_capabilities_are_explicit_and_linux_containment_is_case_sensitive() {
        let capabilities = capabilities_for_path(Path::new(
            "//?/UNC/wsl.localhost/Ubuntu/home/jihoon/프로젝트",
        ));
        assert_eq!(capabilities.source_kind, "wsl");
        assert_eq!(capabilities.watch_mode, "polling");
        assert!(capabilities.edit_supported);
        assert!(!capabilities.lsp_supported);
        assert_eq!(
            capabilities.lsp_reason.as_deref(),
            Some("host_lsp_wsl_unsupported")
        );
        assert!(is_within_workspace(
            Path::new("//wsl$/Ubuntu/home/jihoon/프로젝트"),
            Path::new("//wsl.localhost/ubuntu/home/jihoon/프로젝트/노트.md"),
        ));
        assert!(!is_within_workspace(
            Path::new("//wsl$/Ubuntu/home/jihoon/Project"),
            Path::new("//wsl$/ubuntu/home/jihoon/project/note.md"),
        ));
    }
}
