//! Metadata-only, reviewed configuration replacement. The component context
//! write permit serializes apply with normal settings saves and actor starts.
use super::{settings::Settings, Result};
use crate::{
    core::legacy_lsp::{self, StoredConfig},
    definitions::digest,
    host::Host,
    platform::definition_write::DefinitionTarget,
    private_metadata::MetadataRoot,
};
use code_pad_lib::lsp::LspConfig;
use product_contract::ProjectContext;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

struct Preview {
    settings: Settings,
    source_id: String,
    config: LspConfig,
    restore: Option<StoredConfig>,
    created: Instant,
}
#[derive(Default)]
pub(super) struct Imports {
    previews: HashMap<String, Preview>,
}
fn stored(settings: &Settings) -> Result<StoredConfig> {
    settings
        .original
        .as_deref()
        .map(StoredConfig::decode)
        .transpose()
        .map(|value| value.unwrap_or_default())
}
fn read_history(history: &MetadataRoot, id: &str) -> Result<StoredConfig> {
    if !legacy_lsp::history_id(id) {
        return Err("invalid_request");
    }
    let bytes = history
        .read(&format!("{id}.json"))?
        .ok_or("files_store_unavailable")?;
    if digest(&bytes) != id {
        return Err("files_store_changed");
    }
    StoredConfig::decode(&bytes)
}
impl Imports {
    pub(super) fn expire(&mut self) {
        self.previews
            .retain(|_, preview| preview.created.elapsed() < Duration::from_secs(180));
    }
    pub(super) fn preview(
        &mut self,
        host: &Host,
        context: &ProjectContext,
        job_id: &str,
    ) -> Result<Value> {
        let (id, config) = host.legacy.lsp_source(job_id)?;
        self.insert(host, context, id, config, None)
    }
    pub(super) fn restore(
        &mut self,
        host: &Host,
        context: &ProjectContext,
        id: &str,
    ) -> Result<Value> {
        let settings = Settings::open(host, context)?;
        let history = settings.private().child("config-history")?;
        let stored = read_history(&history, id)?;
        self.insert(
            host,
            context,
            id.into(),
            stored.config.clone(),
            Some(stored),
        )
    }
    fn insert(
        &mut self,
        host: &Host,
        context: &ProjectContext,
        source_id: String,
        config: LspConfig,
        restore: Option<StoredConfig>,
    ) -> Result<Value> {
        self.expire();
        if self.previews.len() >= 4 {
            return Err("legacy_lsp_review_limit");
        }
        let settings = Settings::open(host, context)?;
        let current = stored(&settings)?;
        let config = legacy_lsp::retarget(config, &settings.binding().root)?;
        let id = uuid::Uuid::new_v4().to_string();
        let result = json!({"previewId":id,"config":config,"currentConfig":current.config,"conflict":settings.original.is_some(),"alreadyImported":restore.is_none() && current.imports.contains(&source_id),"restoring":restore.is_some()});
        self.previews.insert(
            id,
            Preview {
                settings,
                source_id,
                config,
                restore,
                created: Instant::now(),
            },
        );
        Ok(result)
    }
    pub(super) fn cancel(&mut self, context: &ProjectContext, id: &str) -> Result<Value> {
        // Tokens are single-use, including a wrong-context attempt.
        let preview = self.previews.remove(id).ok_or("legacy_lsp_review_stale")?;
        if &preview.settings.context != context {
            return Err("legacy_lsp_review_stale");
        }
        Ok(Value::Null)
    }
    pub(super) fn history(host: &Host, context: &ProjectContext) -> Result<Value> {
        let settings = Settings::open(host, context)?;
        let history = settings.private().child("config-history")?;
        let mut items = vec![];
        let mut unrecognized = 0;
        for entry in std::fs::read_dir(history.path())
            .map_err(|_| "files_store_unavailable")?
            .take(33)
        {
            let entry = entry.map_err(|_| "files_store_unavailable")?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(id) = name
                .strip_suffix(".json")
                .filter(|id| legacy_lsp::history_id(id))
            else {
                unrecognized += 1;
                continue;
            };
            match read_history(&history, id) {
                Ok(stored) => items.push(json!({"id":id,"languages":stored.config.server_by_language.len(),"issue":null})),
                Err(issue) => items.push(json!({"id":id,"languages":null,"issue":issue})),
            }
        }
        items.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        history.revalidate()?;
        settings.revalidate(host)?;
        Ok(json!({"items":items,"unrecognized":unrecognized}))
    }
    pub(super) fn apply(
        &mut self,
        host: &Host,
        context: &ProjectContext,
        id: &str,
        replace_existing: bool,
        deadline: u64,
    ) -> Result<Value> {
        let preview = self.previews.remove(id).ok_or("legacy_lsp_review_stale")?;
        if preview.created.elapsed() >= Duration::from_secs(180)
            || &preview.settings.context != context
        {
            return Err("legacy_lsp_review_stale");
        }
        let settings = preview.settings;
        crate::files_host::current_deadline(deadline)?;
        settings.revalidate(host)?;
        let current = stored(&settings)?;
        if preview.restore.is_none() && current.imports.contains(&preview.source_id) {
            return Ok(json!({"reused":true,"restored":false}));
        }
        if settings.original.is_some() && !replace_existing {
            return Err("legacy_lsp_conflict");
        }
        let restoring = preview.restore.is_some();
        let next = if let Some(mut restored) = preview.restore {
            restored.config = preview.config;
            restored
        } else {
            current.import(&preview.source_id, preview.config)?
        };
        let bytes = next.encode()?;
        let target = DefinitionTarget::capture(
            &settings.private().path().join("config.json"),
            settings.original.as_deref(),
        )?;
        let history = settings.private().child("config-history")?;
        let backup = settings
            .original
            .as_ref()
            .map(|before| format!("{}.json", digest(before)));
        if let (Some(before), Some(name)) = (&settings.original, &backup) {
            if history.read(name)?.is_none()
                && std::fs::read_dir(history.path())
                    .map_err(|_| "files_store_unavailable")?
                    .take(32)
                    .count()
                    >= 32
            {
                return Err("legacy_lsp_limit");
            }
            history.preserve(name, before)?;
        }
        target.write_utf8(&bytes, || {
            crate::files_host::current_deadline(deadline)?;
            settings.revalidate(host)?;
            if let Some(name) = &backup {
                if history.read(name)? != settings.original {
                    return Err("files_store_changed");
                }
            }
            Ok(())
        })?;
        Ok(json!({"reused":false,"restored":restoring}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn context(host: &Host, root: &std::path::Path) -> ProjectContext {
        let owner = host.projects().unwrap();
        let preview = owner.preview_fixture(root).unwrap();
        owner
            .apply(
                &preview.preview_id,
                "fixture",
                crate::project_owner::RegistrationAction::Register,
            )
            .unwrap()
            .1
    }
    fn id(preview: &Value) -> &str {
        preview["previewId"].as_str().unwrap()
    }
    #[test]
    fn verified_source_loss_repeat_stale_save_and_reviewed_restore_preserve_metadata() {
        let base = tempfile::tempdir().unwrap();
        let data = base.path().join("workspace");
        std::fs::create_dir(&data).unwrap();
        let root = tempfile::tempdir().unwrap();
        let host = Host::open(&data).unwrap();
        host.start_empty().unwrap();
        let context = context(&host, root.path());
        let source = base
            .path()
            .join(crate::core::legacy_inventory::Source::CodePad.identifier())
            .join("lsp");
        std::fs::create_dir_all(&source).unwrap();
        let config = LspConfig {
            enabled: true,
            workspace_root: "C:/former".into(),
            ..Default::default()
        };
        std::fs::write(
            source.join("config.json"),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        let job = json!(host
            .legacy
            .start(crate::core::legacy_inventory::Source::CodePad)
            .unwrap());
        let start = Instant::now();
        while json!(host.legacy.status().unwrap())["phase"] != "ready" {
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(5));
        }
        std::fs::remove_file(source.join("config.json")).unwrap();
        let job_id = job["id"].as_str().unwrap();
        let mut imports = Imports::default();
        let initial = Settings::open(&host, &context).unwrap();
        let mut view = initial.view().unwrap();
        view["config"]["enabled"] = json!(true);
        let save = json!({"config":view["config"],"nativeRevision":view["nativeRevision"],"recoverInvalid":false});
        initial.save(&host, save, u64::MAX).unwrap();
        let before = Settings::open(&host, &context).unwrap();
        let original = before.original.clone().unwrap();
        let old_view = before.view().unwrap();
        let preview = imports.preview(&host, &context, job_id).unwrap();
        assert_eq!(preview["config"]["enabled"], false);
        assert_eq!(
            preview["config"]["workspace_root"],
            context_root(&host, &context)
        );
        assert_eq!(
            imports.apply(&host, &context, id(&preview), false, u64::MAX),
            Err("legacy_lsp_conflict")
        );
        assert_eq!(
            imports.apply(&host, &context, id(&preview), true, u64::MAX),
            Err("legacy_lsp_review_stale")
        );
        let preview = imports.preview(&host, &context, job_id).unwrap();
        imports
            .apply(&host, &context, id(&preview), true, u64::MAX)
            .unwrap();
        assert_eq!(before.save(&host, json!({"config":old_view["config"],"nativeRevision":old_view["nativeRevision"],"recoverInvalid":false}), u64::MAX), Err("lsp_config_changed"));
        let settings = Settings::open(&host, &context).unwrap();
        assert!(matches!(
            settings.execution_config(),
            Err("lsp_settings_save_required")
        ));
        let backup_id = digest(&original);
        assert_eq!(
            settings
                .private()
                .child("config-history")
                .unwrap()
                .read(&format!("{backup_id}.json"))
                .unwrap()
                .unwrap(),
            original
        );
        // A normal save must retain the receipt even if the server preference changes.
        let mut view = settings.view().unwrap();
        view["config"]["server_by_language"] =
            json!({"rust":{"kind":"custom","executable":"never-run","args":[]}});
        settings.save(&host, json!({"config":view["config"],"nativeRevision":view["nativeRevision"],"recoverInvalid":false}), u64::MAX).unwrap();
        let changed = Settings::open(&host, &context).unwrap().original.unwrap();
        let repeated = imports.preview(&host, &context, job_id).unwrap();
        assert_eq!(repeated["alreadyImported"], true);
        assert_eq!(
            imports
                .apply(&host, &context, id(&repeated), false, u64::MAX)
                .unwrap()["reused"],
            true
        );
        assert_eq!(
            Settings::open(&host, &context).unwrap().original.unwrap(),
            changed
        );
        assert_eq!(
            Imports::history(&host, &context).unwrap()["items"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let restored = imports.restore(&host, &context, &backup_id).unwrap();
        imports
            .apply(&host, &context, id(&restored), true, u64::MAX)
            .unwrap();
        assert!(stored(&Settings::open(&host, &context).unwrap())
            .unwrap()
            .config
            .server_by_language
            .is_empty());
        assert!(
            !stored(&Settings::open(&host, &context).unwrap())
                .unwrap()
                .config
                .enabled
        );
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
        let stale = imports.preview(&host, &context, job_id).unwrap();
        let settings = Settings::open(&host, &context).unwrap();
        settings
            .private()
            .write("config.json", b"corrupt future evidence")
            .unwrap();
        assert_eq!(
            imports.apply(&host, &context, id(&stale), true, u64::MAX),
            Err("lsp_config_changed")
        );
        assert!(imports.preview(&host, &context, job_id).is_err());
        assert_eq!(
            settings.private().read("config.json").unwrap().unwrap(),
            b"corrupt future evidence"
        );
        let history = settings.private().child("config-history").unwrap();
        history
            .write(&format!("{backup_id}.json"), b"tampered")
            .unwrap();
        assert_eq!(
            imports.restore(&host, &context, &backup_id),
            Err("files_store_changed")
        );
        assert_eq!(
            history.preserve(&format!("{backup_id}.json"), &original),
            Err("files_store_changed")
        );
    }
    #[test]
    fn offline_metadata_review_expires_and_rejects_foreign_contexts_without_writes() {
        let data = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let host = Host::open(data.path()).unwrap();
        host.start_empty().unwrap();
        let context = context(&host, root.path());
        let config = LspConfig::default();
        root.close().unwrap();
        let mut imports = Imports::default();
        let preview = imports
            .insert(&host, &context, "a".repeat(64), config.clone(), None)
            .unwrap();
        let mut foreign = context.clone();
        foreign.revision += 1;
        assert_eq!(
            imports.apply(&host, &foreign, id(&preview), true, u64::MAX),
            Err("legacy_lsp_review_stale")
        );
        assert_eq!(
            imports.apply(&host, &context, id(&preview), true, u64::MAX),
            Err("legacy_lsp_review_stale")
        );
        let preview = imports
            .insert(&host, &context, "a".repeat(64), config.clone(), None)
            .unwrap();
        imports.previews.get_mut(id(&preview)).unwrap().created =
            Instant::now() - Duration::from_secs(181);
        assert_eq!(
            imports.apply(&host, &context, id(&preview), true, u64::MAX),
            Err("legacy_lsp_review_stale")
        );
        for _ in 0..4 {
            imports
                .insert(&host, &context, "a".repeat(64), config.clone(), None)
                .unwrap();
        }
        assert_eq!(
            imports.insert(&host, &context, "a".repeat(64), config, None),
            Err("legacy_lsp_review_limit")
        );
        assert!(Settings::open(&host, &context).unwrap().original.is_none());
    }
    fn context_root(host: &Host, context: &ProjectContext) -> String {
        host.projects().unwrap().binding(context).unwrap().root
    }
}
