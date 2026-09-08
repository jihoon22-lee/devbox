//! Git execution review observes files and the native environment without
//! launching Git, hooks, credential helpers, a package manager or a distro.
use super::{
    git_files::{self, GitFiles},
    project_probe::ProjectLease,
};
use crate::{core::git_config, definitions::digest};
use serde::Serialize;
use std::{
    collections::{BTreeSet, VecDeque},
    ffi::OsString,
    path::{Path, PathBuf},
};

type Result<T> = std::result::Result<T, &'static str>;
const MAX_CONFIGS: usize = 128;
const MAX_INCLUDE_DEPTH: usize = 10;

/// Canonical directory spelling stays case-sensitive even on Windows. Git
/// output cannot admit another NTFS worktree that differs only by letter case.
fn same_repository_path(expected: &str, actual: &str) -> bool {
    use devbox_filesystem::{parse_safe_project_path, ProjectPathKind};
    let normalize = |value: &str| -> Option<(ProjectPathKind, String)> {
        let parsed = parse_safe_project_path(value)?;
        let normalized = match parsed.kind() {
            ProjectPathKind::Posix => parsed.as_str().to_owned(),
            ProjectPathKind::WindowsDrive => format!(
                "{}:/{}",
                (parsed.as_str().as_bytes()[0] as char).to_ascii_uppercase(),
                parsed.as_str()[3..]
                    .split(['/', '\\'])
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join("/")
            ),
            ProjectPathKind::WindowsUnc => {
                let mut parts = parsed
                    .as_str()
                    .split(['/', '\\'])
                    .filter(|part| !part.is_empty());
                format!(
                    "//{}/{}/{}",
                    parts.next()?.to_ascii_lowercase(),
                    parts.next()?.to_ascii_lowercase(),
                    parts.collect::<Vec<_>>().join("/")
                )
            }
        };
        Some((parsed.kind(), normalized))
    };
    normalize(expected).is_some_and(|expected| normalize(actual).as_ref() == Some(&expected))
}

pub struct GitEnvironment {
    pub program: PathBuf,
    executables: Vec<PathBuf>,
    pub environment: Vec<(OsString, OsString)>,
    home: PathBuf,
    prefixes: Vec<PathBuf>,
    configs: Vec<PathBuf>,
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
impl GitEnvironment {
    pub fn native(project: &Path, deadline: u64) -> Result<Self> {
        #[cfg(windows)]
        {
            crate::files_host::current_deadline(deadline)?;
            let mut environment = std::env::vars_os().collect::<Vec<_>>();
            let home = get(&environment, "HOME")
                .filter(|value| !value.is_empty())
                .or_else(|| {
                    let drive = get(&environment, "HOMEDRIVE").filter(|value| !value.is_empty())?;
                    let relative =
                        get(&environment, "HOMEPATH").filter(|value| !value.is_empty())?;
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
            Ok(Self {
                executables: vec![program.clone(), core_program],
                program,
                environment,
                home,
                prefixes: vec![runtime, install],
                configs,
            })
        }
        #[cfg(not(windows))]
        {
            let _ = (project, deadline);
            Err("windows_required")
        }
    }
    fn interpolate(
        &self,
        project: &Path,
        base: &Path,
        raw: &str,
        deadline: u64,
    ) -> Result<Vec<PathBuf>> {
        let raw = raw.strip_prefix(":(optional)").unwrap_or(raw);
        let candidates = if let Some(relative) = raw.strip_prefix("~/") {
            vec![self.home.join(relative)]
        } else if raw.starts_with('~') {
            return Err("git_source_path_unsupported");
        } else if let Some(relative) = raw.strip_prefix("%(prefix)/") {
            // Git for Windows distributions use both installation and runtime
            // prefixes. Observe both possible sources without executing Git.
            self.prefixes
                .iter()
                .map(|prefix| prefix.join(relative))
                .collect()
        } else {
            vec![PathBuf::from(raw)]
        };
        candidates
            .iter()
            .map(|path| git_files::resolve(project, base, path, deadline))
            .collect()
    }
    fn digest(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut hash = Sha256::new();
        let mut environment = self.environment.iter().collect::<Vec<_>>();
        environment.sort_by(|(left, _), (right, _)| left.cmp(right));
        for (key, value) in environment {
            for value in [key, value] {
                let bytes = value.as_encoded_bytes();
                hash.update((bytes.len() as u64).to_be_bytes());
                hash.update(bytes);
            }
        }
        hash.finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewedSource {
    pub path: String,
    pub kind: &'static str,
    pub digest: Option<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitReview {
    pub executable: String,
    pub sources: Vec<ReviewedSource>,
    pub execution_keys: BTreeSet<String>,
    pub environment_keys: Vec<String>,
}
pub struct GitTrust {
    lease: ProjectLease,
    files: GitFiles,
    pub environment: GitEnvironment,
    pub review: GitReview,
    digest: String,
}
impl GitTrust {
    pub fn capture(
        lease: ProjectLease,
        environment: GitEnvironment,
        deadline: u64,
    ) -> Result<Self> {
        lease.revalidate()?;
        let root = PathBuf::from(&lease.binding().root);
        let (git_dir, common) = lease
            .git_directories()
            .ok_or("source_requires_repository")?;
        let git_dir = git_dir.to_owned();
        let common = common.to_owned();
        let mut files = GitFiles::default();
        let mut review = GitReview {
            executable: environment.program.to_string_lossy().into_owned(),
            sources: vec![],
            execution_keys: BTreeSet::new(),
            environment_keys: environment
                .environment
                .iter()
                .filter_map(|(key, _)| key.to_str())
                .filter(|key| {
                    key.starts_with("GIT_")
                        || matches!(*key, "SSH_ASKPASS" | "SSH_AUTH_SOCK" | "PATH")
                })
                .map(str::to_owned)
                .collect(),
        };
        // Keep Git for Windows' launcher environment behavior while pinning
        // both the launcher and the actual Git executable it dispatches.
        for path in &environment.executables {
            let bytes = files
                .file(&root, path, 32 * 1024 * 1024, deadline)?
                .ok_or("git_installation_unavailable")?;
            review.sources.push(ReviewedSource {
                path: path.to_string_lossy().into_owned(),
                kind: "executable",
                digest: Some(digest(&bytes)),
            });
        }
        review.environment_keys.sort();
        let mut pending = environment
            .configs
            .iter()
            .cloned()
            .map(|path| (path, 0))
            .collect::<VecDeque<_>>();
        pending.push_back((common.join("config"), 0));
        pending.push_back((git_dir.join("config.worktree"), 0));
        let mut seen = BTreeSet::new();
        let mut hook_paths = BTreeSet::from([common.join("hooks"), git_dir.join("hooks")]);
        while let Some((path, depth)) = pending.pop_front() {
            crate::files_host::current_deadline(deadline)?;
            if !seen.insert(path.clone()) {
                continue;
            }
            if seen.len() > MAX_CONFIGS || depth > MAX_INCLUDE_DEPTH {
                return Err("git_source_limit");
            }
            let bytes = files.file(&root, &path, git_config::MAX_CONFIG_BYTES, deadline)?;
            review.sources.push(ReviewedSource {
                path: path.to_string_lossy().into_owned(),
                kind: "config",
                digest: bytes.as_deref().map(digest),
            });
            let Some(bytes) = bytes else {
                continue;
            };
            let references = git_config::inspect(&bytes)?;
            review.execution_keys.extend(references.execution_keys);
            for include in references.includes {
                for path in environment.interpolate(
                    &root,
                    path.parent().ok_or("git_source_path_invalid")?,
                    &include,
                    deadline,
                )? {
                    pending.push_back((path, depth + 1));
                }
            }
            for hooks in references.hook_paths {
                if hooks == "/dev/null" {
                    continue;
                }
                // Hooks invoked by local worktree and repository-only Git
                // commands have different cwd rules. Neither can widen trust.
                for base in [&root, &git_dir] {
                    hook_paths.extend(environment.interpolate(&root, base, &hooks, deadline)?);
                }
            }
        }
        if hook_paths.len() > 128 {
            return Err("git_source_limit");
        }
        for directory in hook_paths {
            let hooks = files.hooks(&root, &directory, deadline)?;
            review.sources.push(ReviewedSource {
                path: directory.to_string_lossy().into_owned(),
                kind: "hooksDirectory",
                digest: None,
            });
            for hook in hooks {
                let bytes = files
                    .file(&root, &hook, 2 * 1024 * 1024, deadline)?
                    .ok_or("git_sources_changed")?;
                review.sources.push(ReviewedSource {
                    path: hook.to_string_lossy().into_owned(),
                    kind: "hook",
                    digest: Some(digest(&bytes)),
                });
            }
        }
        files.revalidate(deadline)?;
        lease.revalidate()?;
        let digest = digest(format!("{}\n{}", files.digest(), environment.digest()).as_bytes());
        Ok(Self {
            lease,
            files,
            environment,
            review,
            digest,
        })
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
    pub fn directories(&self) -> Option<(&Path, &Path)> {
        self.lease.git_directories()
    }
    pub fn revalidate(&self, deadline: u64) -> Result<()> {
        self.lease.revalidate()?;
        self.files.revalidate(deadline)?;
        self.lease.revalidate()
    }
    pub fn repository(
        &self,
        target: &devbox_git::GitTarget,
        deadline: u64,
    ) -> Result<devbox_git::execution::NativeRepository> {
        if target.is_wsl() || !same_repository_path(&self.lease.binding().root, target.cwd()) {
            return Err("source_context_changed");
        }
        self.revalidate(deadline)?;
        let (git_dir, common) = self
            .lease
            .git_directories()
            .ok_or("source_requires_repository")?;
        Ok(devbox_git::execution::NativeRepository {
            worktree: PathBuf::from(&self.lease.binding().root),
            git_dir: git_dir.to_owned(),
            common_dir: common.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn git_returned_case_variants_cannot_admit_a_distinct_ntfs_worktree() {
        assert!(same_repository_path(
            r"C:\Projects\Repo",
            "c:/Projects/Repo"
        ));
        assert!(!same_repository_path(
            r"C:\Projects\Repo",
            r"C:\Projects\repo"
        ));
        assert!(!same_repository_path(
            r"C:\Projects\Repo",
            r"C:\projects\Repo"
        ));
        assert!(same_repository_path(
            r"\\Server\Share\Repo",
            "//SERVER/share/Repo"
        ));
        assert!(!same_repository_path(
            r"\\Server\Share\Repo",
            "//server/share/repo"
        ));
        assert!(!same_repository_path("/project/Repo", "/project/repo"));
    }
    fn fixture(root: &Path) -> GitEnvironment {
        std::fs::create_dir_all(root.join(".git/objects")).unwrap();
        std::fs::write(root.join(".git/HEAD"), b"ref: refs/heads/main\n").unwrap();
        std::fs::write(root.join("owned-git"), b"not executed").unwrap();
        GitEnvironment {
            program: root.join("owned-git"),
            executables: vec![root.join("owned-git")],
            environment: vec![("GIT_ASKPASS".into(), "synthetic-private-value".into())],
            home: root.into(),
            prefixes: vec![root.into()],
            configs: vec![root.join("global.config")],
        }
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
