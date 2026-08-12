//! `session.json` persistence.
//!
//! Session state is disposable metadata, not a document cache.  A malformed
//! or unknown-version file therefore becomes an empty session in memory and is
//! never overwritten merely because it was read.

use crate::core::session::Session;
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

#[cfg(unix)]
use std::fs::File;

pub const SESSION_FILE_NAME: &str = "session.json";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedSession {
    pub session: Session,
    /// False means the on-disk file was corrupt or had an unknown schema. The
    /// frontend may start with an empty session, but must not immediately
    /// overwrite the evidence before the user makes a change.
    pub persist_allowed: bool,
}

pub fn session_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|directory| directory.join(SESSION_FILE_NAME))
        .map_err(|error| format!("앱 데이터 폴더를 확인할 수 없습니다: {error}"))
}

/// Loads a session from an explicit path.  Missing files and invalid JSON use
/// the same empty-session policy; filesystem errors other than NotFound are
/// returned so permission problems remain visible to the caller.
pub fn load_from_path(path: &Path) -> Result<Session, String> {
    Ok(load_from_path_with_status(path)?.session)
}

pub fn load_from_path_with_status(path: &Path) -> Result<LoadedSession, String> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(LoadedSession {
                session: Session::empty(),
                persist_allowed: true,
            })
        }
        Err(error) => return Err(format!("세션을 읽을 수 없습니다: {error}")),
    };
    match Session::from_json(&contents) {
        Ok(session) => Ok(LoadedSession {
            session,
            persist_allowed: true,
        }),
        Err(_) => Ok(LoadedSession {
            session: Session::empty(),
            persist_allowed: false,
        }),
    }
}

/// Writes a complete file through a sibling temporary file and an atomic
/// replacement.  The helper is public so the atomicity contract can be tested
/// without constructing a Tauri `AppHandle`.
pub fn atomic_write(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "session path has no parent"))?;
    fs::create_dir_all(parent)?;

    let file_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "session path has no filename"))?
        .to_string_lossy();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let pid = std::process::id();

    for attempt in 0..100u32 {
        let temporary = parent.join(format!(".{file_name}.tmp-{pid}-{nonce}-{attempt}"));
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
            // Windows does not allow replacing an open temporary file. Unix
            // permits the rename, but closing it here keeps the atomic commit
            // contract identical on both platforms.
            drop(file);
            replace_file(&temporary, path)?;
            // The rename is the commit point.  Parent fsync is best effort:
            // reporting an error after a successful replacement would make
            // callers retry and obscure the already-persisted session.
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
        "too many session temporary-file collisions",
    ))
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
fn sync_parent(path: &Path) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "session path has no parent"))?;
    File::open(parent).and_then(|directory| directory.sync_all())
}

#[cfg(not(unix))]
fn sync_parent(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// Tauri command for restoring persisted metadata.
#[tauri::command]
pub async fn load_session(app: AppHandle) -> Result<LoadedSession, String> {
    let path = session_path(&app)?;
    tauri::async_runtime::spawn_blocking(move || load_from_path_with_status(&path))
        .await
        .map_err(|error| format!("세션 읽기 작업이 중단되었습니다: {error}"))?
}

/// Tauri command for writing persisted metadata.  The core validator rejects
/// malformed view/document relationships before anything reaches disk.
#[tauri::command]
pub async fn save_session(app: AppHandle, session: Session) -> Result<(), String> {
    session.validate().map_err(|error| error.to_string())?;
    let json = session
        .to_json()
        .map_err(|error| format!("세션을 직렬화할 수 없습니다: {error}"))?;
    let path = session_path(&app)?;
    tauri::async_runtime::spawn_blocking(move || atomic_write(&path, json.as_bytes()))
        .await
        .map_err(|error| format!("세션 저장 작업이 중단되었습니다: {error}"))?
        .map_err(|error| format!("세션을 저장할 수 없습니다: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::session::{SessionDoc, SESSION_VERSION};

    #[test]
    fn corrupt_or_version_mismatched_files_are_empty_without_rewrite() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(SESSION_FILE_NAME);
        fs::write(&path, "{not json").unwrap();
        let before = fs::read(&path).unwrap();
        assert_eq!(load_from_path(&path).unwrap(), Session::empty());
        assert!(!load_from_path_with_status(&path).unwrap().persist_allowed);
        assert_eq!(fs::read(&path).unwrap(), before);

        let mut unknown = Session::empty();
        unknown.version = SESSION_VERSION + 1;
        fs::write(&path, serde_json::to_vec(&unknown).unwrap()).unwrap();
        let before = fs::read(&path).unwrap();
        assert_eq!(load_from_path(&path).unwrap(), Session::empty());
        assert!(!load_from_path_with_status(&path).unwrap().persist_allowed);
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn atomic_write_replaces_complete_json() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(SESSION_FILE_NAME);
        let mut session = Session::empty();
        session.docs.push(SessionDoc::new("doc", "/tmp/doc.rs"));
        session.views[0].push("doc".to_string());
        atomic_write(&path, session.to_json().unwrap().as_bytes()).unwrap();
        assert_eq!(
            Session::from_json(&fs::read_to_string(&path).unwrap()).unwrap(),
            session
        );
    }
}
