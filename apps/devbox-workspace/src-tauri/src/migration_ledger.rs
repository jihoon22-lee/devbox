//! Bounded summaries of retained owner mapping records. Counts describe ledger
//! entries, not total migrated user data or complete legacy-source coverage.
use crate::{host::Host, private_metadata::MetadataRoot};
use product_contract::migration_status::MappingSummary;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    time::{Duration, Instant},
};
type Result<T> = std::result::Result<T, &'static str>;
struct Ledger {
    hash: Sha256,
    count: u64,
    bytes: usize,
    started: Instant,
}
impl Ledger {
    fn new() -> Self {
        Self {
            hash: Sha256::new(),
            count: 0,
            bytes: 0,
            started: Instant::now(),
        }
    }
    fn check(&self) -> Result<()> {
        if self.bytes > 32 * 1024 * 1024
            || self.count > 1_000_000
            || self.started.elapsed() > Duration::from_secs(5)
        {
            return Err("workspace_mapping_limit");
        }
        Ok(())
    }
    fn add(&mut self, scope: &str, count: u64, value: Value) -> Result<()> {
        let bytes = serde_json::to_vec(&(scope, value)).map_err(|_| "workspace_mapping_invalid")?;
        self.bytes += bytes.len();
        self.count += count;
        self.check()?;
        self.hash.update(bytes);
        Ok(())
    }
    fn read(&mut self, root: &MetadataRoot, name: &str) -> Result<Option<Vec<u8>>> {
        self.check()?;
        let bytes = root.read(name)?;
        self.bytes += bytes.as_ref().map_or(0, Vec::len);
        self.check()?;
        Ok(bytes)
    }
}
fn optional(path: &Path) -> Result<Option<MetadataRoot>> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("workspace_mapping_unavailable"),
        Ok(_) => MetadataRoot::open(path).map(Some),
    }
}
pub(crate) fn summarize(host: &Host) -> Result<MappingSummary> {
    Ok(summarize_with_sources(host)?.0)
}
pub(crate) fn summarize_with_sources(
    host: &Host,
) -> Result<(MappingSummary, std::collections::BTreeSet<String>)> {
    let mut sources = std::collections::BTreeSet::new();
    let registry = host.projects()?.snapshot()?;
    let mut ledger = Ledger::new();
    ledger.add(
        "registry-references",
        registry.legacy_references.len() as u64,
        json!(registry.legacy_references),
    )?;
    sources.extend(
        registry
            .imported_templates
            .iter()
            .filter_map(|row| row.source_snapshot_id.clone()),
    );
    sources.extend(
        registry
            .imported_profiles
            .iter()
            .filter_map(|row| row.source_snapshot_id.clone()),
    );
    let templates = registry
        .imported_templates
        .iter()
        .filter(|row| row.source_snapshot_id.is_some())
        .map(|row| json!([row.source_snapshot_id, row.template.id, row.id]))
        .collect::<Vec<_>>();
    ledger.add(
        "registry-templates",
        templates.len() as u64,
        json!(templates),
    )?;
    let profiles = registry
        .imported_profiles
        .iter()
        .filter(|row| row.source_snapshot_id.is_some())
        .map(|row| json!([row.source_snapshot_id, row.profile.id, row.id]))
        .collect::<Vec<_>>();
    ledger.add("registry-profiles", profiles.len() as u64, json!(profiles))?;
    ledger.add(
        "registry-profile-bindings",
        registry.imported_profile_bindings.len() as u64,
        json!(registry.imported_profile_bindings),
    )?;
    let (count, revision) =
        run_manager_lib::component::migration_mapping_summary(&host.component("runtime")?)
            .map_err(|_| "workspace_mapping_unavailable")?;
    ledger.add("runtime", count, json!(revision))?;
    let processes =
        port_manager_lib::component::product_preferences::load(&host.component("processes")?)?;
    sources.extend(processes.imported_snapshot.clone());
    ledger.add(
        "process-preferences",
        u64::from(processes.imported_snapshot.is_some()),
        json!(processes.imported_snapshot),
    )?;
    let logs = log_lens_lib::core::product_saved_views::load(&host.component("logs")?)?;
    sources.extend(logs.imported_snapshot.clone());
    ledger.add(
        "saved-views",
        u64::from(logs.imported_snapshot.is_some()),
        json!(logs.imported_snapshot),
    )?;
    let terminal = MetadataRoot::open(&host.component("terminal")?)?;
    let (count, records) = crate::terminal_profiles::mapping_records(&terminal)?;
    ledger.add("terminal", count, records)?;
    let files = host.component("files")?;
    if let Some(views) = optional(&files.join("views"))? {
        let mut names = Vec::new();
        for entry in std::fs::read_dir(views.path()).map_err(|_| "workspace_mapping_unavailable")? {
            ledger.check()?;
            let entry = entry.map_err(|_| "workspace_mapping_unavailable")?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| "workspace_mapping_invalid")?;
            if name != "single-file"
                && !name
                    .strip_prefix("worktree-")
                    .is_some_and(product_contract::commands::opaque_id)
            {
                return Err("workspace_mapping_invalid");
            }
            names.push(name);
            if names.len() > 1024 {
                return Err("workspace_mapping_limit");
            }
        }
        names.sort();
        for name in names {
            let view = MetadataRoot::open(&views.path().join(&name))?;
            if let Some(bytes) = ledger.read(&view, "session.json")? {
                let saved = crate::core::editor_sessions::StoredSession::decode(&bytes)?;
                sources.extend(saved.imports.iter().map(|row| row.snapshot_id.clone()));
                let count = saved
                    .imports
                    .iter()
                    .map(|receipt| {
                        receipt.document_ids.len() as u64
                            + receipt.recent_files.len() as u64
                            + u64::from(receipt.source_workspace.is_some())
                    })
                    .sum();
                ledger.add(&format!("{name}/session"), count, json!(saved.imports))?;
            }
            if let Some(bytes) = ledger.read(&view, "recovery.json")? {
                let saved = crate::core::legacy_recovery::StoredRecovery::decode(&bytes)?;
                sources.extend(saved.imports.iter().map(|row| row.snapshot_id.clone()));
                let count = saved
                    .imports
                    .iter()
                    .map(|receipt| receipt.paths.len() as u64)
                    .sum();
                ledger.add(&format!("{name}/recovery"), count, json!(saved.imports))?;
            }
            if let Some(lsp) = optional(&view.path().join("lsp"))? {
                if let Some(bytes) = ledger.read(&lsp, "config.json")? {
                    let saved = crate::lsp_host::config::StoredConfig::decode(&bytes)?;
                    sources.extend(saved.imports.iter().cloned());
                    ledger.add(
                        &format!("{name}/lsp"),
                        saved.imports.len() as u64,
                        json!(saved.imports),
                    )?;
                }
            }
            view.revalidate()?;
        }
        views.revalidate()?;
    }
    if host.projects()?.snapshot()? != registry {
        return Err("workspace_mapping_changed");
    }
    ledger.check()?;
    let summary = MappingSummary::new(
        ledger.count,
        ledger
            .hash
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
    )?;
    Ok((summary, sources))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_ledger_is_read_only_and_corruption_never_becomes_zero_imports() {
        let root = tempfile::tempdir().unwrap();
        let host = Host::open(root.path()).unwrap();
        host.start_empty().unwrap();
        let empty = summarize(&host).unwrap();
        assert_eq!(empty.record_count, 0);
        let views = host.component("files").unwrap().join("views");
        assert!(!views.exists());
        assert!(!host.component("runtime").unwrap().join("data.db").exists());
        let view = views.join("single-file");
        std::fs::create_dir_all(&view).unwrap();
        let mut saved = crate::core::editor_sessions::StoredSession::default();
        saved.imports.push(crate::core::editor_sessions::Receipt {
            snapshot_id: "a".repeat(64),
            document_ids: vec!["retained-source-document".into()],
            recent_files: vec![],
            source_workspace: None,
        });
        std::fs::write(view.join("session.json"), saved.encode().unwrap()).unwrap();
        let imported = summarize(&host).unwrap();
        assert_eq!(imported.record_count, 1);
        assert_ne!(imported.revision, empty.revision);
        assert_eq!(imported, summarize(&host).unwrap());
        std::fs::write(view.join("session.json"), b"{\"schemaVersion\":999}").unwrap();
        assert!(summarize(&host).is_err());
    }
}
