//! Producer-owned, explicitly saved masked results for an unavailable Knowledge
//! receiver. These authoritative drafts do not expire with AppLink leases.
use product_contract::{
    references::{OwnedArtifactKind, OwnedArtifactReference},
    Provenance,
};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_DRAFTS: usize = 50;
const MAX_FILE_BYTES: usize = 600 * 1024;
const ERROR: &str = "knowledge_storage_unavailable";
pub use product_contract::knowledge_draft::{Draft, Summary};
fn io<T>(value: std::io::Result<T>) -> Result<T, String> {
    value.map_err(|_| ERROR.into())
}
fn ensure_directory(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => io(devbox_filesystem::ensure_no_links(path)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            ensure_directory(path.parent().ok_or(ERROR)?)?;
            match fs::create_dir(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if !io(fs::symlink_metadata(path))?.is_dir() {
                        return Err(ERROR.into());
                    }
                }
                Err(_) => return Err(ERROR.into()),
            }
            io(devbox_filesystem::ensure_no_links(path))
        }
        _ => Err(ERROR.into()),
    }
}
fn valid_id(id: &str) -> bool {
    id.len() == 36 && uuid::Uuid::parse_str(id).is_ok_and(|value| value.to_string() == id)
}
fn owner(component: &str) -> Result<&'static str, String> {
    match component {
        "api-studio.api" => Ok("api"),
        "api-studio.transforms" => Ok("transforms"),
        _ => Err("knowledge_owner_invalid".into()),
    }
}
pub struct Store {
    directory: PathBuf,
    component: String,
    _lock: fs::File,
}
impl Store {
    pub fn open(root: &Path, component: &str) -> Result<Self, String> {
        let directory = root.join("drafts/knowledge/v1").join(owner(component)?);
        ensure_directory(&directory)?;
        let lock_path = directory.join(".lock");
        if fs::symlink_metadata(&lock_path).is_ok() {
            io(devbox_filesystem::ensure_no_links(&lock_path))?;
        }
        let lock = io(fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path))?;
        if !io(devbox_filesystem::try_lock_exclusive(&lock))? {
            return Err(ERROR.into());
        }
        Ok(Self {
            directory,
            component: component.into(),
            _lock: lock,
        })
    }
    fn path(&self, id: &str) -> Result<PathBuf, String> {
        if !valid_id(id) {
            return Err("knowledge_draft_invalid".into());
        }
        let path = self.directory.join(format!("{id}.json"));
        io(devbox_filesystem::ensure_no_links(&self.directory))?;
        if fs::symlink_metadata(&path).is_ok() {
            io(devbox_filesystem::ensure_no_links(&path))?;
        }
        Ok(path)
    }
    fn validate(&self, draft: &Draft) -> Result<(), String> {
        draft.validate_for(&self.component).map_err(str::to_owned)
    }
    fn title(&self) -> String {
        format!(
            "API Studio · {} 결과",
            if self.component == "api-studio.api" {
                "Requests"
            } else {
                "Transforms"
            }
        )
    }
    pub fn get(&self, id: &str) -> Result<Draft, String> {
        let path = self.path(id)?;
        let (file, _) = io(devbox_filesystem::open_filesystem_object(&path, false))?;
        let mut bytes = Vec::new();
        io(file
            .take((MAX_FILE_BYTES + 1) as u64)
            .read_to_end(&mut bytes))?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err("knowledge_draft_invalid".into());
        }
        let draft: Draft = serde_json::from_slice(&bytes).map_err(|_| "knowledge_draft_invalid")?;
        self.validate(&draft)?;
        if draft.artifact.id != id {
            return Err("knowledge_draft_invalid".into());
        }
        Ok(draft)
    }
    pub fn list(&self) -> Result<Vec<Summary>, String> {
        let mut drafts = Vec::new();
        for (index, entry) in io(fs::read_dir(&self.directory))?.enumerate() {
            if index > MAX_DRAFTS + 8 {
                return Err(ERROR.into());
            }
            let entry = io(entry)?;
            let path = entry.path();
            if path.file_name().is_some_and(|name| name == ".lock") {
                continue;
            }
            // An interrupted atomic write is never authoritative and never a draft.
            if path.extension().is_some_and(|extension| extension == "tmp") {
                continue;
            }
            let id = path.file_stem().and_then(|id| id.to_str()).ok_or(ERROR)?;
            if path.extension().is_none_or(|extension| extension != "json") {
                return Err(ERROR.into());
            }
            drafts.push(self.get(id)?.summary());
            if drafts.len() > MAX_DRAFTS {
                return Err(ERROR.into());
            }
        }
        drafts.sort_by(|a, b| {
            b.created_at_ms
                .cmp(&a.created_at_ms)
                .then(a.artifact.id.cmp(&b.artifact.id))
        });
        Ok(drafts)
    }
    pub fn save(&self, output: &str, provenance: Provenance, now: u64) -> Result<Draft, String> {
        if output.len() > 512 * 1024 || output.chars().count() > 256_000 {
            return Err("knowledge_draft_invalid".into());
        }
        let masked = applink::redact_handoff_text(output).map_err(|_| "knowledge_draft_invalid")?;
        let draft = Draft {
            artifact: OwnedArtifactReference {
                provenance,
                id: uuid::Uuid::new_v4().to_string(),
                kind: OwnedArtifactKind::KnowledgeDraft,
            },
            created_at_ms: now,
            title: self.title(),
            body: masked.text,
            redacted: masked.redacted,
        };
        self.validate(&draft)?;
        if self.list()?.len() >= MAX_DRAFTS {
            return Err("knowledge_storage_full".into());
        }
        let path = self.path(&draft.artifact.id)?;
        if path.exists() {
            return Err(ERROR.into());
        }
        let bytes = serde_json::to_vec(&draft).map_err(|_| ERROR)?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err("knowledge_draft_invalid".into());
        }
        io(devbox_filesystem::atomic_write(&path, &bytes))?;
        Ok(draft)
    }
    pub fn delete(&self, id: &str) -> Result<(), String> {
        self.get(id)?; // Never delete an unknown/corrupt version or foreign record.
        io(fs::remove_file(self.path(id)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn provenance(component: &str) -> Provenance {
        Provenance {
            product: "api-studio".into(),
            component: component.into(),
            request_id: "fixture".into(),
            revision: 1,
        }
    }
    #[test]
    fn masked_draft_survives_restart_but_not_foreign_owner_or_installation() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path(), "api-studio.transforms").unwrap();
        let draft = store
            .save(
                "safe\npassword=synthetic-secret",
                provenance("api-studio.transforms"),
                1000,
            )
            .unwrap();
        assert!(draft.redacted && !draft.body.contains("synthetic-secret"));
        let id = draft.artifact.id.clone();
        drop(store);
        let store = Store::open(root.path(), "api-studio.transforms").unwrap();
        assert_eq!(store.get(&id).unwrap(), draft);
        assert_eq!(store.list().unwrap().len(), 1);
        assert!(Store::open(root.path(), "api-studio.api")
            .unwrap()
            .get(&id)
            .is_err());
        assert!(Store::open(
            &root.path().join("other-installation"),
            "api-studio.transforms"
        )
        .unwrap()
        .get(&id)
        .is_err());
        assert!(store.get("../foreign").is_err());
        let path = store.path(&id).unwrap();
        let mut invalid = serde_json::to_value(&draft).unwrap();
        invalid["artifact"]["kind"] = serde_json::json!("knowledge-draft/v999");
        fs::write(&path, serde_json::to_vec(&invalid).unwrap()).unwrap();
        assert!(store.get(&id).is_err() && store.delete(&id).is_err());
        assert!(path.exists());
    }
    #[test]
    fn full_store_preserves_records_until_explicit_delete() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path(), "api-studio.api").unwrap();
        for _ in 0..MAX_DRAFTS {
            store
                .save("safe", provenance("api-studio.api"), 1000)
                .unwrap();
        }
        assert_eq!(
            store.save("overflow", provenance("api-studio.api"), 1000),
            Err("knowledge_storage_full".into())
        );
        let first = store.list().unwrap()[0].artifact.id.clone();
        store.delete(&first).unwrap();
        assert_eq!(store.list().unwrap().len(), MAX_DRAFTS - 1);
        assert!(store
            .save("new", provenance("api-studio.api"), 1000)
            .is_ok());
    }
    #[cfg(unix)]
    #[test]
    fn linked_store_is_not_followed() {
        let root = tempfile::tempdir().unwrap();
        let foreign = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(foreign.path(), root.path().join("drafts")).unwrap();
        assert!(Store::open(root.path(), "api-studio.api").is_err());
        assert_eq!(fs::read_dir(foreign.path()).unwrap().count(), 0);
    }
}
