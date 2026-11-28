//! Claude may exit successfully before it ever writes a conversation. Use the transcript named
//! by this launch's SessionStart hook, or the exact CLI rejection in a failed resume's terminal.
//! A missing guessed path, an unreadable directory or a generic exit code is not evidence.
use crate::{
    model::{AgentTask, RecoverConversation},
    profile::Hooks,
    service::{AgentsService, Result, now_ms},
    state::{AgentEvent, AgentState, CloseHow},
    tool_lifecycle::Phase,
    tracker::Binding,
};
use serde::Deserialize;
use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
};

pub(crate) const REASON: &str = "claude_conversation_missing";
pub(crate) const MESSAGE: &str = "이 작업에서 재개할 Claude Code 대화 기록을 찾지 못했습니다.";

fn projects(home: &Path) -> Option<PathBuf> {
    config_projects(
        home,
        std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from),
    )
}
fn config_projects(home: &Path, configured: Option<PathBuf>) -> Option<PathBuf> {
    let config = configured.unwrap_or_else(|| home.join(".claude"));
    config.is_absolute().then(|| config.join("projects"))
}

#[derive(Deserialize)]
struct SessionStart<'a> {
    hook_event_name: &'a str,
    cwd: &'a str,
    session_id: &'a str,
    transcript_path: &'a str,
}

pub(crate) fn transcript(home: &Path, task: &AgentTask, payload: &str) -> Option<PathBuf> {
    let h: SessionStart<'_> = serde_json::from_str(payload).ok()?;
    if h.hook_event_name != "SessionStart"
        || h.cwd != task.workdir
        || h.transcript_path.len() > 4096
        || crate::limits::identity(h.session_id).is_err()
    {
        return None;
    }
    let file = Path::new(h.transcript_path);
    let root = projects(home)?;
    checked_name(&root, file, h.session_id).then(|| file.to_owned())
}

fn checked_name(root: &Path, file: &Path, session: &str) -> bool {
    let Ok(rel) = file.strip_prefix(root) else {
        return false;
    };
    let mut parts = rel.components();
    matches!(parts.next(), Some(Component::Normal(_)))
        && matches!(parts.next(), Some(Component::Normal(name)) if name == std::ffi::OsStr::new(&format!("{session}.jsonl")))
        && parts.next().is_none()
        && !file.components().any(|p| matches!(p, Component::ParentDir))
}

fn transcript_absent(root: &Path, named: Option<&Path>, session: &str) -> bool {
    // A legacy task may have no recorded transcript path. Its CLI rejection can be checked
    // against the configured root, but the absent file alone never diagnoses such a task.
    let parent = if let Some(path) = named {
        if !checked_name(root, path, session) {
            return false;
        }
        path.parent().expect("checked filename")
    } else {
        root
    };
    for dir in parent.ancestors() {
        match std::fs::symlink_metadata(dir) {
            Ok(meta) if meta.is_dir() => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            _ => return false,
        }
    }
    // Claude can move a transcript between project folders (e.g. worktrees). Fall back to its
    // exact-ID lookup if another folder holds it, or we cannot complete this bounded check.
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) => return e.kind() == std::io::ErrorKind::NotFound,
    };
    let name = format!("{session}.jsonl");
    for (i, entry) in entries.enumerate() {
        if i >= 2048 {
            return false;
        }
        let Ok(entry) = entry else { return false };
        let Ok(kind) = entry.file_type() else {
            return false;
        };
        if kind.is_symlink() {
            return false;
        }
        if kind.is_dir()
            && !matches!(std::fs::symlink_metadata(entry.path().join(&name)), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
        {
            return false;
        }
    }
    true
}

/// Update the bound evidence on a Claude hook. A resume must confirm the requested id;
/// a fresh blank launch has no old id and becomes idle on its own startup hook.
pub(crate) fn session_start(
    task: &AgentTask,
    binding: &mut Binding,
    events: &mut Vec<AgentEvent>,
    source: Option<&str>,
    named: Option<PathBuf>,
) -> std::result::Result<bool, ()> {
    let started = events.iter().find_map(|e| match e {
        AgentEvent::SessionStarted { session_id } => Some(session_id.as_str()),
        _ => None,
    });
    let must_confirm =
        binding.unconfirmed_session || (binding.resumed_session && task.session_id.is_some());
    if must_confirm && started.is_some() && started != task.session_id.as_deref() {
        return Err(());
    }
    let confirmed = binding.unconfirmed_session && started.is_some();
    if confirmed {
        binding.unconfirmed_session = false;
    }
    let path_changed = started.is_some() && binding.claude_transcript != named;
    if path_changed {
        binding.claude_transcript = named;
    }
    if events.contains(&AgentEvent::PromptSubmitted) {
        binding.resumed_session = false;
    } else if started.is_some()
        && binding.awaits_first_instruction(task)
        && (source == Some("resume") || (task.session_id.is_none() && source == Some("startup")))
    {
        binding.resumed_session = false;
        events.push(AgentEvent::TurnComplete { message: None });
    }
    Ok(path_changed || confirmed)
}

// The exit hook runs before the holder prints its footer; a later recovery may see either
// footer line as well. A diagnostic quoted in unrelated output is not a CLI rejection.
fn rejected_screen(text: &str, session: &str) -> bool {
    const EXIT: &str = "[devbox] 에이전트가 끝났습니다(종료 코드 1).";
    const OLD_END: &str = "[devbox] 작업이 끝났습니다. 재개 버튼으로 다시 시작하세요.";
    const END: &str = "[devbox] 작업이 끝났습니다. 에이전트 화면에서 다음 동작을 선택하세요.";
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    lines.next() == Some(format!("No conversation found with session ID: {session}").as_str())
        && match lines.next() {
            None => true,
            Some(EXIT) => {
                matches!(lines.next(), None | Some(OLD_END | END)) && lines.next().is_none()
            }
            _ => false,
        }
}

impl AgentsService {
    pub(crate) fn claude(&self, t: &AgentTask) -> bool {
        crate::profile::resolve(&self.profiles, &t.profile)
            .is_some_and(|p| p.hooks == Hooks::Claude)
    }
    pub(crate) async fn missing_conversation_at_exit(
        &self,
        t: &AgentTask,
        b: &Binding,
        code: i32,
    ) -> bool {
        if !self.claude(t) || !matches!(code, 0 | 1) {
            return false;
        }
        let Some(id) = t.session_id.as_ref() else {
            return code == 0;
        };
        if crate::limits::identity(id).is_err() {
            return false;
        }
        if code == 1 {
            if !b.resumed_session {
                return false;
            }
            let Some(terminal) = &t.terminal_id else {
                return false;
            };
            let Ok(text) = self
                .terminal
                .tmux()
                .capture_tail(terminal, crate::watch::TAIL_ROWS)
                .await
            else {
                return false;
            };
            if !rejected_screen(&text, id) {
                return false;
            }
        } else if b.claude_transcript.is_none() {
            return false;
        }
        let Some(root) = projects(&self.paths.home) else {
            return false;
        };
        let named = b.claude_transcript.clone();
        let id = id.clone();
        tokio::task::spawn_blocking(move || transcript_absent(&root, named.as_deref(), &id))
            .await
            .unwrap_or(false)
    }

    pub(crate) async fn classify_claude_exit(
        &self,
        t: &AgentTask,
        b: &Binding,
        event: AgentEvent,
    ) -> AgentEvent {
        if let AgentEvent::Exited { code } = event
            && self.missing_conversation_at_exit(t, b, code).await
        {
            return AgentEvent::ClaudeConversationMissing { failed: code != 0 };
        }
        event
    }

    /// Upgrade already-ended tasks once on startup. A concurrent resume or close wins the lock.
    pub(crate) async fn reconcile_claude_conversation(&self, previous: &AgentTask) -> Result<()> {
        if !previous.tool_exited
            || previous.state == AgentState::Closed
            || previous.reason.as_deref() == Some(REASON)
            || !self.claude(previous)
        {
            return Ok(());
        }
        let _guard = self.actions.lock().await;
        let t = self.get(&previous.id).await?;
        if t != *previous {
            return Ok(());
        }
        if let Some(Phase::Ended(AgentEvent::Exited { code })) = self.tool_phase(&t).await?
            && self
                .missing_conversation_at_exit(&t, &self.launch_binding(&t.id).await?, code)
                .await
        {
            self.mark_conversation_missing(&t.id, code != 0).await?;
        }
        Ok(())
    }
    pub(crate) async fn mark_conversation_missing(&self, id: &str, failed: bool) -> Result<()> {
        self.update(id, move |t| {
            crate::events::apply_event(
                t,
                &AgentEvent::ClaudeConversationMissing { failed },
                now_ms(),
            )
        })
        .await?;
        Ok(())
    }
    pub async fn recover_conversation(
        self: &Arc<Self>,
        request: RecoverConversation,
    ) -> Result<AgentTask> {
        let id = request.id.clone();
        self.launch_after_exit(&id, Some(request)).await
    }

    pub(crate) async fn archive_conversation(&self, task: &AgentTask) -> Result<AgentTask> {
        // The script has recorded its exit, and the lifecycle lock excludes a replacement launch.
        // Retain the resource binding and all Git files. Only the old terminal holder is removed.
        if let Some(terminal) = &task.terminal_id {
            self.terminal.close_internal(terminal).await.map_err(|e| {
                devbox_protocol::Fail::Internal(format!("close ended holder: {e:?}"))
            })?;
        }
        self.update(&task.id, |t| {
            let now = now_ms();
            t.closed_how = Some(CloseHow::Archived);
            t.closed_ms = Some(now);
            t.terminal_id = None;
            crate::events::apply_event(
                t,
                &AgentEvent::Closed {
                    how: CloseHow::Archived,
                },
                now,
            )
        })
        .await?;
        self.get(&task.id).await
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn custom_root_is_used_only_if_absolute() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let config = dir.path().join("custom");
        let root = config_projects(&home, Some(config.clone())).unwrap();
        assert_eq!(root, config.join("projects"));
        assert!(config_projects(&home, Some("relative".into())).is_none());
        assert!(!checked_name(
            &root,
            &home.join(".claude/projects/other/id.jsonl"),
            "id"
        ));
        assert!(transcript_absent(
            &root,
            Some(&root.join("work/id.jsonl")),
            "id"
        ));
    }
    #[test]
    fn a_moved_record_or_incomplete_lookup_never_marks_a_conversation_missing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("projects");
        let path = root.join("original/id.jsonl");
        assert!(transcript_absent(&root, Some(&path), "id"));
        assert!(!transcript_absent(&root, Some(&path), "different"));
        std::fs::create_dir_all(root.join("moved")).unwrap();
        std::fs::write(root.join("moved/id.jsonl"), b"opaque").unwrap();
        assert!(!transcript_absent(&root, Some(&path), "id"));
        std::fs::remove_file(root.join("moved/id.jsonl")).unwrap();
        std::os::unix::fs::symlink(&root, root.join("link")).unwrap();
        assert!(!transcript_absent(&root, Some(&path), "id"));
    }
}
