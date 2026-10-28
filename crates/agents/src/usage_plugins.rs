//! Token usage of agent CLIs whose sessions only the CLI itself reads back.
use crate::{
    model::{AgentTask, AgentUsage, Tokens},
    profile::ToolProfile,
    service::AgentsService,
};
use serde::Deserialize;
use std::{
    collections::HashMap,
    ffi::OsStr,
    path::Path,
    sync::{Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};

/// One opencode export gets this long, and at most this much of its stdout is read.
const EXPORT_TIMEOUT: Duration = Duration::from_secs(10);
const EXPORT_LIMIT: usize = 64 * 1024 * 1024;
/// How long a failed export stands for the usage before it is tried again.
const FAILURE_KEPT: Duration = Duration::from_secs(30);

/// What usage reads of `opencode export` output; everything else in it is ignored.
#[derive(Deserialize)]
struct Export {
    info: ExportedSession,
    messages: Vec<ExportedMessage>,
}

#[derive(Deserialize)]
struct ExportedSession {
    id: String,
    directory: String,
}

#[derive(Deserialize)]
struct ExportedMessage {
    info: MessageInfo,
}

#[derive(Deserialize)]
struct MessageInfo {
    role: String,
    /// A message that failed before the model answered has none.
    tokens: Option<MessageTokens>,
}

/// opencode 1.19.3 keeps reasoning apart from the visible output, and input apart from the
/// cached input.
#[derive(Deserialize)]
struct MessageTokens {
    input: u64,
    output: u64,
    #[serde(default)]
    reasoning: u64,
    #[serde(default)]
    cache: CacheTokens,
}

#[derive(Default, Deserialize)]
struct CacheTokens {
    read: u64,
    write: u64,
}

/// The tokens of the assistant messages in `opencode export` output, reasoning counted as
/// output. Output that is cut off or malformed, or that belongs to another session or folder,
/// is reported as unread rather than read as complete usage.
pub(crate) fn opencode_tokens(export: &[u8], session: &str, workdir: &str) -> AgentUsage {
    let unread = AgentUsage {
        tokens: None,
        truncated: true,
    };
    let Ok(export) = serde_json::from_slice::<Export>(export) else {
        return unread;
    };
    if export.info.id != session || export.info.directory != workdir {
        return unread;
    }
    let mut sum: Option<Tokens> = None;
    let mut truncated = false;
    let mut add = |total: &mut u64, value: u64| {
        *total = total.checked_add(value).unwrap_or_else(|| {
            truncated = true;
            u64::MAX
        });
    };
    for message in export.messages {
        let (role, Some(tokens)) = (message.info.role, message.info.tokens) else {
            continue;
        };
        if role != "assistant" {
            continue;
        }
        let total = sum.get_or_insert_with(Tokens::default);
        add(&mut total.input, tokens.input);
        add(&mut total.output, tokens.output);
        add(&mut total.output, tokens.reasoning);
        add(&mut total.cache_read, tokens.cache.read);
        add(&mut total.cache_write, tokens.cache.write);
    }
    AgentUsage {
        tokens: sum,
        truncated,
    }
}

/// `<program> export --pure <session>` in `workdir`, with the `PATH` the start script gives it.
/// `--pure` keeps the user's plugins from loading for a read; the export is the same.
pub(crate) async fn opencode_export(
    program: &Path,
    session: &str,
    workdir: &Path,
    service_path: &OsStr,
    timeout: Duration,
    limit: usize,
) -> Option<Vec<u8>> {
    let mut command = tokio::process::Command::new(program);
    command
        .args(["export", "--pure", session])
        .current_dir(workdir)
        .env(
            "PATH",
            crate::command_path::tool_path(program, service_path),
        )
        .env("NO_COLOR", "1")
        .env("OPENCODE_DISABLE_AUTOUPDATE", "1")
        .env("OPENCODE_DISABLE_MODELS_FETCH", "1");
    let (success, stdout) = crate::bounded::run(command, timeout, limit).await?;
    success.then_some(stdout)
}

/// An id the way opencode makes them (`ses_…`): never empty, never read as an option.
fn exportable(session: &str) -> bool {
    session.len() <= 128
        && session.starts_with(|c: char| c.is_ascii_alphanumeric())
        && session
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Usage read from exports, per task, kept until the task finishes another turn. A failed
/// read is kept for 30 seconds only.
#[derive(Default)]
pub(crate) struct ExportCache {
    tasks: Mutex<HashMap<String, TaskExport>>,
    /// One export runs at a time.
    running: tokio::sync::Mutex<()>,
}

#[derive(Default)]
struct TaskExport {
    /// Turns the task finished since its usage was first read.
    turns: u64,
    read: Option<Read>,
}

struct Read {
    session: String,
    /// The turn count it was read at.
    turns: u64,
    usage: AgentUsage,
    /// When the read failed, the time to try again.
    retry_at: Option<Instant>,
}

/// An export that could not be read at all, as opposed to one that was read.
fn failed(usage: &AgentUsage) -> bool {
    usage.tokens.is_none() && usage.truncated
}

impl ExportCache {
    fn tasks(&self) -> MutexGuard<'_, HashMap<String, TaskExport>> {
        self.tasks.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The task finished a turn, which may have added tokens.
    pub(crate) fn turn_completed(&self, task: &str) {
        if let Some(entry) = self.tasks().get_mut(task) {
            entry.turns = entry.turns.wrapping_add(1);
        }
    }

    /// The usage last read for `session`, when the task has finished no turn since. A failed
    /// read counts until it is 30 seconds old, and not at all when `retry_failure` is set.
    fn get(&self, task: &str, session: &str, retry_failure: bool) -> Option<AgentUsage> {
        let tasks = self.tasks();
        let entry = tasks.get(task)?;
        let read = entry.read.as_ref().filter(|read| {
            read.session == session
                && read.turns == entry.turns
                && read
                    .retry_at
                    .is_none_or(|at| !retry_failure && Instant::now() < at)
        })?;
        Some(read.usage.clone())
    }

    /// The turns the task has finished; from now on each one is counted.
    fn turns(&self, task: &str) -> u64 {
        self.tasks().entry(task.to_owned()).or_default().turns
    }

    /// Ages the task's failed read past its 30 seconds.
    #[cfg(test)]
    pub(crate) fn expire_failures(&self, task: &str) {
        if let Some(read) = self.tasks().get_mut(task).and_then(|e| e.read.as_mut()) {
            read.retry_at = read.retry_at.map(|_| Instant::now());
        }
    }

    fn store(&self, task: &str, session: &str, turns: u64, usage: &AgentUsage) {
        if let Some(entry) = self.tasks().get_mut(task) {
            entry.read = Some(Read {
                session: session.to_owned(),
                turns,
                usage: usage.clone(),
                retry_at: failed(usage).then(|| Instant::now() + FAILURE_KEPT),
            });
        }
    }
}

impl AgentsService {
    /// The task's opencode session usage, exported with the CLI the task runs, once per
    /// session and finished turn. `closing` tries a failed read once more, since a closed task
    /// keeps what is read now.
    pub(crate) async fn opencode_usage(
        &self,
        task: &AgentTask,
        profile: &ToolProfile,
        closing: bool,
    ) -> AgentUsage {
        let Some(session) = task.session_id.as_deref().filter(|id| exportable(id)) else {
            return AgentUsage::default();
        };
        // Another program could read `export` as something else to run.
        let runs_opencode = crate::command_path::program_of(&profile.command)
            .and_then(|program| Path::new(program).file_name())
            .is_some_and(|name| name == "opencode");
        if !runs_opencode {
            return AgentUsage::default();
        }
        if let Some(usage) = self.exports.get(&task.id, session, closing) {
            return usage;
        }
        let _one_export = self.exports.running.lock().await;
        if let Some(usage) = self.exports.get(&task.id, session, closing) {
            return usage;
        }
        // Counted before the export starts, so a turn that ends meanwhile reads again.
        let turns = self.exports.turns(&task.id);
        let output = match self.resolve_tool(profile).await {
            Ok(Some(tool)) => {
                let service_path = std::env::var_os("PATH").unwrap_or_default();
                let workdir = Path::new(&task.workdir);
                opencode_export(
                    &tool.program,
                    session,
                    workdir,
                    &service_path,
                    EXPORT_TIMEOUT,
                    EXPORT_LIMIT,
                )
                .await
            }
            _ => None,
        };
        let usage = match output {
            Some(output) => opencode_tokens(&output, session, &task.workdir),
            None => AgentUsage {
                tokens: None,
                truncated: true,
            },
        };
        self.exports.store(&task.id, session, turns, &usage);
        usage
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const EXPORT: &str = include_str!("../tests/fixtures/opencode-1.19.3/export.json");
    const WORKDIR: &str = "/tmp/devbox-opencode-fixture/repo";
    type Read = (Option<Tokens>, bool);
    fn usage(tokens: Option<Tokens>, truncated: bool) -> Read {
        (tokens, truncated)
    }
    /// `opencode_tokens` as (tokens, truncated).
    fn tokens(export: &[u8], session: &str, workdir: &str) -> Read {
        let usage = opencode_tokens(export, session, workdir);
        (usage.tokens, usage.truncated)
    }
    #[test]
    fn opencode_export_fixture_sums_assistant_tokens() {
        // Two assistant messages: the task tool call and the final reply. The subagent's
        // tokens live in its own session, and step-finish parts repeat their message's.
        let summed = Tokens {
            input: 1160 + 1340,
            output: 11 + 13,
            cache_read: 240 + 260,
            cache_write: 0,
        };
        assert_eq!(
            tokens(EXPORT.as_bytes(), "ses_taskroot", WORKDIR),
            usage(Some(summed), false)
        );
        // Another session's export, or one made in another folder, is not this task's.
        for (session, workdir) in [("ses_taskchild", WORKDIR), ("ses_taskroot", "/elsewhere")] {
            assert_eq!(
                tokens(EXPORT.as_bytes(), session, workdir),
                usage(None, true),
                "{session} {workdir}"
            );
        }
        // Cut-off or malformed output is reported, never read as complete usage.
        let cut = &EXPORT.as_bytes()[..EXPORT.len() / 2];
        for broken in [cut, b"", b"Exporting session", b"{\"info\":{}}"] {
            assert_eq!(tokens(broken, "ses_taskroot", WORKDIR), usage(None, true));
        }
    }
    #[test]
    fn opencode_reasoning_counts_as_output_and_messages_without_tokens_are_skipped() {
        let export = r#"{"info":{"id":"s","directory":"/w"},"messages":[
            {"info":{"role":"user"},"parts":[{"type":"text","text":"hi"}]},
            {"info":{"role":"assistant","tokens":{"input":1,"output":2,"reasoning":9,"cache":{"read":3,"write":4}},"cost":0.5}},
            {"info":{"role":"assistant","tokens":{"input":10,"output":20,"cache":{"read":30,"write":40}}}},
            {"info":{"role":"assistant","error":{"name":"APIError"}}}]}"#;
        assert_eq!(
            tokens(export.as_bytes(), "s", "/w"),
            usage(
                Some(Tokens {
                    input: 11,
                    output: 2 + 9 + 20,
                    cache_read: 33,
                    cache_write: 44,
                }),
                false
            )
        );
        // A session that has not answered yet has no usage, which is complete.
        let fresh = r#"{"info":{"id":"s","directory":"/w"},"messages":[{"info":{"role":"user"}}]}"#;
        assert_eq!(tokens(fresh.as_bytes(), "s", "/w"), usage(None, false));
        // Sums saturate instead of wrapping.
        let huge = format!(
            r#"{{"info":{{"id":"s","directory":"/w"}},"messages":[{0},{0}]}}"#,
            r#"{"info":{"role":"assistant","tokens":{"input":18446744073709551615,"output":0,"cache":{"read":0,"write":0}}}}"#
        );
        let (summed, truncated) = tokens(huge.as_bytes(), "s", "/w");
        assert!(truncated);
        assert_eq!(summed.expect("tokens").input, u64::MAX);
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn opencode_export_runs_the_found_cli_with_bounded_output() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().expect("owned export fixture");
        let tools = root.path().join("tools");
        std::fs::create_dir(&tools).expect("owned tools folder");
        let cli = tools.join("opencode");
        // Records its arguments, its PATH and whether stdin had anything, then answers.
        std::fs::write(
            &cli,
            "#!/bin/sh\nprintf '%s|' \"$@\" > args\nprintf '%s' \"$PATH\" > path\ncat > stdin\npwd > cwd\nprintf '%s' \"$OPENCODE_DISABLE_AUTOUPDATE:$OPENCODE_DISABLE_MODELS_FETCH\" > env\ncase \"$2 $3\" in\n  '--pure slow') sleep 5 ;;\n  '--pure loud') head -c 4096 /dev/zero ;;\n  *) printf '{\"ok\":1}' ;;\nesac\n",
        )
        .expect("owned fake CLI");
        std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o700)).expect("owned mode");
        let work = root.path().join("work");
        std::fs::create_dir(&work).expect("owned work folder");
        let run = |session: &'static str, timeout: Duration| {
            let (cli, work) = (cli.clone(), work.clone());
            async move {
                opencode_export(
                    &cli,
                    session,
                    &work,
                    OsStr::new("/usr/bin:/bin"),
                    timeout,
                    1024,
                )
                .await
            }
        };
        assert_eq!(
            run("ses_1", Duration::from_secs(5)).await.as_deref(),
            Some(&b"{\"ok\":1}"[..])
        );
        let read = |name: &str| std::fs::read_to_string(work.join(name)).expect("receipt");
        assert_eq!(read("args"), "export|--pure|ses_1|");
        assert_eq!(read("path"), format!("{}:/usr/bin:/bin", tools.display()));
        assert_eq!(read("stdin"), "");
        assert_eq!(read("cwd").trim_end(), work.to_str().expect("UTF-8"));
        // A usage read neither updates opencode nor fetches its model list.
        assert_eq!(read("env"), "1:1");
        let started = std::time::Instant::now();
        assert!(run("slow", Duration::from_millis(300)).await.is_none());
        assert!(started.elapsed() < Duration::from_secs(4));
        assert!(
            run("loud", Duration::from_secs(5)).await.is_none(),
            "over the limit"
        );
    }
}
