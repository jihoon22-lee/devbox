use crate::ignore::is_ignored_dir;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// 인덱스할 파일 하나
#[derive(Debug, Clone)]
pub struct IndexedFile {
    pub path: PathBuf,
    pub size: i64,
    pub modified_ts: i64,
}

/// 파일 순회의 결과와 상한 초과 여부.
#[derive(Debug, Clone)]
pub struct WalkResult {
    /// 상한 안에 수집된 파일 목록.
    pub files: Vec<IndexedFile>,
    /// File-count or directory-entry work budget was exceeded.
    pub truncated: bool,
    /// At least one directory entry or metadata record could not be read.
    /// Consumers that reconcile deletions must retain their previous state
    /// when this is true because absence from the snapshot is not evidence of
    /// deletion.
    pub incomplete: bool,
}

/// 루트 디렉터리를 순회하며 인덱스 대상 파일을 수집한다 (제외 규칙 적용).
pub fn collect(root: &Path) -> Vec<IndexedFile> {
    collect_inner(root, None, None).files
}

/// 루트 디렉터리를 순회하며 최대 `max_entries`개의 파일을 수집한다.
///
/// 기존 [`collect`]와 같은 ignore·파일 판정·metadata 오류 무시 규칙을 사용한다.
/// File-count exhaustion requires an additional eligible file. Independently, visiting
/// more than max(1024, 4 * max_entries) entries reports truncation even without files.
pub fn collect_limited(root: &Path, max_entries: usize) -> WalkResult {
    collect_bounded(root, max_entries, max_entries.saturating_mul(4).max(1024))
}

/// Bound files and all visited entries independently, including empty/ignored directories.
pub fn collect_bounded(root: &Path, max_files: usize, max_visited: usize) -> WalkResult {
    collect_inner(root, Some(max_files), Some(max_visited))
}
fn collect_inner(
    root: &Path,
    max_entries: Option<usize>,
    max_visited: Option<usize>,
) -> WalkResult {
    let mut out = Vec::new();
    let mut incomplete = false;
    let mut walker = WalkDir::new(root).into_iter();
    let mut visited = 0usize;
    while let Some(entry) = walker.next() {
        if max_visited.is_some_and(|limit| visited >= limit) {
            return WalkResult {
                files: out,
                truncated: true,
                incomplete,
            };
        }
        visited = visited.saturating_add(1);
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                incomplete = true;
                continue;
            }
        };
        if entry.file_type().is_dir() && is_ignored_dir(&entry.file_name().to_string_lossy()) {
            walker.skip_current_dir();
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => {
                incomplete = true;
                continue;
            }
        };

        if let Some(limit) = max_entries {
            if out.len() >= limit {
                return WalkResult {
                    files: out,
                    truncated: true,
                    incomplete,
                };
            }
        }

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

    WalkResult {
        files: out,
        truncated: false,
        incomplete,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEST_DIR: AtomicUsize = AtomicUsize::new(0);

    fn new_test_dir() -> PathBuf {
        let id = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("filesystem-test-{}-{id}", std::process::id()))
    }

    #[test]
    fn empty_directories_exhaust_work_without_exhausting_file_count() {
        let root = new_test_dir();
        fs::create_dir_all(&root).unwrap();
        for index in 0..40 {
            fs::create_dir(root.join(format!("empty-{index}"))).unwrap();
        }
        let result = collect_bounded(&root, 100, 10);
        assert!(result.truncated);
        assert!(!result.incomplete);
        assert!(result.files.is_empty());
        assert!(!collect_bounded(&root, 100, 41).truncated);
        fs::remove_dir_all(root).unwrap();
    }

    fn setup() -> PathBuf {
        let dir = new_test_dir();
        fs::create_dir_all(dir.join("src/nested")).unwrap();
        fs::create_dir_all(dir.join("node_modules/pkg")).unwrap();
        fs::write(dir.join("src/a.rs"), "fn main(){}").unwrap();
        fs::write(dir.join("src/nested/b.md"), "# hi").unwrap();
        fs::write(dir.join("node_modules/pkg/c.js"), "x").unwrap();
        fs::write(dir.join("README.md"), "readme").unwrap();
        dir
    }

    fn setup_empty() -> PathBuf {
        let dir = new_test_dir();
        fs::create_dir_all(&dir).unwrap();
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

    #[test]
    fn collect_limited_truncates_when_another_file_exists() {
        let dir = setup();
        let result = collect_limited(&dir, 2);

        assert_eq!(result.files.len(), 2);
        assert!(result.truncated);
        assert!(!result.incomplete);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn collect_limited_is_not_truncated_at_exact_file_count() {
        let dir = setup();
        let result = collect_limited(&dir, 3);

        assert_eq!(result.files.len(), 3);
        assert!(!result.truncated);
        assert!(!result.incomplete);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn collect_limited_zero_reports_only_when_a_file_exists() {
        let dir = setup();
        let result = collect_limited(&dir, 0);

        assert!(result.files.is_empty());
        assert!(result.truncated);
        assert!(!result.incomplete);
        let _ = fs::remove_dir_all(&dir);

        let empty_dir = setup_empty();
        let result = collect_limited(&empty_dir, 0);

        assert!(result.files.is_empty());
        assert!(!result.truncated);
        assert!(!result.incomplete);
        let _ = fs::remove_dir_all(&empty_dir);
    }

    #[test]
    fn missing_root_is_incomplete_instead_of_an_empty_authoritative_snapshot() {
        let missing = new_test_dir().join("missing");
        let result = collect_limited(&missing, 10);
        assert!(result.files.is_empty());
        assert!(!result.truncated);
        assert!(result.incomplete);
    }
}
