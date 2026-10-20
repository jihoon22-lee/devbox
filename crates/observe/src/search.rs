use crate::{
    container::ContainerSource,
    logs::Filter,
    sources::{canonical, identity},
    *,
};
use devbox_protocol::Internal;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
};
use tokio_util::sync::CancellationToken;
const PAGE_BYTES: usize = 768 * 1024;
const SCAN_BYTES: u64 = 8 * 1024 * 1024;

struct Reverse {
    file: File,
    cursor: SourceCursor,
    scanned: u64,
}
impl Reverse {
    fn open(
        path: PathBuf,
        source: LogSource,
        stream: LogStream,
        before: Option<&SourceCursor>,
    ) -> R<Self> {
        let mut file = File::open(path).internal("open reverse log")?;
        let meta = file.metadata().internal("stat reverse log")?;
        // A stable first-line prefix distinguishes replacement/rotation of Docker
        // snapshots without depending on temporary file inode or read timestamps.
        let mut prefix = [0u8; 128];
        let n = file.read(&mut prefix).internal("fingerprint log")?;
        let end = prefix[..n]
            .iter()
            .position(|b| *b == b'\n')
            .map_or(n, |i| i + 1);
        let base = if matches!(source, LogSource::Container { .. }) {
            "container".into()
        } else {
            identity(&meta)
        };
        let identity = format!("{base}:{:x}", Sha256::digest(&prefix[..end]));
        let offset = if let Some(before) = before {
            if before.source != source
                || before.stream != stream
                || (before.offset > 0
                    && (before.identity != identity || before.offset > meta.len()))
            {
                return Err(ObserveError::InvalidCursor.into());
            }
            before.offset
        } else {
            meta.len()
        };
        Ok(Self {
            file,
            cursor: SourceCursor {
                source,
                stream,
                identity,
                offset,
            },
            scanned: 0,
        })
    }
    fn previous(&mut self, cancel: &CancellationToken) -> R<Option<(u64, LogLine)>> {
        let end = self.cursor.offset;
        if end == 0 {
            return Ok(None);
        }
        let mut position = end;
        let mut reversed = Vec::new();
        let mut first = true;
        let mut overflow = false;
        'scan: while position > 0 {
            if cancel.is_cancelled() {
                return Err(ObserveError::SourceUnavailable.into());
            }
            let start = position.saturating_sub(8192);
            let mut block = vec![0; (position - start) as usize];
            self.file
                .seek(SeekFrom::Start(start))
                .internal("seek reverse log")?;
            self.file
                .read_exact(&mut block)
                .internal("read reverse log")?;
            self.scanned += block.len() as u64;
            if self.scanned > 64 * 1024 * 1024 {
                return Err(ObserveError::LimitExceeded.into());
            }
            for (index, byte) in block.into_iter().enumerate().rev() {
                position = start + index as u64;
                if first && byte == b'\n' {
                    first = false;
                    continue;
                }
                first = false;
                if byte == b'\n' {
                    position += 1;
                    break 'scan;
                }
                if reversed.len() < MAX_LINE {
                    reversed.push(byte);
                } else {
                    overflow = true;
                }
            }
        }
        reversed.reverse();
        let mut line = if matches!(self.cursor.source, LogSource::Container { .. }) {
            crate::logs::parse_snapshot(&self.cursor.source, &reversed, self.cursor.stream)
        } else {
            crate::logs::parse(&self.cursor.source, &reversed, self.cursor.stream)
        };
        if overflow {
            line.text.push_str(" [log line truncated]");
        }
        Ok(Some((position, line)))
    }
    fn head(&mut self, filter: &Filter, cancel: &CancellationToken) -> R<Option<(u64, LogLine)>> {
        while self.cursor.offset > 0 && self.scanned < SCAN_BYTES {
            let Some((start, line)) = self.previous(cancel)? else {
                return Ok(None);
            };
            if filter.accepts(&line) {
                return Ok(Some((start, line)));
            }
            self.cursor.offset = start;
        }
        Ok(None)
    }
}

pub(crate) async fn search(
    p: SearchLogs,
    sources: Vec<ResolvedSource>,
    roots: Vec<PathBuf>,
    docker: PathBuf,
    cancel: CancellationToken,
) -> R<SearchResult> {
    validate_sources(&p.sources)?;
    if p.limit == 0 || p.limit > 256 || sources.iter().map(|s| &s.source).ne(p.sources.iter()) {
        return Err(ObserveError::InvalidInput.into());
    }
    let filter = Filter::new(if p.regex {
        LogFilter {
            regex: Some(p.query),
            ..Default::default()
        }
    } else {
        LogFilter {
            text: Some(p.query),
            ..Default::default()
        }
    })?;
    let mut inputs = vec![];
    let mut snapshots = vec![];
    for source in sources {
        match &source.source {
            LogSource::Container { .. } => {
                let temp = ContainerSource::open(&docker, &source.source, false, false, 0)?
                    .snapshot(cancel.clone())
                    .await?;
                for (i, file) in temp.iter().enumerate() {
                    inputs.push((
                        file.path().to_owned(),
                        source.source.clone(),
                        if i == 0 {
                            LogStream::Stdout
                        } else {
                            LogStream::Stderr
                        },
                    ));
                }
                snapshots.extend(temp);
            }
            _ => {
                let path = source.path.ok_or(ObserveError::InvalidInput)?;
                let path = canonical(
                    &path,
                    matches!(source.source, LogSource::File { .. }).then_some(roots.as_slice()),
                )
                .await?;
                inputs.push((path, source.source, LogStream::Stdout));
            }
        }
    }
    tokio::task::spawn_blocking(move || {
        let _snapshots = snapshots;
        if p.before
            .as_ref()
            .is_some_and(|c| c.sources.len() != inputs.len())
        {
            return Err(ObserveError::InvalidCursor.into());
        }
        let mut readers = inputs
            .into_iter()
            .enumerate()
            .map(|(i, (path, source, stream))| {
                Reverse::open(
                    path,
                    source,
                    stream,
                    p.before.as_ref().map(|c| &c.sources[i]),
                )
            })
            .collect::<R<Vec<_>>>()?;
        let mut heads = readers
            .iter_mut()
            .map(|r| r.head(&filter, &cancel))
            .collect::<R<Vec<_>>>()?;
        let mut result = SearchResult {
            items: vec![],
            next_cursor: None,
        };
        while result.items.len() < p.limit as usize {
            // A scan-budget stop must not let another source overtake unknown history.
            if readers
                .iter()
                .zip(&heads)
                .any(|(r, h)| h.is_none() && r.cursor.offset > 0)
            {
                break;
            }
            let Some(index) = heads
                .iter()
                .enumerate()
                .filter_map(|(i, h)| {
                    h.as_ref()
                        .map(|(_, line)| (crate::logs::timestamp(&line.ts), std::cmp::Reverse(i)))
                })
                .max()
                .map(|(_, i)| i.0)
            else {
                break;
            };
            let (position, mut line) = heads[index].take().expect("selected search head");
            // Keep a single escaped JSON row below the response cap. Cursor consumes
            // the entire original physical record, including an oversized line.
            let mut boundary = line.text.len().min(64 * 1024);
            while !line.text.is_char_boundary(boundary) {
                boundary -= 1;
            }
            if boundary < line.text.len() {
                line.text.truncate(boundary);
                line.text.push_str(" [log line truncated]");
            }
            result.items.push(line);
            let previous = readers[index].cursor.offset;
            readers[index].cursor.offset = position;
            result.next_cursor = Some(SearchCursor {
                sources: readers.iter().map(|r| r.cursor.clone()).collect(),
            });
            if serde_json::to_vec(&result)
                .internal("measure log search page")?
                .len()
                > PAGE_BYTES
            {
                result.items.pop();
                readers[index].cursor.offset = previous;
                break;
            }
            heads[index] = readers[index].head(&filter, &cancel)?;
        }
        result.next_cursor = readers
            .iter()
            .any(|r| r.cursor.offset > 0)
            .then(|| SearchCursor {
                sources: readers.iter().map(|r| r.cursor.clone()).collect(),
            });
        Ok(result)
    })
    .await
    .internal("join reverse log scan")?
}
