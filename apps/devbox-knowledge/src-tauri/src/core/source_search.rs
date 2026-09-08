//! Bounded, cancellable read jobs. Blocking filesystem probes retain their
//! worker permit until they return; cancellation never starts replacement
//! workers without accounting for the old OS call.
use super::retirement::{Lease, Pool};
use devbox_filesystem::FilesystemIdentity;
type Objects = (std::fs::File, std::fs::File, Option<Arc<std::fs::File>>);

#[derive(Clone)]
pub struct ProjectReference {
    pub epoch: String,
    pub root: PathBuf,
    pub identity: FilesystemIdentity,
}
pub struct VerifiedProject {
    pub reference: ProjectReference,
    pub object: Arc<std::fs::File>,
}
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
    pub project: Option<ProjectReference>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_context: Option<product_contract::ProjectContext>,
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
    worker_source: String,
    project_valid: Option<Arc<AtomicBool>>,
}
impl Drop for Work {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.owner.0.lock() {
            if let Some(count) = inner.workers.get_mut(&self.worker_source) {
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
        let worker_source = if source == "current_project" {
            "files"
        } else {
            source
        };
        if inner.workers.get(worker_source).copied().unwrap_or(0) >= 2 {
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
        let pool = if matches!(worker_source, "files" | "notes") {
            if !inner.pools.contains_key(worker_source) {
                inner
                    .pools
                    .insert(worker_source.into(), Pool::new(MAX_JOBS * MAX_ROWS)?);
            }
            inner.pools.get(worker_source).cloned()
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
                    project_context: None,
                    bounds: serde_json::json!({"maxRows":MAX_ROWS,"maxBytes":MAX_BYTES,"timeoutMs":QUERY_TIME.as_millis(),"referenceTtlSeconds":TTL.as_secs(),"maxObjectLeasesPerSource":MAX_JOBS*MAX_ROWS}),
                },
                started,
                cancelled: cancelled.clone(),
                references: HashMap::new(),
            },
        );
        *inner.workers.entry(worker_source.into()).or_default() += 1;
        Ok(Work {
            owner: self.clone(),
            generation,
            source: source.into(),
            store_generation: store_generation.into(),
            cancelled,
            deadline: started + QUERY_TIME,
            pool,
            worker_source: worker_source.into(),
            project_valid: None,
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
        let mut snapshot = job.snapshot.clone();
        let source = if snapshot.source == "current_project" {
            "files"
        } else {
            &snapshot.source
        };
        // Logical cancellation does not imply that blocking probes or remote
        // handle retirement have completed. Report that retained capacity
        // separately without doing filesystem IO under this metadata lock.
        snapshot.bounds["retainedObjects"] =
            serde_json::json!(inner.pools.get(source).map_or(0, |pool| pool.usage()));
        snapshot.bounds["runningWorkers"] =
            serde_json::json!(inner.workers.get(source).copied().unwrap_or(0));
        Ok(snapshot)
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
    pub fn cancel_project(&self) {
        if let Ok(mut inner) = self.0.lock() {
            for job in inner
                .jobs
                .values_mut()
                .filter(|j| j.snapshot.source == "current_project")
            {
                job.cancelled.store(true, Ordering::Release);
                job.references.clear();
                job.snapshot.rows.clear();
                job.snapshot.state = "cancelled".into();
            }
        }
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
    pub fn bind_project(&mut self, selection: &super::project_provider::Selection) {
        self.project_valid = Some(selection.valid.clone());
        if let Ok(mut inner) = self.owner.0.lock() {
            if let Some(job) = inner.jobs.get_mut(&self.generation) {
                job.snapshot.project_context = Some(selection.project.context.clone());
            }
        }
    }
    pub fn stopped(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
            || self
                .project_valid
                .as_ref()
                .is_some_and(|valid| !valid.load(Ordering::Acquire))
            || Instant::now() >= self.deadline
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
        self.verify_with_project(index, candidate, None);
    }
    pub fn verify_with_project(
        &self,
        index: usize,
        candidate: &Candidate,
        project: Option<&VerifiedProject>,
    ) {
        if self.stopped() {
            return;
        }
        let identities = (|| {
            if (self.source == "current_project" && project.is_none())
                || project
                    .is_some_and(|project| !candidate.path.starts_with(&project.reference.root))
                || candidate.offline
                || !self.pool.as_ref().is_some_and(|pool| pool.available())
            {
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
                .hold((file, root, project.map(|p| p.object.clone())))
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
                                project: project.map(|p| p.reference.clone()),
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
                    if self
                        .project_valid
                        .as_ref()
                        .is_some_and(|valid| !valid.load(Ordering::Acquire))
                    {
                        job.cancelled.store(true, Ordering::Release);
                        job.references.clear();
                        job.snapshot.rows.clear();
                        job.snapshot.state = "cancelled".into();
                        return;
                    }
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

    #[cfg(windows)]
    fn owned_wsl_fixture_root() -> std::path::PathBuf {
        let raw = std::env::var("DEVBOX_WSL_FIXTURE_ROOT")
            .expect("an explicitly owned WSL fixture root is required");
        let token = std::env::var("DEVBOX_WSL_FIXTURE_OWNER").expect("fixture owner is required");
        assert_eq!(token.len(), 36);
        assert!(token.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'));
        let parsed = devbox_wsl::path::parse_wsl_unc_path(&raw)
            .unwrap()
            .expect("actual WSL UNC root required");
        assert_eq!(
            parsed.linux_path(),
            format!("/tmp/devbox-knowledge-wsl2-fixture-{token}")
        );
        let root = std::path::PathBuf::from(raw);
        devbox_filesystem::ensure_no_links(&root).unwrap();
        let marker = root.join("fixture-owner.txt");
        devbox_filesystem::ensure_no_links(&marker).unwrap();
        assert_eq!(
            std::fs::metadata(&marker).unwrap().len(),
            token.len() as u64
        );
        assert_eq!(std::fs::read_to_string(marker).unwrap(), token);
        root
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires an explicitly owned live WSL root; never starts the product or legacy apps"]
    fn native_wsl_owned_fixture() {
        let base = owned_wsl_fixture_root();
        let root = tempfile::Builder::new()
            .prefix("Source 한글 ")
            .tempdir_in(&base)
            .unwrap();
        let candidate = candidate(root.path());
        std::fs::write(&candidate.path, "original WSL file").unwrap();
        let jobs = SearchJobs::default();
        let work = jobs.begin("notes", "owned-wsl-fixture").unwrap();
        work.publish_candidates(std::slice::from_ref(&candidate), false)
            .unwrap();
        work.verify(0, &candidate);
        let snapshot = jobs.snapshot(&work.generation).unwrap();
        assert_eq!(snapshot.rows[0].availability, "available");
        let reference = snapshot.rows[0].reference.as_ref().unwrap();
        let issued = jobs.resolve(reference).unwrap();
        std::fs::rename(&candidate.path, root.path().join("previous.md")).unwrap();
        std::fs::write(&candidate.path, "replacement WSL file").unwrap();
        assert_ne!(
            issued.file_identity,
            devbox_filesystem::filesystem_identity(&candidate.path, false).unwrap()
        );
        jobs.cancel(&work.generation).unwrap();
        assert!(jobs.resolve(reference).is_err());
        drop(work);
        let mut normalized = candidate.clone();
        normalized.root = PathBuf::from(normalized.root.to_string_lossy().replace('\\', "/"));
        normalized.path = PathBuf::from(normalized.path.to_string_lossy().replace('\\', "/"));
        let slash_work = jobs.begin("files", "owned-wsl-fixture").unwrap();
        slash_work
            .publish_candidates(std::slice::from_ref(&normalized), false)
            .unwrap();
        slash_work.verify(0, &normalized);
        assert_eq!(
            jobs.snapshot(&slash_work.generation).unwrap().rows[0].availability,
            "available"
        );
        jobs.cancel(&slash_work.generation).unwrap();
        drop(slash_work);
        let mut unavailable = Candidate {
            path: candidate.path.clone(),
            root: candidate.root.clone(),
            root_key: candidate.root_key.clone(),
            revision: candidate.revision.clone(),
            value: candidate.value.clone(),
            index_stale: false,
            offline: false,
        };
        unavailable.offline = true;
        let work = jobs.begin("files", "owned-wsl-fixture").unwrap();
        work.publish_candidates(std::slice::from_ref(&unavailable), true)
            .unwrap();
        let snapshot = jobs.snapshot(&work.generation).unwrap();
        assert_eq!(snapshot.rows.len(), 1);
        assert!(snapshot.rows[0].reference.is_none());
        let local = jobs.begin("notes", "owned-local-fixture").unwrap();
        assert!(!local.stopped());
    }
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
    fn project_provider_fixture_pins_scope_and_invalidates_only_its_references() {
        use super::super::project_provider::Registry;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let mut candidate = candidate(directory.path());
        candidate.path = root.join("note.md");
        std::fs::write(&candidate.path, "fixture").unwrap();
        let registry = Registry::default();
        let context = serde_json::json!({"projectId":"fixture-project","worktreeId":"fixture-worktree","target":{"kind":"windows"},"revision":1});
        let mut snapshot = serde_json::json!({"schemaVersion":1,"revision":1,"current":context,"projects":[{"context":context,"root":root,"availability":"available","activityPaths":[]}]});
        registry
            .replace(&serde_json::to_vec(&snapshot).unwrap())
            .unwrap();
        let selection = registry.current().unwrap();
        let jobs = SearchJobs::default();
        let files = jobs.begin("files", "store").unwrap();
        files
            .publish_candidates(std::slice::from_ref(&candidate), false)
            .unwrap();
        files.verify(0, &candidate);
        let file_ref = jobs.snapshot(&files.generation).unwrap().rows[0]
            .reference
            .clone()
            .unwrap();
        let mut work = jobs.begin("current_project", "store").unwrap();
        work.bind_project(&selection);
        assert!(jobs.begin("files", "store").is_err());
        assert!(jobs.begin("current_project", "store").is_err());
        work.publish_candidates(std::slice::from_ref(&candidate), false)
            .unwrap();
        let (object, identity) = devbox_filesystem::open_filesystem_object(&root, true).unwrap();
        let verified = VerifiedProject {
            reference: ProjectReference {
                epoch: selection.epoch.clone(),
                root,
                identity,
            },
            object: Arc::new(object),
        };
        work.verify_with_project(0, &candidate, Some(&verified));
        let result = jobs.snapshot(&work.generation).unwrap();
        assert_eq!(
            result.project_context.as_ref().unwrap().worktree_id,
            "fixture-worktree"
        );
        let project_ref = result.rows[0].reference.clone().unwrap();
        assert_eq!(
            jobs.resolve(&project_ref)
                .unwrap()
                .project
                .as_ref()
                .unwrap()
                .identity,
            identity
        );
        snapshot["revision"] = 2.into();
        registry
            .replace(&serde_json::to_vec(&snapshot).unwrap())
            .unwrap();
        jobs.cancel_project();
        assert!(jobs.resolve(&project_ref).is_err());
        assert!(jobs.resolve(&file_ref).is_ok());
        assert!(work.stopped());
        // A registration can change between taking its snapshot and admitting
        // a query. Such late admission cannot publish old-context candidates.
        drop(work);
        let mut late = jobs.begin("current_project", "store").unwrap();
        late.bind_project(&selection);
        assert!(late.publish_candidates(&[candidate], false).is_err());
        late.finish("complete");
        assert_eq!(jobs.snapshot(&late.generation).unwrap().state, "cancelled");
        assert!(jobs.begin("notes", "store").is_ok());
    }

    #[test]
    fn cancelled_snapshot_reports_workers_and_native_objects_still_retained() {
        let root = tempfile::tempdir().unwrap();
        let candidate = candidate(root.path());
        std::fs::write(&candidate.path, "fixture").unwrap();
        let jobs = SearchJobs::default();
        let work = jobs.begin("notes", "store").unwrap();
        let generation = work.generation.clone();
        work.publish_candidates(std::slice::from_ref(&candidate), false)
            .unwrap();
        work.verify(0, &candidate);
        let snapshot = jobs.snapshot(&generation).unwrap();
        let issued = jobs
            .resolve(snapshot.rows[0].reference.as_ref().unwrap())
            .unwrap();
        jobs.cancel(&generation).unwrap();
        let cancelled = jobs.snapshot(&generation).unwrap();
        assert_eq!(cancelled.state, "cancelled");
        assert_eq!(cancelled.bounds["runningWorkers"], 1);
        assert_eq!(cancelled.bounds["retainedObjects"], 1);
        drop(work);
        let stopped = jobs.snapshot(&generation).unwrap();
        assert_eq!(stopped.bounds["runningWorkers"], 0);
        assert_eq!(stopped.bounds["retainedObjects"], 1);
        drop(issued);
        for _ in 0..100 {
            if jobs.snapshot(&generation).unwrap().bounds["retainedObjects"] == 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            jobs.snapshot(&generation).unwrap().bounds["retainedObjects"],
            0
        );
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
