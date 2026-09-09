//! Native-owned snapshot jobs. Paths and source file names never come from a
//! renderer. Snapshot preparation changes no activated component or repository.
use crate::{
    core::{
        legacy_inventory::Source,
        legacy_snapshot::{Manifest, Snapshot},
        stores::StoreRoot,
    },
    private_metadata::MetadataRoot,
};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Phase {
    Reading,
    Preserving,
    Ready,
    Cancelled,
    Failed,
}
impl Phase {
    fn active(self) -> bool {
        matches!(self, Self::Reading | Self::Preserving)
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Job {
    id: String,
    source: Source,
    phase: Phase,
    snapshot_id: Option<String>,
    manifest: Option<Manifest>,
    issue: Option<&'static str>,
    #[serde(skip)]
    cancelled: Arc<AtomicBool>,
}
pub(crate) struct LegacyImports {
    stores: Arc<StoreRoot>,
    base: PathBuf,
    current: Arc<Mutex<Option<Job>>>,
}
impl LegacyImports {
    pub(crate) fn new(stores: Arc<StoreRoot>) -> Result<Self> {
        // Host receives app_local_data_dir from native startup. Legacy owners
        // use fixed sibling identifiers under that same local-data directory.
        let base = stores
            .root()
            .parent()
            .ok_or("legacy_path_unavailable")?
            .to_owned();
        Ok(Self {
            stores,
            base,
            current: Arc::default(),
        })
    }
    pub(crate) fn status(&self) -> Result<Option<Job>> {
        Ok(self
            .current
            .lock()
            .map_err(|_| "legacy_import_busy")?
            .clone())
    }
    pub(crate) fn cancel(&self, id: &str) -> Result<()> {
        let current = self.current.lock().map_err(|_| "legacy_import_busy")?;
        let job = current
            .as_ref()
            .filter(|job| job.id == id)
            .ok_or("legacy_import_stale")?;
        if job.phase.active() {
            job.cancelled.store(true, Ordering::Release);
        }
        Ok(())
    }
    pub(crate) fn start(&self, source: Source) -> Result<Job> {
        self.stores.read()?;
        let mut current = self.current.lock().map_err(|_| "legacy_import_busy")?;
        if current.as_ref().is_some_and(|job| job.phase.active()) {
            return Err("legacy_import_busy");
        }
        let job = Job {
            id: uuid::Uuid::new_v4().to_string(),
            source,
            phase: Phase::Reading,
            snapshot_id: None,
            manifest: None,
            issue: None,
            cancelled: Arc::default(),
        };
        *current = Some(job.clone());
        let stores = self.stores.clone();
        let base = self.base.clone();
        let state = self.current.clone();
        let cancelled = job.cancelled.clone();
        let worker = std::thread::Builder::new()
            .name("workspace-legacy-snapshot".into())
            .spawn(move || {
                let started = Instant::now();
                let check = || {
                    if cancelled.load(Ordering::Acquire) {
                        return Err("legacy_import_cancelled");
                    }
                    if started.elapsed() > Duration::from_secs(30) {
                        return Err("legacy_import_timeout");
                    }
                    stores.read()?;
                    Ok(())
                };
                let result = (|| {
                    let snapshot = Snapshot::acquire(&base, source, check)?;
                    check()?;
                    if let Ok(mut state) = state.lock() {
                        if let Some(job) = state.as_mut() {
                            job.phase = Phase::Preserving;
                        }
                    }
                    let destination = MetadataRoot::open(stores.root())?.child("legacy-imports")?;
                    let id = snapshot.persist(destination.path(), || {
                        check()?;
                        destination.revalidate()
                    })?;
                    Ok((id, snapshot.manifest))
                })();
                if let Ok(mut state) = state.lock() {
                    if let Some(job) = state.as_mut() {
                        match result {
                            Ok((id, manifest)) => {
                                job.phase = Phase::Ready;
                                job.snapshot_id = Some(id);
                                job.manifest = Some(manifest);
                            }
                            Err(issue) => {
                                job.phase = if issue == "legacy_import_cancelled" {
                                    Phase::Cancelled
                                } else {
                                    Phase::Failed
                                };
                                job.issue = Some(issue);
                            }
                        }
                    }
                }
            });
        if worker.is_err() {
            if let Some(job) = current.as_mut() {
                job.phase = Phase::Failed;
                job.issue = Some("worker_unavailable");
            }
            return Err("worker_unavailable");
        }
        Ok(job)
    }
}
impl Drop for LegacyImports {
    fn drop(&mut self) {
        if let Ok(state) = self.current.lock() {
            if let Some(job) = state.as_ref() {
                if job.phase.active() {
                    job.cancelled.store(true, Ordering::Release);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_job_preserves_fixed_sources_and_never_activates_or_grants_paths() {
        let base = tempfile::tempdir().unwrap();
        let root = base.path().join("workspace");
        std::fs::create_dir(&root).unwrap();
        let source = base.path().join(Source::Workbench.identifier());
        std::fs::create_dir(&source).unwrap();
        let bytes = br#"{"version":1,"profiles":[]}"#;
        std::fs::write(source.join("project-profiles.json"), bytes).unwrap();
        let stores = Arc::new(StoreRoot::open(&root).unwrap());
        let owner = LegacyImports::new(stores.clone()).unwrap();
        assert!(owner.status().unwrap().is_none());
        let wait = || {
            let start = Instant::now();
            loop {
                let job = owner.status().unwrap().unwrap();
                if !job.phase.active() {
                    return job;
                }
                assert!(start.elapsed() < Duration::from_secs(5));
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        let accepted = owner.start(Source::Workbench).unwrap();
        let finished = wait();
        assert_eq!(accepted.id, finished.id);
        assert!(finished.phase == Phase::Ready);
        assert!(stores.read().unwrap().is_none());
        assert!(!root.join("stores").exists());
        assert_eq!(
            std::fs::read(source.join("project-profiles.json")).unwrap(),
            bytes
        );
        let snapshot = Snapshot::load(
            &root.join("legacy-imports"),
            finished.snapshot_id.as_ref().unwrap(),
        )
        .unwrap();
        assert_eq!(snapshot.bytes("project-profiles.json").unwrap(), bytes);
        assert_eq!(owner.cancel("foreign-job"), Err("legacy_import_stale"));
        owner.start(Source::Workbench).unwrap();
        assert_eq!(wait().snapshot_id, finished.snapshot_id);
        assert_eq!(
            std::fs::read_dir(root.join("legacy-imports"))
                .unwrap()
                .count(),
            1
        );
    }
}
