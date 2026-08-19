//! 파일 순회와 디렉터리 제외 규칙을 제공하는 공용 크레이트.
//!
//! - [`is_ignored_dir`]: gitignore 스타일로 흔히 제외되는 디렉터리 이름 판정
//! - [`collect`] / [`IndexedFile`]: 루트 디렉터리를 순회하며 제외 규칙을 적용한
//!   파일 목록 수집
//! - [`collect_limited`] / [`WalkResult`]: 같은 순회를 상한과 `truncated` 상태와
//!   함께 수행하는 빠른 열기용 API
//! - [`migrate_legacy_identifier_dir`]: 새 identifier 디렉터리가 아직 없을 때
//!   구 identifier 디렉터리를 통째로 옮기는 rename-only migration
//!
//! 이 크레이트가 담지 않는 것:
//! - **watcher (파일 변경 감시)**: 아직 실제 소비자가 없다. `notify` 등으로
//!   구현할 필요가 생기면 그때 추가한다 (`CONVENTIONS.md`의 "두 번 이상
//!   실제로 필요해진 코드만 추출" 원칙).
//! - **"내용을 인덱싱할 파일인가" 판단** (텍스트 확장자, 최대 크기 등): 이는
//!   순회된 파일을 어떻게 쓸지 결정하는 소비자 고유의 관심사다. 예를 들어
//!   everything-plus는 내용 검색을 위해 텍스트 확장자만 골라 읽지만,
//!   code-pad 같은 다른 소비자는 다른 기준을 쓸 수 있다. 각 소비자가 직접
//!   구현한다.

pub mod ignore;
pub mod walk;

pub use ignore::is_ignored_dir;
pub use walk::{collect, collect_limited, IndexedFile, WalkResult};

use std::path::Path;

/// Move a legacy identifier directory into its current identifier directory.
///
/// The destination is never merged with or overwritten. If the destination
/// already exists, or the legacy directory is absent, this is a no-op. A
/// rename error is returned unchanged so callers can log it and retry on the
/// next launch.
pub fn migrate_legacy_identifier_dir(
    base_dir: impl AsRef<Path>,
    legacy_identifier: &str,
    current_identifier: &str,
) -> std::io::Result<()> {
    let base_dir = base_dir.as_ref();
    let current_dir = base_dir.join(current_identifier);
    if current_dir.try_exists()? {
        return Ok(());
    }

    let legacy_dir = base_dir.join(legacy_identifier);
    if !legacy_dir.try_exists()? {
        return Ok(());
    }

    std::fs::rename(legacy_dir, current_dir)
}

#[cfg(test)]
mod migration_tests {
    use super::migrate_legacy_identifier_dir;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEST_DIR: AtomicUsize = AtomicUsize::new(0);

    fn new_test_dir() -> PathBuf {
        let id = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "filesystem-migration-test-{}-{id}",
            std::process::id()
        ))
    }

    #[test]
    fn renames_complete_legacy_identifier_directory_without_losing_bytes() {
        let root = new_test_dir();
        let legacy = root.join("com.workbench.example");
        let marker = legacy.join("EBWebView/Default/marker.bin");
        let expected = [0, 1, 2, 127, 128, 255];
        fs::create_dir_all(marker.parent().unwrap()).unwrap();
        fs::write(&marker, expected).unwrap();

        migrate_legacy_identifier_dir(&root, "com.workbench.example", "com.devbox.example")
            .unwrap();

        assert!(!legacy.exists());
        assert_eq!(
            fs::read(root.join("com.devbox.example/EBWebView/Default/marker.bin")).unwrap(),
            expected
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn leaves_legacy_and_current_directories_untouched_when_current_exists() {
        let root = new_test_dir();
        let legacy = root.join("com.workbench.example");
        let current = root.join("com.devbox.example");
        fs::create_dir_all(&legacy).unwrap();
        fs::create_dir_all(&current).unwrap();
        fs::write(legacy.join("legacy.bin"), [1, 2, 3]).unwrap();
        fs::write(current.join("current.bin"), [4, 5, 6]).unwrap();

        migrate_legacy_identifier_dir(&root, "com.workbench.example", "com.devbox.example")
            .unwrap();

        assert_eq!(fs::read(legacy.join("legacy.bin")).unwrap(), [1, 2, 3]);
        assert_eq!(fs::read(current.join("current.bin")).unwrap(), [4, 5, 6]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn does_nothing_when_legacy_directory_is_absent() {
        let root = new_test_dir();
        fs::create_dir_all(&root).unwrap();

        migrate_legacy_identifier_dir(&root, "com.workbench.example", "com.devbox.example")
            .unwrap();

        assert!(!root.join("com.devbox.example").exists());
        let _ = fs::remove_dir_all(root);
    }
}
