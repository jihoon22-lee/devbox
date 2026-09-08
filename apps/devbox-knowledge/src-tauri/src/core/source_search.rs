//! Bounded, cancellable read jobs. Blocking filesystem probes retain their
//! worker permit until they return; cancellation never starts replacement
//! workers without accounting for the old OS call.
use super::retirement::{Lease, Pool};
use devbox_filesystem::FilesystemIdentity;
type Objects = (std::fs::File, std::fs::File);
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

pub const QUERY_TIME: Duration = Duration::from_millis(1500);
const TTL: Duration = Duration::from_secs(180);
const MAX_JOBS: usize = 4;
const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_ROWS: usize = 2000;

#[derive(Clone)]
pub struct Candidate {
    pub path: PathBuf,
    pub root: PathBuf,
    pub root_key: String,
    pub revision: Value,
    pub value: Value,
    pub index_stale: bool,
    pub offline: bool,
}
#[derive(Clone)]
pub struct Reference {
    pub candidate: Candidate,
    pub file_identity: FilesystemIdentity,
    pub root_identity: FilesystemIdentity,
    pub source: String,
    pub store_generation: String,
    // Object IDs cannot be recycled while this bounded lease is alive.
    // Its final close is deferred, including when a job expires under a lock.
    _objects: Lease<Objects>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub source: String,
    pub root_identity: String,
    pub reference: Option<String>,
    pub availability: String,
    pub index_stale: bool,
    pub value: Value,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub generation: String,
    pub store_generation: String,
    pub source: String,
    pub state: String,
    pub partial: bool,
    pub rows: Vec<Row>,
    pub bounds: Value,
}
struct Job {
    snapshot: Snapshot,
    started: Instant,
    cancelled: Arc<AtomicBool>,
    references: HashMap<String, Reference>,
}
#[derive(Default)]
struct Inner {
    jobs: HashMap<String, Job>,
    workers: HashMap<String, usize>,
    pools: HashMap<String, Arc<Pool<Objects>>>,
}
#[derive(Clone, Default)]
pub struct SearchJobs(Arc<Mutex<Inner>>);
pub struct Work {
    owner: SearchJobs,
    pub generation: String,
    pub source: String,
    pub store_generation: String,
    pub cancelled: Arc<AtomicBool>,
    pub deadline: Instant,
    pool: Option<Arc<Pool<Objects>>>,
}
impl Drop for Work {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.owner.0.lock() {
            if let Some(count) = inner.workers.get_mut(&self.source) {
                *count = count.saturating_sub(1);
            }
        }
    }
}
impl SearchJobs {
    fn expire(&self) {
        if let Ok(mut inner) = self.0.lock() {
            inner.jobs.retain(|_, job| job.started.elapsed() < TTL);
        }
    }
    pub fn start_expiry(&self) -> Result<(), String> {
        let weak = Arc::downgrade(&self.0);
        std::thread::Builder::new()
            .name("knowledge-search-expiry".into())
            .spawn(move || {
                while let Some(inner) = weak.upgrade() {
                    SearchJobs(inner).expire();
                    std::thread::sleep(Duration::from_secs(1));
                }
            })
            .map_err(|_| "search_unavailable".to_owned())?;
        Ok(())
    }
    pub fn begin(&self, source: &str, store_generation: &str) -> Result<Work, String> {
        let mut inner = self.0.lock().map_err(|_| "search_unavailable")?;
        inner.jobs.retain(|_, job| job.started.elapsed() < TTL);
        // Each consumer cancels its own opaque generation. Another consumer's
        // query must not revoke this reader's references or revive a late job.
        if inner.workers.get(source).copied().unwrap_or(0) >= 2 {
            return Err("search_busy".into());
        }
        if inner.jobs.len() >= MAX_JOBS {
            let oldest = inner
                .jobs
                .iter()
                .min_by_key(|(_, job)| job.started)
                .map(|(key, _)| key.clone());
            if let Some(key) = oldest {
                if let Some(job) = inner.jobs.remove(&key) {
                    job.cancelled.store(true, Ordering::Release);
                }
            }
        }
        let pool = if matches!(source, "files" | "notes") {
            if !inner.pools.contains_key(source) {
                inner
                    .pools
                    .insert(source.into(), Pool::new(MAX_JOBS * MAX_ROWS)?);
            }
            inner.pools.get(source).cloned()
        } else {
            None
        };
        let generation = uuid::Uuid::new_v4().to_string();
        let cancelled = Arc::new(AtomicBool::new(false));
        let started = Instant::now();
        inner.jobs.insert(
            generation.clone(),
            Job {
                snapshot: Snapshot {
                    generation: generation.clone(),
                    store_generation: store_generation.into(),
                    source: source.into(),
                    state: "running".into(),
                    partial: false,
                    rows: Vec::new(),
                    bounds: serde_json::json!({"maxRows":MAX_ROWS,"maxBytes":MAX_BYTES,"timeoutMs":QUERY_TIME.as_millis(),"referenceTtlSeconds":TTL.as_secs(),"maxObjectLeasesPerSource":MAX_JOBS*MAX_ROWS}),
                },
                started,
                cancelled: cancelled.clone(),
                references: HashMap::new(),
            },
        );
        *inner.workers.entry(source.into()).or_default() += 1;
        Ok(Work {
            owner: self.clone(),
            generation,
            source: source.into(),
            store_generation: store_generation.into(),
            cancelled,
            deadline: started + QUERY_TIME,
            pool,
        })
    }
    pub fn snapshot(&self, generation: &str) -> Result<Snapshot, String> {
        let mut inner = self.0.lock().map_err(|_| "search_unavailable")?;
        let job = inner.jobs.get_mut(generation).ok_or("search_stale")?;
        if job.started.elapsed() >= TTL {
            return Err("search_stale".into());
        }
        if job.snapshot.state == "running" && job.started.elapsed() >= QUERY_TIME {
            job.snapshot.state = "timed_out".into();
            job.snapshot.partial = true;
        }
        Ok(job.snapshot.clone())
    }
    pub fn cancel(&self, generation: &str) -> Result<(), String> {
        let mut inner = self.0.lock().map_err(|_| "search_unavailable")?;
        if let Some(job) = inner.jobs.get_mut(generation) {
            job.cancelled.store(true, Ordering::Release);
            job.references.clear();
            job.snapshot.rows.clear();
            job.snapshot.state = "cancelled".into();
        }
        Ok(())
    }
    pub fn resolve(&self, reference: &str) -> Result<Reference, String> {
        let inner = self.0.lock().map_err(|_| "search_unavailable")?;
        inner
            .jobs
            .values()
            .filter(|job| job.started.elapsed() < TTL && !job.cancelled.load(Ordering::Acquire))
            .find_map(|job| job.references.get(reference).cloned())
            .ok_or_else(|| "search_stale".into())
    }
}
impl Work {
    pub fn stopped(&self) -> bool {
        self.cancelled.load(Ordering::Acquire) || Instant::now() >= self.deadline
    }
    pub fn publish_candidates(&self, candidates: &[Candidate], capped: bool) -> Result<(), String> {
        if self.stopped() {
            return Err("search_cancelled".into());
        }
        let bytes = candidates.iter().try_fold(0usize, |total, item| {
            let value = serde_json::to_vec(&item.value).map_err(|_| "search_limit")?;
            let revision = serde_json::to_vec(&item.revision).map_err(|_| "search_limit")?;
            total
                .checked_add(
                    value.len()
                        + revision.len()
                        + item.path.as_os_str().len()
                        + item.root.as_os_str().len()
                        + item.root_key.len(),
                )
                .ok_or("search_limit")
        })?;
        if candidates.len() > MAX_ROWS || bytes > MAX_BYTES {
            return Err("search_limit".into());
        }
        let mut inner = self.owner.0.lock().map_err(|_| "search_unavailable")?;
        let job = inner.jobs.get_mut(&self.generation).ok_or("search_stale")?;
        if self.stopped() {
            return Err("search_cancelled".into());
        }
        job.snapshot.partial = capped || candidates.iter().any(|row| row.index_stale);
        job.snapshot.rows = candidates
            .iter()
            .map(|item| Row {
                source: self.source.clone(),
                root_identity: item.root_key.clone(),
                reference: None,
                availability: "unverified".into(),
                index_stale: item.index_stale,
                value: item.value.clone(),
            })
            .collect();
        Ok(())
    }
    pub fn verify(&self, index: usize, candidate: &Candidate) {
        if self.stopped() {
            return;
        }
        let identities = (|| {
            if candidate.offline || !self.pool.as_ref().is_some_and(|pool| pool.available()) {
                return None;
            }
            if !candidate.path.is_absolute()
                || !candidate.root.is_absolute()
                || !candidate.path.starts_with(&candidate.root)
                || candidate.path.components().any(|c| {
                    matches!(
                        c,
                        std::path::Component::ParentDir | std::path::Component::CurDir
                    )
                })
            {
                return None;
            }
            devbox_filesystem::ensure_no_links(&candidate.path).ok()?;
            Some((
                devbox_filesystem::open_filesystem_object(&candidate.root, true).ok()?,
                devbox_filesystem::open_filesystem_object(&candidate.path, false).ok()?,
            ))
        })();
        let identities = identities.and_then(|((root, root_identity), (file, file_identity))| {
            self.pool
                .as_ref()?
                .hold((file, root))
                .map(|objects| (root_identity, file_identity, objects))
        });
        // A late filesystem reply can neither publish rows nor revive refs.
        if self.stopped() {
            return;
        }
        if let Ok(mut inner) = self.owner.0.lock() {
            if self.stopped() {
                return;
            }
            if let Some(job) = inner.jobs.get_mut(&self.generation) {
                if let Some(row) = job.snapshot.rows.get_mut(index) {
                    if let Some((root_identity, file_identity, objects)) = identities {
                        let reference = uuid::Uuid::new_v4().to_string();
                        row.reference = Some(reference.clone());
                        row.availability = "available".into();
                        job.references.insert(
                            reference,
                            Reference {
                                candidate: candidate.clone(),
                                root_identity,
                                file_identity,
                                source: self.source.clone(),
                                store_generation: self.store_generation.clone(),
                                _objects: objects,
                            },
                        );
                    } else {
                        row.availability = "stale".into();
                        job.snapshot.partial = true;
                    }
                }
            }
        }
    }
    pub fn finish(&self, state: &str) {
        if let Ok(mut inner) = self.owner.0.lock() {
            if let Some(job) = inner.jobs.get_mut(&self.generation) {
                if !job.cancelled.load(Ordering::Acquire) {
                    let timed_out = Instant::now() >= self.deadline;
                    job.snapshot.state = if timed_out { "timed_out" } else { state }.into();
                    job.snapshot.partial |= timed_out || state != "complete";
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn candidate(root: &std::path::Path) -> Candidate {
        Candidate {
            path: root.join("note.md"),
            root: root.into(),
            root_key: "notes:vault".into(),
            revision: serde_json::json!([1]),
            value: serde_json::json!({"name":"note.md"}),
            index_stale: false,
            offline: false,
        }
    }
    #[test]
    fn cancellation_keeps_blocked_permits_bounded_and_other_source_independent() {
        let jobs = SearchJobs::default();
        let first = jobs.begin("files", "store").unwrap();
        let second = jobs.begin("files", "store").unwrap();
        jobs.cancel(&first.generation).unwrap();
        assert!(first.stopped());
        assert!(jobs.begin("files", "store").is_err());
        jobs.cancel(&second.generation).unwrap();
        assert!(second.stopped());
        let notes = jobs.begin("notes", "store").unwrap();
        assert!(!notes.stopped());
        drop(first);
        assert!(jobs.begin("files", "store").is_ok());
    }
    #[test]
    fn same_path_replacement_has_different_identity_and_cancel_revokes_reference() {
        let root = tempfile::tempdir().unwrap();
        let candidate = candidate(root.path());
        std::fs::write(&candidate.path, "old").unwrap();
        let jobs = SearchJobs::default();
        let work = jobs.begin("notes", "store").unwrap();
        work.publish_candidates(std::slice::from_ref(&candidate), false)
            .unwrap();
        work.verify(0, &candidate);
        let snapshot = jobs.snapshot(&work.generation).unwrap();
        let reference = snapshot.rows[0].reference.as_ref().unwrap();
        let issued = jobs.resolve(reference).unwrap();
        std::fs::rename(&candidate.path, root.path().join("old.md")).unwrap();
        std::fs::write(&candidate.path, "new").unwrap();
        assert_ne!(
            issued.file_identity,
            devbox_filesystem::filesystem_identity(&candidate.path, false).unwrap()
        );
        jobs.cancel(&work.generation).unwrap();
        work.verify(0, &candidate);
        work.finish("complete");
        assert!(jobs.resolve(reference).is_err());
        assert!(jobs.snapshot(&work.generation).unwrap().rows.is_empty());
    }
    #[test]
    fn expired_worker_cannot_publish_and_missing_file_keeps_readonly_row() {
        let root = tempfile::tempdir().unwrap();
        let candidate = candidate(root.path());
        let jobs = SearchJobs::default();
        let mut work = jobs.begin("notes", "store").unwrap();
        work.publish_candidates(std::slice::from_ref(&candidate), false)
            .unwrap();
        work.verify(0, &candidate);
        assert_eq!(
            jobs.snapshot(&work.generation).unwrap().rows[0].availability,
            "stale"
        );
        work.deadline = Instant::now();
        std::fs::write(&candidate.path, "late").unwrap();
        work.verify(0, &candidate);
        work.finish("complete");
        let result = jobs.snapshot(&work.generation).unwrap();
        assert_eq!(result.state, "timed_out");
        assert!(result.rows[0].reference.is_none());
    }
    #[test]
    fn expiry_releases_object_leases_and_delete_recreate_cannot_recycle_identity() {
        let root = tempfile::tempdir().unwrap();
        let candidate = candidate(root.path());
        std::fs::write(&candidate.path, "old").unwrap();
        let jobs = SearchJobs::default();
        let work = jobs.begin("notes", "store").unwrap();
        work.publish_candidates(std::slice::from_ref(&candidate), false)
            .unwrap();
        work.verify(0, &candidate);
        let snapshot = jobs.snapshot(&work.generation).unwrap();
        let token = snapshot.rows[0].reference.as_ref().unwrap();
        let reference = jobs.resolve(token).unwrap();
        let pool = jobs.0.lock().unwrap().pools["notes"].clone();
        #[cfg(unix)]
        {
            std::fs::remove_file(&candidate.path).unwrap();
            std::fs::write(&candidate.path, "new").unwrap();
            assert_ne!(
                reference.file_identity,
                devbox_filesystem::filesystem_identity(&candidate.path, false).unwrap()
            );
        }
        drop(reference);
        jobs.0
            .lock()
            .unwrap()
            .jobs
            .get_mut(&work.generation)
            .unwrap()
            .started = Instant::now() - TTL;
        jobs.expire();
        for _ in 0..100 {
            if pool.usage() == 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(pool.usage(), 0);
        assert!(jobs.resolve(token).is_err());
    }
}
