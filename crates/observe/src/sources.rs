use crate::*;
use devbox_protocol::Internal;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

#[derive(Debug, Clone)]
pub struct ResolvedSource {
    pub source: LogSource,
    pub path: Option<PathBuf>,
}
pub fn validate_sources(sources: &[LogSource]) -> R<()> {
    if sources.is_empty() || sources.len() > MAX_SOURCES {
        return Err(ObserveError::InvalidInput.into());
    }
    for (i, source) in sources.iter().enumerate() {
        if sources[..i].contains(source) {
            return Err(ObserveError::InvalidInput.into());
        }
        let value = match source {
            LogSource::File { path } => path,
            LogSource::Run { run_id } => run_id,
            LogSource::Container { id } => id,
        };
        if value.is_empty() || value.len() > 4096 || value.contains('\0') {
            return Err(ObserveError::InvalidInput.into());
        }
        if let LogSource::File { path } = source {
            if !Path::new(path).is_absolute() {
                return Err(ObserveError::InvalidInput.into());
            }
        }
        if let LogSource::Container { id } = source {
            if id.len() > 256
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                || !id.as_bytes()[0].is_ascii_alphanumeric()
            {
                return Err(ObserveError::InvalidInput.into());
            }
        }
    }
    Ok(())
}
pub(crate) async fn canonical(path: &Path, roots: Option<&[PathBuf]>) -> R<PathBuf> {
    let path = tokio::fs::canonicalize(path)
        .await
        .map_err(|_| ObserveError::SourceUnavailable)?;
    if let Some(roots) = roots {
        let mut allowed = false;
        for root in roots {
            // Missing default vaults are not created; they become readable when present.
            if let Ok(root) = tokio::fs::canonicalize(root).await {
                allowed |= path.starts_with(root);
            }
        }
        if !allowed {
            return Err(ObserveError::FileOutsideRoots.into());
        }
    }
    if !tokio::fs::metadata(&path)
        .await
        .map_err(|_| ObserveError::SourceUnavailable)?
        .is_file()
    {
        return Err(ObserveError::SourceUnavailable.into());
    }
    Ok(path)
}
pub(crate) fn identity(meta: &std::fs::Metadata) -> String {
    use std::os::unix::fs::MetadataExt;
    format!("{}:{}", meta.dev(), meta.ino())
}

/// A fixed chunk plus at most one physical line. Long foreign lines are consumed
/// completely and explicitly marked, never accumulated without a bound.
#[derive(Default)]
pub(crate) struct LineBuffer {
    bytes: Vec<u8>,
    overflow: bool,
}
impl LineBuffer {
    pub(crate) fn push(&mut self, bytes: &[u8]) {
        let keep = bytes.len().min(MAX_LINE.saturating_sub(self.bytes.len()));
        self.bytes.extend_from_slice(&bytes[..keep]);
        self.overflow |= keep < bytes.len();
    }
    pub(crate) fn take(&mut self) -> Vec<u8> {
        let mut bytes = std::mem::take(&mut self.bytes);
        if std::mem::take(&mut self.overflow) {
            bytes.extend_from_slice(b" [log line truncated]");
        }
        bytes
    }
    pub(crate) fn overflowed(&self) -> bool {
        self.overflow
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.bytes.is_empty() && !self.overflow
    }
}
pub(crate) struct FileSource {
    pub source: LogSource,
    path: PathBuf,
    real: PathBuf,
    roots: Option<Vec<PathBuf>>,
    file: tokio::fs::File,
    id: String,
    pub offset: u64,
    line: LineBuffer,
    chunk: Vec<u8>,
    used: usize,
    pub bytes_read: u64,
}
impl FileSource {
    pub(crate) async fn open(
        resolved: &ResolvedSource,
        roots: &[PathBuf],
        from: &LogFrom,
    ) -> R<Self> {
        let path = resolved.path.clone().ok_or(ObserveError::InvalidInput)?;
        let roots = matches!(resolved.source, LogSource::File { .. }).then(|| roots.to_vec());
        let real = canonical(&path, roots.as_deref()).await?;
        let mut file = tokio::fs::File::open(&real)
            .await
            .map_err(|_| ObserveError::SourceUnavailable)?;
        let meta = file.metadata().await.internal("stat log source")?;
        let offset = match from {
            LogFrom::Tail(_) => meta.len(),
            LogFrom::Offset { offset } => (*offset).min(meta.len()),
        };
        file.seek(std::io::SeekFrom::Start(offset))
            .await
            .internal("seek log source")?;
        Ok(Self {
            source: resolved.source.clone(),
            path,
            real,
            roots,
            file,
            id: identity(&meta),
            offset,
            line: LineBuffer::default(),
            chunk: vec![],
            used: 0,
            bytes_read: 0,
        })
    }
    pub(crate) fn file_identity(&self) -> &str {
        &self.id
    }
    pub(crate) fn parents(&self) -> [&Path; 2] {
        [
            self.path.parent().expect("requested log parent"),
            self.real.parent().expect("canonical log parent"),
        ]
    }
    pub(crate) async fn refresh(&mut self) -> R<()> {
        let real = match canonical(&self.path, self.roots.as_deref()).await {
            Err(devbox_protocol::Fail::Domain(ObserveError::SourceUnavailable)) => return Ok(()),
            result => result?,
        };
        let meta = tokio::fs::metadata(&real)
            .await
            .internal("stat followed log")?;
        if identity(&meta) != self.id || meta.len() < self.offset {
            self.file = tokio::fs::File::open(&real)
                .await
                .internal("reopen rotated log")?;
            self.id = identity(&self.file.metadata().await.internal("stat reopened log")?);
            self.offset = 0;
            self.line = LineBuffer::default();
            self.chunk.clear();
            self.used = 0;
        }
        self.real = real;
        Ok(())
    }
    pub(crate) async fn next(&mut self) -> R<Option<LogLine>> {
        self.next_line(true).await
    }
    /// Diagnostics need a complete physical record; raw log display may expose
    /// an EOF fragment through next(). Keep this buffer until its LF arrives.
    pub(crate) async fn next_complete(&mut self) -> R<Option<LogLine>> {
        self.next_line(false).await
    }
    pub(crate) fn has_pending_line(&self) -> bool {
        !self.line.is_empty()
    }
    async fn next_line(&mut self, show_eof_fragment: bool) -> R<Option<LogLine>> {
        loop {
            if self.used < self.chunk.len() {
                let rest = &self.chunk[self.used..];
                let n = rest
                    .iter()
                    .position(|b| *b == b'\n')
                    .map_or(rest.len(), |i| i + 1);
                self.line.push(&rest[..n]);
                self.used += n;
                if self.line.overflowed() {
                    return Err(ObserveError::LimitExceeded.into());
                }
                if rest[n - 1] == b'\n' {
                    return Ok(Some(crate::logs::parse(
                        &self.source,
                        &self.line.take(),
                        LogStream::Stdout,
                    )));
                }
            }
            let mut bytes = [0; 8192];
            let n = self
                .file
                .read(&mut bytes)
                .await
                .internal("read followed log")?;
            self.offset += n as u64;
            self.bytes_read += n as u64;
            self.chunk.clear();
            self.chunk.extend_from_slice(&bytes[..n]);
            self.used = 0;
            if n == 0 {
                return Ok((show_eof_fragment && !self.line.is_empty()).then(|| {
                    crate::logs::parse(&self.source, &self.line.take(), LogStream::Stdout)
                }));
            }
        }
    }
}
