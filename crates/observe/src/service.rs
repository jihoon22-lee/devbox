use crate::{container::ContainerSource, logs::Filter, sources::FileSource, *};
use devbox_protocol::{Ctx, Internal, StreamOpened};
use notify::{RecursiveMode, Watcher};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    task::Poll,
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

pub struct ObserveService {
    pub(crate) docker: PathBuf,
    pub(crate) proc: ProcFs,
    pub(crate) host_slots: Arc<tokio::sync::Semaphore>,
    pub(crate) process_sampler: Arc<std::sync::Mutex<crate::processes::ProcessSampler>>,
    cancel: CancellationToken,
    tasks: TaskTracker,
    /// Counts actual file bytes read, useful for bounded-prefetch diagnostics/tests.
    read_bytes: Arc<AtomicU64>,
}
impl Default for ObserveService {
    fn default() -> Self {
        Self::new("docker".into())
    }
}
impl ObserveService {
    pub fn new(docker: PathBuf) -> Self {
        Self {
            docker,
            proc: ProcFs::default(),
            host_slots: Arc::new(tokio::sync::Semaphore::new(2)),
            process_sampler: Arc::default(),
            cancel: CancellationToken::new(),
            tasks: TaskTracker::new(),
            read_bytes: Arc::default(),
        }
    }
    pub fn file_bytes_read(&self) -> u64 {
        self.read_bytes.load(Ordering::Relaxed)
    }
    pub async fn shutdown(&self) {
        self.cancel.cancel();
        self.tasks.close();
        self.tasks.wait().await;
    }
    pub async fn open(
        &self,
        ctx: Ctx,
        p: OpenLogs,
        resolved: Vec<ResolvedSource>,
        roots: Vec<PathBuf>,
    ) -> R<StreamOpened> {
        validate_sources(&p.sources)?;
        let filter = Filter::new(p.filter.unwrap_or_default())?;
        if resolved.iter().map(|s| &s.source).ne(p.sources.iter()) {
            return Err(ObserveError::InvalidInput.into());
        }
        let mut files = vec![];
        // Validate file paths before any subprocess/stream side effects.
        for (i, source) in resolved.iter().enumerate() {
            if !matches!(source.source, LogSource::Container { .. }) {
                files.push((i, FileSource::open(source, &roots, &p.from).await?));
            }
        }
        let (send, mut events) = tokio::sync::mpsc::channel(1);
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if !event.as_ref().is_ok_and(|e| e.kind.is_access()) {
                    let _ = send.try_send(());
                }
            })
            .internal("watch log sources")?;
        let mut parents = std::collections::HashSet::new();
        sync_watches(&mut watcher, &mut parents, &files)?;
        let ends = ctx
            .open_stream()
            .map_err(|_| ObserveError::StreamUnsupported)?;
        let id = ends.id;
        let stop = self.cancel.clone();
        let closed = ends.tx.closed();
        let docker = self.docker.clone();
        let read_bytes = self.read_bytes.clone();
        // No source is read and no credit is awaited before the RPC returns.
        self.tasks.spawn(async move {
            let mut containers = vec![];
            let result = async {
                for (i, source) in p.sources.iter().enumerate() {
                    if matches!(source, LogSource::Container { .. }) {
                        containers.push((i, ContainerSource::open(&docker, source, true, matches!(p.from, LogFrom::Tail(_)), match p.from { LogFrom::Offset { offset } => offset, _ => 0 })?));
                    }
                }
                let mut heads = vec![None; p.sources.len()];
                let mut refresh = true;
                loop {
                    if refresh {
                        for (_, file) in &mut files { file.refresh().await?; }
                        sync_watches(&mut watcher, &mut parents, &files)?;
                        refresh = false;
                    }
                    for (i, file) in &mut files {
                        if heads[*i].is_none() {
                            let before = file.bytes_read;
                            let next = file.next().await;
                            read_bytes.fetch_add(file.bytes_read - before, Ordering::Relaxed);
                            heads[*i] = next?;
                        }
                    }
                    // Poll each pipe once to collect the currently available frontier.
                    std::future::poll_fn(|cx| {
                        for (i, source) in &mut containers {
                            if heads[*i].is_none() && !source.done {
                                if let Poll::Ready(line) = source.poll_next(cx) { heads[*i] = line?; }
                            }
                        }
                        Poll::Ready(Ok::<_, devbox_protocol::Fail<ObserveError>>(()))
                    }).await?;
                    for (_, child) in &mut containers { if child.done { child.stop().await; } }
                    if let Some(i) = crate::logs::oldest_head(&heads) {
                        let line = heads[i].take().expect("selected head");
                        if filter.accepts(&line) {
                            let mut bytes = serde_json::to_vec(&line).internal("encode log line")?;
                            bytes.push(b'\n');
                            ends.tx.write(&bytes).await.map_err(|_| ObserveError::SourceUnavailable)?;
                        }
                        tokio::task::yield_now().await;
                        continue;
                    }
                    if files.is_empty() && containers.iter().all(|(_, c)| c.done) { return Ok(()); }
                    tokio::select! {
                        _ = events.recv() => refresh = true,
                        result = std::future::poll_fn(|cx| {
                            let mut ready = false;
                            for (i, source) in &mut containers {
                                if !source.done { if let Poll::Ready(line) = source.poll_next(cx) { heads[*i] = line?; ready = true; } }
                            }
                            if ready { Poll::Ready(Ok::<_, devbox_protocol::Fail<ObserveError>>(())) } else { Poll::Pending }
                        }) => result?,
                    }
                }
            };
            let outcome: R<()> = tokio::select! {
                _ = stop.cancelled() => Ok(()),
                _ = closed.cancelled() => Ok(()),
                outcome = result => outcome,
            };
            // Reap every owned child even when cancellation interrupts credit or idle reads.
            for (_, child) in &mut containers { child.stop().await; }
            drop(watcher);
            if let Err(error) = outcome { tracing::debug!(?error, "log stream ended"); }
            ends.tx.finish().await;
        });
        Ok(StreamOpened { stream_id: id })
    }
    pub async fn search(
        &self,
        ctx: Ctx,
        p: SearchLogs,
        sources: Vec<ResolvedSource>,
        roots: Vec<PathBuf>,
    ) -> R<SearchResult> {
        let docker = self.docker.clone();
        let stop = self.cancel.clone();
        let (mut send, recv) = tokio::sync::oneshot::channel();
        self.tasks.spawn(async move {
            let cancel = CancellationToken::new();
            let work = crate::search::search(p, sources, roots, docker, cancel.clone());
            tokio::pin!(work);
            let result = tokio::select! {
                result = &mut work => result,
                _ = stop.cancelled() => { cancel.cancel(); work.await },
                _ = ctx.cancel.cancelled() => { cancel.cancel(); work.await },
                _ = send.closed() => { cancel.cancel(); work.await },
            };
            let _ = send.send(result);
        });
        recv.await.internal("join log search")?
    }
}

impl Drop for ObserveService {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

fn sync_watches(
    watcher: &mut notify::RecommendedWatcher,
    parents: &mut std::collections::HashSet<PathBuf>,
    files: &[(usize, FileSource)],
) -> R<()> {
    let wanted: std::collections::HashSet<_> = files
        .iter()
        .flat_map(|(_, file)| file.parents().map(PathBuf::from))
        .collect();
    for removed in parents.difference(&wanted) {
        watcher
            .unwatch(removed)
            .internal("unwatch old log parent")?;
    }
    for added in wanted.difference(parents) {
        watcher
            .watch(added, RecursiveMode::NonRecursive)
            .internal("watch log parent")?;
    }
    *parents = wanted;
    Ok(())
}
