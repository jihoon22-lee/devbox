use crate::{sources::LineBuffer, *};
use devbox_protocol::Internal;
use std::{
    path::Path,
    pin::Pin,
    process::Stdio,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWriteExt, ReadBuf};
use tokio::process::{Child, Command};
use tokio_util::sync::CancellationToken;

struct Pipe {
    reader: Pin<Box<dyn AsyncRead + Send>>,
    buffer: LineBuffer,
    chunk: Vec<u8>,
    used: usize,
    eof: bool,
    skip: u64,
}
impl Pipe {
    fn new(reader: impl AsyncRead + Send + 'static, skip: u64) -> Self {
        Self {
            reader: Box::pin(reader),
            buffer: LineBuffer::default(),
            chunk: vec![],
            used: 0,
            eof: false,
            skip,
        }
    }
    fn poll_line(&mut self, cx: &mut Context<'_>) -> Poll<std::io::Result<Option<Vec<u8>>>> {
        // Cooperative budget also covers an arbitrarily long unterminated line.
        for _ in 0..32 {
            if self.used < self.chunk.len() {
                let rest = &self.chunk[self.used..];
                let n = rest
                    .iter()
                    .position(|b| *b == b'\n')
                    .map_or(rest.len(), |i| i + 1);
                self.buffer.push(&rest[..n]);
                self.used += n;
                if self.buffer.overflowed() {
                    return Poll::Ready(Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "container log line exceeds limit",
                    )));
                }
                if rest[n - 1] == b'\n' {
                    return Poll::Ready(Ok(Some(self.buffer.take())));
                }
            }
            if self.eof {
                return Poll::Ready(Ok((!self.buffer.is_empty()).then(|| self.buffer.take())));
            }
            let mut bytes = [0; 8192];
            let mut read = ReadBuf::new(&mut bytes);
            match self.reader.as_mut().poll_read(cx, &mut read) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Ready(Ok(())) => (),
            }
            self.eof = read.filled().is_empty();
            let skip = self.skip.min(read.filled().len() as u64) as usize;
            self.skip -= skip as u64;
            self.chunk.clear();
            self.chunk.extend_from_slice(&read.filled()[skip..]);
            self.used = 0;
        }
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}
pub(crate) struct ContainerSource {
    source: LogSource,
    child: Child,
    pipes: [Pipe; 2],
    heads: [Option<LogLine>; 2],
    pub done: bool,
    stopped: bool,
}
impl ContainerSource {
    pub(crate) fn open(
        program: &Path,
        source: &LogSource,
        follow: bool,
        tail: bool,
        offset: u64,
    ) -> R<Self> {
        let LogSource::Container { id } = source else {
            return Err(ObserveError::InvalidInput.into());
        };
        let mut command = Command::new(program);
        command.args(["logs", "--timestamps"]);
        if follow {
            command.arg("--follow");
        }
        if tail {
            command.args(["--tail", "0"]);
        }
        let mut child = command
            .arg(id)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| ObserveError::SourceUnavailable)?;
        let out = child.stdout.take().expect("piped docker stdout");
        let err = child.stderr.take().expect("piped docker stderr");
        Ok(Self {
            source: source.clone(),
            child,
            pipes: [Pipe::new(out, offset), Pipe::new(err, offset)],
            heads: [None, None],
            done: false,
            stopped: false,
        })
    }
    pub(crate) fn poll_next(&mut self, cx: &mut Context<'_>) -> Poll<R<Option<LogLine>>> {
        let mut ended = 0;
        for i in 0..2 {
            if self.heads[i].is_some() {
                continue;
            }
            match self.pipes[i].poll_line(cx) {
                Poll::Ready(Ok(Some(bytes))) => {
                    self.heads[i] = Some(crate::logs::parse(
                        &self.source,
                        &bytes,
                        if i == 0 {
                            LogStream::Stdout
                        } else {
                            LogStream::Stderr
                        },
                    ))
                }
                Poll::Ready(Ok(None)) => ended += 1,
                Poll::Ready(Err(error)) => {
                    return Poll::Ready(Err(devbox_protocol::Fail::Internal(format!(
                        "read container log: {error}"
                    ))));
                }
                Poll::Pending => (),
            }
        }
        if let Some(index) = crate::logs::oldest_head(&self.heads) {
            return Poll::Ready(Ok(self.heads[index].take()));
        }
        if ended == 2 {
            self.done = true;
            Poll::Ready(Ok(None))
        } else {
            Poll::Pending
        }
    }
    pub(crate) async fn stop(&mut self) {
        if std::mem::replace(&mut self.stopped, true) {
            return;
        }
        // Only this docker logs process is ours; no container management commands.
        let _ = self.child.start_kill();
        let _ = self.child.wait().await;
    }
    pub(crate) async fn snapshot(
        mut self,
        cancel: CancellationToken,
    ) -> R<[tempfile::NamedTempFile; 2]> {
        let result = async {
            let temp = [
                tempfile::NamedTempFile::new().internal("create container stdout snapshot")?,
                tempfile::NamedTempFile::new().internal("create container stderr snapshot")?,
            ];
            let mut files = [
                tokio::fs::File::from_std(temp[0].reopen().internal("open container snapshot")?),
                tokio::fs::File::from_std(temp[1].reopen().internal("open container snapshot")?),
            ];
            let mut bytes = 0usize;
            loop {
                let line = tokio::select! {
                    _ = cancel.cancelled() => return Err(ObserveError::SourceUnavailable.into()),
                    line = std::future::poll_fn(|cx| self.poll_next(cx)) => line?,
                };
                let Some(line) = line else {
                    break;
                };
                let i = usize::from(line.stream == LogStream::Stderr);
                let stream = match line.stream {
                    LogStream::Stdout => "o",
                    LogStream::Stderr => "e",
                    LogStream::System => "s",
                };
                let line = format!("{}\t{}\t{}\n", line.ts, stream, line.text);
                bytes += line.len();
                if bytes > 64 * 1024 * 1024 {
                    return Err(ObserveError::LimitExceeded.into());
                }
                files[i]
                    .write_all(line.as_bytes())
                    .await
                    .internal("write container snapshot")?;
            }
            let status = tokio::select! {
                _ = cancel.cancelled() => return Err(ObserveError::SourceUnavailable.into()),
                status = self.child.wait() => status.internal("wait container logs")?,
            };
            if !status.success() {
                return Err(ObserveError::SourceUnavailable.into());
            }
            for file in &mut files {
                file.flush().await.internal("flush container snapshot")?;
            }
            Ok(temp)
        }
        .await;
        self.stop().await;
        result
    }
}
