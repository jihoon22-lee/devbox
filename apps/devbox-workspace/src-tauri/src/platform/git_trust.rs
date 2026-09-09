//! Windows environment capture for shared native Git execution review.
#[cfg(windows)]
use super::git_files;
#[cfg(windows)]
use std::ffi::OsString;
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;
pub use workspace_wsl::git_trust::GitEnvironment;
pub type GitTrust = workspace_wsl::git_trust::GitTrust<super::project_probe::ProjectLease>;
type Result<T> = std::result::Result<T, &'static str>;
impl workspace_wsl::git_trust::GitLease for super::project_probe::ProjectLease {
    fn git_directories(&self) -> Option<(&Path, &Path)> {
        self.git_directories()
    }
}
/// Owned by the native Host for this product process. Windows UI/COM code may
/// mutate the ambient process environment after a picker opens. Git always gets
/// these same captured values through env_clear, including during later reviews.
/// Values stay in native memory; persisted approvals contain only their digest.
pub(crate) struct SourceEnvironment {
    #[cfg(windows)]
    values: Vec<(OsString, OsString)>,
}
impl SourceEnvironment {
    pub(crate) fn capture() -> Self {
        Self {
            #[cfg(windows)]
            values: std::env::vars_os().collect(),
        }
    }
}

#[cfg(windows)]
fn get(environment: &[(OsString, OsString)], name: &str) -> Option<OsString> {
    environment
        .iter()
        .find(|(key, _)| {
            key.to_str()
                .is_some_and(|key| key.eq_ignore_ascii_case(name))
        })
        .map(|(_, value)| value.clone())
}
#[cfg(windows)]
fn set(environment: &mut Vec<(OsString, OsString)>, name: &str, value: OsString) {
    environment.retain(|(key, _)| {
        !key.to_str()
            .is_some_and(|key| key.eq_ignore_ascii_case(name))
    });
    environment.push((name.into(), value));
}
pub(crate) fn native_environment(
    project: &Path,
    source: &SourceEnvironment,
    deadline: u64,
) -> Result<GitEnvironment> {
    #[cfg(windows)]
    {
        crate::files_host::current_deadline(deadline)?;
        let mut environment = source.values.clone();
        let home = get(&environment, "HOME")
            .filter(|value| !value.is_empty())
            .or_else(|| {
                let drive = get(&environment, "HOMEDRIVE").filter(|value| !value.is_empty())?;
                let relative = get(&environment, "HOMEPATH").filter(|value| !value.is_empty())?;
                let mut path = drive;
                path.push(relative);
                Some(path)
            })
            .or_else(|| get(&environment, "USERPROFILE"))
            .ok_or("git_home_unavailable")?;
        let home = PathBuf::from(home);
        if !home.is_absolute() {
            return Err("git_home_unavailable");
        }
        git_files::transport(project, &home)?;
        // Fix the same native home for review and every child, including
        // a GUI process whose inherited environment omitted HOME.
        set(&mut environment, "HOME", home.as_os_str().to_owned());
        // The legacy resolver probes candidates with exists(). Native
        // review must inspect links/transport before touching each path.
        let mut candidates = devbox_git::KNOWN_GIT_PATHS
            .iter()
            .map(|path| PathBuf::from(*path))
            .collect::<Vec<_>>();
        if let Some(paths) = get(&environment, "PATH") {
            candidates.extend(
                std::env::split_paths(&paths)
                    .filter(|path| path.is_absolute())
                    .map(|path| path.join("git.exe")),
            );
        }
        if candidates.len() > 256 {
            return Err("git_source_limit");
        }
        let mut installation = None;
        for candidate in candidates {
            crate::files_host::current_deadline(deadline)?;
            // A PATH entry cannot start WSL or introduce a network lookup.
            let Some(parsed) = candidate
                .to_str()
                .and_then(devbox_filesystem::parse_safe_project_path)
            else {
                continue;
            };
            if parsed.kind() != devbox_filesystem::ProjectPathKind::WindowsDrive {
                continue;
            }
            let Some(bin) = candidate.parent() else {
                continue;
            };
            let Some(parent) = bin.parent() else {
                continue;
            };
            let mut roots = vec![parent.to_owned()];
            if let Some(root) = parent.parent() {
                roots.push(root.to_owned());
            }
            for root in roots {
                for architecture in ["mingw64", "mingw32"] {
                    let runtime = root.join(architecture);
                    let program = runtime.join("bin/git.exe");
                    let launcher = root.join("cmd/git.exe");
                    if super::windows_path::admit(&program).is_err()
                        || super::windows_path::admit(&launcher).is_err()
                    {
                        continue;
                    }
                    if devbox_filesystem::ensure_no_links(&program).is_ok()
                        && devbox_filesystem::ensure_no_links(&launcher).is_ok()
                        && devbox_filesystem::ensure_no_links(root.join("etc")).is_ok()
                        && program.is_file()
                        && launcher.is_file()
                        && root.join("etc").is_dir()
                    {
                        installation = Some((launcher, program, root, runtime));
                        break;
                    }
                }
                if installation.is_some() {
                    break;
                }
            }
            if installation.is_some() {
                break;
            }
        }
        let (program, core_program, install, runtime) =
            installation.ok_or("git_installation_unavailable")?;
        let mut configs = Vec::new();
        if let Some(path) = get(&environment, "GIT_CONFIG_SYSTEM") {
            if !path.is_empty() {
                configs.push(git_files::resolve(
                    project,
                    project,
                    Path::new(&path),
                    deadline,
                )?);
            }
        } else {
            configs.extend([install.join("etc/gitconfig"), runtime.join("etc/gitconfig")]);
            if let Some(data) = get(&environment, "PROGRAMDATA") {
                configs.push(PathBuf::from(data).join("Git/config"));
            }
        }
        if let Some(path) = get(&environment, "GIT_CONFIG_GLOBAL") {
            if !path.is_empty() {
                configs.push(git_files::resolve(
                    project,
                    project,
                    Path::new(&path),
                    deadline,
                )?);
            }
        } else {
            configs.push(home.join(".gitconfig"));
            let xdg = get(&environment, "XDG_CONFIG_HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".config"));
            configs.push(xdg.join("git/config"));
        }
        Ok(GitEnvironment::from_native_parts(
            program.clone(),
            vec![program, core_program],
            environment,
            home,
            vec![runtime, install],
            configs,
            super::windows_path::admit,
        ))
    }
    #[cfg(not(windows))]
    {
        let _ = (project, source, deadline);
        Err("windows_required")
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn native_git_uses_the_hosts_captured_environment_for_review_and_execution() {
        let directory = tempfile::tempdir().unwrap();
        let root =
            super::super::storage_paths::display(&std::fs::canonicalize(directory.path()).unwrap())
                .unwrap();
        let empty = root.join("fixture.gitconfig");
        std::fs::write(&empty, b"").unwrap();
        let mut source = SourceEnvironment::capture();
        set(&mut source.values, "HOME", root.as_os_str().to_owned());
        set(
            &mut source.values,
            "GIT_CONFIG_SYSTEM",
            empty.as_os_str().to_owned(),
        );
        set(
            &mut source.values,
            "GIT_CONFIG_GLOBAL",
            empty.as_os_str().to_owned(),
        );
        set(
            &mut source.values,
            "DEVBOX_SOURCE_FROZEN_ENV_TEST",
            "reviewed-fixture-value".into(),
        );
        let reviewed = native_environment(&root, &source, u64::MAX).unwrap();
        assert_eq!(
            get(&reviewed.environment, "DEVBOX_SOURCE_FROZEN_ENV_TEST"),
            Some("reviewed-fixture-value".into())
        );
        // A separate owner's snapshot cannot change this Host's execution. No
        // test mutates ambient variables while other Rust tests are running.
        let mut other = SourceEnvironment {
            values: source.values.clone(),
        };
        set(
            &mut other.values,
            "DEVBOX_SOURCE_FROZEN_ENV_TEST",
            "another-owner-value".into(),
        );
        assert_ne!(
            reviewed.environment_digest(),
            native_environment(&root, &other, u64::MAX)
                .unwrap()
                .environment_digest()
        );
        let next = native_environment(&root, &source, u64::MAX).unwrap();
        assert_eq!(reviewed.environment_digest(), next.environment_digest());
        let output = std::process::Command::new(&next.program)
            .env_clear()
            .envs(next.environment.iter().cloned())
            .current_dir(&root)
            .args([
                "-c",
                "alias.fixture-env=!printf '%s' \"$DEVBOX_SOURCE_FROZEN_ENV_TEST\"",
                "fixture-env",
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"reviewed-fixture-value");
    }
    fn fixture(root: &Path) -> GitEnvironment {
        std::fs::create_dir_all(root.join(".git/objects")).unwrap();
        std::fs::write(root.join(".git/HEAD"), b"ref: refs/heads/main\n").unwrap();
        std::fs::write(root.join("owned-git"), b"not executed").unwrap();
        GitEnvironment::from_native_parts(
            root.join("owned-git"),
            vec![root.join("owned-git")],
            vec![("GIT_ASKPASS".into(), "synthetic-private-value".into())],
            root.into(),
            vec![root.into()],
            vec![root.join("global.config")],
            super::super::windows_path::admit,
        )
    }
    #[test]
    fn conditional_includes_and_changed_tools_are_bound_without_execution_or_secrets() {
        let root = tempfile::tempdir().unwrap();
        let environment = fixture(root.path());
        std::fs::write(
            root.path().join("global.config"),
            b"[includeIf \"onbranch:other/**\"]\npath=~/included.config\n",
        )
        .unwrap();
        std::fs::write(
            root.path().join("included.config"),
            b"[credential]\nhelper=!synthetic-private-value\n",
        )
        .unwrap();
        let lease = super::super::project_probe::probe_fixture(root.path()).unwrap();
        let trust = GitTrust::capture(lease, environment, u64::MAX).unwrap();
        assert!(trust.review.execution_keys.contains("credential.helper"));
        assert!(!serde_json::to_string(&trust.review)
            .unwrap()
            .contains("synthetic-private-value"));
        assert!(trust.revalidate(u64::MAX).is_ok());
        std::fs::write(
            root.path().join("included.config"),
            b"[credential]\nhelper=other\n",
        )
        .unwrap();
        assert!(trust.revalidate(u64::MAX).is_err());
        let environment = fixture(root.path());
        let lease = super::super::project_probe::probe_fixture(root.path()).unwrap();
        let trust = GitTrust::capture(lease, environment, u64::MAX).unwrap();
        std::fs::write(root.path().join("owned-git"), b"changed tool").unwrap();
        assert!(trust.revalidate(u64::MAX).is_err());
    }
}
