//! Explicit, identity-pinned recovery for hosted consumers. Construction only
//! reads evidence; applying consumes the plan and rechecks every target first.
use super::*;

type Result<T> = std::result::Result<T, String>;
const INVALID: &str = "이름 변경 복구 기록이 손상되었거나 변경되었습니다";
const CONFLICT: &str = "복구 대상이 변경되었거나 접근할 수 없습니다. 원본 백업을 보존했습니다";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameRecoveryRecord {
    pub journal_id: String,
    pub files: usize,
    pub available: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameRecoveryListing {
    pub records: Vec<RenameRecoveryRecord>,
    pub truncated: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameRecoveryFile {
    pub path: String,
    pub restore: bool,
    pub current: String,
    pub original: String,
    pub current_size: u64,
    pub original_size: u64,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameRecoveryResult {
    pub complete: bool,
    pub restored: Vec<String>,
    pub cleanup_pending: bool,
    pub error: Option<String>,
}

struct Evidence {
    path: PathBuf,
    identity: FilesystemIdentity,
    mtime: i64,
    size: u64,
    hash: String,
}
impl Evidence {
    fn read(path: &Path, limit: u64) -> Result<(Self, Vec<u8>)> {
        devbox_filesystem::ensure_no_links(path).map_err(|_| INVALID.to_owned())?;
        let identity =
            devbox_filesystem::filesystem_identity(path, false).map_err(|_| INVALID.to_owned())?;
        let (metadata, bytes) = file_commands::read_stable_limited(path, Some(limit))
            .map_err(|_| INVALID.to_owned())?;
        if devbox_filesystem::filesystem_identity(path, false).ok() != Some(identity) {
            return Err(INVALID.into());
        }
        Ok((
            Self {
                path: path.into(),
                identity,
                mtime: file_commands::modified_epoch_nanos(&metadata)
                    .map_err(|_| INVALID.to_owned())?,
                size: metadata.len(),
                hash: file_commands::content_hash(&bytes),
            },
            bytes,
        ))
    }
    fn revalidate(&self, limit: u64) -> Result<()> {
        let (current, _) = Self::read(&self.path, limit)?;
        if current.identity != self.identity
            || current.mtime != self.mtime
            || current.size != self.size
            || current.hash != self.hash
        {
            return Err(CONFLICT.into());
        }
        Ok(())
    }
    fn expected(&self) -> file_commands::ExpectedFileSnapshot<'_> {
        file_commands::ExpectedFileSnapshot {
            mtime: self.mtime,
            size: self.size,
            content_hash: &self.hash,
            identity: Some(self.identity),
        }
    }
}
struct Entry {
    target: Evidence,
    backup: Evidence,
    display: String,
    restore: bool,
}
pub struct RenameRecoveryPlan {
    workspace: WorkspaceRoot,
    root: PathBuf,
    root_identity: FilesystemIdentity,
    directory: PathBuf,
    directory_identity: FilesystemIdentity,
    journal: Evidence,
    record: RenameJournal,
    entries: Vec<Entry>,
    pub files: Vec<RenameRecoveryFile>,
}
fn directory_identity(path: &Path) -> Result<FilesystemIdentity> {
    devbox_filesystem::ensure_no_links(path).map_err(|_| INVALID.to_owned())?;
    devbox_filesystem::filesystem_identity(path, true).map_err(|_| INVALID.to_owned())
}

/// This metadata-only list never follows any path recorded inside a journal.
/// The host callback must pin its private generation before each disk access.
pub fn list_rename_recovery(
    root: &Path,
    check: &dyn Fn() -> Result<()>,
) -> Result<RenameRecoveryListing> {
    check()?;
    directory_identity(root)?;
    let mut records = Vec::new();
    let mut truncated = false;
    for (inspected, entry) in fs::read_dir(root)
        .map_err(|_| INVALID.to_owned())?
        .enumerate()
    {
        check()?;
        if inspected == MAX_PENDING_RENAMES {
            truncated = true;
            break;
        }
        let entry = entry.map_err(|_| INVALID.to_owned())?;
        let Some(id) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !valid_rename_plan_id(&id) {
            continue;
        }
        let directory = entry.path();
        let parsed: Result<RenameJournal> = (|| {
            directory_identity(&directory)?;
            let (_, bytes) =
                Evidence::read(&directory.join("journal.json"), MAX_RENAME_JOURNAL_BYTES)?;
            let journal: RenameJournal =
                serde_json::from_slice(&bytes).map_err(|_| INVALID.to_owned())?;
            if !valid_rename_journal(&journal) || journal.plan_id != id {
                return Err(INVALID.into());
            }
            Ok(journal)
        })();
        match parsed {
            Ok(journal) if journal.state == RenameJournalState::Committed => {}
            Ok(journal) => records.push(RenameRecoveryRecord {
                journal_id: id,
                files: journal.entries.len(),
                available: true,
            }),
            Err(_) => records.push(RenameRecoveryRecord {
                journal_id: id,
                files: 0,
                available: false,
            }),
        }
    }
    check()?;
    Ok(RenameRecoveryListing { records, truncated })
}
impl RenameRecoveryPlan {
    pub fn prepare(
        workspace: WorkspaceRoot,
        root: &Path,
        id: &str,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<Self> {
        check()?;
        if !valid_rename_plan_id(id) {
            return Err(INVALID.into());
        }
        let root_identity = directory_identity(root)?;
        let directory = root.join(id);
        let directory_identity = directory_identity(&directory)?;
        let (journal, bytes) =
            Evidence::read(&directory.join("journal.json"), MAX_RENAME_JOURNAL_BYTES)?;
        let record: RenameJournal =
            serde_json::from_slice(&bytes).map_err(|_| INVALID.to_owned())?;
        if !valid_rename_journal(&record)
            || record.plan_id != id
            || record.state == RenameJournalState::Committed
            || Path::new(&record.workspace_root) != workspace.path()
            || record.entries.is_empty()
        {
            return Err(INVALID.into());
        }
        let mut targets = BTreeSet::new();
        let mut backups = BTreeSet::new();
        // Authorize ALL recorded targets lexically before resolving any of them.
        for entry in &record.entries {
            let target = Path::new(&entry.target);
            let backup = Path::new(&entry.backup);
            if !target.is_absolute()
                || is_sensitive_rename_path(target)
                || !backup.is_absolute()
                || backup.parent() != Some(&directory)
                || !target.starts_with(workspace.path())
                || backup
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_none_or(|name| !name.starts_with("backup-") || !name.ends_with(".bak"))
                || !targets.insert(target.to_owned())
                || !backups.insert(backup.to_owned())
            {
                return Err(INVALID.into());
            }
            workspace
                .validate_write_access(target)
                .map_err(|_| CONFLICT.to_owned())?;
        }
        let mut entries = Vec::new();
        let mut files = Vec::new();
        for item in &record.entries {
            check()?;
            let target = Path::new(&item.target);
            workspace
                .validate_write_access(target)
                .map_err(|_| CONFLICT.to_owned())?;
            let resolved = workspace
                .resolve_document(target)
                .map_err(|_| CONFLICT.to_owned())?
                .0;
            if resolved != target {
                return Err(CONFLICT.into());
            }
            let (target, current) = Evidence::read(target, MAX_RENAME_TOTAL_BYTES as u64)?;
            let (backup, original) =
                Evidence::read(Path::new(&item.backup), MAX_RENAME_TOTAL_BYTES as u64)?;
            if backup.size != item.before_size || backup.hash != item.before_hash {
                return Err(INVALID.into());
            }
            let restore = if target.size == item.before_size && target.hash == item.before_hash {
                false
            } else if target.size == item.after_size && target.hash == item.after_hash {
                true
            } else {
                return Err(CONFLICT.into());
            };
            let display = workspace
                .relative_path(&target.path)
                .and_then(|path| path.to_str().map(str::to_owned))
                .ok_or_else(|| INVALID.to_owned())?;
            let current = crate::core::encoding::decode_detect(&current);
            let original = crate::core::encoding::decode_detect(&original);
            if current.lossy || original.lossy {
                return Err(INVALID.into());
            }
            files.push(RenameRecoveryFile {
                path: display.clone(),
                restore,
                current: bounded_rename_excerpt(&current.text),
                original: bounded_rename_excerpt(&original.text),
                current_size: target.size,
                original_size: backup.size,
            });
            entries.push(Entry {
                target,
                backup,
                display,
                restore,
            });
        }
        let plan = Self {
            workspace,
            root: root.into(),
            root_identity,
            directory,
            directory_identity,
            journal,
            record,
            entries,
            files,
        };
        plan.revalidate(check)?;
        Ok(plan)
    }
    fn storage(&self, check: &dyn Fn() -> Result<()>) -> Result<()> {
        check()?;
        if directory_identity(&self.root)? != self.root_identity
            || directory_identity(&self.directory)? != self.directory_identity
        {
            return Err(INVALID.into());
        }
        self.journal.revalidate(MAX_RENAME_JOURNAL_BYTES)
    }
    fn revalidate(&self, check: &dyn Fn() -> Result<()>) -> Result<()> {
        self.storage(check)?;
        for entry in &self.entries {
            check()?;
            self.workspace
                .validate_write_access(&entry.target.path)
                .map_err(|_| CONFLICT.to_owned())?;
            entry.target.revalidate(MAX_RENAME_TOTAL_BYTES as u64)?;
            entry.backup.revalidate(MAX_RENAME_TOTAL_BYTES as u64)?;
        }
        Ok(())
    }
    pub fn apply(mut self, check: &dyn Fn() -> Result<()>) -> Result<RenameRecoveryResult> {
        self.revalidate(check)?;
        let mut restored = Vec::new();
        for entry in &self.entries {
            if !entry.restore {
                continue;
            }
            let guard = || {
                self.storage(check)?;
                self.workspace
                    .validate_write_access(&entry.target.path)
                    .map_err(|_| CONFLICT.to_owned())
            };
            let backup = file_commands::CreatedBackup {
                path: entry.backup.path.clone(),
                identity: entry.backup.identity,
                size: entry.backup.size,
                content_hash: entry.backup.hash.clone(),
            };
            let result = guard().and_then(|()| {
                file_commands::restore_sibling_backup_if_current_limited_with_guard(
                    &entry.target.path,
                    &backup,
                    Some(entry.target.expected()),
                    Some(MAX_RENAME_TOTAL_BYTES as u64),
                    &|| guard().map_err(|_| file_commands::FileError::BackupIntegrity),
                )
                .map_err(|_| CONFLICT.to_owned())
            });
            if let Err(error) = result {
                return Ok(RenameRecoveryResult {
                    complete: false,
                    restored,
                    cleanup_pending: true,
                    error: Some(error),
                });
            }
            restored.push(entry.display.clone());
        }
        // Keep the recovery journal unless every replacement was completed.
        // Cleanup removes only known, still-owned files; unexpected children stay.
        let cleanup = (|| {
            self.storage(check)?;
            self.record.state = RenameJournalState::Committed;
            write_rename_journal(&self.directory, &self.record)?;
            self.journal = Evidence::read(&self.journal.path, MAX_RENAME_JOURNAL_BYTES)?.0;
            for entry in &self.entries {
                self.storage(check)?;
                entry.backup.revalidate(MAX_RENAME_TOTAL_BYTES as u64)?;
                fs::remove_file(&entry.backup.path).map_err(|_| INVALID.to_owned())?;
            }
            self.storage(check)?;
            fs::remove_file(&self.journal.path).map_err(|_| INVALID.to_owned())?;
            fs::remove_dir(&self.directory).map_err(|_| INVALID.to_owned())
        })();
        Ok(RenameRecoveryResult {
            complete: true,
            restored,
            cleanup_pending: cleanup.is_err(),
            error: cleanup
                .err()
                .map(|_| "복원은 완료됐지만 일부 백업 정리가 남아 있습니다".into()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        _workspace: tempfile::TempDir,
        _data: tempfile::TempDir,
        workspace: WorkspaceRoot,
        root: PathBuf,
        directory: PathBuf,
        targets: Vec<PathBuf>,
    }
    impl Fixture {
        fn new(count: usize) -> Self {
            let workspace = tempfile::tempdir().unwrap();
            let data = tempfile::tempdir().unwrap();
            let root = fs::canonicalize(data.path()).unwrap();
            let workspace_root = WorkspaceRoot::new(workspace.path()).unwrap();
            let directory =
                file_commands::create_private_backup_dir(&root, "recovery-fixture").unwrap();
            let mut entries = Vec::new();
            let mut targets = Vec::new();
            for index in 0..count {
                let target = workspace_root.path().join(format!("file-{index}.rs"));
                fs::write(&target, b"before\r\n").unwrap();
                let backup = file_commands::create_sibling_backup(
                    &directory,
                    &target,
                    b"before\r\n",
                    &fs::metadata(&target).unwrap().permissions(),
                    1,
                    index,
                )
                .unwrap();
                fs::write(&target, b"after\r\n").unwrap();
                entries.push(RenameJournalEntry {
                    target: target.to_str().unwrap().into(),
                    backup: backup.path.to_str().unwrap().into(),
                    before_size: 8,
                    before_hash: file_commands::content_hash(b"before\r\n"),
                    after_size: 7,
                    after_hash: file_commands::content_hash(b"after\r\n"),
                });
                targets.push(target);
            }
            write_rename_journal(
                &directory,
                &RenameJournal {
                    schema: 1,
                    plan_id: "recovery-fixture".into(),
                    workspace_root: workspace_root.path().to_str().unwrap().into(),
                    state: RenameJournalState::Applying,
                    entries,
                },
            )
            .unwrap();
            Self {
                _workspace: workspace,
                _data: data,
                workspace: workspace_root,
                root,
                directory,
                targets,
            }
        }
        fn plan(&self) -> RenameRecoveryPlan {
            RenameRecoveryPlan::prepare(
                self.workspace.clone(),
                &self.root,
                "recovery-fixture",
                &|| Ok(()),
            )
            .unwrap()
        }
    }
    #[test]
    fn explicit_recovery_preserves_preview_and_restores_exact_crlf_bytes() {
        let fixture = Fixture::new(2);
        let plan = fixture.plan();
        assert_eq!(plan.files.len(), 2);
        assert_eq!(plan.files[0].original, "before\r\n");
        assert_eq!(fs::read(&fixture.targets[0]).unwrap(), b"after\r\n");
        let result = plan.apply(&|| Ok(())).unwrap();
        assert!(result.complete);
        assert_eq!(result.restored.len(), 2);
        assert!(!result.cleanup_pending);
        assert!(!fixture.directory.exists());
        for target in &fixture.targets {
            assert_eq!(fs::read(target).unwrap(), b"before\r\n");
        }
    }
    #[test]
    fn all_targets_and_backups_are_rechecked_before_the_first_replacement() {
        for change_backup in [false, true] {
            let fixture = Fixture::new(2);
            let plan = fixture.plan();
            let changed = if change_backup {
                &plan.entries[1].backup.path
            } else {
                &fixture.targets[1]
            };
            fs::write(changed, b"external edit").unwrap();
            assert!(plan.apply(&|| Ok(())).is_err());
            assert_eq!(fs::read(&fixture.targets[0]).unwrap(), b"after\r\n");
            assert!(fixture.directory.join("journal.json").exists());
        }
    }
    #[test]
    fn partial_recovery_retains_journal_and_resumes_without_overwriting_newer_bytes() {
        let fixture = Fixture::new(2);
        let result = fixture
            .plan()
            .apply(&|| {
                if fs::read(&fixture.targets[0]).unwrap() == b"before\r\n" {
                    Err("authority retired".into())
                } else {
                    Ok(())
                }
            })
            .unwrap();
        assert!(!result.complete);
        assert_eq!(result.restored, ["file-0.rs"]);
        assert!(result.cleanup_pending);
        assert_eq!(fs::read(&fixture.targets[1]).unwrap(), b"after\r\n");
        let resumed = fixture.plan();
        assert!(!resumed.files[0].restore);
        assert!(resumed.files[1].restore);
        assert!(resumed.apply(&|| Ok(())).unwrap().complete);
    }
    #[test]
    fn restore_guard_rechecks_authority_after_temporary_bytes_are_prepared() {
        let fixture = Fixture::new(1);
        let plan = fixture.plan();
        let entry = &plan.entries[0];
        let calls = std::cell::Cell::new(0);
        let backup = file_commands::CreatedBackup {
            path: entry.backup.path.clone(),
            identity: entry.backup.identity,
            size: entry.backup.size,
            content_hash: entry.backup.hash.clone(),
        };
        assert!(
            file_commands::restore_sibling_backup_if_current_limited_with_guard(
                &entry.target.path,
                &backup,
                Some(entry.target.expected()),
                Some(MAX_RENAME_TOTAL_BYTES as u64),
                &|| {
                    calls.set(calls.get() + 1);
                    if calls.get() > 1 {
                        Err(file_commands::FileError::BackupIntegrity)
                    } else {
                        Ok(())
                    }
                }
            )
            .is_err()
        );
        assert_eq!(fs::read(&fixture.targets[0]).unwrap(), b"after\r\n");
        assert_eq!(fs::read_dir(fixture.workspace.path()).unwrap().count(), 1);
    }
    #[test]
    fn external_change_during_final_authority_check_is_not_overwritten() {
        let fixture = Fixture::new(1);
        let plan = fixture.plan();
        let entry = &plan.entries[0];
        let calls = std::cell::Cell::new(0);
        let backup = file_commands::CreatedBackup {
            path: entry.backup.path.clone(),
            identity: entry.backup.identity,
            size: entry.backup.size,
            content_hash: entry.backup.hash.clone(),
        };
        let result = file_commands::restore_sibling_backup_if_current_limited_with_guard(
            &entry.target.path,
            &backup,
            Some(entry.target.expected()),
            Some(MAX_RENAME_TOTAL_BYTES as u64),
            &|| {
                calls.set(calls.get() + 1);
                if calls.get() > 1 {
                    fs::write(&entry.target.path, b"external edit").unwrap();
                }
                Ok(())
            },
        );
        assert!(result.is_err());
        assert_eq!(fs::read(&fixture.targets[0]).unwrap(), b"external edit");
    }
    #[test]
    fn invalid_foreign_duplicate_and_linked_journals_preserve_targets() {
        for mutation in ["root", "target", "backup", "duplicate", "future"] {
            let fixture = Fixture::new(1);
            let path = fixture.directory.join("journal.json");
            let mut journal: RenameJournal =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            match mutation {
                "root" => journal.workspace_root = fixture.root.to_str().unwrap().into(),
                "target" => {
                    journal.entries[0].target =
                        fixture.root.join("unavailable").to_str().unwrap().into()
                }
                "backup" => {
                    journal.entries[0].backup = fixture
                        .workspace
                        .path()
                        .join("unavailable")
                        .to_str()
                        .unwrap()
                        .into()
                }
                "duplicate" => journal.entries.push(journal.entries[0].clone()),
                "future" => journal.schema = 2,
                _ => unreachable!(),
            }
            write_rename_journal(&fixture.directory, &journal).unwrap();
            assert!(RenameRecoveryPlan::prepare(
                fixture.workspace.clone(),
                &fixture.root,
                "recovery-fixture",
                &|| Ok(())
            )
            .is_err());
            assert_eq!(fs::read(&fixture.targets[0]).unwrap(), b"after\r\n");
        }
        #[cfg(unix)]
        {
            let fixture = Fixture::new(1);
            let plan = fixture.plan();
            let backup = &plan.entries[0].backup.path;
            fs::remove_file(backup).unwrap();
            std::os::unix::fs::symlink(&fixture.targets[0], backup).unwrap();
            assert!(plan.apply(&|| Ok(())).is_err());
            assert_eq!(fs::read(&fixture.targets[0]).unwrap(), b"after\r\n");
        }
    }
    #[test]
    fn metadata_scan_is_bounded_and_unknown_cleanup_children_are_preserved() {
        let fixture = Fixture::new(1);
        for index in 0..MAX_PENDING_RENAMES {
            fs::create_dir(fixture.root.join(format!("unknown-{index}"))).unwrap();
        }
        assert!(
            list_rename_recovery(&fixture.root, &|| Ok(()))
                .unwrap()
                .truncated
        );
        let unknown = fixture.directory.join("unknown.txt");
        fs::write(&unknown, b"preserve").unwrap();
        let result = fixture.plan().apply(&|| Ok(())).unwrap();
        assert!(result.complete);
        assert!(result.cleanup_pending);
        assert_eq!(fs::read(unknown).unwrap(), b"preserve");
    }
}
