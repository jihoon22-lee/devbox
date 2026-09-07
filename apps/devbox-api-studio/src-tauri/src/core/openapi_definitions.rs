//! Explicitly saved OpenAPI operation projections. Original documents and
//! unsupported schema semantics remain with their source, never this store.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};
const INVALID: &str = "openapi_definition_invalid";
const STORAGE: &str = "openapi_definition_unavailable";
const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_DEFINITIONS: usize = 32;
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Operation {
    pub label: String,
    pub method: String,
    pub request_target: String,
    pub mock_status: Option<u16>,
    pub request: Value,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Definition {
    pub schema_version: u8,
    pub id: String,
    pub name: String,
    pub open_api_version: String,
    pub operations: Vec<Operation>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Create {
    pub name: String,
    pub open_api_version: String,
    pub operations: Vec<Operation>,
    pub environment: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub id: String,
    pub name: String,
    pub operation_count: usize,
}
fn uuid_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|value| value.to_string() == id)
}
fn label(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 480 && !value.chars().any(char::is_control)
}
impl Definition {
    fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || !uuid_id(&self.id)
            || !label(&self.name)
            || !matches!(self.open_api_version.as_str(), "3.0" | "3.1")
            || self.operations.is_empty()
            || self.operations.len() > 1000
        {
            return Err(INVALID.into());
        }
        for operation in &self.operations {
            if !label(&operation.label)
                || operation.request_target.len() > 4096
                || !operation.request_target.starts_with('/')
                || operation.request_target.chars().any(char::is_control)
                || operation.method != operation.request["method"].as_str().unwrap_or_default()
                || operation
                    .mock_status
                    .is_some_and(|status| !(100..=599).contains(&status))
                || api_playground_lib::component::normalize_openapi_request(
                    operation.request.clone(),
                )? != operation.request
            {
                return Err(INVALID.into());
            }
        }
        Ok(())
    }
    pub fn prepare(input: Create) -> Result<Self, String> {
        let mut definition = Self {
            schema_version: 1,
            id: uuid::Uuid::new_v4().to_string(),
            name: input.name.trim().into(),
            open_api_version: input.open_api_version,
            operations: input.operations,
        };
        for operation in &mut definition.operations {
            operation.request =
                api_playground_lib::component::sanitize_openapi_request(operation.request.clone())?;
        }
        definition.validate()?;
        let raw = zeroize::Zeroizing::new(serde_json::to_string(&definition).map_err(|_| INVALID)?);
        if raw.len() > MAX_BYTES {
            return Err(INVALID.into());
        }
        // Same native DPAPI/persistence sanitizer as Collections, including
        // supplied current environment secret references, never saved themselves.
        let safe = api_playground_lib::component::sanitize_legacy_json(
            raw.to_string(),
            &input.environment,
        )
        .map_err(|_| INVALID)?;
        let definition: Self = serde_json::from_str(&safe).map_err(|_| INVALID)?;
        definition.validate()?;
        Ok(definition)
    }
}
pub struct Store {
    directory: PathBuf,
    _lock: fs::File,
}
impl Store {
    pub fn open(root: &Path) -> Result<Self, String> {
        let directory = root.join("api/openapi/v1");
        super::api_workspace::ensure_directory(&directory).map_err(|_| STORAGE)?;
        let path = directory.join(".lock");
        if fs::symlink_metadata(&path).is_ok() {
            devbox_filesystem::ensure_no_links(&path).map_err(|_| STORAGE)?;
        }
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|_| STORAGE)?;
        if !devbox_filesystem::try_lock_exclusive(&lock).map_err(|_| STORAGE)? {
            return Err(STORAGE.into());
        }
        Ok(Self {
            directory,
            _lock: lock,
        })
    }
    fn path(&self, id: &str) -> Result<PathBuf, String> {
        if !uuid_id(id) {
            return Err(INVALID.into());
        }
        devbox_filesystem::ensure_no_links(&self.directory).map_err(|_| STORAGE)?;
        Ok(self.directory.join(format!("{id}.json")))
    }
    pub fn get(&self, id: &str) -> Result<Definition, String> {
        let raw = super::import_repository::read_file(&self.path(id)?, MAX_BYTES)
            .map_err(|_| STORAGE)?
            .ok_or(INVALID)?;
        let definition: Definition = serde_json::from_str(&raw).map_err(|_| INVALID)?;
        definition.validate()?;
        if definition.id != id {
            return Err(INVALID.into());
        }
        Ok(definition)
    }
    pub fn list(&self) -> Result<Vec<Summary>, String> {
        let mut result = vec![];
        for (index, entry) in fs::read_dir(&self.directory)
            .map_err(|_| STORAGE)?
            .enumerate()
        {
            if index > MAX_DEFINITIONS + 8 {
                return Err(STORAGE.into());
            }
            let entry = entry.map_err(|_| STORAGE)?;
            let name = entry.file_name().into_string().map_err(|_| STORAGE)?;
            if name == ".lock" {
                continue;
            }
            let id = name.strip_suffix(".json").ok_or(STORAGE)?;
            if result.len() >= MAX_DEFINITIONS {
                return Err("openapi_definition_full".into());
            }
            let definition = self.get(id)?;
            result.push(Summary {
                id: definition.id,
                name: definition.name,
                operation_count: definition.operations.len(),
            });
        }
        result.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        Ok(result)
    }
    pub fn save(&self, definition: &Definition) -> Result<(), String> {
        if self.list()?.len() >= MAX_DEFINITIONS {
            return Err("openapi_definition_full".into());
        }
        definition.validate()?;
        let path = self.path(&definition.id)?;
        match fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err(INVALID.into()),
        }
        let bytes = serde_json::to_vec(definition).map_err(|_| INVALID)?;
        if bytes.len() > MAX_BYTES {
            return Err(INVALID.into());
        }
        devbox_filesystem::atomic_write(&path, &bytes).map_err(|_| STORAGE.into())
    }
    pub fn delete(&self, id: &str) -> Result<(), String> {
        self.get(id)?;
        fs::remove_file(self.path(id)?).map_err(|_| STORAGE.into())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> Create {
        serde_json::from_value(serde_json::json!({"name":"Fixture", "openApiVersion":"3.1", "environment":"{\"version\":1,\"environments\":[]}", "operations":[{"label":"GET /fixture","method":"GET","requestTarget":"/fixture","mockStatus":200,"request":{"method":"GET","url":"http://127.0.0.1/fixture","headers":[{"key":"Authorization","value":"Bearer synthetic-secret","enabled":true}],"params":[],"body_kind":"none","body":"","auth":null,"timeout_ms":30000}}]})).unwrap()
    }
    #[test]
    fn saved_projection_uses_native_redaction_and_reopens_without_original_source() {
        let root = tempfile::tempdir().unwrap();
        let mut input = input();
        input.operations[0].request["headers"] = serde_json::json!([
            {"key":"X-Repeat","value":"one","enabled":true},
            {"key":"X-Repeat","value":"two","enabled":false},
            {"key":"Authorization","value":"Bearer synthetic-secret","enabled":false},
            {"key":"Authorization","value":"${TOKEN}","enabled":true}
        ]);
        input.operations[0].request["body_kind"] = "json".into();
        input.operations[0].request["body"] = r#"{"echo":"synthetic-secret"}"#.into();
        let definition = Definition::prepare(input).unwrap();
        assert_eq!(
            definition.operations[0].request["headers"][0]["value"],
            "one"
        );
        assert_eq!(
            definition.operations[0].request["headers"][1]["enabled"],
            false
        );
        assert_eq!(
            definition.operations[0].request["headers"][3]["value"],
            "${TOKEN}"
        );
        let raw = serde_json::to_string(&definition).unwrap();
        assert!(!raw.contains("synthetic-secret"));
        assert!(raw.contains("REDACTED"));
        assert_eq!(
            definition.operations[0].request["requiresSecretReview"],
            true
        );
        {
            let store = Store::open(root.path()).unwrap();
            store.save(&definition).unwrap();
            assert!(store.save(&definition).is_err());
        }
        let store = Store::open(root.path()).unwrap();
        assert_eq!(store.get(&definition.id).unwrap(), definition);
        assert_eq!(store.list().unwrap().len(), 1);
        store.delete(&definition.id).unwrap();
        assert!(store.list().unwrap().is_empty());
    }
    #[test]
    fn stored_future_or_corrupt_definition_is_preserved_and_never_treated_as_empty() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let definition = Definition::prepare(input()).unwrap();
        store.save(&definition).unwrap();
        let path = store.path(&definition.id).unwrap();
        for raw in ["{broken", "{\"schemaVersion\":99}"] {
            fs::write(&path, raw).unwrap();
            assert!(store.list().is_err());
            assert!(store.delete(&definition.id).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), raw);
        }
        assert!(store.get("../outside").is_err());
    }
}
