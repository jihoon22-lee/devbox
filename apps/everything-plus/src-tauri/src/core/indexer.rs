use crate::core::ignore::is_ignored_dir;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// 인덱스할 파일 하나
#[derive(Debug, Clone)]
pub struct IndexedFile {
    pub path: PathBuf,
    pub size: i64,
    pub modified_ts: i64,
}

/// 루트 디렉터리를 순회하며 인덱스 대상 파일을 수집한다 (제외 규칙 적용).
pub fn collect(root: &Path) -> Vec<IndexedFile> {
    let mut out = Vec::new();
    for entry in WalkDir::new(root).into_iter().filter_entry(|e| {
        !(e.file_type().is_dir() && is_ignored_dir(&e.file_name().to_string_lossy()))
    }) {
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_file() {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        out.push(IndexedFile {
            path: entry.path().to_path_buf(),
            size: meta.len() as i64,
            modified_ts: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("evp-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src/nested")).unwrap();
        fs::create_dir_all(dir.join("node_modules/pkg")).unwrap();
        fs::write(dir.join("src/a.rs"), "fn main(){}").unwrap();
        fs::write(dir.join("src/nested/b.md"), "# hi").unwrap();
        fs::write(dir.join("node_modules/pkg/c.js"), "x").unwrap();
        fs::write(dir.join("README.md"), "readme").unwrap();
        dir
    }

    #[test]
    fn collects_files_but_skips_ignored_dirs() {
        let dir = setup();
        let files = collect(&dir);
        let paths: Vec<String> = files
            .iter()
            .map(|f| f.path.to_string_lossy().into_owned())
            .collect();
        assert!(paths.iter().any(|p| p.ends_with("a.rs")));
        assert!(paths.iter().any(|p| p.ends_with("b.md")));
        assert!(paths.iter().any(|p| p.ends_with("README.md")));
        assert!(!paths.iter().any(|p| p.contains("node_modules")));
        let _ = fs::remove_dir_all(&dir);
    }
}
