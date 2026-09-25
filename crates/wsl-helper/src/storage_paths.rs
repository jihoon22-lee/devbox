//! Product authority stores are not user-editable files or worktree targets.
use devbox_filesystem::parse_safe_project_path;
use std::path::{Path, PathBuf};
type Result<T> = std::result::Result<T, &'static str>;

pub fn display(path: &Path) -> Result<PathBuf> {
    let value = path.to_str().ok_or("invalid_files_store")?;
    Ok(PathBuf::from(
        if let Some(unc) = value.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{unc}")
        } else {
            value.strip_prefix(r"\\?\").unwrap_or(value).to_owned()
        },
    ))
}
fn key(path: &Path) -> Result<String> {
    Ok(
        parse_safe_project_path(display(path)?.to_str().ok_or("invalid_files_store")?)
            .ok_or("invalid_files_store")?
            .identity()
            .replace('\\', "/"),
    )
}
fn inside(root: &str, path: &str) -> bool {
    path == root
        || path
            .strip_prefix(root.trim_end_matches('/'))
            .is_some_and(|tail| tail.starts_with('/'))
}
#[derive(Clone)]
pub struct ProtectedStorage {
    root: String,
    parents: Vec<String>,
    identifiers: Vec<String>,
}
impl ProtectedStorage {
    pub fn new(root: &Path, identifiers: Vec<String>) -> Result<Self> {
        let root = display(&std::fs::canonicalize(root).map_err(|_| "invalid_files_store")?)?;
        let parent = key(root.parent().ok_or("invalid_files_store")?)?;
        Ok(Self {
            root: key(&root)?,
            parents: vec![parent],
            identifiers,
        })
    }
    pub fn add_parent(&mut self, parent: &Path) -> Result<()> {
        let parent = key(parent)?;
        if !self.parents.contains(&parent) {
            self.parents.push(parent);
        }
        Ok(())
    }
    pub fn ensure_user_path(&self, path: &Path) -> Result<()> {
        let path = key(path)?;
        if inside(&self.root, &path) {
            return Err("file_owner_path");
        }
        for parent in &self.parents {
            if let Some(relative) = path.strip_prefix(&format!("{parent}/")) {
                let directory = relative.split('/').next().unwrap_or_default();
                if self.identifiers.iter().any(|id| {
                    directory == id
                        || directory
                            .strip_prefix(&format!("{id}.i"))
                            .is_some_and(|suffix| {
                                suffix.len() == 64
                                    && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
                            })
                }) {
                    return Err("file_owner_path");
                }
            }
        }
        Ok(())
    }
}
