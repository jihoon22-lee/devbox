//! Append-only run output with bounded lines and whole-line tail rotation.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use chrono::{SecondsFormat, Utc};

pub const MAX_LINE_BYTES: usize = 1024 * 1024;
const DEFAULT_LIMIT: u64 = 64 * 1024 * 1024;
const MAX_RECORD_BYTES: usize = MAX_LINE_BYTES * 3 + 128;
const MAX_READ_LINES: usize = 256;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug)]
pub enum Stream {
    Out,
    Err,
    System,
}

impl Stream {
    fn marker(self) -> char {
        match self {
            Self::Out => 'o',
            Self::Err => 'e',
            Self::System => 's',
        }
    }
}

pub struct Writer {
    path: PathBuf,
    file: File,
    limit: u64,
}

impl Writer {
    /// Opens an existing log for append, or creates it, with a 64 MiB limit.
    /// A log has one writer; the CLI owns the lock enforcing this requirement.
    pub fn open(path: &Path) -> io::Result<Self> {
        Self::with_limit(path, DEFAULT_LIMIT)
    }

    /// Configures the rotation threshold. A single record plus the rotation
    /// marker can exceed a small custom threshold; the default accommodates
    /// every supported record without exceeding its limit.
    pub fn with_limit(path: &Path, limit: u64) -> io::Result<Self> {
        if limit == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "log rotation limit must be positive",
            ));
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(path)?;
        Ok(Self {
            path: path.to_owned(),
            file,
            limit,
        })
    }

    /// Begin a new invocation on its own physical line, preserving any bytes
    /// left by an interrupted previous writer. Other writes keep their existing
    /// append semantics; only this invocation boundary repairs a missing LF.
    pub fn begin_attempt(&mut self, attempt: &str) -> io::Result<()> {
        if self.file.metadata()?.len() > 0 {
            self.file.seek(SeekFrom::End(-1))?;
            let mut last = [0];
            self.file.read_exact(&mut last)?;
            if last[0] != b'\n' {
                self.file.write_all(b"\n")?;
            }
        }
        self.write_line(
            Stream::System,
            devbox_core::run_log::attempt_marker(attempt).as_bytes(),
        )
    }

    /// Writes one line without an LF terminator. Callers split embedded LFs.
    /// Truncation applies to raw bytes before lossy UTF-8 conversion.
    /// Every successful call is flushed to the file (not fsynced to disk).
    pub fn write_line(&mut self, stream: Stream, bytes: &[u8]) -> io::Result<()> {
        let truncated = bytes.len() > MAX_LINE_BYTES;
        let text = String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_LINE_BYTES)]);
        let suffix = if truncated { "…(잘림)" } else { "" };
        let line = record(stream, &format!("{text}{suffix}"));
        let length = self.file.metadata()?.len();
        if length.saturating_add(line.len() as u64) > self.limit {
            self.rotate(length, line.len() as u64)?;
        }
        self.file.write_all(line.as_bytes())?;
        self.file.flush()
    }

    fn rotate(&mut self, length: u64, incoming: u64) -> io::Result<()> {
        let marker = record(Stream::System, "로그 크기 제한으로 회전했습니다");
        let budget =
            (self.limit / 2).min(self.limit.saturating_sub(incoming + marker.len() as u64));
        let mut source = BufReader::new(self.file.try_clone()?);
        let start = length.saturating_sub(budget);
        let mut start = next_line_boundary(&mut source, start, length)?;
        // Preserve the invocation which was effective at the cut, not the
        // newest invocation (the retained tail can still contain older output).
        let attempt = loop {
            let attempt = attempt_before(&mut source, start)?;
            let overhead = attempt.as_ref().map_or(0, |s| s.len() as u64);
            let budget = (self.limit / 2).min(
                self.limit
                    .saturating_sub(incoming + marker.len() as u64 + overhead),
            );
            let next = next_line_boundary(
                &mut source,
                start.max(length.saturating_sub(budget)),
                length,
            )?;
            if next == start {
                break attempt;
            }
            start = next;
        };
        let temporary = RotationFile::create(&self.path)?;
        temporary
            .file
            .set_permissions(self.file.metadata()?.permissions())?;
        let mut replacement = temporary.file.try_clone()?;
        replacement.write_all(marker.as_bytes())?;
        if let Some(attempt) = attempt {
            replacement.write_all(attempt.as_bytes())?;
        }
        source.seek(SeekFrom::Start(start))?;
        io::copy(&mut source.take(length - start), &mut replacement)?;
        replacement.flush()?;
        fs::rename(&temporary.path, &self.path)?;
        self.file = replacement;
        Ok(())
    }
}

fn attempt_before(reader: &mut BufReader<File>, end: u64) -> io::Result<Option<String>> {
    reader.seek(SeekFrom::Start(0))?;
    let mut position = 0;
    let mut last = None;
    while position < end {
        let Some(line) = read_complete_line(reader)? else {
            break;
        };
        position += line.len() as u64;
        let text = String::from_utf8_lossy(&line);
        let mut fields = text.trim_end_matches(['\n', '\r']).splitn(3, '\t');
        if fields
            .next()
            .is_some_and(|ts| chrono::DateTime::parse_from_rfc3339(ts).is_ok())
            && fields.next() == Some("s")
            && fields
                .next()
                .and_then(devbox_core::run_log::parse_attempt_marker)
                .is_some()
        {
            // Copy the original timestamp as well as the invocation identity.
            last = Some(text.into_owned());
        }
    }
    Ok(last)
}

fn record(stream: Stream, text: &str) -> String {
    format!(
        "{}\t{}\t{text}\n",
        Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        stream.marker()
    )
}

fn next_line_boundary(reader: &mut BufReader<File>, start: u64, length: u64) -> io::Result<u64> {
    if start == 0 {
        reader.seek(SeekFrom::Start(0))?;
        return Ok(0);
    }
    reader.seek(SeekFrom::Start(start - 1))?;
    let mut previous = [0];
    reader.read_exact(&mut previous)?;
    if previous[0] == b'\n' {
        return Ok(start);
    }
    let mut position = start;
    while position < length {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            break;
        }
        let end = buffer.iter().position(|byte| *byte == b'\n');
        let count = end.map_or(buffer.len(), |index| index + 1);
        reader.consume(count);
        position += count as u64;
        if end.is_some() {
            break;
        }
    }
    Ok(position.min(length))
}

/// Owns only the uniquely created temporary path, including on failure.
struct RotationFile {
    path: PathBuf,
    file: File,
}

impl RotationFile {
    fn create(path: &Path) -> io::Result<Self> {
        let directory = path.parent().unwrap_or_else(|| Path::new("."));
        for _ in 0..128 {
            let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = directory.join(format!(".devbox-log-{}-{sequence}.tmp", std::process::id()));
            match OpenOptions::new()
                .create_new(true)
                .append(true)
                .read(true)
                .open(&path)
            {
                Ok(file) => return Ok(Self { path, file }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate log rotation file",
        ))
    }
}

impl Drop for RotationFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Returns complete physical lines (including timestamp and stream marker,
/// excluding LF) and the next byte offset. An incomplete final line is retried
/// from its original offset on the next call. An offset past EOF resets to zero
/// so a reader can recover after rotation. At most 256 lines are returned;
/// malformed records exceeding the writer's maximum size are rejected.
pub fn read_range(
    path: &Path,
    from_offset: u64,
    max_lines: usize,
) -> io::Result<(Vec<String>, u64)> {
    let file = File::open(path)?;
    let mut offset = if from_offset > file.metadata()?.len() {
        0
    } else {
        from_offset
    };
    let mut reader = BufReader::new(file);
    reader.seek(SeekFrom::Start(offset))?;
    let mut lines = Vec::new();
    for _ in 0..max_lines.min(MAX_READ_LINES) {
        let Some(bytes) = read_complete_line(&mut reader)? else {
            break;
        };
        offset += bytes.len() as u64;
        lines.push(String::from_utf8_lossy(&bytes[..bytes.len() - 1]).into_owned());
    }
    Ok((lines, offset))
}

fn read_complete_line(reader: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            return Ok(None);
        }
        let end = buffer.iter().position(|byte| *byte == b'\n');
        let count = end.map_or(buffer.len(), |index| index + 1);
        if bytes.len() + count > MAX_RECORD_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "log record exceeds maximum line size",
            ));
        }
        bytes.extend_from_slice(&buffer[..count]);
        reader.consume(count);
        if end.is_some() {
            return Ok(Some(bytes));
        }
    }
}

#[cfg(test)]
#[path = "logfile_tests.rs"]
mod tests;
