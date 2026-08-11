use std::path::{Component, Path, PathBuf};

/// 루트 외부로 벗어나지 않는 안전한 상대 경로 결합.
pub fn safe_join(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let path = root.join(rel);
    if path
        .components()
        .any(|c| matches!(c, Component::ParentDir | Component::RootDir))
    {
        return Err("경로가 루트 밖을 벗어납니다".into());
    }
    if !path.starts_with(root) {
        return Err("경로가 루트 밖을 벗어납니다".into());
    }
    Ok(path)
}

/// 루트 내부의 파일·폴더 트리를 상대 경로로 수집한다.
pub fn tree(root: &Path) -> Result<Vec<(String, bool)>, String> {
    let mut out = Vec::new();
    fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, bool)>) -> std::io::Result<()> {
        let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let ft = e.file_type()?;
            let rel = e
                .path()
                .strip_prefix(root)
                .unwrap_or(&e.path())
                .to_string_lossy()
                .into_owned();
            let is_dir = ft.is_dir();
            out.push((rel, is_dir));
            if is_dir {
                walk(&e.path(), root, out)?;
            }
        }
        Ok(())
    }
    walk(root, root, &mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

pub fn read_file(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| e.to_string())
}

pub fn write_file(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, content).map_err(|e| e.to_string())
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
    for sub in ["Projects", "Notes", "Journal", "Reference", "Archive"] {
        std::fs::create_dir_all(root.join(sub)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn safe_join_rejects_traversal() {
        let root = Path::new("C:/knowledge");
        assert!(safe_join(root, "../evil").is_err());
        assert!(safe_join(root, "/etc/passwd").is_err());
        assert!(safe_join(root, "Notes/a.md").is_ok());
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
}
