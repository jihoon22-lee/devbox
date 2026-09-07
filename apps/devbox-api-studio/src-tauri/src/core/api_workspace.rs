//! API-owned ID sets. Selecting a workspace grants no project, network or secret
//! authority and never selects an environment or replaces an in-memory draft.
use product_contract::context::ProjectContext;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};
const INVALID: &str = "api_workspace_invalid";
const STORAGE: &str = "api_workspace_unavailable";
const STALE: &str = "api_workspace_stale";
const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_REVISION: u64 = 9_007_199_254_740_991;
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Links {
    pub collection_ids: Vec<String>,
    pub environment_ids: Vec<String>,
    pub open_api_definition_ids: Vec<String>,
    pub mock_profile_ids: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub project_id: Option<String>,
    pub links: Links,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Document {
    pub schema_version: u8,
    pub revision: u64,
    pub selected_id: Option<String>,
    pub workspaces: Vec<Workspace>,
}
impl Default for Document {
    fn default() -> Self {
        Self {
            schema_version: 1,
            revision: 0,
            selected_id: None,
            workspaces: vec![],
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Association {
    Keep,
    Standalone,
    CurrentProject,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Save {
    pub expected_revision: u64,
    pub id: Option<String>,
    pub name: String,
    pub association: Association,
    pub links: Links,
}
fn uuid_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|value| value.to_string() == id)
}
fn reference_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 4096 && !id.chars().any(char::is_control)
}
fn project_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}
impl Links {
    fn validate(&self) -> Result<(), String> {
        for ids in [
            &self.collection_ids,
            &self.environment_ids,
            &self.open_api_definition_ids,
            &self.mock_profile_ids,
        ] {
            let mut seen = HashSet::new();
            if ids.len() > 10_000 || ids.iter().any(|id| !reference_id(id) || !seen.insert(id)) {
                return Err(INVALID.into());
            }
        }
        if self
            .open_api_definition_ids
            .iter()
            .chain(&self.mock_profile_ids)
            .any(|id| !uuid_id(id))
        {
            return Err(INVALID.into());
        }
        Ok(())
    }
}
impl Document {
    pub fn validate(&self) -> Result<(), String> {
        let mut ids = HashSet::new();
        if self.schema_version != 1 || self.revision > MAX_REVISION || self.workspaces.len() > 64 {
            return Err(INVALID.into());
        }
        for workspace in &self.workspaces {
            if !uuid_id(&workspace.id)
                || !ids.insert(&workspace.id)
                || workspace.name.trim().is_empty()
                || workspace.name.len() > 240
                || workspace.name.chars().any(char::is_control)
                || workspace
                    .project_id
                    .as_deref()
                    .is_some_and(|id| !project_id(id))
            {
                return Err(INVALID.into());
            }
            workspace.links.validate()?;
        }
        if self
            .selected_id
            .as_ref()
            .is_some_and(|id| !ids.contains(id))
        {
            return Err(INVALID.into());
        }
        Ok(())
    }
    fn check_revision(&self, expected: u64) -> Result<(), String> {
        if self.revision != expected {
            return Err(STALE.into());
        }
        if expected >= MAX_REVISION {
            return Err(INVALID.into());
        }
        Ok(())
    }
    pub fn save(
        &self,
        input: Save,
        verified_context: Option<&ProjectContext>,
    ) -> Result<Self, String> {
        self.check_revision(input.expected_revision)?;
        let mut next = self.clone();
        let old = match &input.id {
            Some(id) => Some(
                next.workspaces
                    .iter()
                    .position(|v| &v.id == id)
                    .ok_or(INVALID)?,
            ),
            None => None,
        };
        let project_id = match input.association {
            Association::Keep => old.and_then(|index| next.workspaces[index].project_id.clone()),
            Association::Standalone => None,
            Association::CurrentProject => {
                let context = verified_context.ok_or("api_workspace_project_unavailable")?;
                context
                    .validate()
                    .map_err(|_| "api_workspace_project_unavailable")?;
                Some(context.project_id.clone())
            }
        };
        let workspace = Workspace {
            id: input.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            name: input.name.trim().into(),
            project_id,
            links: input.links,
        };
        if let Some(index) = old {
            next.workspaces[index] = workspace;
        } else {
            next.workspaces.push(workspace);
        }
        next.revision += 1;
        next.validate()?;
        Ok(next)
    }
    pub fn select(&self, expected: u64, id: Option<String>) -> Result<Self, String> {
        self.check_revision(expected)?;
        let mut next = self.clone();
        next.selected_id = id;
        next.revision += 1;
        next.validate()?;
        Ok(next)
    }
    pub fn delete(&self, expected: u64, id: &str) -> Result<Self, String> {
        self.check_revision(expected)?;
        let mut next = self.clone();
        let index = next
            .workspaces
            .iter()
            .position(|v| v.id == id)
            .ok_or(INVALID)?;
        next.workspaces.remove(index);
        if next.selected_id.as_deref() == Some(id) {
            next.selected_id = None;
        }
        next.revision += 1;
        next.validate()?;
        Ok(next)
    }
}
pub(super) fn ensure_directory(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => {
            devbox_filesystem::ensure_no_links(path).map_err(|_| STORAGE.into())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            ensure_directory(path.parent().ok_or(STORAGE)?)?;
            match fs::create_dir(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err(STORAGE.into()),
            }
            if !fs::symlink_metadata(path).map_err(|_| STORAGE)?.is_dir() {
                return Err(STORAGE.into());
            }
            devbox_filesystem::ensure_no_links(path).map_err(|_| STORAGE.into())
        }
        _ => Err(STORAGE.into()),
    }
}
pub struct Store {
    path: PathBuf,
    _lock: fs::File,
}
impl Store {
    pub fn open(root: &Path) -> Result<Self, String> {
        let directory = root.join("api/workspaces");
        ensure_directory(&directory)?;
        let lock_path = directory.join(".lock");
        if fs::symlink_metadata(&lock_path).is_ok() {
            devbox_filesystem::ensure_no_links(&lock_path).map_err(|_| STORAGE)?;
        }
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)
            .map_err(|_| STORAGE)?;
        if !devbox_filesystem::try_lock_exclusive(&lock).map_err(|_| STORAGE)? {
            return Err(STORAGE.into());
        }
        Ok(Self {
            path: directory.join("v1.json"),
            _lock: lock,
        })
    }
    pub fn load(&self) -> Result<Document, String> {
        let Some(raw) = crate::core::import_repository::read_file(&self.path, MAX_BYTES)
            .map_err(|_| STORAGE)?
        else {
            return Ok(Document::default());
        };
        let document: Document = serde_json::from_str(&raw).map_err(|_| INVALID)?;
        document.validate()?;
        Ok(document)
    }
    pub fn write(&self, document: &Document) -> Result<(), String> {
        document.validate()?;
        let bytes = serde_json::to_vec(document).map_err(|_| INVALID)?;
        if bytes.len() > MAX_BYTES {
            return Err(INVALID.into());
        }
        devbox_filesystem::ensure_no_links(self.path.parent().ok_or(STORAGE)?)
            .map_err(|_| STORAGE)?;
        if fs::symlink_metadata(&self.path).is_ok() {
            devbox_filesystem::ensure_no_links(&self.path).map_err(|_| STORAGE)?;
        }
        devbox_filesystem::atomic_write(&self.path, &bytes).map_err(|_| STORAGE.into())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn input(revision: u64) -> Save {
        Save {
            expected_revision: revision,
            id: None,
            name: "Fixture".into(),
            association: Association::Standalone,
            links: Links {
                collection_ids: vec!["legacy-id".into()],
                ..Links::default()
            },
        }
    }
    #[test]
    fn workspace_switch_only_changes_metadata_and_rejects_stale_saves() {
        let first = Document::default().save(input(0), None).unwrap();
        let second = first.save(input(1), None).unwrap();
        assert_ne!(second.workspaces[0].id, second.workspaces[1].id);
        assert!(second.save(input(1), None).is_err());
        let selected = second
            .select(2, Some(second.workspaces[0].id.clone()))
            .unwrap();
        assert_eq!(selected.workspaces, second.workspaces);
        let removed = selected
            .delete(3, selected.selected_id.as_deref().unwrap())
            .unwrap();
        assert!(removed.selected_id.is_none());
        assert_eq!(removed.workspaces[0].links.collection_ids, ["legacy-id"]);
    }
    #[test]
    fn project_association_requires_the_native_verified_context_and_never_accepts_paths() {
        let mut save = input(0);
        save.association = Association::CurrentProject;
        assert!(Document::default().save(save, None).is_err());
        let context = ProjectContext {
            project_id: "project-1".into(),
            worktree_id: "worktree-2".into(),
            target: product_contract::context::ExecutionTarget::Windows,
            revision: 1,
        };
        let mut save = input(0);
        save.association = Association::CurrentProject;
        let mut document = Document::default().save(save, Some(&context)).unwrap();
        assert_eq!(
            document.workspaces[0].project_id.as_deref(),
            Some("project-1")
        );
        document.workspaces[0].project_id = Some("C:/project".into());
        assert!(document.validate().is_err());
    }
    #[test]
    fn native_store_reopens_and_preserves_corrupt_future_records() {
        let root = tempfile::tempdir().unwrap();
        let document = Document::default().save(input(0), None).unwrap();
        {
            let store = Store::open(root.path()).unwrap();
            store.write(&document).unwrap();
            assert!(Store::open(root.path()).is_err());
        }
        let store = Store::open(root.path()).unwrap();
        assert_eq!(store.load().unwrap(), document);
        for raw in [
            "{broken",
            "{\"schemaVersion\":99,\"revision\":0,\"selectedId\":null,\"workspaces\":[]}",
        ] {
            fs::write(&store.path, raw).unwrap();
            assert!(store.load().is_err());
            assert_eq!(fs::read_to_string(&store.path).unwrap(), raw);
        }
    }
}
