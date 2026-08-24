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

#[cfg(unix)]
use std::fs::File;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ATOMIC_FILE: AtomicU64 = AtomicU64::new(0);

/// Atomically replace one file with complete bytes from a unique sibling.
///
/// The temporary file is created with `create_new`, flushed, synced, closed,
/// and then committed with an overwrite-capable rename. A failed commit cleans
/// up only its own unique temporary file. The target's parent directory must
/// already exist so callers retain ownership of directory policy.
pub fn atomic_write(path: impl AsRef<Path>, contents: &[u8]) -> io::Result<()> {
    let path = path.as_ref();
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))?;
    if !parent.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "target parent directory does not exist",
        ));
    }

    for _ in 0..32 {
        let temporary = atomic_temporary_path(path)?;
        let open = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary);
        let mut file = match open {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };

        let result = (|| {
            file.write_all(contents)?;
            file.flush()?;
            file.sync_all()?;
            drop(file);
            replace_file(&temporary, path)?;
            let _ = sync_parent(path);
            Ok(())
        })();
        if let Err(error) = result {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        return Ok(());
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "too many atomic temporary-file collisions",
    ))
}

fn atomic_temporary_path(target: &Path) -> io::Result<PathBuf> {
    let file_name = target
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?
        .to_string_lossy();
    let sequence = NEXT_ATOMIC_FILE.fetch_add(1, Ordering::Relaxed);
    Ok(target.with_file_name(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        sequence
    )))
}

#[cfg(not(windows))]
fn replace_file(temporary: &Path, target: &Path) -> io::Result<()> {
    fs::rename(temporary, target)
}

#[cfg(windows)]
fn replace_file(temporary: &Path, target: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let temporary: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        MoveFileExW(
            PCWSTR(temporary.as_ptr()),
            PCWSTR(target.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(io::Error::other)
}

#[cfg(unix)]
fn sync_parent(target: &Path) -> io::Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))?;
    File::open(parent).and_then(|directory| directory.sync_all())
}

#[cfg(not(unix))]
fn sync_parent(_target: &Path) -> io::Result<()> {
    Ok(())
}

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

#[cfg(test)]
mod atomic_write_tests {
    use super::atomic_write;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEST_DIR: AtomicUsize = AtomicUsize::new(0);

    fn new_test_dir() -> PathBuf {
        let id = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "filesystem-atomic-test-{}-{id}",
            std::process::id()
        ))
    }

    #[test]
    fn creates_and_replaces_complete_contents_without_leaving_temporary_files() {
        let root = new_test_dir();
        let target = root.join("state.json");
        fs::create_dir_all(&root).unwrap();

        atomic_write(&target, br#"{"revision":1}"#).unwrap();
        atomic_write(&target, br#"{"revision":2,"complete":true}"#).unwrap();

        assert_eq!(
            fs::read(&target).unwrap(),
            br#"{"revision":2,"complete":true}"#
        );
        let names = fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["state.json"]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn refuses_to_create_an_unowned_parent_directory() {
        let root = new_test_dir();
        let error = atomic_write(root.join("missing/state.json"), b"x").unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        assert!(!root.exists());
    }
}
