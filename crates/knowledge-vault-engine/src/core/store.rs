use std::path::Path;

#[derive(Clone, Copy)]
pub struct TreeLimits {
    pub entries: usize,
    pub directories: usize,
    pub depth: usize,
    pub response_bytes: usize,
    pub duration: std::time::Duration,
}

impl Default for TreeLimits {
    fn default() -> Self {
        Self {
            entries: 20_000,
            directories: 5_000,
            depth: 64,
            response_bytes: 4 * 1024 * 1024,
            duration: std::time::Duration::from_secs(5),
        }
    }
}

/// 루트 내부의 파일·폴더 트리를 상대 경로로 수집한다.
///
/// 반환하는 상대 경로는 항상 `/` 구분자를 쓴다. 프론트(`App.tsx`)가
/// `path.split("/")`로 들여쓰기 깊이를 계산하고 파일명을 뽑아 쓰므로, OS
/// 네이티브 구분자(Windows는 `\`)를 그대로 넘기면 트리 들여쓰기가 무너지고
/// 파일명 자리에 전체 상대 경로가 나온다.
pub fn tree(root: &Path) -> Result<Vec<(String, bool)>, String> {
    tree_bounded(root, TreeLimits::default(), || false)
}

/// All-or-error: a limited, cancelled or unreadable scan never becomes deletion
/// evidence. Directory iteration is streaming; even an empty-directory forest
/// consumes work. Budgets are cooperative between OS calls, not I/O preemption.
pub fn tree_bounded(
    root: &Path,
    limits: TreeLimits,
    cancelled: impl Fn() -> bool,
) -> Result<Vec<(String, bool)>, String> {
    let deadline = std::time::Instant::now() + limits.duration;
    let check = || -> Result<(), String> {
        if cancelled() {
            return Err("metadata_cancelled".into());
        }
        if std::time::Instant::now() >= deadline {
            return Err("metadata_timeout".into());
        }
        Ok(())
    };
    check()?;
    let open = |path: &Path| {
        devbox_filesystem::ensure_no_links(path).map_err(|_| "metadata_incomplete")?;
        std::fs::read_dir(path).map_err(|_| "metadata_incomplete")
    };
    if limits.directories == 0 {
        return Err("metadata_limit".into());
    }
    let mut stack = vec![open(root)?];
    let mut directories = 1;
    let mut bytes = 2usize;
    let mut out = Vec::new();
    while !stack.is_empty() {
        check()?;
        let Some(entry) = stack.last_mut().unwrap().next() else {
            stack.pop();
            continue;
        };
        let entry = entry.map_err(|_| "metadata_incomplete")?;
        if out.len() >= limits.entries || stack.len() > limits.depth {
            return Err("metadata_limit".into());
        }
        let kind = entry.file_type().map_err(|_| "metadata_incomplete")?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|_| "metadata_incomplete")?
            .to_str()
            .ok_or("metadata_incomplete")?
            .replace('\\', "/");
        // JSON escaping takes at most six bytes per UTF-8 byte. Include field
        // names, booleans, separators and braces rather than path bytes alone.
        bytes = bytes.saturating_add(relative.len().saturating_mul(6).saturating_add(64));
        if bytes > limits.response_bytes {
            return Err("metadata_limit".into());
        }
        out.push((relative, kind.is_dir()));
        if kind.is_dir() {
            directories += 1;
            if directories > limits.directories {
                return Err("metadata_limit".into());
            }
            check()?;
            stack.push(open(&path)?);
        }
    }
    check()?;
    // Preserve parent-first, per-directory filename order without buffering an
    // unbounded directory before the first budget check.
    out.sort_by(|a, b| a.0.split('/').cmp(b.0.split('/')));
    check()?;
    Ok(out)
}

#[cfg(all(test, windows))]
pub fn read_file(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| e.to_string())
}

#[cfg(test)]
pub fn write_file(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    devbox_filesystem::atomic_write(path, content.as_bytes()).map_err(|e| e.to_string())
}

pub fn delete_file(path: &Path) -> Result<(), String> {
    if path.is_dir() {
        std::fs::remove_dir_all(path).map_err(|e| e.to_string())
    } else {
        std::fs::remove_file(path).map_err(|e| e.to_string())
    }
}

/// KnowledgeRoot 기본 하위 폴더 구조를 만든다.
pub fn ensure_layout(root: &Path) -> Result<(), String> {
    fn directory(path: &Path) -> Result<(), String> {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_dir() => devbox_filesystem::ensure_no_links(path)
                .map_err(|_| "저장 위치에 링크를 사용할 수 없습니다".to_owned()),
            Ok(_) => Err("저장 위치가 디렉터리가 아닙니다".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                directory(path.parent().ok_or("저장 위치를 확인할 수 없습니다")?)?;
                std::fs::create_dir(path).map_err(|_| "저장 위치를 만들 수 없습니다")?;
                devbox_filesystem::ensure_no_links(path)
                    .map_err(|_| "저장 위치에 링크를 사용할 수 없습니다".to_owned())
            }
            Err(_) => Err("저장 위치를 확인할 수 없습니다".into()),
        }
    }
    directory(root)?;
    let folders = ["Projects", "Notes", "Journal", "Reference", "Archive"];
    // Inspect every existing child before creating any missing layout folder.
    for sub in folders {
        let path = root.join(sub);
        match std::fs::symlink_metadata(&path) {
            Ok(_) => directory(&path)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("저장 위치를 확인할 수 없습니다".into()),
        }
    }
    for sub in folders {
        directory(&root.join(sub))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[test]
    fn tree_limits_include_empty_directories_entries_depth_and_json_bytes() {
        let root = tempfile::tempdir().unwrap();
        for i in 0..20 {
            fs::create_dir(root.path().join(format!("empty-{i}"))).unwrap();
        }
        let base = TreeLimits::default();
        for limits in [
            TreeLimits {
                entries: 10,
                ..base
            },
            TreeLimits {
                directories: 10,
                ..base
            },
            TreeLimits {
                response_bytes: 128,
                ..base
            },
        ] {
            assert_eq!(
                tree_bounded(root.path(), limits, || false).unwrap_err(),
                "metadata_limit"
            );
        }
        assert_eq!(tree(root.path()).unwrap().len(), 20);
        fs::create_dir_all(root.path().join("a/b/c")).unwrap();
        fs::write(root.path().join("a/b/c/note.md"), "keep").unwrap();
        assert_eq!(
            tree_bounded(root.path(), TreeLimits { depth: 2, ..base }, || false).unwrap_err(),
            "metadata_limit"
        );
    }

    #[test]
    fn cancellation_and_deadline_stop_mid_scan_without_returning_partial_entries() {
        let root = tempfile::tempdir().unwrap();
        for i in 0..20 {
            fs::write(root.path().join(format!("{i}.md")), "x").unwrap();
        }
        let checks = std::cell::Cell::new(0);
        let result = tree_bounded(root.path(), TreeLimits::default(), || {
            checks.set(checks.get() + 1);
            checks.get() == 6
        });
        assert_eq!(result.unwrap_err(), "metadata_cancelled");
        assert_eq!(checks.get(), 6);
        let checks = std::cell::Cell::new(0);
        let result = tree_bounded(
            root.path(),
            TreeLimits {
                duration: std::time::Duration::from_millis(5),
                ..Default::default()
            },
            || {
                checks.set(checks.get() + 1);
                if checks.get() == 3 {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                false
            },
        );
        assert_eq!(result.unwrap_err(), "metadata_timeout");
        assert!(checks.get() <= 3);
        assert_eq!(
            tree(&root.path().join("missing")).unwrap_err(),
            "metadata_incomplete"
        );
    }

    #[test]
    fn tree_preserves_parent_first_order_and_bounds_serialized_response() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("a")).unwrap();
        fs::write(root.path().join("a/z.md"), "x").unwrap();
        fs::write(root.path().join("a.md"), "x").unwrap();
        let entries = tree(root.path()).unwrap();
        assert_eq!(
            entries.iter().map(|e| e.0.as_str()).collect::<Vec<_>>(),
            ["a", "a/z.md", "a.md"]
        );
        let response = entries
            .iter()
            .map(|(path, is_dir)| serde_json::json!({ "path": path, "is_dir": is_dir }))
            .collect::<Vec<_>>();
        assert!(
            serde_json::to_vec(&response).unwrap().len() <= TreeLimits::default().response_bytes
        );
    }

    #[test]
    #[cfg(unix)]
    fn layout_rejects_linked_vault_or_child_before_creating_outside_folders() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let vault = root.path().join("vault");
        std::os::unix::fs::symlink(outside.path(), &vault).unwrap();
        assert!(ensure_layout(&vault).is_err());
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
        std::fs::remove_file(&vault).unwrap();
        std::fs::create_dir(&vault).unwrap();
        std::os::unix::fs::symlink(outside.path(), vault.join("Notes")).unwrap();
        assert!(ensure_layout(&vault).is_err());
        assert!(!vault.join("Projects").exists());
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
    }

    #[test]
    fn tree_lists_files_and_dirs() {
        let dir = std::env::temp_dir().join(format!("kb-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("Notes/nested")).unwrap();
        fs::write(dir.join("Notes/a.md"), "a").unwrap();
        fs::write(dir.join("Notes/nested/b.md"), "b").unwrap();

        let entries = tree(&dir).unwrap();
        assert!(entries.iter().any(|(p, d)| p == "Notes" && *d));
        assert!(entries.iter().any(|(p, d)| p == "Notes/a.md" && !*d));
        assert!(entries.iter().any(|(p, d)| p == "Notes/nested/b.md" && !*d));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_file_atomically_replaces_contents_without_temporary_residue() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("Notes/note.md");
        write_file(&target, "first").unwrap();
        write_file(&target, "second").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "second");
        let names = fs::read_dir(target.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["note.md"]);
    }
}
