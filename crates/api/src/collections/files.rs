//! 루트 안의 파일만 읽고 쓴다. 경로 조각마다 symlink를 거부하며 만들고, 마지막 조각은 `O_NOFOLLOW`로 연다.
//! 쓰기는 같은 폴더 임시 파일 → (있음 확인) rename/hard link라 중간 상태가 남지 않는다.
use crate::ApiError;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

pub(crate) const MAX_FILE_BYTES: u64 = 1024 * 1024;
pub(crate) const MAX_FILES: usize = 2_000;
const MAX_DEPTH: usize = 16;
const MAX_PATH_BYTES: usize = 1024;
const MAX_SEGMENT_BYTES: usize = 255;
/// 임시 파일 접두사. 목록·감시·사용자 경로에서 모두 뺀다.
pub(crate) const TEMP_PREFIX: &str = ".devbox-tmp-";

pub(crate) fn digest(bytes: &[u8]) -> String {
    data_encoding::HEXLOWER.encode(&Sha256::digest(bytes))
}

/// 루트 기준 상대 경로 조각. 빈 조각·`.`·`..`·절대 경로·역슬래시·제어 문자·`.json` 아닌 끝을 거부한다.
pub(crate) fn segments(path: &str) -> Result<Vec<&str>, ApiError> {
    let invalid = ApiError::CollectionPathInvalid;
    if path.is_empty()
        || path.len() > MAX_PATH_BYTES
        || path.starts_with('/')
        || path.contains('\\')
        || path.chars().any(char::is_control)
        || !path.ends_with(".json")
    {
        return Err(invalid);
    }
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() > MAX_DEPTH
        || parts.iter().any(|p| {
            p.is_empty()
                || *p == "."
                || *p == ".."
                || p.len() > MAX_SEGMENT_BYTES
                || p.starts_with(TEMP_PREFIX)
        })
    {
        return Err(invalid);
    }
    Ok(parts)
}

pub(crate) enum Failure {
    Domain(ApiError),
    Io(io::Error),
}
impl From<ApiError> for Failure {
    fn from(e: ApiError) -> Self {
        Self::Domain(e)
    }
}
impl From<io::Error> for Failure {
    fn from(e: io::Error) -> Self {
        // symlink를 따라가려 하면(ELOOP) 경로 오류다.
        #[cfg(unix)]
        if e.raw_os_error() == Some(nix::libc::ELOOP) {
            return Self::Domain(ApiError::CollectionPathInvalid);
        }
        Self::Io(e)
    }
}
pub(crate) type F<T> = Result<T, Failure>;

/// 루트 폴더. `anchor`는 루트가 그 아래에 있어야 하는 폴더(개인 = config_dir, 프로젝트 = 프로젝트 폴더).
#[derive(Clone)]
pub(crate) struct Root {
    pub(crate) anchor: PathBuf,
    /// anchor 아래 조각(개인 `["api"]`, 프로젝트 `[".devbox", "api"]`).
    pub(crate) below: &'static [&'static str],
    pub(crate) private: bool,
}

impl Root {
    /// 루트까지의 폴더를 확인한다. `create`면 없는 조각을 만들고, 아니면 없을 때 `None`.
    fn open(&self, create: bool) -> F<Option<PathBuf>> {
        if create && self.private {
            fs::create_dir_all(&self.anchor)?;
        }
        let anchor = fs::canonicalize(&self.anchor)?;
        let mut dir = anchor.clone();
        for part in self.below {
            match step(&dir, part, create, self.private)? {
                Some(next) => dir = next,
                None => return Ok(None),
            }
        }
        if !fs::canonicalize(&dir)?.starts_with(&anchor) {
            return Err(ApiError::CollectionPathInvalid.into());
        }
        Ok(Some(dir))
    }
}

/// 폴더 한 조각: symlink·파일이면 거부, 없으면 만들거나 `None`.
fn step(dir: &Path, part: &str, create: bool, private: bool) -> F<Option<PathBuf>> {
    let next = dir.join(part);
    match fs::symlink_metadata(&next) {
        Ok(meta) if meta.file_type().is_dir() => Ok(Some(next)),
        Ok(_) => Err(ApiError::CollectionPathInvalid.into()),
        Err(e) if e.kind() == io::ErrorKind::NotFound && create => {
            make_dir(&next, private)?;
            let meta = fs::symlink_metadata(&next)?;
            if meta.file_type().is_dir() {
                Ok(Some(next))
            } else {
                Err(ApiError::CollectionPathInvalid.into())
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn make_dir(path: &Path, private: bool) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(if private { 0o700 } else { 0o755 });
    }
    #[cfg(not(unix))]
    let _ = private;
    match builder.create(path) {
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        other => other,
    }
}

/// 같은 폴더에 대소문자만 다른 이름이 있으면 그 경로(Windows 체크아웃에서 겹치지 않게).
fn case_twin(dir: &Path, name: &str) -> io::Result<Option<PathBuf>> {
    let lower = name.to_lowercase();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let other = entry.file_name();
        let other = other.to_string_lossy();
        if other != name && other.to_lowercase() == lower {
            return Ok(Some(entry.path()));
        }
    }
    Ok(None)
}

/// 부모 폴더까지 걸어 내려간다. `create`면 새 조각을 만들되 대소문자 쌍둥이가 있으면 충돌.
fn parent(root: &Root, parts: &[&str], create: bool) -> F<Option<PathBuf>> {
    let Some(mut dir) = root.open(create)? else {
        return Ok(None);
    };
    let base = dir.clone();
    for part in &parts[..parts.len() - 1] {
        if create
            && fs::symlink_metadata(dir.join(part)).is_err()
            && case_twin(&dir, part)?.is_some()
        {
            return Err(ApiError::CollectionConflict { digest: None }.into());
        }
        match step(&dir, part, create, root.private)? {
            Some(next) => dir = next,
            None => return Ok(None),
        }
    }
    if !fs::canonicalize(&dir)?.starts_with(fs::canonicalize(&base)?) {
        return Err(ApiError::CollectionPathInvalid.into());
    }
    Ok(Some(dir))
}

fn open_nofollow(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(nix::libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    #[cfg(not(unix))]
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    Ok(file)
}

/// 파일 bytes(없으면 `None`). 일반 파일이 아니거나 1MiB를 넘으면 오류.
fn read_file(path: &Path) -> F<Option<Vec<u8>>> {
    let file = match open_nofollow(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(ApiError::CollectionPathInvalid.into());
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(ApiError::CollectionTooLarge {
            limit: MAX_FILE_BYTES,
        }
        .into());
    }
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(ApiError::CollectionTooLarge {
            limit: MAX_FILE_BYTES,
        }
        .into());
    }
    Ok(Some(bytes))
}

pub(crate) fn read(root: &Root, path: &str) -> F<Vec<u8>> {
    let parts = segments(path)?;
    let dir = parent(root, &parts, false)?.ok_or(ApiError::CollectionNotFound)?;
    read_file(&dir.join(parts[parts.len() - 1]))
        .and_then(|b| b.ok_or(ApiError::CollectionNotFound.into()))
}

pub(crate) enum Condition<'a> {
    Digest(&'a str),
    Absent,
}

/// 원자적 쓰기. 호출자는 루트별 잠금을 잡고 부른다. 결과는 새 digest.
pub(crate) fn write(root: &Root, path: &str, text: &[u8], condition: Condition<'_>) -> F<String> {
    if text.len() as u64 > MAX_FILE_BYTES {
        return Err(ApiError::CollectionTooLarge {
            limit: MAX_FILE_BYTES,
        }
        .into());
    }
    let parts = segments(path)?;
    let name = parts[parts.len() - 1];
    let creating = matches!(condition, Condition::Absent);
    let dir = match parent(root, &parts, creating)? {
        Some(dir) => dir,
        None => return Err(ApiError::CollectionConflict { digest: None }.into()),
    };
    let target = dir.join(name);
    let current = read_file(&target)?;
    match (&condition, &current) {
        (Condition::Digest(expected), Some(bytes)) if digest(bytes) == *expected => {}
        (Condition::Absent, None) => {
            if let Some(twin) = case_twin(&dir, name)? {
                let other = read_file(&twin).ok().flatten();
                return Err(ApiError::CollectionConflict {
                    digest: other.map(|b| digest(&b)),
                }
                .into());
            }
        }
        (_, current) => {
            return Err(ApiError::CollectionConflict {
                digest: current.as_deref().map(digest),
            }
            .into());
        }
    }
    let temp = dir.join(format!(
        "{TEMP_PREFIX}{}.tmp",
        uuid::Uuid::new_v4().simple()
    ));
    // 덮어쓰기는 사용자가 정한 원래 권한을 유지한다.
    let keep = match condition {
        Condition::Digest(_) => fs::symlink_metadata(&target).ok().map(|m| m.permissions()),
        Condition::Absent => None,
    };
    let result = write_temp(&temp, text, root.private, keep).and_then(|()| match condition {
        Condition::Digest(_) => fs::rename(&temp, &target).map_err(Failure::from),
        Condition::Absent => link_new(&temp, &target),
    });
    let _ = fs::remove_file(&temp);
    result?;
    sync_dir(&dir);
    Ok(digest(text))
}

fn write_temp(temp: &Path, text: &[u8], private: bool, keep: Option<fs::Permissions>) -> F<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(if private { 0o600 } else { 0o644 });
    }
    #[cfg(not(unix))]
    let _ = private;
    let mut file = options.open(temp)?;
    if let Some(permissions) = keep {
        file.set_permissions(permissions)?;
    }
    file.write_all(text)?;
    file.sync_all()?;
    Ok(())
}

/// 이미 있으면 덮어쓰지 않는다. hard link가 안 되는 파일 시스템은 `create_new`로 쓴다.
fn link_new(temp: &Path, target: &Path) -> F<()> {
    match fs::hard_link(temp, target) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            let current = read_file(target)?;
            Err(ApiError::CollectionConflict {
                digest: current.as_deref().map(digest),
            }
            .into())
        }
        Err(_) => {
            let bytes = fs::read(temp)?;
            let mut file = match OpenOptions::new().write(true).create_new(true).open(target) {
                Ok(file) => file,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    return Err(ApiError::CollectionConflict { digest: None }.into());
                }
                Err(e) => return Err(e.into()),
            };
            file.write_all(&bytes)?;
            file.sync_all()?;
            Ok(())
        }
    }
}

fn sync_dir(dir: &Path) {
    if let Ok(dir) = File::open(dir) {
        let _ = dir.sync_all();
    }
}

/// 화면이 본 digest와 같을 때만 지운다.
pub(crate) fn delete(root: &Root, path: &str, expected: &str) -> F<()> {
    let parts = segments(path)?;
    let dir = parent(root, &parts, false)?.ok_or(ApiError::CollectionNotFound)?;
    let target = dir.join(parts[parts.len() - 1]);
    let current = read_file(&target)?.ok_or(ApiError::CollectionNotFound)?;
    let now = digest(&current);
    if now != expected {
        return Err(ApiError::CollectionConflict { digest: Some(now) }.into());
    }
    fs::remove_file(&target)?;
    sync_dir(&dir);
    Ok(())
}

/// 목록 항목. 파일 내용은 한 번에 하나만 읽어 digest·재검토 판정만 남기고 버린다.
pub(crate) struct Listed {
    pub(crate) path: String,
    pub(crate) size: i64,
    pub(crate) digest: Option<String>,
    /// 1MiB 초과처럼 읽지 못한 파일도 참(fail-closed).
    pub(crate) risky: bool,
    pub(crate) unreadable: bool,
}

/// 루트 아래 `.json` 파일(경로순). symlink는 따라가지 않고 임시 파일은 뺀다. 루트가 없으면 빈 목록.
pub(crate) fn list(root: &Root) -> F<(Vec<Listed>, bool)> {
    let Some(dir) = root.open(false)? else {
        return Ok((Vec::new(), false));
    };
    let mut out = Vec::new();
    let mut truncated = false;
    walk(&dir, "", 1, &mut out, &mut truncated)?;
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok((out, truncated))
}

fn walk(
    dir: &Path,
    prefix: &str,
    depth: usize,
    out: &mut Vec<Listed>,
    truncated: &mut bool,
) -> F<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        if *truncated {
            return Ok(());
        }
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if name.starts_with(TEMP_PREFIX) {
            continue;
        }
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let kind = entry.file_type()?;
        if kind.is_dir() {
            if depth < MAX_DEPTH {
                walk(&entry.path(), &rel, depth + 1, out, truncated)?;
            }
        } else if kind.is_file() && segments(&rel).is_ok() {
            if out.len() == MAX_FILES {
                *truncated = true;
                return Ok(());
            }
            let size = entry.metadata()?.len();
            // 파일 하나를 읽지 못해도(권한 등) 목록은 계속하고 그 항목만 표시한다.
            let (bytes, unreadable) = match read_file(&entry.path()) {
                Ok(bytes) => (bytes, false),
                Err(Failure::Domain(_)) => (None, false),
                Err(Failure::Io(_)) => (None, true),
            };
            out.push(Listed {
                path: rel,
                size: size as i64,
                digest: bytes.as_deref().map(digest),
                risky: bytes.as_deref().is_none_or(super::reviews::needs_review),
                unreadable,
            });
        }
    }
    Ok(())
}

/// 감시를 붙일 루트 폴더(있을 때만).
pub(crate) fn existing_dir(root: &Root) -> Option<PathBuf> {
    root.open(false).ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 존재 확인과 연결 사이에 다른 쓰기가 끼어든 경우: 덮어쓰지 않고 conflict.
    #[test]
    fn link_new_never_replaces_a_file_created_after_the_check() {
        let dir = tempfile::tempdir().expect("owned fixture");
        let temp = dir.path().join(format!("{TEMP_PREFIX}t.tmp"));
        let target = dir.path().join("a.json");
        fs::write(&temp, b"mine").expect("temp");
        fs::write(&target, b"theirs").expect("raced file");
        match link_new(&temp, &target) {
            Err(Failure::Domain(ApiError::CollectionConflict { digest })) => {
                assert_eq!(digest, Some(super::digest(b"theirs")));
            }
            _ => panic!("expected a conflict"),
        }
        assert_eq!(fs::read(&target).expect("target"), b"theirs");
    }
}
