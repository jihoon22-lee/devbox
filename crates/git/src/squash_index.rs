//! Hold Git's shared-index lock while mutations use a disposable private index.
use super::{baseline, failed_merge};
use crate::{GitError, current_branch, run};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

pub(super) struct Index {
    index: PathBuf,
    lock: PathBuf,
    owner: File,
    original: Option<Vec<u8>>,
    private: PathBuf,
    _directory: tempfile::TempDir,
}

impl Index {
    pub(super) async fn acquire(repo: &Path) -> Result<Self, GitError> {
        let index = run::git(
            repo,
            &["rev-parse", "--path-format=absolute", "--git-path", "index"],
        )
        .await?;
        let index = PathBuf::from(index.trim_end_matches('\n'));
        let mut name = index.as_os_str().to_owned();
        name.push(".lock");
        let lock = PathBuf::from(name);
        let directory =
            tempfile::tempdir().map_err(|error| run::internal(error, "allocate squash index"))?;
        let owner = match OpenOptions::new().write(true).create_new(true).open(&lock) {
            Ok(owner) => owner,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(GitError::StateChanged);
            }
            Err(error) => return Err(run::internal(error, "lock squash index")),
        };
        let mut guard = Self {
            index,
            lock,
            owner,
            original: None,
            private: directory.path().join("index"),
            _directory: directory,
        };
        guard.original = read_index(&guard.index)?;
        if guard.original.is_some() {
            fs::copy(&guard.index, &guard.private)
                .map_err(|error| run::internal(error, "copy squash index"))?;
            let permissions = fs::metadata(&guard.index)
                .map_err(|error| run::internal(error, "inspect squash index"))?
                .permissions();
            guard
                .owner
                .set_permissions(permissions)
                .map_err(|error| run::internal(error, "preserve index permissions"))?;
        }
        Ok(guard)
    }

    pub(super) async fn output(
        &self,
        repo: &Path,
        args: &[&str],
    ) -> Result<(i32, String, String), GitError> {
        let mut command = run::command(repo, args);
        // This override is restricted to this owned transaction. Legitimate Git
        // hooks inherit the private index exactly as they do for an ordinary commit.
        command.env("GIT_INDEX_FILE", &self.private);
        run::execute(command, run::TIMEOUT).await
    }

    pub(super) async fn git(&self, repo: &Path, args: &[&str]) -> Result<String, GitError> {
        let (code, out, err) = self.output(repo, args).await?;
        if code == 0 {
            return Ok(out);
        }
        Err(GitError::Failed {
            op: args.first().unwrap_or(&"git").to_string(),
            stderr: err.trim().into(),
        })
    }

    pub(super) fn publish(mut self) -> Result<(), GitError> {
        if !self.unchanged()? {
            return Err(GitError::StateChanged);
        }
        let Some(private) = read_index(&self.private)? else {
            return Ok(());
        };
        self.owner
            .write_all(&private)
            .map_err(|error| run::internal(error, "write squash index"))?;
        self.owner
            .sync_all()
            .map_err(|error| run::internal(error, "flush squash index"))?;
        if !self.owns_lock() {
            return Err(GitError::StateChanged);
        }
        fs::rename(&self.lock, &self.index)
            .map_err(|error| run::internal(error, "publish squash index"))?;
        Ok(())
    }

    fn unchanged(&self) -> Result<bool, GitError> {
        Ok(self.owns_lock() && read_index(&self.index)? == self.original)
    }

    fn owns_lock(&self) -> bool {
        let Ok(path) = fs::symlink_metadata(&self.lock) else {
            return false;
        };
        let Ok(owner) = self.owner.metadata() else {
            return false;
        };
        if !path.is_file() {
            return false;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            path.dev() == owner.dev() && path.ino() == owner.ino()
        }
        #[cfg(not(unix))]
        {
            // The daemon runs on Linux. Retain an identity check for portable
            // callers rather than unlinking any file merely because its name matches.
            path.created()
                .ok()
                .is_some_and(|created| owner.created().ok() == Some(created))
                && path.len() == owner.len()
        }
    }
}

impl Drop for Index {
    fn drop(&mut self) {
        if self.owns_lock() {
            let _ = fs::remove_file(&self.lock);
        }
    }
}

fn read_index(path: &Path) -> Result<Option<Vec<u8>>, GitError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(run::internal(error, "read squash index")),
    }
}

pub(super) async fn squash(
    dir: &Path,
    base_branch: &str,
    original: &str,
    source_oid: &str,
    expected_branch: Option<&str>,
    message: &str,
) -> Result<(), GitError> {
    let index = Index::acquire(dir).await?;
    // Recheck after taking the standard Git lock: staging admitted before
    // the transaction belongs to the caller and must remain untouched.
    if baseline(dir, base_branch).await? != original {
        return Err(GitError::StateChanged);
    }
    let mut mutation_failed = false;
    let result = async {
        if let Err(error) = index.git(dir, &["merge", "--squash", source_oid]).await {
            mutation_failed = true;
            if !index.unchanged()? {
                return Err(GitError::StateChanged);
            }
            return failed_merge(dir, base_branch, original, error, Some(&index))
                .await
                .map(|_| ());
        }
        if current_branch(dir).await?.as_deref() != Some(base_branch)
            || run::resolve(dir, "HEAD").await? != original
            || if let Some(branch) = expected_branch {
                run::resolve(dir, branch).await? != source_oid
            } else {
                false
            }
        {
            return Err(GitError::StateChanged);
        }
        let (code, _, stderr) = index
            .output(
                dir,
                &[
                    "diff",
                    "--cached",
                    "--quiet",
                    "--no-ext-diff",
                    "--no-textconv",
                    original,
                    "--",
                ],
            )
            .await?;
        match code {
            // A fresh confirmed retry of an already-applied squash needs no
            // extra commit, but still follows the caller's normal close path.
            0 => {
                if current_branch(dir).await?.as_deref() != Some(base_branch)
                    || run::resolve(dir, "HEAD").await? != original
                    || if let Some(branch) = expected_branch {
                        run::resolve(dir, branch).await? != source_oid
                    } else {
                        false
                    }
                {
                    return Err(GitError::StateChanged);
                }
                Ok(())
            }
            1 => match index.git(dir, &["commit", "-m", message]).await {
                Ok(_) => Ok(()),
                Err(error) => {
                    mutation_failed = true;
                    if !index.unchanged()? {
                        return Err(GitError::StateChanged);
                    }
                    failed_merge(dir, base_branch, original, error, Some(&index))
                        .await
                        .map(|_| ())
                }
            },
            _ => Err(GitError::Failed {
                op: "inspect squash index".into(),
                stderr,
            }),
        }
    }
    .await;
    match result {
        Ok(()) => index.publish(),
        Err(error) => {
            // A rejected validation must not stage the private patch. Restore
            // only while the original base and our shared-index ownership hold.
            if !index.unchanged()? {
                return Err(GitError::StateChanged);
            }
            let base_unchanged = current_branch(dir).await?.as_deref() == Some(base_branch)
                && run::resolve(dir, "HEAD").await? == original;
            if mutation_failed {
                // A legitimate failing Git hook may have moved HEAD. Preserve
                // its staged index as before; never reset that changed checkout.
                if !base_unchanged {
                    index.publish()?;
                }
                return Err(error);
            }
            if base_unchanged {
                failed_merge(dir, base_branch, original, error, Some(&index))
                    .await
                    .map(|_| ())
            } else {
                Err(error)
            }
        }
    }
}
