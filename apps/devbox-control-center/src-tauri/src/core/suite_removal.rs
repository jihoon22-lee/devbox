//! A resumable package-only deletion plan. User data and unknown entries are
//! never enumerated for deletion; each file was pinned by the native installer.
use devbox_filesystem::{ensure_no_links, filesystem_identity, open_filesystem_object};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};
type Result<T> = std::result::Result<T, &'static str>;
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct File {
    pub relative: String,
    pub identity: (u64, u64),
    pub bytes: u64,
    pub sha256: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Plan {
    pub schema_version: u32,
    pub installation_key: String,
    pub root_identity: (u64, u64),
    pub files: Vec<File>,
}
fn relative(value: &str) -> bool {
    !value.is_empty()
        && value.len() < 1024
        && !value.contains(['\\', ':', '\0'])
        && !value.starts_with('/')
        && value.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.ends_with('.')
                && part.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b' ')
                })
                && !part.ends_with(' ')
        })
        && Path::new(value)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}
fn inspect(path: &Path, name: &str) -> Result<File> {
    ensure_no_links(path).map_err(|_| "suite_remove_path_unsafe")?;
    let (mut file, identity) =
        open_filesystem_object(path, false).map_err(|_| "suite_remove_file_unavailable")?;
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| "suite_remove_file_unavailable")?;
        if count == 0 {
            break;
        }
        bytes = bytes
            .checked_add(count as u64)
            .filter(|bytes| *bytes <= 2 * 1024 * 1024 * 1024)
            .ok_or("suite_remove_file_large")?;
        hash.update(&buffer[..count]);
    }
    if filesystem_identity(path, false).map_err(|_| "suite_remove_file_changed")? != identity {
        return Err("suite_remove_file_changed");
    }
    Ok(File {
        relative: name.into(),
        identity: identity.components(),
        bytes,
        sha256: hash
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    })
}
impl Plan {
    /// `files` is the native verified package closure, never renderer arguments.
    pub fn capture(root: &Path, installation_key: &str, files: &[String]) -> Result<Self> {
        ensure_no_links(root).map_err(|_| "suite_remove_path_unsafe")?;
        if files.is_empty() || files.len() > 4096 || files.iter().any(|name| !relative(name)) {
            return Err("suite_remove_plan_invalid");
        }
        let mut seen = BTreeSet::new();
        let mut records = Vec::new();
        for name in files {
            if !seen.insert(name.to_ascii_lowercase()) {
                return Err("suite_remove_plan_invalid");
            }
            records.push(inspect(&root.join(name), name)?);
        }
        let plan = Self {
            schema_version: 1,
            installation_key: installation_key.into(),
            root_identity: filesystem_identity(root, true)
                .map_err(|_| "suite_remove_root_changed")?
                .components(),
            files: records,
        };
        plan.validate()?;
        Ok(plan)
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1
            || !product_contract::commands::revision(&self.installation_key)
            || self.files.is_empty()
            || self.files.len() > 4096
        {
            return Err("suite_remove_plan_invalid");
        }
        let mut seen = BTreeSet::new();
        for file in &self.files {
            if !relative(&file.relative)
                || !seen.insert(file.relative.to_ascii_lowercase())
                || !product_contract::commands::revision(&file.sha256)
                || file.bytes > 2 * 1024 * 1024 * 1024
            {
                return Err("suite_remove_plan_invalid");
            }
        }
        Ok(())
    }
    fn check_root(&self, root: &Path) -> Result<()> {
        ensure_no_links(root).map_err(|_| "suite_remove_path_unsafe")?;
        if filesystem_identity(root, true)
            .map_err(|_| "suite_remove_root_changed")?
            .components()
            != self.root_identity
        {
            return Err("suite_remove_root_changed");
        }
        Ok(())
    }
    fn check_file(&self, root: &Path, file: &File) -> Result<bool> {
        let path = root.join(&file.relative);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(_) => Err("suite_remove_file_unavailable"),
            Ok(_) => {
                let actual = inspect(&path, &file.relative)?;
                if actual.identity != file.identity
                    || actual.bytes != file.bytes
                    || actual.sha256 != file.sha256
                {
                    return Err("suite_remove_file_changed");
                }
                Ok(true)
            }
        }
    }
    /// All remaining files are checked before deletion starts. Missing files
    /// represent an interrupted earlier attempt, never permission to remove a
    /// same-name replacement. Native callers retain their installation gate.
    pub fn remove(&self, root: &Path) -> Result<()> {
        self.verify_remaining(root)?;
        let mut directories = BTreeSet::<PathBuf>::new();
        for file in &self.files {
            self.check_root(root)?;
            if self.check_file(root, file)? {
                fs::remove_file(root.join(&file.relative))
                    .map_err(|_| "suite_remove_file_locked")?;
            }
            let mut directory = Path::new(&file.relative).parent();
            while let Some(path) = directory.filter(|path| !path.as_os_str().is_empty()) {
                directories.insert(path.into());
                directory = path.parent();
            }
        }
        let mut directories = directories.into_iter().collect::<Vec<_>>();
        directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
        for directory in directories {
            let path = root.join(directory);
            match fs::symlink_metadata(&path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return Err("suite_remove_path_unsafe"),
                Ok(_) => ensure_no_links(&path).map_err(|_| "suite_remove_path_unsafe")?,
            }
            match fs::remove_dir(&path) {
                Ok(()) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
                    ) => {}
                Err(_) => return Err("suite_remove_directory_locked"),
            }
        }
        Ok(())
    }
    pub fn verify_remaining(&self, root: &Path) -> Result<()> {
        self.validate()?;
        self.check_root(root)?;
        for file in &self.files {
            self.check_file(root, file)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn fixture() -> Temp {
        let root = std::env::temp_dir().join(format!("devbox-removal-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("products/workspace")).unwrap();
        fs::write(root.join("products/workspace/app.exe"), b"owned package").unwrap();
        fs::write(root.join("products/workspace/notice.txt"), b"owned notice").unwrap();
        Temp(root)
    }
    fn plan(root: &Path) -> Plan {
        Plan::capture(
            root,
            &"a".repeat(64),
            &[
                "products/workspace/app.exe".into(),
                "products/workspace/notice.txt".into(),
            ],
        )
        .unwrap()
    }
    #[test]
    fn interrupted_removal_resumes_and_retains_foreign_files_and_unlisted_data() {
        let root = fixture();
        let plan = plan(&root.0);
        fs::write(root.0.join("products/workspace/user.txt"), b"keep").unwrap();
        fs::write(root.0.join("user.db"), b"keep database").unwrap();
        fs::remove_file(root.0.join("products/workspace/app.exe")).unwrap();
        plan.remove(&root.0).unwrap();
        plan.remove(&root.0).unwrap();
        assert_eq!(
            fs::read(root.0.join("products/workspace/user.txt")).unwrap(),
            b"keep"
        );
        assert_eq!(fs::read(root.0.join("user.db")).unwrap(), b"keep database");
    }
    #[test]
    fn changed_file_aborts_before_removing_any_other_package_file() {
        let root = fixture();
        let plan = plan(&root.0);
        fs::write(
            root.0.join("products/workspace/notice.txt"),
            b"user replacement",
        )
        .unwrap();
        assert!(plan.remove(&root.0).is_err());
        assert!(root.0.join("products/workspace/app.exe").is_file());
        let mut invalid = plan.clone();
        invalid.files[0].relative = "../outside".into();
        assert!(invalid.validate().is_err());
    }
}
