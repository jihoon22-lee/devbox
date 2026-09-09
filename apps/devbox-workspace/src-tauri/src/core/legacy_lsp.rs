//! Strict legacy LSP configuration and destination receipts. Conversion never
//! carries activation or execution authority into a new context.
use code_pad_lib::lsp::{LspConfig, LSP_CONFIG_SCHEMA_VERSION};
use serde::{Deserialize, Serialize};
use serde_json::Value;
type Result<T> = std::result::Result<T, &'static str>;
const MAX_BYTES: usize = 64 * 1024;
const MAX_STORE_BYTES: usize = 128 * 1024;

// Ignore no unknown field, including nested runtime/server metadata. Missing
// optional fields may still take the legacy schema's documented defaults.
fn known_shape(raw: &Value, normalized: &Value) -> bool {
    match (raw, normalized) {
        (Value::Object(raw), Value::Object(normalized)) => raw.iter().all(|(key, value)| {
            normalized
                .get(key)
                .is_some_and(|expected| known_shape(value, expected))
        }),
        (Value::Array(raw), Value::Array(normalized)) => {
            raw.len() == normalized.len()
                && raw
                    .iter()
                    .zip(normalized)
                    .all(|(value, expected)| known_shape(value, expected))
        }
        _ => raw == normalized,
    }
}
pub fn decode(bytes: &[u8]) -> Result<LspConfig> {
    if bytes.len() > MAX_BYTES {
        return Err("lsp_config_invalid");
    }
    let raw: Value = serde_json::from_slice(bytes).map_err(|_| "lsp_config_invalid")?;
    if raw
        .get("version")
        .and_then(Value::as_u64)
        .is_some_and(|v| v != u64::from(LSP_CONFIG_SCHEMA_VERSION))
    {
        return Err("lsp_config_future");
    }
    let config: LspConfig =
        serde_json::from_value(raw.clone()).map_err(|_| "lsp_config_invalid")?;
    config.validate().map_err(|_| "lsp_config_invalid")?;
    let normalized = serde_json::to_value(&config).map_err(|_| "lsp_config_invalid")?;
    if !known_shape(&raw, &normalized) {
        return Err("lsp_config_future");
    }
    Ok(config)
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StoredConfig {
    schema_version: u32,
    pub config: LspConfig,
    pub imports: Vec<String>,
}
impl Default for StoredConfig {
    fn default() -> Self {
        Self {
            schema_version: 1,
            config: LspConfig::default(),
            imports: vec![],
        }
    }
}
impl StoredConfig {
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_STORE_BYTES {
            return Err("lsp_config_invalid");
        }
        let raw: Value = serde_json::from_slice(bytes).map_err(|_| "lsp_config_invalid")?;
        if raw.get("schemaVersion").is_none() {
            return Ok(Self {
                config: decode(bytes)?,
                ..Self::default()
            });
        }
        if raw.get("schemaVersion").and_then(Value::as_u64) != Some(1)
            || raw.as_object().is_none_or(|object| {
                object
                    .keys()
                    .any(|key| !["schemaVersion", "config", "imports"].contains(&key.as_str()))
            })
        {
            return Err("lsp_config_future");
        }
        let config = decode(
            &serde_json::to_vec(raw.get("config").ok_or("lsp_config_invalid")?)
                .map_err(|_| "lsp_config_invalid")?,
        )?;
        let mut record: Self = serde_json::from_value(raw).map_err(|_| "lsp_config_invalid")?;
        record.config = config;
        record.validate()?;
        Ok(record)
    }
    fn validate(&self) -> Result<()> {
        if self.schema_version != 1
            || self.imports.len() > 32
            || self.imports.iter().any(|id| !history_id(id))
            || self
                .imports
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != self.imports.len()
        {
            return Err("lsp_config_invalid");
        }
        decode(&serde_json::to_vec(&self.config).map_err(|_| "lsp_config_invalid")?)?;
        Ok(())
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let bytes = if self.imports.is_empty() {
            serde_json::to_vec(&self.config)
        } else {
            serde_json::to_vec(self)
        }
        .map_err(|_| "lsp_config_invalid")?;
        if bytes.len() > MAX_STORE_BYTES {
            return Err("lsp_config_invalid");
        }
        Ok(bytes)
    }
    pub fn import(&self, snapshot_id: &str, config: LspConfig) -> Result<Self> {
        if self.imports.iter().any(|id| id == snapshot_id) {
            return Ok(self.clone());
        }
        if self.imports.len() >= 32 {
            return Err("legacy_lsp_limit");
        }
        let mut next = self.clone();
        next.config = config;
        next.imports.push(snapshot_id.into());
        next.validate()?;
        Ok(next)
    }
}
pub fn history_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub fn retarget(mut config: LspConfig, root: &str) -> Result<LspConfig> {
    config.workspace_root = root.into();
    config.enabled = false;
    decode(&serde_json::to_vec(&config).map_err(|_| "lsp_config_invalid")?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conversion_disables_execution_and_receipts_survive_normal_changes() {
        let mut raw = serde_json::to_value(LspConfig::default()).unwrap();
        raw["enabled"] = serde_json::json!(true);
        raw["workspace_root"] = serde_json::json!("C:/old");
        raw["server_by_language"] =
            serde_json::json!({"rust":{"kind":"custom","executable":"missing-server","args":[]}});
        let config = retarget(
            decode(&serde_json::to_vec(&raw).unwrap()).unwrap(),
            "C:/new",
        )
        .unwrap();
        assert!(!config.enabled);
        assert_eq!(config.workspace_root, "C:/new");
        assert_eq!(config.server_by_language.len(), 1);
        let id = "a".repeat(64);
        let mut stored = StoredConfig::default().import(&id, config.clone()).unwrap();
        stored.config.server_by_language.clear();
        let reloaded = StoredConfig::decode(&stored.encode().unwrap()).unwrap();
        assert!(reloaded
            .import(&id, config)
            .unwrap()
            .config
            .server_by_language
            .is_empty());
        let mut future = serde_json::to_value(&reloaded).unwrap();
        future["unknown"] = serde_json::json!(true);
        assert!(matches!(
            StoredConfig::decode(&serde_json::to_vec(&future).unwrap()),
            Err("lsp_config_future")
        ));
    }
}
