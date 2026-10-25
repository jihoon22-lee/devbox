//! Capture-only reader. Ordinary document reads retain their full-file CAS hash.
use crate::{CaptureReason, capture_budget::Budget, sys};
use rustix::fs::{AtFlags, FileType, Mode, OFlags, Stat, fstat, openat, statat};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, os::fd::OwnedFd};

const CHUNK: usize = 64 * 1024;

fn unchanged(a: &Stat, b: &Stat) -> bool {
    a.st_dev == b.st_dev
        && a.st_ino == b.st_ino
        && a.st_size == b.st_size
        && a.st_mtime == b.st_mtime
        && a.st_mtime_nsec == b.st_mtime_nsec
        && a.st_ctime == b.st_ctime
        && a.st_ctime_nsec == b.st_ctime_nsec
}

pub(crate) fn bounded_read(
    dir: &OwnedFd,
    name: &str,
    budget: &mut Budget,
) -> Result<sys::DiskFile, CaptureReason> {
    read_with(dir, name, budget, || {})
}

fn read_with(
    dir: &OwnedFd,
    name: &str,
    budget: &mut Budget,
    mut after_chunk: impl FnMut(),
) -> Result<sys::DiskFile, CaptureReason> {
    budget.check()?;
    let flags =
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK | OFlags::NOCTTY;
    let fd = openat(dir, name, flags, Mode::empty()).map_err(|_| CaptureReason::Unavailable)?;
    let before = fstat(&fd).map_err(|_| CaptureReason::Unavailable)?;
    if FileType::from_raw_mode(before.st_mode) != FileType::RegularFile {
        return Err(CaptureReason::Unavailable);
    }
    if before.st_size < 0 || before.st_size as u64 > budget.limits.file_bytes {
        return Err(CaptureReason::TooLarge);
    }
    let mut file = File::from(fd);
    let mut bytes = Vec::new();
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; CHUNK];
    loop {
        budget.check()?;
        let remaining = budget.limits.bytes.saturating_sub(budget.bytes_read);
        let file_remaining = budget.limits.file_bytes.saturating_sub(bytes.len() as u64);
        let allowance = remaining.min(file_remaining).min(CHUNK as u64) as usize;
        if allowance == 0 {
            // No extra probe byte: fstat checks whether an exact-limit regular file
            // ended there. A growing file is rejected within the actual IO budget.
            let now = fstat(&file).map_err(|_| CaptureReason::Unavailable)?;
            if now.st_size == bytes.len() as i64 && unchanged(&before, &now) {
                break;
            }
            return Err(if remaining == 0 {
                budget.stop(CaptureReason::ByteLimit)
            } else {
                CaptureReason::TooLarge
            });
        }
        let n = file
            .read(&mut buffer[..allowance])
            .map_err(|_| CaptureReason::Unavailable)?;
        budget.bytes_read += n as u64;
        if n == 0 {
            break;
        }
        budget.check()?;
        hash.update(&buffer[..n]);
        bytes.extend_from_slice(&buffer[..n]);
        after_chunk();
    }
    budget.check()?;
    let after = fstat(&file).map_err(|_| CaptureReason::Unavailable)?;
    let named =
        statat(dir, name, AtFlags::SYMLINK_NOFOLLOW).map_err(|_| CaptureReason::Unavailable)?;
    if !unchanged(&before, &after)
        || !unchanged(&after, &named)
        || after.st_size != bytes.len() as i64
    {
        return Err(CaptureReason::Changed);
    }
    Ok(sys::DiskFile {
        rev: sys::rev_of(sys::hex(&hash.finalize()), bytes.len() as u64, &after),
        bytes: Some(bytes),
    })
}

/// Hash the same UTF-8/BOM/CRLF normalization as document decoding, in chunks.
pub(crate) fn text_hash(bytes: &[u8], budget: &mut Budget) -> Result<String, CaptureReason> {
    budget.check()?;
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    std::str::from_utf8(bytes).map_err(|_| CaptureReason::Encoding)?;
    let mut hash = Sha256::new();
    let mut pending_cr = false;
    for chunk in bytes.chunks(CHUNK) {
        budget.check()?;
        let mut normalized = Vec::with_capacity(chunk.len() + 1);
        for &byte in chunk {
            if byte == 0 {
                return Err(CaptureReason::Binary);
            }
            if pending_cr && byte != b'\n' {
                normalized.push(b'\r');
            }
            pending_cr = byte == b'\r';
            if !pending_cr {
                normalized.push(byte);
            }
        }
        budget.check()?;
        hash.update(normalized);
    }
    budget.check()?;
    if pending_cr {
        hash.update(b"\r");
    }
    Ok(sys::hex(&hash.finalize()))
}

#[cfg(test)]
#[path = "capture_io_tests.rs"]
mod tests;
