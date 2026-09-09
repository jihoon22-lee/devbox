//! Context-owned configuration only. Saving settings performs no project IO,
//! executable resolution, runtime probe or execution approval.
use super::{input, Result};
use crate::core::legacy_lsp::{decode, StoredConfig};
use crate::{
    core::registry::Binding, definitions::digest, host::Host,
    platform::definition_write::DefinitionTarget, private_metadata::MetadataRoot,
};
use code_pad_lib::lsp::LspConfig;
use product_contract::ProjectContext;
use serde::Deserialize;
use serde_json::{json, Value};

const FILE: &str = "config.json";

pub(super) struct Settings {
    pub(super) context: ProjectContext,
    binding: Binding,
    data: MetadataRoot,
    private: MetadataRoot,
    pub(super) original: Option<Vec<u8>>,
}
impl Settings {
    pub(super) fn open(host: &Host, context: &ProjectContext) -> Result<Self> {
        let binding = host.projects()?.binding(context)?;
        let data = MetadataRoot::open(&host.component("files")?)?;
        let private = data
            .child("views")?
            .child(&format!("worktree-{}", context.worktree_id))?
            .child("lsp")?;
        let original = private.read(FILE)?;
        let settings = Self {
            context: context.clone(),
            binding,
            data,
            private,
            original,
        };
        settings.revalidate(host)?;
        Ok(settings)
    }
    pub(super) fn revalidate(&self, host: &Host) -> Result<()> {
        if host.component("files")? != self.data.path()
            || host.projects()?.binding(&self.context)? != self.binding
        {
            return Err("lsp_context_changed");
        }
        self.data.revalidate()?;
        self.private.revalidate()?;
        if self.private.read(FILE)? != self.original {
            return Err("lsp_config_changed");
        }
        Ok(())
    }
    pub(super) fn revision(&self) -> Result<String> {
        Ok(digest(
            &serde_json::to_vec(&(&self.context, &self.binding, &self.original))
                .map_err(|_| "lsp_config_invalid")?,
        ))
    }
    pub(super) fn binding(&self) -> &Binding {
        &self.binding
    }
    pub(super) fn private(&self) -> &MetadataRoot {
        &self.private
    }
    pub(super) fn execution_config(&self) -> Result<LspConfig> {
        let config = StoredConfig::decode(
            self.original
                .as_deref()
                .ok_or("lsp_settings_save_required")?,
        )?
        .config;
        if !config.enabled || config.workspace_root != self.binding.root {
            return Err("lsp_settings_save_required");
        }
        Ok(config)
    }
    pub(super) fn view(&self) -> Result<Value> {
        let loaded = self
            .original
            .as_deref()
            .map(StoredConfig::decode)
            .transpose();
        let issue = loaded.as_ref().err().copied();
        let mut config = loaded.unwrap_or(None).unwrap_or_default().config;
        // A new/rebound context never inherits activation from the former root.
        // Keep the original bytes until an explicit revision-checked save.
        if config.workspace_root != self.binding.root {
            config.workspace_root = self.binding.root.clone();
            config.enabled = false;
        }
        Ok(
            json!({"config":config,"persist_allowed":issue.is_none(),"recoveryAllowed":issue == Some("lsp_config_invalid"),"error":issue.map(|_| "저장된 LSP 설정을 복구해야 합니다"),"nativeRevision":self.revision()?}),
        )
    }
    pub(super) fn save(self, host: &Host, args: Value, deadline: u64) -> Result<Value> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Save {
            config: Value,
            recover_invalid: bool,
            native_revision: String,
        }
        let save: Save = input(args)?;
        if self.revision()? != save.native_revision {
            return Err("lsp_config_changed");
        }
        let bytes = serde_json::to_vec(&save.config).map_err(|_| "lsp_config_invalid")?;
        let config = decode(&bytes)?;
        if config.workspace_root != self.binding.root {
            return Err("lsp_context_changed");
        }
        let invalid = self
            .original
            .as_deref()
            .map(StoredConfig::decode)
            .transpose()
            .err();
        if invalid == Some("lsp_config_future") {
            return Err("lsp_config_future");
        }
        if invalid.is_some() && !save.recover_invalid {
            return Err("lsp_config_recovery_required");
        }
        crate::files_host::current_deadline(deadline)?;
        self.revalidate(host)?;
        let target =
            DefinitionTarget::capture(&self.private.path().join(FILE), self.original.as_deref())?;
        if invalid.is_some() {
            // A content-addressed recovery copy is idempotent across retries.
            let original = self.original.as_deref().ok_or("lsp_config_invalid")?;
            let backup = format!("invalid-config-{}.json", digest(original));
            match self.private.read(&backup)? {
                Some(bytes) if bytes != original => return Err("lsp_config_changed"),
                Some(_) => {}
                None => self.private.write(&backup, original)?,
            }
        }
        let mut stored = self
            .original
            .as_deref()
            .map(StoredConfig::decode)
            .transpose()
            .unwrap_or(None)
            .unwrap_or_default();
        stored.config = config;
        let bytes = stored.encode()?;
        target.write_utf8(&bytes, || {
            crate::files_host::current_deadline(deadline)?;
            self.revalidate(host)
        })?;
        Ok(Value::Null)
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
    fn request(settings: &Settings) -> Value {
        let view = settings.view().unwrap();
        json!({"config":view["config"],"nativeRevision":view["nativeRevision"],"recoverInvalid":false})
    }
    #[test]
    fn settings_are_context_local_and_stale_save_does_not_touch_the_file() {
        let directory = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let host = Host::open(directory.path()).unwrap();
        host.start_empty().unwrap();
        let current = context(&host, root.path());
        let sibling = context(&host, other.path());
        let settings = Settings::open(&host, &current).unwrap();
        let stale = Settings::open(&host, &current).unwrap();
        let path = settings.private.path().join(FILE);
        let mut save = request(&settings);
        // This nonexistent command must not be resolved by load or save.
        save["config"]["server_by_language"] =
            json!({"rust":{"kind":"custom","executable":"uninstalled-language-server","args":[]}});
        // Settings remain accessible when the project is offline/missing.
        let missing_root = root.path().to_path_buf();
        root.close().unwrap();
        settings.save(&host, save.clone(), u64::MAX).unwrap();
        let written = std::fs::read(&path).unwrap();
        assert_eq!(
            stale.save(&host, save, u64::MAX).unwrap_err(),
            "lsp_config_changed"
        );
        assert_eq!(std::fs::read(&path).unwrap(), written);
        assert!(!missing_root.exists());
        assert!(
            Settings::open(&host, &sibling).unwrap().view().unwrap()["config"]
                ["server_by_language"]
                .as_object()
                .unwrap()
                .is_empty()
        );
        let fresh = Settings::open(&host, &current).unwrap();
        let mut foreign = request(&fresh);
        foreign["config"]["workspace_root"] = json!(other.path().to_str().unwrap());
        assert_eq!(
            fresh.save(&host, foreign, u64::MAX).unwrap_err(),
            "lsp_context_changed"
        );
        assert_eq!(std::fs::read(path).unwrap(), written);
    }
    #[test]
    fn explicit_corrupt_recovery_preserves_bytes_and_future_schema_is_never_replaced() {
        let directory = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let host = Host::open(directory.path()).unwrap();
        host.start_empty().unwrap();
        let context = context(&host, root.path());
        let settings = Settings::open(&host, &context).unwrap();
        let invalid = b"\xff\x00broken JSON";
        settings.private.write(FILE, invalid).unwrap();
        let invalid_settings = Settings::open(&host, &context).unwrap();
        let mut save = request(&invalid_settings);
        assert_eq!(
            invalid_settings
                .save(&host, save.clone(), u64::MAX)
                .unwrap_err(),
            "lsp_config_recovery_required"
        );
        let invalid_settings = Settings::open(&host, &context).unwrap();
        save["recoverInvalid"] = json!(true);
        invalid_settings.save(&host, save, u64::MAX).unwrap();
        let recovered = Settings::open(&host, &context).unwrap();
        assert!(decode(recovered.original.as_deref().unwrap()).is_ok());
        assert_eq!(
            recovered
                .private
                .read(&format!("invalid-config-{}.json", digest(invalid)))
                .unwrap()
                .unwrap(),
            invalid
        );

        let future = br#"{"version":99,"unknown":"preserve"}"#;
        recovered.private.write(FILE, future).unwrap();
        let future_settings = Settings::open(&host, &context).unwrap();
        let mut save = request(&future_settings);
        save["recoverInvalid"] = json!(true);
        assert_eq!(
            future_settings.save(&host, save, u64::MAX).unwrap_err(),
            "lsp_config_future"
        );
        assert_eq!(recovered.private.read(FILE).unwrap().unwrap(), future);
    }
    #[test]
    fn unknown_and_future_settings_cannot_be_silently_rewritten() {
        let config = LspConfig {
            workspace_root: "C:/fixture".into(),
            ..Default::default()
        };
        let raw = serde_json::to_value(config).unwrap();
        assert!(decode(&serde_json::to_vec(&raw).unwrap()).is_ok());
        for future in [
            json!({"version":99}),
            {
                let mut v = raw.clone();
                v["unknown"] = json!(true);
                v
            },
            {
                let mut v = raw.clone();
                v["server_by_language"] = json!({"rust":{"kind":"custom","executable":"server","args":[],"unknown":true}});
                v
            },
        ] {
            assert_eq!(
                decode(&serde_json::to_vec(&future).unwrap()).unwrap_err(),
                "lsp_config_future"
            );
        }
        assert_eq!(decode(b"{invalid").unwrap_err(), "lsp_config_invalid");
    }
}
