//! Bounded chunks on a dedicated thread: a blocked stdin never pins Tokio shutdown.
use std::io::Read;
use std::time::Duration;
use tokio::sync::mpsc;

pub const MAX_LINE: usize = 1024 * 1024;
const INPUT_DEADLINE: Duration = Duration::from_secs(30);

pub enum Line {
    Message(Vec<u8>),
    Oversized,
}
pub struct Input {
    chunks: mpsc::Receiver<std::io::Result<Vec<u8>>>,
    chunk: Vec<u8>,
    offset: usize,
    line: Vec<u8>,
    oversized: bool,
    deadline: Option<tokio::time::Instant>,
}
impl Input {
    pub fn stdin() -> std::io::Result<Self> {
        let (tx, chunks) = mpsc::channel(2);
        std::thread::Builder::new()
            .name("mcp-input".into())
            .spawn(move || {
                let mut input = std::io::stdin().lock();
                loop {
                    let mut buf = vec![0; 8192];
                    let message = match input.read(&mut buf) {
                        Ok(0) => break,
                        Ok(size) => {
                            buf.truncate(size);
                            Ok(buf)
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(error) => Err(error),
                    };
                    let failed = message.is_err();
                    if tx.blocking_send(message).is_err() || failed {
                        break;
                    }
                }
            })?;
        Ok(Self {
            chunks,
            chunk: Vec::new(),
            offset: 0,
            line: Vec::new(),
            oversized: false,
            deadline: None,
        })
    }
    fn completed(&mut self) -> Line {
        self.deadline = None;
        if std::mem::take(&mut self.oversized) {
            self.line.clear();
            Line::Oversized
        } else {
            Line::Message(std::mem::take(&mut self.line))
        }
    }
    pub async fn next(&mut self) -> std::io::Result<Option<Line>> {
        loop {
            if self
                .deadline
                .is_some_and(|deadline| tokio::time::Instant::now() >= deadline)
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "MCP 입력 시간이 초과되었습니다.",
                ));
            }
            while self.offset < self.chunk.len() {
                let byte = self.chunk[self.offset];
                self.offset += 1;
                if byte == b'\n' {
                    return Ok(Some(self.completed()));
                }
                self.deadline
                    .get_or_insert_with(|| tokio::time::Instant::now() + INPUT_DEADLINE);
                if self.line.len() < MAX_LINE && !self.oversized {
                    self.line.push(byte);
                } else {
                    self.oversized = true;
                    self.line.clear();
                }
            }
            let chunk = if let Some(deadline) = self.deadline {
                tokio::time::timeout_at(deadline, self.chunks.recv())
                    .await
                    .map_err(|_| {
                        std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "MCP 입력 시간이 초과되었습니다.",
                        )
                    })?
            } else {
                self.chunks.recv().await
            };
            match chunk {
                Some(chunk) => {
                    self.chunk = chunk?;
                    self.offset = 0;
                }
                None if self.oversized || !self.line.is_empty() => {
                    return Ok(Some(self.completed()));
                }
                None => return Ok(None),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (mpsc::Sender<std::io::Result<Vec<u8>>>, Input) {
        let (tx, chunks) = mpsc::channel(2);
        let input = Input {
            chunks,
            chunk: Vec::new(),
            offset: 0,
            line: Vec::new(),
            oversized: false,
            deadline: None,
        };
        (tx, input)
    }
    #[tokio::test]
    async fn partial_line_deadline_cannot_be_extended_by_ready_chunks() {
        let (tx, mut input) = fixture();
        input.line = b"partial".to_vec();
        input.deadline = Some(tokio::time::Instant::now() - Duration::from_millis(1));
        tx.send(Ok(b"more\n".to_vec())).await.unwrap();
        assert_eq!(
            input
                .next()
                .await
                .err()
                .expect("expired input deadline")
                .kind(),
            std::io::ErrorKind::TimedOut
        );
    }
    #[tokio::test]
    async fn eof_processes_final_line_once_and_empty_eof_is_clean() {
        let (tx, mut input) = fixture();
        tx.send(Ok(b"{}\n{}".to_vec())).await.unwrap();
        drop(tx);
        for _ in 0..2 {
            assert!(
                matches!(input.next().await.unwrap(), Some(Line::Message(line)) if line == b"{}")
            );
        }
        assert!(input.next().await.unwrap().is_none());
    }
}
