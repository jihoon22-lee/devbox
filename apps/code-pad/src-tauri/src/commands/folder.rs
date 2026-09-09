//! Workspace discovery commands used by Quick Open.
//!
//! The filesystem walk deliberately lives on the Rust side.  Quick Open gets
//! one bounded snapshot and performs only its query ranking in the frontend.

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

/// Native product listing admits each directory before reading its children.
/// Rejected paths are omitted; cancellation and a changed root abort the result.
/// Bounds apply to traversal, retained directory handles and serialized output.
pub fn list_workspace_files_guarded(
    root: &Path,
    admit: &dyn Fn(&Path) -> Result<(), String>,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<WorkspaceFiles, String> {
    use devbox_filesystem::{filesystem_identity, open_filesystem_metadata_object};
    check()?;
    admit(root)?;
    let root = canonical_workspace(root)?;
    admit(&root)?;
    let open = |path: &Path| {
        admit(path)?;
        devbox_filesystem::ensure_no_links(path).map_err(|_| "file_listing_changed")?;
        let (handle, identity) =
            open_filesystem_metadata_object(path, true).map_err(|_| "file_listing_unavailable")?;
        let entries = std::fs::read_dir(path).map_err(|_| "file_listing_unavailable")?;
        Ok::<_, String>((path.to_path_buf(), handle, identity, entries))
    };
    let mut directories = vec![open(&root)?];
    let root_identity = directories[0].2;
    let mut result = WorkspaceFiles {
        files: Vec::new(),
        truncated: false,
        incomplete: false,
    };
    let mut visited = 0usize;
    let mut output_bytes = 0usize;
    while let Some((path, _handle, identity, entries)) = directories.last_mut() {
        check()?;
        if filesystem_identity(path.as_path(), true).ok() != Some(*identity) {
            return Err("file_listing_changed".into());
        }
        let Some(entry) = entries.next() else {
            directories.pop();
            continue;
        };
        visited += 1;
        if visited > QUICK_OPEN_MAX_ENTRIES * 8 {
            result.truncated = true;
            break;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                result.incomplete = true;
                continue;
            }
        };
        let child = entry.path();
        if !is_within_workspace(&root, &child)
            || admit(&child).is_err()
            || devbox_filesystem::ensure_no_links(&child).is_err()
        {
            result.incomplete = true;
            continue;
        }
        let metadata = match std::fs::symlink_metadata(&child) {
            Ok(metadata) => metadata,
            Err(_) => {
                result.incomplete = true;
                continue;
            }
        };
        if metadata.is_dir() {
            if devbox_filesystem::is_ignored_dir(&entry.file_name().to_string_lossy()) {
                continue;
            }
            if directories.len() >= 128 {
                result.truncated = true;
                continue;
            }
            match open(&child) {
                Ok(directory) => directories.push(directory),
                Err(_) => result.incomplete = true,
            }
        } else if metadata.is_file() {
            let Some(path) = child.to_str() else {
                result.incomplete = true;
                continue;
            };
            let relative = child
                .strip_prefix(&root)
                .map_err(|_| "file_listing_changed")?;
            let Some(relative) = relative.to_str() else {
                result.incomplete = true;
                continue;
            };
            let file = WorkspaceFile {
                path: path.into(),
                relative_path: relative.replace('\\', "/"),
                size: metadata.len(),
            };
            let bytes = serde_json::to_vec(&file)
                .map_err(|_| "file_listing_unavailable")?
                .len()
                + 1;
            if result.files.len() == QUICK_OPEN_MAX_ENTRIES
                || output_bytes + bytes > 8 * 1024 * 1024
            {
                result.truncated = true;
                break;
            }
            output_bytes += bytes;
            result.files.push(file);
        }
    }
    check()?;
    admit(&root)?;
    if filesystem_identity(&root, true).ok() != Some(root_identity) {
        return Err("file_listing_changed".into());
    }
    Ok(result)
}

/// Lists at most [`QUICK_OPEN_MAX_ENTRIES`] files below one canonical root.
///
/// The limit is applied by `collect_limited` while walking, never after a full
/// tree has been materialized in the frontend.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn list_workspace_files(path: String) -> Result<WorkspaceFiles, String> {
    tauri::async_runtime::spawn_blocking(move || list_workspace_files_blocking(&path))
        .await
        .map_err(|error| format!("작업 폴더 목록 작업이 중단되었습니다: {error}"))?
}

#[cfg(any(feature = "desktop", test))]
fn list_workspace_files_blocking(path: &str) -> Result<WorkspaceFiles, String> {
    let root = canonical_workspace(Path::new(path))?;
    let result = devbox_filesystem::collect_limited(&root, QUICK_OPEN_MAX_ENTRIES);
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

#[cfg(any(feature = "desktop", test))]
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
#[cfg(feature = "desktop")]
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
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn workspace_capabilities(path: String) -> Result<WorkspaceCapabilities, String> {
    tauri::async_runtime::spawn_blocking(move || {
        canonical_workspace(Path::new(&path)).map(|path| capabilities_for_path(&path))
    })
    .await
    .map_err(|error| format!("작업 폴더 확인 작업이 중단되었습니다: {error}"))?
}

/// Typed product adapter; the native host owns caller/session/owner admission.
#[cfg(feature = "desktop")]
pub(crate) async fn __component_list_workspace_files(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        path: String,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    let value = list_workspace_files(input.path).await?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
}

/// Typed product adapter; the native host owns caller/session/owner admission.
#[cfg(feature = "desktop")]
pub(crate) async fn __component_canonicalize_workspace(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        path: String,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    let value = canonicalize_workspace(input.path).await?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
}

/// Typed product adapter; the native host owns caller/session/owner admission.
#[cfg(feature = "desktop")]
pub(crate) async fn __component_workspace_capabilities(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        path: String,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    let value = workspace_capabilities(input.path).await?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn guarded_listing_never_descends_into_denied_directories_and_honors_cancellation() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        fs::create_dir(root.join("denied")).unwrap();
        fs::create_dir(root.join("node_modules")).unwrap();
        fs::write(root.join("denied/private.txt"), b"private").unwrap();
        fs::write(root.join("node_modules/ignored.txt"), b"ignored").unwrap();
        fs::write(root.join("visible 한글.txt"), b"visible").unwrap();
        let seen = std::cell::RefCell::new(Vec::new());
        let admit = |path: &Path| {
            seen.borrow_mut().push(path.to_path_buf());
            if path == root.join("denied") {
                Err("denied".into())
            } else {
                Ok(())
            }
        };
        let listed = list_workspace_files_guarded(&root, &admit, &|| Ok(())).unwrap();
        assert_eq!(listed.files.len(), 1);
        assert_eq!(listed.files[0].relative_path, "visible 한글.txt");
        assert!(listed.incomplete);
        assert!(!seen.borrow().contains(&root.join("denied/private.txt")));
        assert!(!seen
            .borrow()
            .contains(&root.join("node_modules/ignored.txt")));
        let checks = std::cell::Cell::new(0);
        let check = || {
            checks.set(checks.get() + 1);
            if checks.get() > 1 {
                Err("cancelled".into())
            } else {
                Ok(())
            }
        };
        assert_eq!(
            list_workspace_files_guarded(&root, &admit, &check).unwrap_err(),
            "cancelled"
        );
    }

    #[test]
    fn guarded_listing_retains_root_identity_during_replacement_attempts() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("root");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("original.txt"), b"original").unwrap();
        let root = root.canonicalize().unwrap();
        let moved = directory.path().join("moved");
        let checks = std::cell::Cell::new(0);
        let replaced = std::cell::Cell::new(false);
        let check = || {
            checks.set(checks.get() + 1);
            if checks.get() == 2 {
                match fs::rename(&root, &moved) {
                    Ok(()) => {
                        replaced.set(true);
                        fs::create_dir(&root).unwrap();
                        fs::write(root.join("replacement.txt"), b"replacement").unwrap();
                    }
                    #[cfg(windows)]
                    Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {}
                    Err(error) => panic!("unexpected fixture rename error: {error}"),
                }
            }
            Ok(())
        };
        let result = list_workspace_files_guarded(&root, &|_| Ok(()), &check);
        if replaced.get() {
            assert_eq!(result.unwrap_err(), "file_listing_changed");
        } else {
            // A platform that pins the directory must release it after the scan.
            let listed = result.unwrap();
            assert_eq!(listed.files.len(), 1);
            assert_eq!(listed.files[0].relative_path, "original.txt");
            fs::rename(&root, &moved).unwrap();
        }
        assert_eq!(fs::read(moved.join("original.txt")).unwrap(), b"original");
    }

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
        let result = list_workspace_files_blocking(&root.to_string_lossy()).unwrap();
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
