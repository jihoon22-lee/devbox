//! Private recovery metadata import. All calls share the native Files queue,
//! context permit and owner mutex with saves, discards and file previews.
use super::*;
use crate::core::legacy_recovery::{self, Candidate, StoredRecovery};

pub(super) struct ImportPreview {
    candidate: Candidate,
    revision: String,
    pub(super) context: Option<ProjectContext>,
    created: Instant,
    restore: Option<StoredRecovery>,
}
pub(super) fn read_recovery(view: &MetadataRoot) -> Result<(StoredRecovery, String)> {
    let bytes = view.read("recovery.json")?;
    let revision = recovery_revision(view, bytes.as_deref());
    Ok((
        bytes
            .as_deref()
            .map(StoredRecovery::decode)
            .transpose()?
            .unwrap_or_default(),
        revision,
    ))
}
fn recovery_revision(view: &MetadataRoot, bytes: Option<&[u8]>) -> String {
    let mut material = b"recovery-v1\0".to_vec();
    material.push(u8::from(bytes.is_some()));
    if let Some(bytes) = bytes {
        material.extend_from_slice(bytes);
    }
    session_revision(view, Some(&material))
}
pub(super) fn write_recovery(
    view: &MetadataRoot,
    next: &StoredRecovery,
    revision: &str,
) -> Result<String> {
    let bytes = next.encode()?;
    let before = view.read("recovery.json")?;
    if recovery_revision(view, before.as_deref()) != revision {
        return Err("files_recovery_changed");
    }
    view.write("recovery.json", &bytes)?;
    Ok(recovery_revision(view, Some(&bytes)))
}
fn read_recovery_history(history: &MetadataRoot, id: &str) -> Result<StoredRecovery> {
    if !session_history_id(id) {
        return Err("invalid_request");
    }
    let bytes = history
        .read(&format!("{id}.json"))?
        .ok_or("files_store_unavailable")?;
    if crate::core::legacy_inventory::digest(&bytes) != id {
        return Err("files_store_changed");
    }
    StoredRecovery::decode(&bytes)
}
impl FilesHost {
    pub(super) fn preview_recovery_import(
        &mut self,
        host: &Host,
        context: Option<&ProjectContext>,
        job_id: &str,
    ) -> Result<Value> {
        let (snapshot_id, source) = host.legacy.recovery_source(job_id)?;
        let root = context
            .map(|context| {
                host.projects()?
                    .binding(context)
                    .map(|binding| binding.root)
            })
            .transpose()?;
        let candidate = legacy_recovery::candidate(snapshot_id, &source, |path| {
            self.session_path_eligible(root.as_deref(), path)
        })?;
        if candidate.recovery.entries.is_empty() {
            return Err("legacy_recovery_no_files");
        }
        self.recovery_preview(host, context, candidate, None)
    }
    pub(super) fn recovery_preview(
        &mut self,
        host: &Host,
        context: Option<&ProjectContext>,
        candidate: Candidate,
        restore: Option<StoredRecovery>,
    ) -> Result<Value> {
        if self.has_documents() {
            return Err("legacy_recovery_documents_open");
        }
        self.recovery_imports
            .retain(|_, preview| preview.created.elapsed() < Duration::from_secs(180));
        if self.recovery_imports.len() >= 4 {
            return Err("legacy_recovery_review_limit");
        }
        let view = self.view(host, context)?;
        let before = view.read("recovery.json")?;
        let stored = before
            .as_deref()
            .map(StoredRecovery::decode)
            .transpose()?
            .unwrap_or_default();
        let id = uuid::Uuid::new_v4().to_string();
        let response = json!({"previewId":id,"candidate":candidate,"currentEntries":stored.recovery.entries, "conflictingPaths":stored.conflicts(&candidate),"conflict":!stored.conflicts(&candidate).is_empty() || (restore.is_some() && !stored.recovery.entries.is_empty()),"alreadyImported":restore.is_none()&&stored.imports.contains(&candidate.receipt),"restoring":restore.is_some()});
        self.recovery_imports.insert(
            id,
            ImportPreview {
                candidate,
                revision: recovery_revision(&view, before.as_deref()),
                context: context.cloned(),
                created: Instant::now(),
                restore,
            },
        );
        Ok(response)
    }
    pub(super) fn recovery_history(
        &mut self,
        host: &Host,
        context: Option<&ProjectContext>,
    ) -> Result<Value> {
        let history = self.view(host, context)?.child("recovery-history")?;
        let mut items = Vec::new();
        let mut unrecognized = 0;
        for entry in fs::read_dir(history.path())
            .map_err(|_| "files_store_unavailable")?
            .take(33)
        {
            let entry = entry.map_err(|_| "files_store_unavailable")?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(id) = name
                .strip_suffix(".json")
                .filter(|id| session_history_id(id))
            else {
                unrecognized += 1;
                continue;
            };
            match read_recovery_history(&history, id) {
                Ok(stored) => items
                    .push(json!({"id":id,"entries":stored.recovery.entries.len(),"issue":null})),
                Err(issue) => items.push(json!({"id":id,"entries":null,"issue":issue})),
            }
        }
        items.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        history.revalidate()?;
        Ok(json!({"items":items,"unrecognized":unrecognized}))
    }
    pub(super) fn preview_recovery_restore(
        &mut self,
        host: &Host,
        context: Option<&ProjectContext>,
        id: &str,
    ) -> Result<Value> {
        if !session_history_id(id) {
            return Err("invalid_request");
        }
        let history = self.view(host, context)?.child("recovery-history")?;
        let mut stored = read_recovery_history(&history, id)?;
        let root = context
            .map(|context| {
                host.projects()?
                    .binding(context)
                    .map(|binding| binding.root)
            })
            .transpose()?;
        let candidate = legacy_recovery::candidate(id.into(), &stored.recovery, |path| {
            self.session_path_eligible(root.as_deref(), path)
        })?;
        stored.recovery = candidate.recovery.clone();
        self.recovery_preview(host, context, candidate, Some(stored))
    }
    pub(super) fn apply_recovery_import(
        &mut self,
        host: &Host,
        context: Option<&ProjectContext>,
        id: &str,
        replace_existing: bool,
        deadline: u64,
    ) -> Result<Value> {
        let preview = self
            .recovery_imports
            .remove(id)
            .ok_or("legacy_recovery_review_stale")?;
        if preview.created.elapsed() >= Duration::from_secs(180)
            || preview.context.as_ref() != context
        {
            return Err("legacy_recovery_review_stale");
        }
        if self.has_documents() {
            return Err("legacy_recovery_documents_open");
        }
        current_deadline(deadline)?;
        let view = self.view(host, context)?;
        let before = view.read("recovery.json")?;
        if recovery_revision(&view, before.as_deref()) != preview.revision {
            return Err("files_recovery_changed");
        }
        let stored = before
            .as_deref()
            .map(StoredRecovery::decode)
            .transpose()?
            .unwrap_or_default();
        if preview.restore.is_none() && stored.imports.contains(&preview.candidate.receipt) {
            return Ok(json!({"importedEntries":0,"reused":true}));
        }
        let restoring = preview.restore.is_some();
        let next = if let Some(restore) = preview.restore {
            if !stored.recovery.entries.is_empty() && !replace_existing {
                return Err("legacy_recovery_conflict");
            }
            restore
        } else {
            stored.apply(&preview.candidate, replace_existing)?
        };
        let bytes = next.encode()?;
        if let Some(before) = &before {
            let history = view.child("recovery-history")?;
            let name = format!("{}.json", crate::core::legacy_inventory::digest(before));
            if history.read(&name)?.is_none()
                && fs::read_dir(history.path())
                    .map_err(|_| "files_store_unavailable")?
                    .take(32)
                    .count()
                    >= 32
            {
                return Err("legacy_recovery_limit");
            }
            history.preserve(&name, before)?;
        }
        current_deadline(deadline)?;
        if view.read("recovery.json")? != before {
            return Err("files_recovery_changed");
        }
        // One atomic publication commits both consumer metadata and its receipt.
        // The unchanged preimage is durable before this point, even on retry.
        view.write("recovery.json", &bytes)?;
        Ok(
            json!({"importedEntries":preview.candidate.recovery.entries.len(),"reused":false,"restored":restoring}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_import_is_metadata_only_and_preserves_history_repeat_and_stale_writes() {
        use crate::core::legacy_inventory::{digest, Source};
        let base = tempfile::tempdir().unwrap();
        let root = base.path().join("workspace");
        fs::create_dir(&root).unwrap();
        let source_root = base.path().join(Source::CodePad.identifier());
        fs::create_dir(&source_root).unwrap();
        let selected_path = base.path().join("선택한 파일.txt");
        fs::write(&selected_path, b"unchanged\r\n").unwrap();
        let host = Host::open(&root).unwrap();
        host.start_empty().unwrap();
        let mut files = FilesHost {
            data: Some(MetadataRoot::open(&host.component("files").unwrap()).unwrap()),
            ..FilesHost::default()
        };
        let selected = files
            .owner
            .approve_native_selection(&selected_path)
            .unwrap();
        let source = RecoveryFile {
            version: 1,
            entries: vec![RecoveryEntry {
                path: selected.clone(),
                content: "imported unsaved 한글\r\n".into(),
                base_hash: Some("original hash".into()),
                snapshot_at_ms: 100,
            }],
        };
        let source_bytes = serde_json::to_vec(&source).unwrap();
        fs::write(source_root.join("recovery.json"), &source_bytes).unwrap();
        let job = json!(host.legacy.start(Source::CodePad).unwrap());
        let job_id = job["id"].as_str().unwrap();
        let start = Instant::now();
        while json!(host.legacy.status().unwrap())["phase"] != "ready" {
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(5));
        }
        // The importer must use its already verified snapshot after source loss.
        fs::remove_file(source_root.join("recovery.json")).unwrap();
        let view = files.view(&host, None).unwrap();
        let (_, empty_revision) = read_recovery(&view).unwrap();
        let mut previous = StoredRecovery::default();
        previous.recovery = source.clone();
        previous.recovery.entries[0].content = "new destination buffer".into();
        let revision = write_recovery(&view, &previous, &empty_revision).unwrap();
        let before = view.read("recovery.json").unwrap().unwrap();
        let preview = files.preview_recovery_import(&host, None, job_id).unwrap();
        assert_eq!(preview["conflict"], true);
        let id = preview["previewId"].as_str().unwrap();
        assert_eq!(
            files.apply_recovery_import(&host, None, id, false, u64::MAX),
            Err("legacy_recovery_conflict")
        );
        assert_eq!(
            files.apply_recovery_import(&host, None, id, true, u64::MAX),
            Err("legacy_recovery_review_stale")
        );
        assert_eq!(view.read("recovery.json").unwrap().unwrap(), before);
        let preview = files.preview_recovery_import(&host, None, job_id).unwrap();
        files
            .apply_recovery_import(
                &host,
                None,
                preview["previewId"].as_str().unwrap(),
                true,
                u64::MAX,
            )
            .unwrap();
        assert_eq!(read_recovery(&view).unwrap().0.recovery, source);
        assert_eq!(
            write_recovery(&view, &previous, &revision),
            Err("files_recovery_changed")
        );
        assert!(!files.owner.has_documents());
        assert_eq!(fs::read(&selected_path).unwrap(), b"unchanged\r\n");
        let history = view.child("recovery-history").unwrap();
        let backup_id = digest(&before);
        assert_eq!(
            history.read(&format!("{backup_id}.json")).unwrap().unwrap(),
            before
        );
        let (mut discarded, current) = read_recovery(&view).unwrap();
        discarded.recovery.entries.clear();
        write_recovery(&view, &discarded, &current).unwrap();
        let preview = files.preview_recovery_import(&host, None, job_id).unwrap();
        assert_eq!(preview["alreadyImported"], true);
        assert_eq!(
            files
                .apply_recovery_import(
                    &host,
                    None,
                    preview["previewId"].as_str().unwrap(),
                    false,
                    u64::MAX
                )
                .unwrap()["reused"],
            true
        );
        assert!(read_recovery(&view).unwrap().0.recovery.entries.is_empty());
        let restore = files
            .preview_recovery_restore(&host, None, &backup_id)
            .unwrap();
        files
            .apply_recovery_import(
                &host,
                None,
                restore["previewId"].as_str().unwrap(),
                true,
                u64::MAX,
            )
            .unwrap();
        assert_eq!(read_recovery(&view).unwrap().0.recovery, previous.recovery);
        let stale = files.preview_recovery_import(&host, None, job_id).unwrap();
        let (mut changed, current) = read_recovery(&view).unwrap();
        changed.recovery.entries.clear();
        write_recovery(&view, &changed, &current).unwrap();
        assert_eq!(
            files.apply_recovery_import(
                &host,
                None,
                stale["previewId"].as_str().unwrap(),
                true,
                u64::MAX
            ),
            Err("files_recovery_changed")
        );
        assert_eq!(
            files.recovery_history(&host, None).unwrap()["items"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        history
            .write(&format!("{backup_id}.json"), b"preserved damaged history")
            .unwrap();
        assert_eq!(
            files.preview_recovery_restore(&host, None, &backup_id),
            Err("files_store_changed")
        );
        assert_eq!(
            history.preserve(&format!("{backup_id}.json"), &before),
            Err("files_store_changed")
        );
        let foreign = view.child("other-view").unwrap();
        assert_eq!(
            write_recovery(&foreign, &previous, &revision),
            Err("files_recovery_changed")
        );
    }
}
