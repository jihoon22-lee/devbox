use crate::GitError;
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};
pub const MAX_OUTPUT: usize = 8 * 1024 * 1024;
pub const TIMEOUT: Duration = Duration::from_secs(30);
#[path = "implicit_checkout.rs"]
pub mod implicit_checkout;
// Cloning can transfer a large history over a slow connection. Keep it bounded
// independently of short queries; execute still kills owned helpers on cancellation.
#[cfg(target_os = "linux")]
const CLONE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Nonzero exit is returned to allow explicit handling of symbolic-ref and no-index.
pub async fn output(dir: &Path, args: &[&str]) -> Result<(i32, String, String), GitError> {
    execute(command(dir, args), TIMEOUT).await
}

/// Run a clone inside a pinned Linux directory without reopening a replaceable target.
#[cfg(target_os = "linux")]
pub async fn clone_output(
    dir: &std::fs::File,
    args: &[&str],
) -> Result<(i32, String, String), GitError> {
    let ssh = if let Some(command) = std::env::var_os("GIT_SSH_COMMAND") {
        command
            .into_string()
            .map_err(|_| GitError::InvalidEncoding)?
    } else if let Some(program) = std::env::var_os("GIT_SSH") {
        let program = program
            .into_string()
            .map_err(|_| GitError::InvalidEncoding)?;
        format!("'{}'", program.replace('\'', "'\\''"))
    } else {
        let (code, output, _) = execute(
            clone_command(dir, &["config", "--get", "core.sshCommand"]),
            TIMEOUT,
        )
        .await?;
        match code {
            0 => output.trim_end_matches('\n').to_owned(),
            1 => "ssh".into(),
            _ => return Err(GitError::Internal),
        }
    };
    let mut command = clone_command(dir, args);
    // Keep configured identities, wrappers, and SSH config. User helper commands are
    // trusted configuration; stdin, askpass, batch mode, output, and time stay bounded.
    command.env("GIT_SSH_COMMAND", ssh_with_batch(&ssh)?);
    execute(command, CLONE_TIMEOUT).await
}

#[cfg(target_os = "linux")]
pub(crate) fn ssh_with_batch(command: &str) -> Result<String, GitError> {
    let command = command.trim_start();
    let mut cursor = 0;
    let mut env_wrapper = false;
    let executable_end = loop {
        let remainder = command[cursor..].trim_start();
        cursor = command.len() - remainder.len();
        let end = shell_word_end(remainder)?;
        let word = &remainder[..end];
        cursor += end;
        if environment_assignment(word) {
            continue;
        }
        let literal = word
            .strip_prefix('\'')
            .and_then(|word| word.strip_suffix('\''))
            .or_else(|| {
                word.strip_prefix('"')
                    .and_then(|word| word.strip_suffix('"'))
            })
            .unwrap_or(word);
        if Path::new(literal)
            .file_name()
            .is_some_and(|name| name == "env")
        {
            env_wrapper = true;
            continue;
        }
        if env_wrapper {
            match literal {
                "--" | "-i" | "--ignore-environment" => continue,
                "-u" | "--unset" | "-C" | "--chdir" => {
                    let value = command[cursor..].trim_start();
                    cursor = command.len() - value.len() + shell_word_end(value)?;
                    continue;
                }
                word if word.starts_with("--unset=") || word.starts_with("--chdir=") => continue,
                _ => (),
            }
        }
        break cursor;
    };
    // OpenSSH uses the first value. Preserve environment prefixes and the exact
    // remaining arguments (including shell-expanded identity paths), then place
    // enforced options immediately after the actual executable.
    Ok(format!(
        "{} -oBatchMode=yes -oStrictHostKeyChecking=yes{}",
        &command[..executable_end],
        &command[executable_end..]
    ))
}

#[cfg(target_os = "linux")]
fn environment_assignment(word: &str) -> bool {
    let word = word
        .strip_prefix('\'')
        .and_then(|word| word.strip_suffix('\''))
        .or_else(|| {
            word.strip_prefix('"')
                .and_then(|word| word.strip_suffix('"'))
        })
        .unwrap_or(word);
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && name.bytes().enumerate().all(|(index, byte)| {
                byte == b'_' || byte.is_ascii_alphabetic() || (index > 0 && byte.is_ascii_digit())
            })
    })
}

#[cfg(target_os = "linux")]
fn shell_word_end(command: &str) -> Result<usize, GitError> {
    let mut quote = None;
    let mut escaped = false;
    let mut end = command.len();
    for (index, character) in command.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match quote {
            Some('\'') => {
                if character == '\'' {
                    quote = None;
                }
            }
            Some('"') => match character {
                '"' => quote = None,
                '\\' => escaped = true,
                _ => (),
            },
            None => match character {
                '\'' | '"' => quote = Some(character),
                '\\' => escaped = true,
                character if character.is_whitespace() => {
                    end = index;
                    break;
                }
                _ => (),
            },
            _ => unreachable!(),
        }
    }
    if end == 0 || quote.is_some() || escaped {
        return Err(GitError::InvalidInput);
    }
    Ok(end)
}

#[cfg(target_os = "linux")]
fn clone_command(dir: &std::fs::File, args: &[&str]) -> Command {
    use std::os::fd::AsRawFd;
    let mut command = command(Path::new("."), args);
    command
        .current_dir(format!(
            "/proc/{}/fd/{}",
            std::process::id(),
            dir.as_raw_fd()
        ))
        .env("GIT_ASKPASS", "")
        .env("SSH_ASKPASS", "")
        .env("SSH_ASKPASS_REQUIRE", "never")
        .env("GCM_INTERACTIVE", "Never")
        .env("GCM_GUI_PROMPT", "false")
        .env_remove("GIT_SSH");
    command
}
pub(crate) fn command(dir: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command
        .arg("--literal-pathspecs")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "core.quotepath=false",
            "-c",
            "color.ui=false",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.pager=cat",
        ])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // Prevent caller environment from silently redirecting the repository/index/object store.
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_NAMESPACE",
        "GIT_CEILING_DIRECTORIES",
        "GIT_DISCOVERY_ACROSS_FILESYSTEM",
    ] {
        command.env_remove(key);
    }
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0");
    command
}

/// Use a disposable caller-owned index without changing the repository index.
pub(crate) async fn with_index(
    dir: &Path,
    index: &Path,
    hooks: &Path,
    args: &[&str],
) -> Result<String, GitError> {
    let mut index_args = vec![
        "-c",
        "core.splitIndex=false",
        "-c",
        "core.sparseCheckout=false",
    ];
    // read-tree can invoke post-index-change even with an isolated index.
    // Restrict hooks only for these read-only temporary-index commands.
    let hook_setting = format!(
        "core.hooksPath={}",
        hooks.to_str().ok_or(GitError::InvalidEncoding)?
    );
    index_args.extend(["-c", hook_setting.as_str()]);
    index_args.extend_from_slice(args);
    let mut command = command(dir, &index_args);
    command.env("GIT_INDEX_FILE", index);
    let (code, out, err) = execute(command, TIMEOUT).await?;
    if code == 0 {
        Ok(out)
    } else {
        Err(GitError::Failed {
            op: args.first().unwrap_or(&"git").to_string(),
            stderr: err.trim().into(),
        })
    }
}

pub(crate) async fn execute(
    command: Command,
    timeout: Duration,
) -> Result<(i32, String, String), GitError> {
    execute_input(command, timeout, None).await
}
pub(crate) async fn execute_input(
    mut command: Command,
    timeout: Duration,
    input: Option<&[u8]>,
) -> Result<(i32, String, String), GitError> {
    if input.is_some() {
        command.stdin(Stdio::piped());
    }
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn().map_err(|error| {
        tracing::error!(%error, "spawn Git command");
        GitError::Internal
    })?;
    #[cfg(unix)]
    let _owned_group = OwnedGroup(child.id().ok_or(GitError::Internal)?);
    let mut stdout = child
        .stdout
        .take()
        .ok_or(GitError::Internal)?
        .take((MAX_OUTPUT + 1) as u64);
    let mut stderr = child
        .stderr
        .take()
        .ok_or(GitError::Internal)?
        .take((MAX_OUTPUT + 1) as u64);
    let mut stdin = child.stdin.take();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let read = async {
        tokio::try_join!(
            read_bounded(&mut stdout, &mut out),
            read_bounded(&mut stderr, &mut err),
            async {
                if let (Some(stdin), Some(input)) = (stdin.as_mut(), input) {
                    stdin
                        .write_all(input)
                        .await
                        .map_err(|e| internal(e, "write Git input"))?;
                    stdin
                        .shutdown()
                        .await
                        .map_err(|e| internal(e, "close Git input"))?;
                }
                drop(stdin.take());
                Ok::<(), GitError>(())
            }
        )?;
        let status = child
            .wait()
            .await
            .map_err(|error| internal(error, "wait Git command"))?;
        Ok(status.code().unwrap_or(-1))
    };
    let result = tokio::time::timeout(timeout, read)
        .await
        .map_err(|_| GitError::TimedOut)
        .and_then(|result| result);
    let code = match result {
        Ok(code) => code,
        Err(error) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(error);
        }
    };
    Ok((
        code,
        String::from_utf8(out).map_err(|_| GitError::InvalidEncoding)?,
        String::from_utf8(err).map_err(|_| GitError::InvalidEncoding)?,
    ))
}
// Killing a command's private group also covers helpers that inherited its pipes.
// The guard runs on timeout, output failure, and future cancellation.
#[cfg(unix)]
struct OwnedGroup(u32);
#[cfg(unix)]
impl Drop for OwnedGroup {
    fn drop(&mut self) {
        if let Ok(pid) = i32::try_from(self.0) {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(pid),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
    }
}

pub async fn git(dir: &Path, args: &[&str]) -> Result<String, GitError> {
    let (code, out, err) = output(dir, args).await?;
    if code == 0 {
        return Ok(out);
    }
    if err.contains("not a git repository") {
        return Err(GitError::NotRepo);
    }
    Err(GitError::Failed {
        op: args.first().unwrap_or(&"git").to_string(),
        stderr: err.trim().to_string(),
    })
}
pub async fn resolve(dir: &Path, reference: &str) -> Result<String, GitError> {
    if reference.is_empty() || reference.starts_with('-') || reference.contains('\0') {
        return Err(GitError::InvalidInput);
    }
    let oid = git(
        dir,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{reference}^{{commit}}"),
        ],
    )
    .await?;
    let oid = oid.trim();
    if !matches!(oid.len(), 40 | 64) || !oid.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(GitError::InvalidOutput);
    }
    Ok(oid.into())
}
pub async fn merge_base(dir: &Path, base: &str) -> Result<String, GitError> {
    let base = resolve(dir, base).await?;
    let head = resolve(dir, "HEAD").await?;
    Ok(git(dir, &["merge-base", &base, &head]).await?.trim().into())
}
pub(crate) fn number(s: &str) -> Result<u32, GitError> {
    s.parse().map_err(|_| GitError::InvalidOutput)
}

async fn read_bounded(
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
    data: &mut Vec<u8>,
) -> Result<(), GitError> {
    reader
        .read_to_end(data)
        .await
        .map_err(|error| internal(error, "read Git output"))?;
    if data.len() > MAX_OUTPUT {
        Err(GitError::LimitExceeded)
    } else {
        Ok(())
    }
}

pub(crate) fn internal(error: std::io::Error, operation: &'static str) -> GitError {
    tracing::error!(%error, operation, "Git I/O failure");
    GitError::Internal
}

/// File query paths are repository-root-relative, including for callers in a subdirectory.
pub(crate) async fn repository_root(dir: &Path) -> Result<std::path::PathBuf, GitError> {
    let root = git(dir, &["rev-parse", "--show-toplevel"]).await?;
    let root = root.strip_suffix('\n').ok_or(GitError::InvalidOutput)?;
    Ok(std::path::PathBuf::from(root))
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;
