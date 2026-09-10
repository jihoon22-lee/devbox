//! Frozen native Linux environment and read-only tool discovery. No Git probe,
//! shell, package manager or interop process runs while producing a review.
use crate::{git_files, git_trust::GitEnvironment};
use std::{
    ffi::OsString,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
type Result<T> = std::result::Result<T, &'static str>;

pub(crate) struct SourceEnvironment {
    values: Vec<(OsString, OsString)>,
}
impl Default for SourceEnvironment {
    fn default() -> Self {
        Self::from_values(std::env::vars_os().collect())
    }
}
impl SourceEnvironment {
    pub(crate) fn from_values(mut values: Vec<(OsString, OsString)>) -> Self {
        // These identify the transient launcher or its cwd, not project command
        // sources. Git's explicit cwd supplies PWD; Linux jobs do not need a
        // Windows interop socket. Never persist or return environment values.
        values.retain(|(key, _)| {
            !matches!(
                key.to_str(),
                Some("PWD" | "OLDPWD" | "SHLVL" | "_" | "WSL_INTEROP")
            )
        });
        Self { values }
    }
    pub(crate) fn observe(&self, project: &Path, deadline: u64) -> Result<GitEnvironment> {
        git_files::boundary(deadline)?;
        let mut environment = self.values.clone();
        let home = get(&environment, "HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or("git_home_unavailable")?;
        git_files::transport_with_admission(project, &home, crate::linux_files::admit)?;
        let paths = get(&environment, "PATH").ok_or("git_installation_unavailable")?;
        let mut program = None;
        for (index, directory) in std::env::split_paths(&paths).enumerate() {
            if index >= 256 {
                return Err("git_source_limit");
            }
            git_files::boundary(deadline)?;
            // Relative PATH entries and links cannot redirect read-only review
            // through the project or a Windows/foreign filesystem.
            if !directory.is_absolute() {
                continue;
            }
            let candidate = directory.join("git");
            if crate::linux_files::admit(&candidate).is_err() {
                continue;
            }
            if std::fs::metadata(&candidate)
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
            {
                program = Some(candidate);
                break;
            }
        }
        let program = program.ok_or("git_installation_unavailable")?;
        let prefix = program
            .parent()
            .and_then(Path::parent)
            .ok_or("git_installation_unavailable")?
            .to_path_buf();
        // Bind the effective system config explicitly, including absence. The
        // distro package uses /etc; a local prefix uses its own etc. An explicit
        // inherited override retains Git's empty-path disabling behavior.
        if get(&environment, "GIT_CONFIG_SYSTEM").is_none() {
            let system = if prefix == Path::new("/usr") {
                PathBuf::from("/etc/gitconfig")
            } else {
                prefix.join("etc/gitconfig")
            };
            environment.push(("GIT_CONFIG_SYSTEM".into(), system.into_os_string()));
        }
        let mut configs = Vec::new();
        for name in ["GIT_CONFIG_SYSTEM", "GIT_CONFIG_GLOBAL"] {
            if let Some(path) = get(&environment, name).filter(|value| !value.is_empty()) {
                configs.push(git_files::resolve_with_admission(
                    project,
                    project,
                    Path::new(&path),
                    deadline,
                    crate::linux_files::admit,
                )?);
            }
        }
        if get(&environment, "GIT_CONFIG_GLOBAL").is_none() {
            configs.push(home.join(".gitconfig"));
            let xdg = get(&environment, "XDG_CONFIG_HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".config"));
            configs.push(git_files::resolve_with_admission(
                project,
                project,
                &xdg.join("git/config"),
                deadline,
                crate::linux_files::admit,
            )?);
        }
        git_files::boundary(deadline)?;
        Ok(GitEnvironment::from_native_parts(
            program.clone(),
            vec![program],
            environment,
            home,
            vec![prefix],
            configs,
            crate::linux_files::admit,
        ))
    }
}
fn get(environment: &[(OsString, OsString)], key: &str) -> Option<OsString> {
    environment
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transient_launcher_fields_do_not_change_captured_command_sources() {
        let stable = vec![
            ("HOME".into(), "/home/fixture".into()),
            ("PATH".into(), "/usr/bin".into()),
            ("GIT_ASKPASS".into(), "fixture-only".into()),
        ];
        let mut transient = stable.clone();
        for key in ["PWD", "OLDPWD", "SHLVL", "_", "WSL_INTEROP"] {
            transient.push((key.into(), "transient-fixture".into()));
        }
        assert_eq!(
            SourceEnvironment::from_values(stable.clone()).values,
            SourceEnvironment::from_values(transient).values
        );
        assert_eq!(get(&stable, "path"), None);
    }
    #[test]
    fn discovery_does_not_execute_a_path_candidate_or_touch_a_linked_config() {
        let root = tempfile::Builder::new()
            .prefix(".git-env-fixture-")
            .tempdir_in(env!("CARGO_MANIFEST_DIR"))
            .unwrap();
        let bin = root.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let marker = root.path().join("must-not-exist");
        let program = bin.join("git");
        std::fs::write(
            &program,
            format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        let source = SourceEnvironment::from_values(vec![
            ("HOME".into(), root.path().into()),
            ("PATH".into(), bin.into_os_string()),
        ]);
        let observed = source.observe(root.path(), u64::MAX).unwrap();
        assert_eq!(observed.program, program);
        assert!(!marker.exists());
        assert_eq!(
            get(&observed.environment, "GIT_CONFIG_SYSTEM"),
            Some(root.path().join("etc/gitconfig").into_os_string())
        );
        assert_eq!(
            source.observe(root.path(), 0).err(),
            Some("request_expired")
        );
        let linked = root.path().join("config-link");
        std::os::unix::fs::symlink("/proc/self/status", &linked).unwrap();
        let mut values = source.values.clone();
        values.push(("GIT_CONFIG_SYSTEM".into(), linked.into_os_string()));
        assert!(SourceEnvironment::from_values(values)
            .observe(root.path(), u64::MAX)
            .is_err());
        assert!(!marker.exists());
    }
}
