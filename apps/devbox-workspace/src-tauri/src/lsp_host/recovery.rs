//! Journal recovery has its own explicit file-write review. It never needs an
//! installed/enabled server, executable approval, or a readable LSP config.
use super::{input, Result};
use crate::{
    core::registry::Binding,
    files_host::FilesHost,
    host::Host,
    platform::{
        project_probe::ProjectLease,
        storage_paths::{display, ProtectedStorage},
    },
    private_metadata::MetadataRoot,
};
use code_pad_lib::lsp::{
    manager::recovery::{list_rename_recovery, RenameRecoveryPlan},
    DocumentError, LspDocumentAuthority, WorkspaceRoot,
};
use product_contract::ProjectContext;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, MutexGuard, TryLockError,
    },
    time::{Duration, Instant},
};

struct Store {
    host: Arc<Host>,
    context: ProjectContext,
    binding: Binding,
    data: MetadataRoot,
    backups: MetadataRoot,
}
impl Store {
    fn open(host: Arc<Host>, context: &ProjectContext) -> Result<Self> {
        let binding = host.projects()?.binding(context)?;
        let data = MetadataRoot::open(&host.component("files")?)?;
        let backups = data
            .child("views")?
            .child(&format!("worktree-{}", context.worktree_id))?
            .child("lsp")?
            .child("runtime")?
            .child("rename-backups")?;
        let store = Self {
            host,
            context: context.clone(),
            binding,
            data,
            backups,
        };
        store.check()?;
        Ok(store)
    }
    fn check(&self) -> Result<()> {
        if self.host.component("files")? != self.data.path()
            || self.host.projects()?.binding(&self.context)? != self.binding
        {
            return Err("lsp_context_changed");
        }
        self.data.revalidate()?;
        self.backups.revalidate()
    }
}
struct Scope {
    store: Store,
    lease: ProjectLease,
    protected: ProtectedStorage,
    files: Arc<Mutex<FilesHost>>,
    deadline: AtomicU64,
}
impl Scope {
    fn file_owner(&self) -> Result<MutexGuard<'_, FilesHost>> {
        let started = Instant::now();
        loop {
            crate::files_host::current_deadline(self.deadline.load(Ordering::Acquire))?;
            match self.files.try_lock() {
                Ok(files) => return Ok(files),
                Err(TryLockError::Poisoned(_)) => return Err("files_unavailable"),
                Err(TryLockError::WouldBlock) if started.elapsed() < Duration::from_secs(5) => {
                    // Session/recovery autosaves use this same metadata owner.
                    // Wait in the bounded native IO worker before deciding if
                    // a target is open; contention is not file evidence.
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(TryLockError::WouldBlock) => return Err("files_unavailable"),
            }
        }
    }
    fn check(&self) -> Result<()> {
        self.store.check()?;
        self.lease.revalidate()
    }
    fn path(&self, path: &Path, write: bool) -> Result<()> {
        self.check()?;
        let path = display(path)?;
        let root = Path::new(&self.store.binding.root);
        if !path.starts_with(root) {
            return Err("lsp_document_denied");
        }
        self.protected.ensure_user_path(&path)?;
        super::evidence::transport(root, &path)?;
        devbox_filesystem::ensure_no_links(&path).map_err(|_| "lsp_document_denied")?;
        let canonical = display(&fs::canonicalize(&path).map_err(|_| "lsp_document_denied")?)?;
        if canonical != path {
            return Err("lsp_document_denied");
        }
        self.protected.ensure_user_path(&canonical)?;
        if !canonical.ancestors().any(|parent| {
            devbox_filesystem::filesystem_identity(parent, true).ok()
                == Some(self.lease.native_root_identity())
        }) {
            return Err("lsp_document_denied");
        }
        if write {
            let files = self.file_owner()?;
            files.guard_recovery_write(
                &self.store.context,
                path.to_str().ok_or("lsp_document_denied")?,
            )?;
            if fs::metadata(&path)
                .map_err(|_| "lsp_document_denied")?
                .permissions()
                .readonly()
            {
                return Err("file_read_only");
            }
        }
        self.check()
    }
}
impl LspDocumentAuthority for Scope {
    fn validate_path(&self, path: &Path) -> std::result::Result<(), DocumentError> {
        self.path(path, false)
            .map_err(|_| DocumentError::PathOutsideWorkspace(path.into()))
    }
    fn validate_write_path(&self, path: &Path) -> std::result::Result<(), DocumentError> {
        self.path(path, true)
            .map_err(|_| DocumentError::PathOutsideWorkspace(path.into()))
    }
}
struct Pending {
    scope: Arc<Scope>,
    plan: RenameRecoveryPlan,
    created: Instant,
}
#[derive(Default)]
pub(super) struct Recovery {
    pending: HashMap<String, Pending>,
}
impl Recovery {
    pub(super) fn expire(&mut self) {
        self.pending
            .retain(|_, pending| pending.created.elapsed() < Duration::from_secs(180));
    }
    pub(super) fn list(host: Arc<Host>, context: &ProjectContext, deadline: u64) -> Result<Value> {
        let store = Store::open(host, context)?;
        let result = list_rename_recovery(store.backups.path(), &|| {
            crate::files_host::current_deadline(deadline)
                .and_then(|()| store.check())
                .map_err(str::to_owned)
        })
        .map_err(|_| "lsp_recovery_unavailable")?;
        serde_json::to_value(result).map_err(|_| "lsp_recovery_unavailable")
    }
    pub(super) fn preview(
        &mut self,
        host: Arc<Host>,
        context: &ProjectContext,
        protected: ProtectedStorage,
        files: Arc<Mutex<FilesHost>>,
        args: Value,
        deadline: u64,
    ) -> Result<Value> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Input {
            journal_id: String,
        }
        let input: Input = input(args)?;
        self.expire();
        if self.pending.len() >= 8 {
            return Err("lsp_recovery_limit");
        }
        crate::files_host::current_deadline(deadline)?;
        let store = Store::open(host.clone(), context)?;
        let lease = host.projects()?.admit(context)?;
        let scope = Arc::new(Scope {
            store,
            lease,
            protected,
            files,
            deadline: AtomicU64::new(deadline),
        });
        let workspace =
            WorkspaceRoot::with_authority(&scope.store.binding.root, Some(scope.clone()))
                .map_err(|_| "lsp_recovery_unavailable")?;
        let plan = RenameRecoveryPlan::prepare(
            workspace,
            scope.store.backups.path(),
            &input.journal_id,
            &|| {
                crate::files_host::current_deadline(deadline)
                    .and_then(|()| scope.check())
                    .map_err(str::to_owned)
            },
        )
        .map_err(|_| "lsp_recovery_conflict")?;
        let id = uuid::Uuid::new_v4().to_string();
        let result = json!({"previewId":id,"journalId":input.journal_id,"files":plan.files});
        self.pending.insert(
            id,
            Pending {
                scope,
                plan,
                created: Instant::now(),
            },
        );
        Ok(result)
    }
    pub(super) fn consume(
        &mut self,
        context: &ProjectContext,
        args: Value,
        apply: bool,
        deadline: u64,
    ) -> Result<Value> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Input {
            preview_id: String,
        }
        let input: Input = input(args)?;
        let pending = self
            .pending
            .remove(&input.preview_id)
            .ok_or("lsp_recovery_stale")?;
        if pending.scope.store.context != *context
            || pending.created.elapsed() >= Duration::from_secs(180)
        {
            return Err("lsp_recovery_stale");
        }
        if !apply {
            return Ok(Value::Null);
        }
        pending.scope.deadline.store(deadline, Ordering::Release);
        let result = pending
            .plan
            .apply(&|| {
                crate::files_host::current_deadline(deadline)
                    .and_then(|()| pending.scope.check())
                    .map_err(str::to_owned)
            })
            .map_err(|_| "lsp_recovery_conflict")?;
        serde_json::to_value(result).map_err(|_| "lsp_recovery_unavailable")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    struct Fixture {
        _data: tempfile::TempDir,
        _root: tempfile::TempDir,
        host: Arc<Host>,
        context: ProjectContext,
        files: Arc<Mutex<FilesHost>>,
        protected: ProtectedStorage,
        target: PathBuf,
        journal: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let data = tempfile::tempdir().unwrap();
            let root = tempfile::tempdir().unwrap();
            let host = Arc::new(Host::open(data.path()).unwrap());
            host.start_empty().unwrap();
            let owner = host.projects().unwrap();
            let preview = owner.preview_fixture(root.path()).unwrap();
            let context = owner
                .apply(
                    &preview.preview_id,
                    "recovery fixture",
                    crate::project_owner::RegistrationAction::Register,
                )
                .unwrap()
                .1;
            let store = Store::open(host.clone(), &context).unwrap();
            let canonical_root = fs::canonicalize(&store.binding.root).unwrap();
            let target = canonical_root.join("fixture.rs");
            fs::write(&target, b"after\r\n").unwrap();
            let directory = store.backups.path().join("fixture-journal");
            fs::create_dir(&directory).unwrap();
            let backup = directory.join("backup-1.bak");
            fs::write(&backup, b"before\r\n").unwrap();
            let journal = directory.join("journal.json");
            fs::write(&journal, serde_json::to_vec(&json!({"schema":1,"planId":"fixture-journal",
                "workspaceRoot":canonical_root,"state":"applying","entries":[{"target":target,"backup":backup,
                "beforeSize":8,"beforeHash":crate::definitions::digest(b"before\r\n"),
                "afterSize":7,"afterHash":crate::definitions::digest(b"after\r\n")}]})).unwrap()).unwrap();
            // Recovery must still work with a corrupt config and no execution approval.
            fs::write(
                store
                    .backups
                    .path()
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .join("config.json"),
                b"corrupt preserved config",
            )
            .unwrap();
            let protected = ProtectedStorage::new(host.storage_root(), vec![]).unwrap();
            Self {
                _data: data,
                _root: root,
                host,
                context,
                files: Default::default(),
                protected,
                target,
                journal,
            }
        }
        fn preview(&self, recovery: &mut Recovery) -> Result<Value> {
            recovery.preview(
                self.host.clone(),
                &self.context,
                self.protected.clone(),
                self.files.clone(),
                json!({"journalId":"fixture-journal"}),
                u64::MAX,
            )
        }
        fn open(&self) {
            let lease = self.host.projects().unwrap().admit(&self.context).unwrap();
            let mut owner = crate::file_owner::FileOwner::default();
            owner
                .open(
                    Some((&self.context, &lease)),
                    code_pad_lib::commands::file::OpenFileRequest {
                        path: display(&self.target).unwrap().to_str().unwrap().into(),
                        encoding: None,
                    },
                )
                .unwrap();
            *self.files.lock().unwrap() = FilesHost::from_editor_fixture(owner);
        }
    }
    #[test]
    fn unrelated_file_metadata_contention_does_not_invent_a_recovery_conflict() {
        let fixture = Fixture::new();
        let files = fixture.files.clone();
        let (ready, receive) = std::sync::mpsc::channel();
        let metadata = std::thread::spawn(move || {
            let _files = files.lock().unwrap();
            ready.send(()).unwrap();
            std::thread::sleep(Duration::from_millis(150));
        });
        receive.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut recovery = Recovery::default();
        let preview = fixture.preview(&mut recovery).unwrap();
        metadata.join().unwrap();
        assert_eq!(fs::read(&fixture.target).unwrap(), b"after\r\n");
        recovery
            .consume(
                &fixture.context,
                json!({"previewId":preview["previewId"]}),
                false,
                u64::MAX,
            )
            .unwrap();
        assert!(fixture.journal.exists());
    }
    #[test]
    fn recovery_is_independent_of_lsp_config_and_uses_one_time_native_approval() {
        let fixture = Fixture::new();
        let listing = Recovery::list(fixture.host.clone(), &fixture.context, u64::MAX).unwrap();
        assert_eq!(listing["records"][0]["journalId"], "fixture-journal");
        let mut recovery = Recovery::default();
        let preview = fixture.preview(&mut recovery).unwrap();
        assert_eq!(fs::read(&fixture.target).unwrap(), b"after\r\n");
        let args = json!({"previewId":preview["previewId"]});
        let result = recovery
            .consume(&fixture.context, args.clone(), true, u64::MAX)
            .unwrap();
        assert_eq!(result["complete"], true);
        assert_eq!(fs::read(&fixture.target).unwrap(), b"before\r\n");
        assert_eq!(
            recovery
                .consume(&fixture.context, args, true, u64::MAX)
                .unwrap_err(),
            "lsp_recovery_stale"
        );
        assert!(!fixture.journal.exists());
    }
    #[test]
    fn clean_or_dirty_open_editors_block_preview_and_later_apply() {
        let fixture = Fixture::new();
        let mut recovery = Recovery::default();
        fixture.open();
        assert_eq!(
            fixture.preview(&mut recovery).unwrap_err(),
            "lsp_recovery_conflict"
        );
        *fixture.files.lock().unwrap() = FilesHost::default();
        let preview = fixture.preview(&mut recovery).unwrap();
        fixture.open();
        assert_eq!(
            recovery
                .consume(
                    &fixture.context,
                    json!({"previewId":preview["previewId"]}),
                    true,
                    u64::MAX
                )
                .unwrap_err(),
            "lsp_recovery_conflict"
        );
        assert_eq!(fs::read(&fixture.target).unwrap(), b"after\r\n");
        assert!(fixture.journal.exists());
    }
    #[test]
    fn cancelled_expired_foreign_and_changed_reviews_preserve_backups() {
        for mode in ["cancel", "expired", "foreign", "changed"] {
            let fixture = Fixture::new();
            let mut recovery = Recovery::default();
            let preview = fixture.preview(&mut recovery).unwrap();
            let args = json!({"previewId":preview["previewId"]});
            let mut context = fixture.context.clone();
            if mode == "foreign" {
                context.revision += 1;
            }
            if mode == "changed" {
                fs::write(&fixture.target, b"external").unwrap();
            }
            let result = recovery.consume(
                &context,
                args.clone(),
                mode != "cancel",
                if mode == "expired" { 0 } else { u64::MAX },
            );
            assert_eq!(result.is_ok(), mode == "cancel");
            assert!(recovery
                .consume(&fixture.context, args, true, u64::MAX)
                .is_err());
            assert_eq!(
                fs::read(&fixture.target).unwrap(),
                if mode == "changed" {
                    b"external".as_slice()
                } else {
                    b"after\r\n"
                }
            );
            assert!(fixture.journal.exists());
        }
    }
    #[test]
    fn metadata_listing_preserves_future_journals_without_following_recorded_paths() {
        let fixture = Fixture::new();
        let mut raw: Value = serde_json::from_slice(&fs::read(&fixture.journal).unwrap()).unwrap();
        raw["schema"] = json!(99);
        raw["entries"][0]["target"] = json!("\\\\missing-server\\never-contact\\fixture.rs");
        let bytes = serde_json::to_vec(&raw).unwrap();
        fs::write(&fixture.journal, &bytes).unwrap();
        let listing = Recovery::list(fixture.host.clone(), &fixture.context, u64::MAX).unwrap();
        assert_eq!(listing["records"][0]["available"], false);
        assert!(fixture.preview(&mut Recovery::default()).is_err());
        assert_eq!(fs::read(&fixture.journal).unwrap(), bytes);
        assert_eq!(fs::read(&fixture.target).unwrap(), b"after\r\n");
    }
}
