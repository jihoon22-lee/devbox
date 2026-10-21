//! Filesystem primitives relative to an already-checked parent directory fd.
//! Ported from v0.9.0 `wsl-helper/document.rs` and `document_publish.rs` with
//! `rustix` instead of raw `libc` syscalls (`unsafe_code = "deny"`, musl builds).
use crate::ledger::TmpLedger;
use crate::model::{DiskRev, Eol, ReadOnlyReason};
use rustix::fs::{
    AtFlags, CWD, FileType, Mode, OFlags, RenameFlags, Stat, fchmod, fstat, fsync, openat,
    renameat_with, statat, unlinkat,
};
use rustix::io::Errno;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::path::Path;

const BOM: &[u8] = b"\xEF\xBB\xBF";

/// Display never includes a path or document text.
#[derive(Debug, thiserror::Error)]
pub enum SysError {
    #[error("document changed on disk")]
    Conflict(Option<DiskRev>),
    #[error("target already exists")]
    Exists,
    #[error("document is missing")]
    NotFound,
    #[error("filesystem does not support atomic exchange")]
    Unsupported,
    #[error("document is a symlink")]
    Link,
    #[error("document is not a regular file")]
    NotFile,
    #[error("interrupted by a test hook")]
    Interrupted,
    #[error("filesystem error: {0}")]
    Io(Errno),
}
impl From<Errno> for SysError {
    fn from(e: Errno) -> Self {
        SysError::Io(e)
    }
}
impl From<std::io::Error> for SysError {
    fn from(e: std::io::Error) -> Self {
        SysError::Io(Errno::from_io_error(&e).unwrap_or(Errno::IO))
    }
}
fn rename_error(e: Errno) -> SysError {
    match e {
        Errno::EXIST | Errno::NOTEMPTY => SysError::Exists,
        Errno::NOENT => SysError::NotFound,
        Errno::NOSYS | Errno::INVAL => SysError::Unsupported,
        e => SysError::Io(e),
    }
}

/// Bytes are kept only while within `limit`; the revision always covers the whole file.
#[derive(Debug)]
pub struct DiskFile {
    pub bytes: Option<Vec<u8>>,
    pub rev: DiskRev,
}

pub fn open_dir(path: &Path) -> std::io::Result<OwnedFd> {
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    Ok(openat(CWD, path, flags, Mode::empty())?)
}

pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::with_capacity(64), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}
fn rev_of(hash: String, size: u64, stat: &Stat) -> DiskRev {
    let mtime = i128::from(stat.st_mtime) * 1_000_000_000 + stat.st_mtime_nsec as i128;
    DiskRev {
        hash,
        size,
        mtime_ns: mtime.to_string(),
        ino: stat.st_ino.to_string(),
    }
}

/// Reads a regular file without following a final symlink. `Ok(None)` = absent.
pub fn read_at(dir: &OwnedFd, name: &str, limit: u64) -> Result<Option<DiskFile>, SysError> {
    let flags =
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK | OFlags::NOCTTY;
    let fd = match openat(dir, name, flags, Mode::empty()) {
        Ok(fd) => fd,
        Err(Errno::NOENT) => return Ok(None),
        Err(Errno::LOOP) => return Err(SysError::Link),
        Err(e) => return Err(SysError::Io(e)),
    };
    if FileType::from_raw_mode(fstat(&fd)?.st_mode) != FileType::RegularFile {
        return Err(SysError::NotFile);
    }
    let mut file = File::from(fd);
    let (mut hasher, mut kept, mut total) = (Sha256::new(), Vec::new(), 0_u64);
    let mut buf = vec![0_u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
        if total <= limit {
            kept.extend_from_slice(&buf[..n]);
        } else if !kept.is_empty() {
            kept = Vec::new();
        }
    }
    let stat = fstat(&file)?;
    Ok(Some(DiskFile {
        bytes: (total <= limit).then_some(kept),
        rev: rev_of(hex(&hasher.finalize()), total, &stat),
    }))
}

/// Creates `name` exclusively, writes, fsyncs and returns its revision.
fn write_new(
    dir: &OwnedFd,
    name: &str,
    bytes: &[u8],
    mode: Option<u32>,
) -> Result<DiskRev, SysError> {
    let flags = OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let fd = openat(dir, name, flags, Mode::from_raw_mode(0o666))?;
    if let Some(mode) = mode {
        // O_CREAT applies umask; the replaced file's exact mode wins.
        fchmod(&fd, Mode::from_raw_mode(mode))?;
    }
    let mut file = File::from(fd);
    file.write_all(bytes)?;
    file.sync_all()?;
    let stat = fstat(&file)?;
    Ok(rev_of(
        hex(&Sha256::digest(bytes)),
        bytes.len() as u64,
        &stat,
    ))
}

/// `(folder, temp name)` callback run after the first exchange (tests only).
pub type AfterExchange<'a> = &'a dyn Fn(&Path, &str);
/// Test pause points inside [`atomic_replace`]; production passes the default.
#[derive(Default)]
pub struct Probe<'a> {
    /// Leave the temp file and its ledger entry as a crash would.
    pub stop_after_tmp: bool,
    /// Runs right after the first exchange with `(folder, temp name)`.
    pub after_exchange: Option<AfterExchange<'a>>,
}

/// Publishes `bytes` at `name`. Without `base` the name must be absent
/// (`RENAME_NOREPLACE`). With `base` the current file is exchanged atomically
/// (`RENAME_EXCHANGE`, mode preserved); if the displaced bytes are not verified
/// to be `base`, an external write raced the check: the exchange is undone, the
/// bytes that come back are deleted only if they are ours, otherwise (or if the
/// undo fails) they are kept under [`recovery_name`], and `Conflict` is returned.
pub fn atomic_replace(
    dir: &OwnedFd,
    dir_path: &Path,
    name: &str,
    bytes: &[u8],
    base: Option<&DiskRev>,
    ledger: &TmpLedger,
    probe: &Probe,
) -> Result<DiskRev, SysError> {
    let mode = match base {
        None => None,
        Some(_) => match statat(dir, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(st) if FileType::from_raw_mode(st.st_mode) == FileType::RegularFile => {
                Some(st.st_mode & 0o7777)
            }
            Ok(_) => return Err(SysError::NotFile),
            Err(Errno::NOENT) => return Err(SysError::Conflict(None)),
            Err(e) => return Err(SysError::Io(e)),
        },
    };
    let tmp = format!(".devbox-tmp-{}", uuid::Uuid::new_v4().simple());
    // The ledger entry is durable before the temp file exists (startup sweep).
    let entry = ledger.record(&dir_path.join(&tmp))?;
    let rev = match write_new(dir, &tmp, bytes, mode) {
        Ok(rev) => rev,
        Err(e) => {
            let _ = unlinkat(dir, tmp.as_str(), AtFlags::empty());
            let _ = ledger.remove(entry);
            return Err(e);
        }
    };
    if probe.stop_after_tmp {
        return Err(SysError::Interrupted);
    }
    let flags = if base.is_some() {
        RenameFlags::EXCHANGE
    } else {
        RenameFlags::NOREPLACE
    };
    if let Err(e) = renameat_with(dir, tmp.as_str(), dir, name, flags) {
        let _ = unlinkat(dir, tmp.as_str(), AtFlags::empty());
        let _ = ledger.remove(entry);
        return Err(match rename_error(e) {
            SysError::NotFound => SysError::Conflict(None),
            e => e,
        });
    }
    if let Some(base) = base {
        // `tmp` now holds the displaced bytes. Only bytes verified to be `base`
        // or our own are ever deleted; anything unverified is put back in place
        // or kept under a visible recovery name, never left to the temp sweep.
        if let Some(hook) = probe.after_exchange {
            hook(dir_path, &tmp);
        }
        let verified = |hash: &str| matches!(read_at(dir, &tmp, 0), Ok(Some(f)) if f.rev.hash == hash || f.rev.same_content(base));
        if matches!(read_at(dir, &tmp, 0), Ok(Some(f)) if f.rev.same_content(base)) {
            unlinkat(dir, tmp.as_str(), AtFlags::empty())?;
        } else {
            // An external write raced the check (or could not be verified): undo.
            let undone = renameat_with(dir, tmp.as_str(), dir, name, RenameFlags::EXCHANGE).is_ok();
            let kept = if undone && verified(&rev.hash) {
                unlinkat(dir, tmp.as_str(), AtFlags::empty()).map_err(SysError::from)
            } else {
                keep_for_recovery(dir, name, &tmp)
            };
            let _ = fsync(dir);
            ledger.remove(entry)?;
            kept?;
            let now = read_at(dir, name, 0).ok().flatten().map(|f| f.rev);
            return Err(SysError::Conflict(now));
        }
    }
    fsync(dir)?;
    ledger.remove(entry)?;
    Ok(rev)
}

/// Visible sibling name for bytes that could not be verified after a raced save:
/// `<name>.devbox-conflict-<12 hex>` (or `devbox-conflict-<12 hex>` when that would
/// exceed the 255-byte name limit). It is not `is_internal`, so trees show it.
pub fn recovery_name(name: &str) -> String {
    let id = &uuid::Uuid::new_v4().simple().to_string()[..12];
    let full = format!("{name}.devbox-conflict-{id}");
    if full.len() <= 255 {
        full
    } else {
        format!("devbox-conflict-{id}")
    }
}
fn keep_for_recovery(dir: &OwnedFd, name: &str, tmp: &str) -> Result<(), SysError> {
    let keep = recovery_name(name);
    renameat_with(dir, tmp, dir, keep.as_str(), RenameFlags::NOREPLACE).map_err(rename_error)
}

/// Moves an entry (file or folder) without replacing an existing destination.
pub fn move_no_replace(
    from_dir: &OwnedFd,
    from: &str,
    to_dir: &OwnedFd,
    to: &str,
) -> Result<(), SysError> {
    renameat_with(from_dir, from, to_dir, to, RenameFlags::NOREPLACE).map_err(rename_error)?;
    fsync(to_dir)?;
    fsync(from_dir)?;
    Ok(())
}

/// Splits disk text into editor text (`\n`, no BOM) and how to write it back.
pub fn decode_text(bytes: impl AsRef<[u8]>) -> Option<(String, Eol, bool)> {
    let bytes = bytes.as_ref();
    let (bytes, bom) = match bytes.strip_prefix(BOM) {
        Some(rest) => (rest, true),
        None => (bytes, false),
    };
    let text = std::str::from_utf8(bytes).ok()?;
    // Mixed files: the majority line ending wins, a tie keeps LF. Editor text is
    // always LF-only, and a save writes every line with the chosen ending.
    let crlf = text.matches("\r\n").count();
    if crlf == 0 {
        return Some((text.to_owned(), Eol::Lf, bom));
    }
    let lone_lf = text.matches('\n').count() - crlf;
    let eol = if crlf > lone_lf { Eol::Crlf } else { Eol::Lf };
    Some((text.replace("\r\n", "\n"), eol, bom))
}
/// The inverse of [`decode_text`] (`sys::normalize_for_disk` in the plan).
pub fn encode_text(text: &str, eol: Eol, bom: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + 3);
    if bom {
        out.extend_from_slice(BOM);
    }
    match eol {
        Eol::Lf => out.extend_from_slice(text.as_bytes()),
        Eol::Crlf => {
            out.extend_from_slice(text.replace("\r\n", "\n").replace('\n', "\r\n").as_bytes())
        }
    }
    out
}
/// Length of `text` inside a JSON string literal (serde_json escaping rules).
pub fn escaped_len(text: &str) -> usize {
    text.chars()
        .map(|c| match c {
            '"' | '\\' | '\n' | '\r' | '\t' | '\u{8}' | '\u{c}' => 2,
            c if (c as u32) < 0x20 => 6,
            c => c.len_utf8(),
        })
        .sum()
}

/// Editor view of a file, or why it opens read-only.
#[derive(Debug, Clone)]
pub struct Content {
    pub text: Option<String>,
    pub eol: Eol,
    pub bom: bool,
    pub read_only: Option<ReadOnlyReason>,
}
pub fn classify(file: &DiskFile, max_wire: usize) -> Content {
    let refuse = |reason| Content {
        text: None,
        eol: Eol::Lf,
        bom: false,
        read_only: Some(reason),
    };
    let Some(bytes) = &file.bytes else {
        return refuse(ReadOnlyReason::TooLarge);
    };
    if bytes.contains(&0) {
        return refuse(ReadOnlyReason::Binary);
    }
    let Some((text, eol, bom)) = decode_text(bytes) else {
        return refuse(ReadOnlyReason::Encoding);
    };
    if escaped_len(&text) > max_wire {
        return refuse(ReadOnlyReason::TooLarge);
    }
    Content {
        text: Some(text),
        eol,
        bom,
        read_only: None,
    }
}
