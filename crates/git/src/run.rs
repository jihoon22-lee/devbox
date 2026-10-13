use crate::GitError;
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{io::AsyncReadExt, process::Command};
pub const MAX_OUTPUT: usize = 8 * 1024 * 1024;
pub const TIMEOUT: Duration = Duration::from_secs(30);

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
    execute(command, TIMEOUT).await
}

#[cfg(target_os = "linux")]
fn ssh_with_batch(command: &str) -> Result<String, GitError> {
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
    mut command: Command,
    timeout: Duration,
) -> Result<(i32, String, String), GitError> {
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
    let mut out = Vec::new();
    let mut err = Vec::new();
    let read = async {
        tokio::try_join!(
            read_bounded(&mut stdout, &mut out),
            read_bounded(&mut stderr, &mut err)
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

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    async fn effective_ssh_options(command: &str) {
        let ssh = ssh_with_batch(command).unwrap();
        let mut process = Command::new("sh");
        process
            .args(["-c", &format!("{ssh} -G fixture.invalid")])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let (code, output, _) = execute(process, TIMEOUT).await.unwrap();
        assert_eq!(code, 0, "SSH environment prefix broke command execution");
        assert!(output.lines().any(|line| line == "batchmode yes"));
        assert!(
            output
                .lines()
                .any(|line| line == "stricthostkeychecking true")
        );
    }
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn clone_ssh_keeps_assignment_prefix_before_executable() {
        effective_ssh_options("SSH_AUTH_SOCK='/tmp/fixture socket' ssh -F /dev/null -i '/tmp/fixture identity' -oBatchMode=no -oStrictHostKeyChecking=no").await;
    }
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn clone_ssh_keeps_env_wrapper_before_executable() {
        effective_ssh_options("env SSH_AUTH_SOCK='/tmp/fixture socket' ssh -F /dev/null -i '/tmp/fixture identity' -oBatchMode=no -oStrictHostKeyChecking=no").await;
    }
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn clone_batch_options_override_conflicting_ssh_options_without_network() {
        let ssh =
            ssh_with_batch("ssh -F /dev/null -oBatchMode=no -oStrictHostKeyChecking=no").unwrap();
        let mut command = Command::new("sh");
        command
            .args(["-c", &format!("{ssh} -G fixture.invalid")])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let (code, output, _) = execute(command, TIMEOUT).await.unwrap();
        assert_eq!(code, 0);
        assert!(
            output.lines().any(|line| line == "batchmode yes"),
            "effective batch mode was not enabled"
        );
        assert!(
            output
                .lines()
                .any(|line| line == "stricthostkeychecking true"),
            "effective strict host key checking was not enabled"
        );
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn clone_credential_environment_cannot_launch_interactive_helpers() {
        let fixture = tempfile::tempdir().unwrap();
        let directory = std::fs::File::open(fixture.path()).unwrap();
        let command = clone_command(&directory, &["clone", "--", "fixture", "."]);
        let variables: std::collections::HashMap<_, _> = command.as_std().get_envs().collect();
        for key in ["GIT_ASKPASS", "SSH_ASKPASS"] {
            assert_eq!(
                variables[std::ffi::OsStr::new(key)],
                Some(std::ffi::OsStr::new(""))
            );
        }
        assert_eq!(
            variables[std::ffi::OsStr::new("SSH_ASKPASS_REQUIRE")],
            Some(std::ffi::OsStr::new("never"))
        );
        assert_eq!(variables[std::ffi::OsStr::new("GIT_SSH")], None);
        assert_eq!(
            variables[std::ffi::OsStr::new("GCM_INTERACTIVE")],
            Some(std::ffi::OsStr::new("Never"))
        );
    }
    #[cfg(unix)]
    fn fixture(script: &str) -> Command {
        let mut command = Command::new("python3");
        command
            .args(["-c", script])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        command
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn timed_out_command_does_not_leave_owned_descendants() {
        let fixture_dir = tempfile::tempdir().unwrap();
        let marker = fixture_dir.path().join("survived");
        let child = format!(
            "import time,pathlib; time.sleep(.3); pathlib.Path({:?}).write_text('survived')",
            marker.to_str().unwrap()
        );
        let script = format!(
            "import subprocess,time; subprocess.Popen(['python3','-c', {child:?}]); time.sleep(60)"
        );
        assert!(matches!(
            execute(fixture(&script), Duration::from_millis(100)).await,
            Err(GitError::TimedOut)
        ));
        tokio::time::sleep(Duration::from_millis(450)).await;
        assert!(
            !marker.exists(),
            "owned descendant survived its command timeout"
        );
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn cancelled_command_does_not_leave_owned_descendants() {
        let fixture_dir = tempfile::tempdir().unwrap();
        let marker = fixture_dir.path().join("survived");
        let ready = fixture_dir.path().join("ready");
        let child = format!(
            "import time,pathlib; time.sleep(.3); pathlib.Path({:?}).write_text('survived')",
            marker.to_str().unwrap()
        );
        let script = format!(
            "import subprocess,time,pathlib; subprocess.Popen(['python3','-c', {child:?}]); pathlib.Path({:?}).write_text('ready'); time.sleep(60)",
            ready.to_str().unwrap()
        );
        let task = tokio::spawn(execute(fixture(&script), Duration::from_secs(30)));
        tokio::time::timeout(Duration::from_secs(2), async {
            while !ready.exists() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        tokio::time::sleep(Duration::from_millis(450)).await;
        assert!(!marker.exists(), "owned descendant survived cancellation");
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn oversized_stream_stops_without_waiting_for_other_pipe() {
        let command = fixture("import os,time; os.write(1,b'x'*(9*1024*1024)); time.sleep(60)");
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            execute(command, Duration::from_secs(30)),
        )
        .await;
        assert!(matches!(result, Ok(Err(GitError::LimitExceeded))));
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn timeout_and_invalid_utf8_are_explicit_failures() {
        assert!(matches!(
            execute(
                fixture("import time; time.sleep(60)"),
                Duration::from_millis(30)
            )
            .await,
            Err(GitError::TimedOut)
        ));
        assert!(matches!(
            execute(
                fixture("import os; os.write(1,b'\\xff')"),
                Duration::from_secs(2)
            )
            .await,
            Err(GitError::InvalidEncoding)
        ));
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
