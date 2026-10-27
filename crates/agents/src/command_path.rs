//! Finds an agent CLI the way the user's terminal does.
//!
//! Version managers usually add their folders to `PATH` in `~/.bashrc`, which the login-shell
//! capture behind the service `PATH` never reads, so the interactive shell is asked first. Its
//! output is whatever the user's shell prints: only the absolute path between the last pair of
//! `\x1e` markers is used, and nothing else of it is logged, kept or returned.
//!
//! A lookup never starts the program it finds. Only the environment check asks the agent CLIs
//! it knows for their versions, with [`version`].
use futures::future::BoxFuture;
use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command, time::Instant};

/// The interactive shell gets this long, and at most this much of its output is read.
const SHELL_TIMEOUT: Duration = Duration::from_secs(5);
const OUTPUT_LIMIT: usize = 64 * 1024;
const VERSION_TIMEOUT: Duration = Duration::from_secs(5);
/// How long [`LookupCache::resolve`] reuses a lookup the shell answered, found or not. Each
/// environment check looks again with [`LookupCache::refresh`], and a lookup the shell did not
/// answer is never kept.
const CACHE_FOR: Duration = Duration::from_secs(5 * 60);
const MARKER: u8 = 0x1e;
/// The program name stays `$1`; it is never part of the script.
const SCRIPT: &str = r#"printf "\036%s\036" "$(command -v -- "$1")""#;
/// Shell keywords and builtins: a command starting with one is shell syntax, not a program.
const SHELL_WORDS: &[&str] = &[
    ".",
    "alias",
    "bg",
    "bind",
    "break",
    "builtin",
    "caller",
    "case",
    "cd",
    "command",
    "compgen",
    "complete",
    "compopt",
    "continue",
    "coproc",
    "declare",
    "dirs",
    "disown",
    "do",
    "done",
    "echo",
    "elif",
    "else",
    "enable",
    "esac",
    "eval",
    "exec",
    "exit",
    "export",
    "false",
    "fc",
    "fg",
    "fi",
    "for",
    "function",
    "getopts",
    "hash",
    "help",
    "history",
    "if",
    "in",
    "jobs",
    "kill",
    "let",
    "local",
    "logout",
    "mapfile",
    "popd",
    "printf",
    "pushd",
    "pwd",
    "read",
    "readarray",
    "readonly",
    "return",
    "select",
    "set",
    "shift",
    "shopt",
    "source",
    "suspend",
    "test",
    "then",
    "time",
    "times",
    "trap",
    "true",
    "type",
    "typeset",
    "ulimit",
    "umask",
    "unalias",
    "unset",
    "until",
    "wait",
    "while",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandSource {
    InteractiveShell,
    /// The daemon's `PATH`, which systemd reads from the settings env file. An explicit path in
    /// a profile command counts as this too: no shell is asked about it.
    ServicePath,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCommand {
    pub program: PathBuf,
    pub source: CommandSource,
    /// Always `None` from a lookup, which never starts the program. [`version`] asks a CLI for
    /// it: only the version token of `--version`, never its other output.
    pub version: Option<String>,
}

impl ResolvedCommand {
    /// The folder a start script puts first on `PATH`, so that `#!/usr/bin/env node` tools find
    /// the interpreter installed next to them. `None` when `PATH` cannot hold it.
    pub fn dir(&self) -> Option<&str> {
        self.program
            .parent()?
            .to_str()
            .filter(|dir| !dir.is_empty() && !dir.contains(':'))
    }
}

/// What the interactive shell answered about one program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellAnswer {
    /// The shell ran the lookup and printed both markers. Between them: an absolute path, or
    /// `None` when `command -v` found nothing or something that is not a file (an alias, a
    /// function, a builtin).
    Answered(Option<PathBuf>),
    /// No answer: the shell timed out, could not start, printed more than 64 KiB, or ended
    /// without its markers.
    NoAnswer,
}

/// Asks the user's interactive shell where a command is.
pub trait ShellProbe: Send + Sync {
    /// The shell's answer for `command -v -- <program>`. Nothing else of what the shell prints
    /// leaves the probe.
    fn command_v<'a>(&'a self, program: &'a str) -> BoxFuture<'a, ShellAnswer>;
}

impl<T: ShellProbe + ?Sized> ShellProbe for Arc<T> {
    fn command_v<'a>(&'a self, program: &'a str) -> BoxFuture<'a, ShellAnswer> {
        (**self).command_v(program)
    }
}

/// Runs `bash -ic 'printf "\036%s\036" "$(command -v -- "$1")"' devbox-probe <program>` and
/// nothing else: stdin `/dev/null`, stderr discarded, 5 seconds, 64 KiB of stdout.
pub struct InteractiveShell;

impl ShellProbe for InteractiveShell {
    fn command_v<'a>(&'a self, program: &'a str) -> BoxFuture<'a, ShellAnswer> {
        let mut command = Command::new("bash");
        command.args(["-ic", SCRIPT, "devbox-probe", program]);
        Box::pin(async move {
            // The exit status says nothing here: the markers are the answer.
            answer(run(command, SHELL_TIMEOUT).await.map(|(_, stdout)| stdout))
        })
    }
}

/// The answer in the shell's stdout: the value between its last two markers, passed on only
/// when it is an absolute path. Output without both markers is no answer; so is none at all (a
/// timeout, a failed start, more than 64 KiB).
fn answer(stdout: Option<Vec<u8>>) -> ShellAnswer {
    let Some(stdout) = stdout.filter(|stdout| stdout.len() <= OUTPUT_LIMIT) else {
        return ShellAnswer::NoAnswer;
    };
    let Some(end) = stdout.iter().rposition(|&b| b == MARKER) else {
        return ShellAnswer::NoAnswer;
    };
    let Some(start) = stdout[..end].iter().rposition(|&b| b == MARKER) else {
        return ShellAnswer::NoAnswer;
    };
    let path = std::str::from_utf8(&stdout[start + 1..end])
        .ok()
        .filter(|text| !text.chars().any(char::is_control))
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());
    ShellAnswer::Answered(path)
}

/// The program a profile command starts, when it can be found the way a shell would: a plain
/// name or an absolute path as the first word. Anything else (assignments, quotes, expansions,
/// relative paths, shell keywords and builtins) runs as written, unresolved.
pub fn program_of(command: &str) -> Option<&str> {
    let word = command.split_ascii_whitespace().next()?;
    let plain = word
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"._+-/".contains(&b));
    let absolute_or_name = !word.contains('/') || word.starts_with('/');
    (plain && absolute_or_name && !word.starts_with('-') && !SHELL_WORDS.contains(&word))
        .then_some(word)
}

/// Where `program` is, kept in the process's [shared](LookupCache::shared) cache: see
/// [`LookupCache::resolve`].
pub async fn resolve(
    program: &str,
    service_path: &OsStr,
    probe: &impl ShellProbe,
) -> Option<ResolvedCommand> {
    LookupCache::shared()
        .resolve(program, service_path, probe)
        .await
}

/// Lookups already made, each reused for 5 minutes. The daemon keeps one for the whole process,
/// [`LookupCache::shared`]: task starts read it and the environment check refreshes it. A test
/// makes its own, with its own clock.
pub struct LookupCache {
    entries: Mutex<HashMap<Key, Entry>>,
    clock: Clock,
}

impl Default for LookupCache {
    fn default() -> Self {
        Self::new()
    }
}

impl LookupCache {
    /// An empty cache on the runtime's clock.
    pub fn new() -> Self {
        Self {
            entries: Mutex::default(),
            clock: Clock::Runtime,
        }
    }

    /// The process's cache. Task starts and the environment check both use it, so a check's
    /// fresh look is what the next start finds.
    pub fn shared() -> Arc<Self> {
        static SHARED: OnceLock<Arc<LookupCache>> = OnceLock::new();
        SHARED.get_or_init(|| Arc::new(Self::new())).clone()
    }

    /// Where `program` is. A path is only checked; a name is asked of the interactive shell
    /// first and looked up in `service_path` after. The result is reused for 5 minutes, unless
    /// the shell gave no answer (it timed out, say): then the service `PATH`'s result serves this
    /// call only, and the next one asks the shell again.
    pub async fn resolve(
        &self,
        program: &str,
        service_path: &OsStr,
        probe: &impl ShellProbe,
    ) -> Option<ResolvedCommand> {
        match self.cached(&(program.to_owned(), service_path.to_owned())) {
            Some(found) => found,
            None => self.refresh(program, service_path, probe).await,
        }
    }

    /// [`LookupCache::resolve`] without the cache read: looks again and, when the shell answered,
    /// keeps the result for the next [`LookupCache::resolve`] with the same `service_path`. The
    /// environment check looks this way, so a CLI installed since the last lookup is used by the
    /// next task start. Without an answer the kept result stays as it was; the service `PATH`'s
    /// result is only returned.
    pub async fn refresh(
        &self,
        program: &str,
        service_path: &OsStr,
        probe: &impl ShellProbe,
    ) -> Option<ResolvedCommand> {
        let lookup = find(program, service_path, probe).await;
        if lookup.keep {
            self.remember(
                (program.to_owned(), service_path.to_owned()),
                lookup.found.clone(),
            );
        }
        lookup.found
    }
}

/// The first executable `program` in the absolute folders of `path`, as a shell finds it.
pub fn search_path(program: &str, path: &OsStr) -> Option<PathBuf> {
    if program.is_empty() || program.contains('/') {
        return None;
    }
    std::env::split_paths(path)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(program))
        .find(|candidate| executable(candidate))
}

/// One lookup's result, and whether it may be kept.
struct Lookup {
    found: Option<ResolvedCommand>,
    /// `false` when the interactive shell gave no answer and the service `PATH` stood in for it:
    /// that result is for this lookup only.
    keep: bool,
}

async fn find(program: &str, service_path: &OsStr, probe: &impl ShellProbe) -> Lookup {
    let found = |program, source| ResolvedCommand {
        program,
        source,
        version: None,
    };
    if program.is_empty() || program.chars().any(char::is_control) {
        return Lookup {
            found: None,
            keep: true,
        };
    }
    if program.contains('/') {
        return Lookup {
            found: usable(Path::new(program)).map(|path| found(path, CommandSource::ServicePath)),
            keep: true,
        };
    }
    let answer = tokio::time::timeout(SHELL_TIMEOUT, probe.command_v(program))
        .await
        .unwrap_or(ShellAnswer::NoAnswer);
    let (shell, keep) = match answer {
        ShellAnswer::Answered(path) => (path.and_then(|path| usable(&path)), true),
        ShellAnswer::NoAnswer => (None, false),
    };
    let found = match shell {
        Some(path) => Some(found(path, CommandSource::InteractiveShell)),
        None => {
            search_path(program, service_path).map(|path| found(path, CommandSource::ServicePath))
        }
    };
    Lookup { found, keep }
}

fn usable(path: &Path) -> Option<PathBuf> {
    let text = path.to_str()?;
    (path.is_absolute() && !text.chars().any(char::is_control) && executable(path))
        .then(|| path.to_path_buf())
}

fn executable(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_file()) && runnable(path)
}

#[cfg(unix)]
fn runnable(path: &Path) -> bool {
    nix::unistd::access(path, nix::unistd::AccessFlags::X_OK).is_ok()
}

#[cfg(not(unix))]
fn runnable(_: &Path) -> bool {
    true
}

/// The version token of `<program> --version`, run now with the program's folder first on
/// `PATH`, as the start script runs it. Only the environment check calls this, and only for the
/// agent CLIs it knows: a task start never runs a program to ask.
pub async fn version(program: &Path, service_path: &OsStr) -> Option<String> {
    let path = program
        .parent()
        .and_then(|dir| {
            let rest = std::env::split_paths(service_path).filter(|p| !p.as_os_str().is_empty());
            std::env::join_paths(std::iter::once(dir.to_path_buf()).chain(rest)).ok()
        })
        .unwrap_or_else(|| service_path.to_owned());
    let mut command = Command::new(program);
    command
        .arg("--version")
        .env("PATH", path)
        .env("NO_COLOR", "1")
        .env("DISABLE_AUTOUPDATER", "1")
        .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1");
    let (success, stdout) = run(command, VERSION_TIMEOUT).await?;
    success
        .then(|| version_token(&String::from_utf8_lossy(&stdout)))
        .flatten()
}

/// The first `1.2.3`-like word, without anything around it.
fn version_token(text: &str) -> Option<String> {
    text.split_whitespace().find_map(|word| {
        let word = word.strip_prefix('v').unwrap_or(word);
        let number: String = word
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        let number = number.trim_end_matches('.');
        let parts: Vec<_> = number.split('.').collect();
        (number.len() <= 32 && parts.len() >= 2 && parts.iter().all(|part| !part.is_empty()))
            .then(|| number.to_owned())
    })
}

/// Runs `command` in a new session with no terminal, no stdin and no stderr, reading at most
/// 64 KiB of stdout within `timeout`. Whatever it started is killed when it ends or is dropped.
async fn run(mut command: Command, timeout: Duration) -> Option<(bool, Vec<u8>)> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    detach(&mut command);
    let mut child = command.spawn().ok()?;
    let group = Group(child.id());
    let mut stdout = child.stdout.take()?;
    let read = async {
        let mut bytes = Vec::new();
        (&mut stdout)
            .take(OUTPUT_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(drop)?;
        if bytes.len() > OUTPUT_LIMIT {
            return Err(());
        }
        Ok(bytes)
    };
    let wait = async {
        let status = child.wait().await.map_err(drop)?;
        // Background jobs it left could hold the pipe open; its own output is complete.
        group.kill();
        Ok(status)
    };
    let (stdout, status) = tokio::time::timeout(timeout, async { tokio::try_join!(read, wait) })
        .await
        .ok()?
        .ok()?;
    Some((status.success(), stdout))
}

/// The process group of a child started by [`detach`] (its id is the child's).
struct Group(Option<u32>);

impl Group {
    fn kill(&self) {
        #[cfg(unix)]
        if let Some(id) = self.0.and_then(|id| i32::try_from(id).ok()) {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(id),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn detach(command: &mut Command) {
    // SAFETY: setsid is async-signal-safe and touches no memory of this process. Without a
    // controlling terminal an interactive bash cannot stop itself on SIGTTIN or take over the
    // daemon's terminal, and everything it starts stays in a group that can be killed.
    unsafe {
        command.pre_exec(|| {
            nix::unistd::setsid()
                .map(drop)
                .map_err(std::io::Error::from)
        });
    }
}

#[cfg(not(unix))]
fn detach(_: &mut Command) {}

type Key = (String, OsString);

struct Entry {
    at: Instant,
    found: Option<ResolvedCommand>,
}

/// Where a [`LookupCache`] reads the time.
enum Clock {
    /// The tokio runtime's clock.
    Runtime,
    /// A clock that stands still until the test moves it with [`LookupCache::advance`].
    #[cfg(test)]
    Stopped(Mutex<Instant>),
}

impl LookupCache {
    fn now(&self) -> Instant {
        match &self.clock {
            Clock::Runtime => Instant::now(),
            #[cfg(test)]
            Clock::Stopped(now) => *now.lock().expect("owned test clock"),
        }
    }

    fn cached(&self, key: &Key) -> Option<Option<ResolvedCommand>> {
        let now = self.now();
        let entries = self.entries.lock().ok()?;
        let entry = entries.get(key)?;
        // A file removed since is looked up again instead of being started.
        let current = entry
            .found
            .as_ref()
            .is_none_or(|found| executable(&found.program));
        (now.saturating_duration_since(entry.at) < CACHE_FOR && current)
            .then(|| entry.found.clone())
    }

    fn remember(&self, key: Key, found: Option<ResolvedCommand>) {
        let now = self.now();
        if let Ok(mut entries) = self.entries.lock() {
            entries.retain(|_, entry| now.saturating_duration_since(entry.at) < CACHE_FOR);
            entries.insert(key, Entry { at: now, found });
        }
    }
}

#[cfg(test)]
impl LookupCache {
    /// An empty cache whose clock stands still until [`LookupCache::advance`] moves it.
    pub(crate) fn stopped() -> Self {
        Self {
            entries: Mutex::default(),
            clock: Clock::Stopped(Mutex::new(Instant::now())),
        }
    }

    /// Moves this cache's stopped clock, and no other.
    pub(crate) fn advance(&self, by: Duration) {
        if let Clock::Stopped(now) = &self.clock {
            *now.lock().expect("owned test clock") += by;
        }
    }
}

/// Unit tests answer every lookup with an owned executable; they never run the user's shell.
#[cfg(test)]
pub(crate) fn fixture_shell(home: &Path) -> Arc<dyn ShellProbe> {
    struct Fixed(PathBuf);
    impl ShellProbe for Fixed {
        fn command_v<'a>(&'a self, _: &'a str) -> BoxFuture<'a, ShellAnswer> {
            Box::pin(async move { ShellAnswer::Answered(Some(self.0.clone())) })
        }
    }
    let dir = home.join("fixture-bin");
    std::fs::create_dir_all(&dir).expect("owned fixture folder");
    let tool = dir.join("agent-cli");
    // A lookup must never start it: running it leaves `agent-cli.invoked` behind.
    std::fs::write(&tool, "#!/bin/sh\n: > \"$0.invoked\"\n").expect("owned fixture tool");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o700))
            .expect("owned fixture mode");
    }
    Arc::new(Fixed(tool))
}

#[cfg(all(test, unix))]
#[path = "command_path_tests.rs"]
mod tests;
