use std::path::Path;

/// 루트 내부의 파일·폴더 트리를 상대 경로로 수집한다.
///
/// 반환하는 상대 경로는 항상 `/` 구분자를 쓴다. 프론트(`App.tsx`)가
/// `path.split("/")`로 들여쓰기 깊이를 계산하고 파일명을 뽑아 쓰므로, OS
/// 네이티브 구분자(Windows는 `\`)를 그대로 넘기면 트리 들여쓰기가 무너지고
/// 파일명 자리에 전체 상대 경로가 나온다.
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
                .replace('\\', "/");
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
