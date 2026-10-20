use crate::{problems::ProblemParser, sources::FileSource, *};
use devbox_protocol::Internal;
use std::{future::Future, sync::atomic::Ordering};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileStamp {
    id: String,
    len: u64,
    modified: Option<std::time::SystemTime>,
}
async fn stamp(path: &std::path::Path) -> Option<FileStamp> {
    let meta = tokio::fs::metadata(path).await.ok()?;
    Some(FileStamp {
        id: crate::sources::identity(&meta),
        len: meta.len(),
        modified: meta.modified().ok(),
    })
}
pub(crate) struct ProblemReader {
    run: ProblemRun,
    file: FileSource,
    stdout: ProblemParser,
    stderr: ProblemParser,
    active: bool,
}
/// The batch owns its advanced reader until commit. Cancellation or an obsolete
/// invocation drops it, so a later read replays instead of losing diagnostics.
#[derive(Default)]
pub struct ProblemBatch {
    pub problems: Vec<Problem>,
    pub overflow: bool,
    reader: Option<ProblemReader>,
}
impl ObserveService {
    /// Serialize metadata resolution, scanning and validation using
    /// problem_refresh_lock; a changed durable attempt discards the batch.
    pub async fn read_problem_batch(&self, run: &ProblemRun) -> R<ProblemBatch> {
        if let Some(stamp) = stamp(&run.path).await {
            if self
                .problems
                .completed
                .lock()
                .await
                .get(&run.run_id)
                .is_some_and(|(old, s)| old == run && s == &stamp)
            {
                return Ok(ProblemBatch::default());
            }
        }
        self.problems.completed.lock().await.remove(&run.run_id);
        let mut readers = self.problems.readers.lock().await;
        let previous = readers.remove(&run.run_id).filter(|r| r.run == *run);
        let mut reader = if let Some(reader) = previous {
            reader
        } else {
            if readers.len() >= crate::problem_store::MAX_LIVE_READERS {
                return Err(ObserveError::LimitExceeded.into());
            }
            if !tokio::fs::try_exists(&run.path)
                .await
                .internal("check run problem log")?
            {
                return Ok(ProblemBatch::default());
            }
            let file = FileSource::open(
                &ResolvedSource {
                    source: LogSource::Run {
                        run_id: run.run_id.clone(),
                    },
                    path: Some(run.path.clone()),
                },
                &[],
                &LogFrom::Offset { offset: 0 },
            )
            .await?;
            ProblemReader {
                run: run.clone(),
                file,
                stdout: ProblemParser::default(),
                stderr: ProblemParser::default(),
                active: false,
            }
        };
        drop(readers);
        let old_identity = reader.file.file_identity().to_owned();
        let old_offset = reader.file.offset;
        reader.file.refresh().await?;
        if reader.file.file_identity() != old_identity || reader.file.offset < old_offset {
            reader.stdout = ProblemParser::default();
            reader.stderr = ProblemParser::default();
        }
        let mut batch = ProblemBatch::default();
        let start = reader.file.bytes_read;
        while let Some(line) = reader.file.next_complete().await? {
            // A writer cannot keep an RPC scanning forever. Owned logs rotate at 64MiB.
            if reader.file.bytes_read - start > 68 * 1024 * 1024 {
                return Err(ObserveError::LimitExceeded.into());
            }
            if line.stream == LogStream::System {
                if let Some(attempt) = devbox_core::run_log::parse_attempt_marker(&line.text) {
                    reader.active = run.attempt_id.as_deref() == Some(attempt);
                    reader.stdout = ProblemParser::default();
                    reader.stderr = ProblemParser::default();
                }
            } else if reader.active {
                let parser = if line.stream == LogStream::Stderr {
                    &mut reader.stderr
                } else {
                    &mut reader.stdout
                };
                if let Some(problem) = parser.push(&line.text, &run.root) {
                    if !batch.problems.contains(&problem) {
                        if batch.problems.len() < crate::problem_store::MAX_PROBLEMS {
                            batch.problems.push(problem);
                        } else {
                            batch.overflow = true;
                        }
                    }
                }
            }
            tokio::task::yield_now().await;
        }
        batch.reader = Some(reader);
        Ok(batch)
    }
    pub async fn commit_problem_batch(
        &self,
        run: ProblemRun,
        batch: ProblemBatch,
        live: bool,
        hub: &devbox_core::Hub,
    ) -> R<()> {
        self.apply_problem_batch(run.clone(), batch.problems, batch.overflow, hub)?;
        if let Some(reader) = batch.reader {
            if live {
                self.problems
                    .readers
                    .lock()
                    .await
                    .insert(run.run_id.clone(), reader);
            } else if let Some(stamp) = stamp(&run.path).await {
                if !reader.file.has_pending_line()
                    && stamp.id == reader.file.file_identity()
                    && stamp.len == reader.file.offset
                {
                    let mut cache = self.problems.completed.lock().await;
                    if cache.len() < crate::problem_store::MAX_RUNS
                        || cache.contains_key(&run.run_id)
                    {
                        cache.insert(run.run_id.clone(), (run, stamp));
                    }
                }
            }
        }
        Ok(())
    }
    pub async fn discard_problem_reader(&self, id: &str) {
        self.problems.readers.lock().await.remove(id);
        self.problems.completed.lock().await.remove(id);
    }
    pub fn start_problem_watch(&self, task: impl Future<Output = ()> + Send + 'static) -> bool {
        if self.problems.watching.swap(true, Ordering::AcqRel) {
            return false;
        }
        self.tasks.spawn(task);
        true
    }
    pub async fn problem_wake(&self) {
        self.problems.wake.notified().await
    }
    pub fn cancellation(&self) -> tokio_util::sync::CancellationToken {
        self.cancel.clone()
    }
}
