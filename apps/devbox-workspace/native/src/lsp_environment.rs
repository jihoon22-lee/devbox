//! Frozen, explicitly allowlisted Linux LSP environment. No runtime probe runs
//! while inspecting PATH or resolving an installed target.
use code_pad_lib::lsp::{EnvironmentAllowlist, RuntimeResolver};
use std::{
    collections::BTreeSet,
    ffi::OsString,
    path::{Path, PathBuf},
};
type Result<T> = std::result::Result<T, &'static str>;

pub(crate) struct Environment {
    path: Option<OsString>,
    home: Option<OsString>,
}
impl Default for Environment {
    fn default() -> Self {
        Self {
            path: std::env::var_os("PATH"),
            home: std::env::var_os("HOME"),
        }
    }
}
impl Environment {
    #[cfg(test)]
    pub(crate) fn fixture(path: &Path, home: &Path) -> Self {
        Self {
            path: Some(path.into()),
            home: Some(home.into()),
        }
    }
    pub(crate) fn resolver(
        &self,
        root: &Path,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<(RuntimeResolver, Vec<PathBuf>)> {
        let mut paths = Vec::new();
        let mut seen = BTreeSet::new();
        if let Some(value) = &self.path {
            if value.as_encoded_bytes().len() > 64 * 1024 {
                return Err("lsp_source_limit");
            }
            for (index, path) in std::env::split_paths(value).enumerate() {
                check()?;
                if index >= 256 {
                    return Err("lsp_source_limit");
                }
                if !path.is_absolute()
                    || crate::lsp_evidence::transport_with_admission(
                        root,
                        &path,
                        crate::linux_files::admit,
                    )
                    .is_err()
                {
                    continue;
                }
                if devbox_filesystem::ensure_no_links(&path).is_err() {
                    continue;
                }
                match std::fs::symlink_metadata(&path) {
                    Ok(metadata) if metadata.is_dir() => {
                        let path = path.canonicalize().map_err(|_| "lsp_source_unavailable")?;
                        crate::lsp_evidence::transport_with_admission(
                            root,
                            &path,
                            crate::linux_files::admit,
                        )?;
                        if seen.insert(path.clone()) {
                            paths.push(path);
                        }
                    }
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(_) => return Err("lsp_source_unavailable"),
                }
            }
        }
        let home = self
            .home
            .as_ref()
            .map(PathBuf::from)
            .ok_or("lsp_environment_unavailable")?;
        if home.as_os_str().as_encoded_bytes().len() > 32768 {
            return Err("lsp_source_limit");
        }
        crate::lsp_evidence::transport_with_admission(root, &home, crate::linux_files::admit)?;
        devbox_filesystem::ensure_no_links(&home).map_err(|_| "lsp_source_path_invalid")?;
        let home = home
            .canonicalize()
            .map_err(|_| "lsp_environment_unavailable")?;
        if !home.is_dir() {
            return Err("lsp_environment_unavailable");
        }
        crate::lsp_evidence::transport_with_admission(root, &home, crate::linux_files::admit)?;
        let environment = if paths.is_empty() {
            EnvironmentAllowlist::new()
        } else {
            EnvironmentAllowlist::with_path(
                std::env::join_paths(&paths).map_err(|_| "lsp_environment_unavailable")?,
            )
        }
        .allow("HOME", home.into_os_string())
        .map_err(|_| "lsp_environment_unavailable")?;
        Ok((RuntimeResolver::new().with_environment(environment), paths))
    }
}
