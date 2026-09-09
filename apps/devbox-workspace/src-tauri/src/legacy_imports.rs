//! Native-owned snapshot jobs. Paths and source file names never come from a
//! renderer. Snapshot preparation changes no activated component or repository.
use crate::{
    core::{
        legacy_inventory::Source,
        legacy_snapshot::{Catalog, Manifest, Snapshot},
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
    Checking,
    Ready,
    Cancelled,
    Failed,
}
impl Phase {
    fn active(self) -> bool {
        matches!(self, Self::Reading | Self::Preserving | Self::Checking)
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Job {
    id: String,
    source: Source,
    operation: Operation,
    phase: Phase,
    snapshot_id: Option<String>,
    manifest: Option<Manifest>,
    issue: Option<&'static str>,
    #[serde(skip)]
    cancelled: Arc<AtomicBool>,
    #[serde(skip)]
    snapshot: Option<Arc<Snapshot>>,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Operation {
    Preserve,
    Verify,
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
    pub(crate) fn profile_source(
        &self,
        job_id: &str,
    ) -> Result<(String, workbench_lib::component::ProfileStore)> {
        let snapshot = {
            let current = self.current.lock().map_err(|_| "legacy_import_busy")?;
            current
                .as_ref()
                .filter(|job| job.id == job_id && job.phase == Phase::Ready)
                .and_then(|job| job.snapshot.clone())
                .ok_or("legacy_import_stale")?
        };
        if snapshot.manifest.source != Source::Workbench {
            return Err("legacy_profiles_unavailable");
        }
        let inventory = snapshot
            .manifest
            .files
            .iter()
            .find(|file| file.name == "project-profiles.json")
            .filter(|file| file.issue.is_none())
            .ok_or("legacy_profiles_unavailable")?;
        let bytes = snapshot
            .bytes(&inventory.name)
            .ok_or("legacy_profiles_unavailable")?;
        let profiles = workbench_lib::component::ProfileStore::load(
            std::str::from_utf8(bytes).map_err(|_| "legacy_profiles_unavailable")?,
        )
        .map_err(|_| "legacy_profiles_unavailable")?;
        Ok((snapshot.id()?, profiles))
    }
    pub(crate) fn template_source(
        &self,
        job_id: &str,
    ) -> Result<(String, workbench_lib::component::ProfileTemplateStore)> {
        let snapshot = {
            let current = self.current.lock().map_err(|_| "legacy_import_busy")?;
            current
                .as_ref()
                .filter(|job| job.id == job_id && job.phase == Phase::Ready)
                .and_then(|job| job.snapshot.clone())
                .ok_or("legacy_import_stale")?
        };
        if snapshot.manifest.source != Source::Workbench {
            return Err("legacy_templates_unavailable");
        }
        let inventory = snapshot
            .manifest
            .files
            .iter()
            .find(|file| file.name == "profile-templates.json")
            .filter(|file| file.issue.is_none())
            .ok_or("legacy_templates_unavailable")?;
        let bytes = snapshot
            .bytes(&inventory.name)
            .ok_or("legacy_templates_unavailable")?;
        let templates = workbench_lib::component::ProfileTemplateStore::load(
            std::str::from_utf8(bytes).map_err(|_| "legacy_templates_unavailable")?,
        )
        .map_err(|_| "legacy_templates_unavailable")?;
        Ok((snapshot.id()?, templates))
    }
    pub(crate) fn session_source(
        &self,
        job_id: &str,
    ) -> Result<(String, code_pad_lib::core::session::Session)> {
        let snapshot = {
            let current = self.current.lock().map_err(|_| "legacy_import_busy")?;
            current
                .as_ref()
                .filter(|job| job.id == job_id && job.phase == Phase::Ready)
                .and_then(|job| job.snapshot.clone())
                .ok_or("legacy_import_stale")?
        };
        if snapshot.manifest.source != Source::CodePad
            || !snapshot
                .manifest
                .files
                .iter()
                .any(|file| file.name == "session.json" && file.issue.is_none())
        {
            return Err("legacy_session_unavailable");
        }
        let bytes = snapshot
            .bytes("session.json")
            .ok_or("legacy_session_unavailable")?;
        let session = code_pad_lib::core::session::Session::from_json(
            std::str::from_utf8(bytes).map_err(|_| "legacy_session_unavailable")?,
        )
        .map_err(|_| "legacy_session_unavailable")?;
        Ok((snapshot.id()?, session))
    }
    pub(crate) fn workspace(
        &self,
        job_id: &str,
    ) -> Result<Option<crate::core::legacy_workspace::Proposal>> {
        let (_, session) = self.session_source(job_id)?;
        crate::core::legacy_workspace::proposal(session.workspace_folder.as_deref())
    }
    pub(crate) fn recovery_source(
        &self,
        job_id: &str,
    ) -> Result<(String, code_pad_lib::core::recovery::RecoveryFile)> {
        let snapshot = {
            let current = self.current.lock().map_err(|_| "legacy_import_busy")?;
            current
                .as_ref()
                .filter(|job| job.id == job_id && job.phase == Phase::Ready)
                .and_then(|job| job.snapshot.clone())
                .ok_or("legacy_import_stale")?
        };
        if snapshot.manifest.source != Source::CodePad
            || !snapshot
                .manifest
                .files
                .iter()
                .any(|file| file.name == "recovery.json" && file.issue.is_none())
        {
            return Err("legacy_recovery_unavailable");
        }
        let bytes = snapshot
            .bytes("recovery.json")
            .ok_or("legacy_recovery_unavailable")?;
        let session = serde_json::from_slice(bytes).map_err(|_| "legacy_recovery_unavailable")?;
        Ok((snapshot.id()?, session))
    }
    pub(crate) fn lsp_source(
        &self,
        job_id: &str,
    ) -> Result<(String, code_pad_lib::lsp::LspConfig)> {
        let snapshot = {
            let current = self.current.lock().map_err(|_| "legacy_import_busy")?;
            current
                .as_ref()
                .filter(|job| job.id == job_id && job.phase == Phase::Ready)
                .and_then(|job| job.snapshot.clone())
                .ok_or("legacy_import_stale")?
        };
        if snapshot.manifest.source != Source::CodePad
            || !snapshot
                .manifest
                .files
                .iter()
                .any(|file| file.name == "lsp/config.json" && file.issue.is_none())
        {
            return Err("legacy_lsp_unavailable");
        }
        let config = crate::core::legacy_lsp::decode(
            snapshot
                .bytes("lsp/config.json")
                .ok_or("legacy_lsp_unavailable")?,
        )?;
        Ok((snapshot.id()?, config))
    }
    pub(crate) fn catalog(&self) -> Result<Catalog> {
        self.stores.read()?;
        let root = MetadataRoot::open(self.stores.root())?;
        let destination = root.path().join("legacy-imports");
        let result = match std::fs::symlink_metadata(&destination) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Catalog::default(),
            Err(_) => return Err("legacy_snapshot_unavailable"),
            Ok(_) => Snapshot::catalog(&destination, || {
                self.stores.read()?;
                root.revalidate()
            })?,
        };
        root.revalidate()?;
        self.stores.read()?;
        Ok(result)
    }
    pub(crate) fn verify(&self, id: String) -> Result<Job> {
        self.stores.read()?;
        let root = MetadataRoot::open(self.stores.root())?;
        let manifest = Snapshot::describe(&root.path().join("legacy-imports"), &id)?;
        root.revalidate()?;
        self.start_job(manifest.source, Some(id))
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
        self.start_job(source, None)
    }
    fn start_job(&self, source: Source, existing: Option<String>) -> Result<Job> {
        self.stores.read()?;
        let mut current = self.current.lock().map_err(|_| "legacy_import_busy")?;
        if current.as_ref().is_some_and(|job| job.phase.active()) {
            return Err("legacy_import_busy");
        }
        let job = Job {
            id: uuid::Uuid::new_v4().to_string(),
            source,
            operation: if existing.is_some() {
                Operation::Verify
            } else {
                Operation::Preserve
            },
            phase: if existing.is_some() {
                Phase::Checking
            } else {
                Phase::Reading
            },
            snapshot_id: None,
            manifest: None,
            issue: None,
            cancelled: Arc::default(),
            snapshot: None,
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
                    if let Some(id) = existing {
                        let root = MetadataRoot::open(stores.root())?;
                        let snapshot = Snapshot::load_checked(
                            &root.path().join("legacy-imports"),
                            &id,
                            || {
                                check()?;
                                root.revalidate()
                            },
                        )?;
                        return Ok((id, Arc::new(snapshot)));
                    }
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
                    Ok((id, Arc::new(snapshot)))
                })();
                if let Ok(mut state) = state.lock() {
                    if let Some(job) = state.as_mut() {
                        match result {
                            Ok((id, snapshot)) => {
                                job.phase = Phase::Ready;
                                job.snapshot_id = Some(id);
                                job.manifest = Some(snapshot.manifest.clone());
                                job.snapshot = Some(snapshot);
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
    fn last_workspace_uses_only_the_verified_job_and_preserves_offline_metadata() {
        let base = tempfile::tempdir().unwrap();
        let root = base.path().join("workspace");
        std::fs::create_dir(&root).unwrap();
        let source = base.path().join(Source::CodePad.identifier());
        std::fs::create_dir(&source).unwrap();
        let mut session = code_pad_lib::core::session::Session::empty();
        session.workspace_folder = Some(r"C:\offline\한글 폴더".into());
        let bytes = session.to_json().unwrap();
        std::fs::write(source.join("session.json"), &bytes).unwrap();
        let stores = Arc::new(StoreRoot::open(&root).unwrap());
        let owner = LegacyImports::new(stores.clone()).unwrap();
        assert_eq!(
            owner.workspace("foreign").unwrap_err(),
            "legacy_import_stale"
        );
        owner.start(Source::CodePad).unwrap();
        let job = wait(&owner);
        assert!(job.phase == Phase::Ready);
        assert_eq!(
            std::fs::read_to_string(source.join("session.json")).unwrap(),
            bytes
        );
        std::fs::remove_dir_all(source).unwrap();
        let proposal = owner.workspace(&job.id).unwrap().unwrap();
        assert_eq!(proposal.path, session.workspace_folder.unwrap());
        assert_eq!(
            proposal.target,
            crate::core::legacy_workspace::Target::Windows
        );
        assert!(stores.read().unwrap().is_none());
        owner.start(Source::RepoManager).unwrap();
        assert_eq!(owner.workspace(&job.id).unwrap_err(), "legacy_import_stale");
        wait(&owner);
    }
    #[test]
    fn template_source_requires_a_ready_verified_workbench_job_and_keeps_original_bytes() {
        let base = tempfile::tempdir().unwrap();
        let root = base.path().join("workspace");
        std::fs::create_dir(&root).unwrap();
        let source = base.path().join(Source::Workbench.identifier());
        std::fs::create_dir(&source).unwrap();
        let mut template = workbench_lib::component::ProfileTemplate::new("빈 경로 기본값");
        template.expected_ports = vec![4321];
        let templates = workbench_lib::component::ProfileTemplateStore {
            version: 1,
            templates: vec![template],
        };
        let bytes = templates.to_json_checked().unwrap();
        std::fs::write(source.join("profile-templates.json"), &bytes).unwrap();
        let stores = Arc::new(StoreRoot::open(&root).unwrap());
        let owner = LegacyImports::new(stores.clone()).unwrap();
        assert_eq!(
            owner.template_source("foreign").unwrap_err(),
            "legacy_import_stale"
        );
        owner.start(Source::Workbench).unwrap();
        let job = wait(&owner);
        assert!(job.phase == Phase::Ready);
        assert_eq!(
            std::fs::read_to_string(source.join("profile-templates.json")).unwrap(),
            bytes
        );
        std::fs::remove_dir_all(source).unwrap();
        let (snapshot_id, saved) = owner.template_source(&job.id).unwrap();
        assert_eq!(Some(&snapshot_id), job.snapshot_id.as_ref());
        assert_eq!(saved, templates);
        assert!(stores.read().unwrap().is_none());
        owner.start(Source::RepoManager).unwrap();
        assert_eq!(
            owner.template_source(&job.id).unwrap_err(),
            "legacy_import_stale"
        );
        let next = wait(&owner);
        assert_eq!(
            owner.template_source(&next.id).unwrap_err(),
            "legacy_templates_unavailable"
        );
    }
    fn wait(owner: &LegacyImports) -> Job {
        let start = Instant::now();
        loop {
            let job = owner.status().unwrap().unwrap();
            if !job.phase.active() {
                return job;
            }
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    fn restarted_owner_verifies_saved_bytes_without_reopening_the_legacy_source() {
        let base = tempfile::tempdir().unwrap();
        let root = base.path().join("workspace");
        std::fs::create_dir(&root).unwrap();
        let source = base.path().join(Source::Workbench.identifier());
        std::fs::create_dir(&source).unwrap();
        std::fs::write(
            source.join("project-profiles.json"),
            br#"{"version":1,"profiles":[]}"#,
        )
        .unwrap();
        let stores = Arc::new(StoreRoot::open(&root).unwrap());
        let owner = LegacyImports::new(stores.clone()).unwrap();
        assert!(owner.catalog().unwrap().snapshots.is_empty());
        assert!(!root.join("legacy-imports").exists());
        owner.start(Source::Workbench).unwrap();
        let id = wait(&owner).snapshot_id.unwrap();
        drop(owner);
        std::fs::remove_dir_all(source).unwrap();
        let owner = LegacyImports::new(stores.clone()).unwrap();
        assert!(owner.status().unwrap().is_none());
        assert_eq!(owner.catalog().unwrap().snapshots[0].id, id);
        assert!(owner.verify("../foreign".into()).is_err());
        owner.verify(id.clone()).unwrap();
        let verified = wait(&owner);
        assert!(matches!(verified.operation, Operation::Verify));
        assert!(verified.phase == Phase::Ready);
        assert_eq!(verified.snapshot_id.as_ref(), Some(&id));
        assert!(stores.read().unwrap().is_none());
        std::fs::write(
            root.join("legacy-imports")
                .join(&id)
                .join("project-profiles.json"),
            b"modified backup",
        )
        .unwrap();
        owner.verify(id).unwrap();
        let failed = wait(&owner);
        assert!(failed.phase == Phase::Failed);
        assert_eq!(failed.issue, Some("legacy_snapshot_changed"));
        assert!(failed.manifest.is_none());
        assert!(!root.join("stores").exists());
    }
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
