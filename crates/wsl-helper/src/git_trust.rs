//! Git execution review observes files and the native environment without
//! launching Git, hooks, credential helpers, a package manager or a distro.
use crate::{
    files::{Admission, RootLease},
    git_config,
    git_files::{self, GitFiles},
};
use serde::{Deserialize, Serialize};
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
    admission: Admission,
}
impl GitEnvironment {
    /// Constructed by a native environment observer; never deserialized from IPC.
    pub fn from_native_parts(
        program: PathBuf,
        executables: Vec<PathBuf>,
        environment: Vec<(OsString, OsString)>,
        home: PathBuf,
        prefixes: Vec<PathBuf>,
        configs: Vec<PathBuf>,
        admission: Admission,
    ) -> Self {
        Self {
            program,
            executables,
            environment,
            home,
            prefixes,
            configs,
            admission,
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
            .map(|path| {
                git_files::resolve_with_admission(project, base, path, deadline, self.admission)
            })
            .collect()
    }
    pub fn environment_digest(&self) -> String {
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

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewedSource {
    pub path: String,
    pub kind: String,
    pub digest: Option<String>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitReview {
    pub executable: String,
    pub sources: Vec<ReviewedSource>,
    pub execution_keys: BTreeSet<String>,
    pub environment_keys: Vec<String>,
}
pub trait GitLease: RootLease {
    fn git_directories(&self) -> Option<(&Path, &Path)>;
}
pub struct GitTrust<L: GitLease> {
    lease: L,
    files: GitFiles,
    pub environment: GitEnvironment,
    pub review: GitReview,
    digest: String,
}
impl<L: GitLease> GitTrust<L> {
    pub fn capture(lease: L, environment: GitEnvironment, deadline: u64) -> Result<Self> {
        lease.revalidate()?;
        let root = lease.root().to_path_buf();
        let (git_dir, common) = lease
            .git_directories()
            .ok_or("source_requires_repository")?;
        let git_dir = git_dir.to_owned();
        let common = common.to_owned();
        let mut files = GitFiles::new(environment.admission);
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
                kind: "executable".into(),
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
            git_files::boundary(deadline)?;
            if !seen.insert(path.clone()) {
                continue;
            }
            if seen.len() > MAX_CONFIGS || depth > MAX_INCLUDE_DEPTH {
                return Err("git_source_limit");
            }
            let bytes = files.file(&root, &path, git_config::MAX_CONFIG_BYTES, deadline)?;
            review.sources.push(ReviewedSource {
                path: path.to_string_lossy().into_owned(),
                kind: "config".into(),
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
                kind: "hooksDirectory".into(),
                digest: None,
            });
            for hook in hooks {
                let bytes = files
                    .file(&root, &hook, 2 * 1024 * 1024, deadline)?
                    .ok_or("git_sources_changed")?;
                review.sources.push(ReviewedSource {
                    path: hook.to_string_lossy().into_owned(),
                    kind: "hook".into(),
                    digest: Some(digest(&bytes)),
                });
            }
        }
        files.revalidate(deadline)?;
        lease.revalidate()?;
        let digest =
            digest(format!("{}\n{}", files.digest(), environment.environment_digest()).as_bytes());
        Ok(Self {
            lease,
            files,
            environment,
            review,
            digest,
        })
    }
    pub fn root(&self) -> &Path {
        self.lease.root()
    }
    pub fn bytes(&self) -> usize {
        self.files.bytes()
    }
    pub fn native_root_identity(&self) -> devbox_filesystem::FilesystemIdentity {
        self.lease.native_root_identity()
    }
    pub fn matches(&self, target: &devbox_git::GitTarget) -> bool {
        !target.is_wsl()
            && same_repository_path(self.lease.root().to_str().unwrap_or(""), target.cwd())
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
    pub fn evidence_digests(&self) -> (String, String) {
        (self.files.digest(), self.environment.environment_digest())
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
        if !self.matches(target) {
            return Err("source_context_changed");
        }
        self.revalidate(deadline)?;
        let (git_dir, common) = self
            .lease
            .git_directories()
            .ok_or("source_requires_repository")?;
        Ok(devbox_git::execution::NativeRepository {
            worktree: self.lease.root().to_path_buf(),
            git_dir: git_dir.to_owned(),
            common_dir: common.to_owned(),
        })
    }
}

fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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
}
