//! What the service does for the Antigravity family: the hooks folder of a launch, the reports of
//! its hooks, and the folder's removal with the work folder.
use crate::{
    antigravity,
    model::{AgentEventParams, AgentTask, AgentsError},
    profile::{Hooks, ToolProfile},
    service::{AgentsService, Result, apply_event, now_ms},
    state::AgentEvent,
};
use devbox_protocol::{Done, Fail};
use std::sync::Arc;

/// A task of a tool family that keeps its connection beside the work folder needs one. Checked
/// before anything is reserved or started.
pub(crate) fn work_folder_check(
    profile: &ToolProfile,
    worktree: bool,
) -> std::result::Result<(), AgentsError> {
    if profile.hooks == Hooks::Antigravity && !worktree {
        return Err(AgentsError::WorkFolderRequired {
            tool: profile.label.clone(),
        });
    }
    Ok(())
}

/// A new task of a family that keeps its connection beside the work folder `worktree` cannot
/// start while something of the hooks folder's name is already there: it is another task's, or
/// not devbox's at all. Checked where the work folder is chosen, before anything is created.
pub(crate) fn hooks_folder_free(
    profile: &ToolProfile,
    worktree: Option<&str>,
) -> std::result::Result<(), AgentsError> {
    let Some(worktree) = worktree.filter(|_| profile.hooks == Hooks::Antigravity) else {
        return Ok(());
    };
    let dir = antigravity::hooks_dir(worktree);
    if std::fs::symlink_metadata(&dir).is_ok() {
        return Err(AgentsError::HooksFolderExists {
            path: dir.to_string_lossy().into_owned(),
        });
    }
    Ok(())
}

impl AgentsService {
    /// [`work_folder_check`] for the profile `id`, as a start checks it. A composite launch asks
    /// for each of its agents before it starts anything. An unknown profile passes: the start
    /// reports that.
    pub fn work_folder_required(
        &self,
        id: &str,
        worktree: bool,
    ) -> std::result::Result<(), AgentsError> {
        match crate::profile::resolve(&self.profiles, id) {
            Some(profile) => work_folder_check(&profile, worktree),
            None => Ok(()),
        }
    }

    /// Whether a start of profile `id` would take `prompt`, as a start checks it: a prompt devbox
    /// pastes itself may hold no control character and may not start with `!` or `/`. A composite
    /// launch asks for each of its agents before it starts anything. An unknown profile passes:
    /// the start reports that.
    pub fn admits_prompt(&self, id: &str, prompt: &str) -> Result<()> {
        match crate::profile::resolve(&self.profiles, id) {
            Some(profile) => crate::first_instruction::admit(&profile, prompt),
            None => Ok(()),
        }
    }

    /// What a launch of `task` loads devbox's connection from: the family's file (`hook_file`),
    /// or for Antigravity the hooks folder beside the task's work folder, written now. `fresh` is
    /// the task's first launch, which makes that folder.
    pub(crate) async fn connection(
        &self,
        task: &AgentTask,
        profile: &ToolProfile,
        fresh: bool,
    ) -> Result<Option<String>> {
        if profile.hooks != Hooks::Antigravity {
            return self.hook_file(profile.hooks).await;
        }
        // Never beside the user's own checkout: only a work folder devbox made has the sibling.
        let Some(workdir) = task
            .worktree_path
            .as_deref()
            .filter(|path| *path == task.workdir)
        else {
            return Err(AgentsError::WorkFolderRequired {
                tool: profile.label.clone(),
            }
            .into());
        };
        let dir = antigravity::hooks_dir(workdir);
        let _hook_files = self.hook_files.lock().await;
        antigravity::write_hooks_dir(&dir, &self.devbox_bin, &task.id, fresh).map_err(|error| {
            match error.kind() {
                // A folder of that name that is not this task's: the user removes or renames it.
                std::io::ErrorKind::AlreadyExists => Fail::Domain(AgentsError::HooksFolderExists {
                    path: dir.to_string_lossy().into_owned(),
                }),
                _ => Fail::Internal(format!("agent hooks folder: {error}")),
            }
        })?;
        Ok(Some(dir.to_string_lossy().into_owned()))
    }

    /// One report of agy's hooks. It changes the task only while the launch it names is current.
    ///
    /// The conversation a report names is the task's session. agy opens a new conversation
    /// without a word when the id a resume names is unknown, so on a resumed launch the first
    /// report must name the task's conversation (`Binding::unconfirmed_session`): another one is
    /// never applied and never becomes the task's session, and fails the task instead
    /// (`session_mismatch`) while the tool goes on in its pane. Once the launch's conversation is
    /// known, a report of another one means the user changed conversations inside agy: the task
    /// follows it, with its transcript.
    ///
    /// Any report of the launch also means agy got an instruction, so a first instruction still
    /// queued for it is never sent (`first_instruction`).
    pub(crate) async fn antigravity_event(self: &Arc<Self>, p: AgentEventParams) -> Result<Done> {
        let unchanged = || Done {
            rev: self.db.rev().to_string(),
        };
        let Some(launch) = p.launch_id.filter(|id| !id.is_empty()) else {
            return Ok(unchanged());
        };
        let Some(report) = antigravity::report(&p.payload) else {
            return Ok(unchanged());
        };
        crate::limits::identity(&report.conversation)?;
        let events = antigravity::events(&p.payload);
        // The path is the tool's word: kept only when it lies in the user's agy data.
        let transcript = report
            .transcript
            .as_deref()
            .and_then(|named| antigravity::transcript_path(&self.paths.home, named));
        let (bound, event) = (launch.clone(), report.event);
        // What the report did to the task's session, for the log once the write is in.
        let session = Arc::new(std::sync::Mutex::new(None));
        let said = session.clone();
        let update = self.update_bound(&p.agent_id, Some(launch), false, move |task, binding| {
            let say = |what: &'static str, changed: bool| {
                if changed {
                    *said.lock().unwrap_or_else(|e| e.into_inner()) = Some(what);
                }
                changed
            };
            let known = task.session_id.is_some();
            let other = task
                .session_id
                .as_deref()
                .is_some_and(|known| known != report.conversation);
            if other && binding.as_ref().is_some_and(|b| b.unconfirmed_session) {
                let cancelled = crate::first_instruction::cancel(task, now_ms());
                let failed = apply_event(task, &AgentEvent::SessionMismatch, now_ms());
                return say("another conversation than the resumed one", failed) || cancelled;
            }
            let mut changed = crate::first_instruction::cancel(task, now_ms());
            if let Some(binding) = binding {
                if binding.unconfirmed_session {
                    binding.unconfirmed_session = false;
                    changed = true;
                }
                // The launch's first prompt ends its wait for one (`Binding::resumed_session`).
                if binding.resumed_session && events.contains(&AgentEvent::PromptSubmitted) {
                    binding.resumed_session = false;
                    changed = true;
                }
                // Another conversation's transcript is never kept for this one.
                if binding.transcript != transcript && (other || transcript.is_some()) {
                    binding.transcript = transcript;
                    changed = true;
                }
            }
            // `SessionStarted` makes the report's conversation the task's.
            for event in &events {
                changed |= apply_event(task, event, now_ms());
            }
            match (known, other) {
                (false, _) => say("session recorded", changed),
                (true, true) => say("session rebound to another conversation", changed),
                (true, false) => changed,
            }
        });
        let (done, turned_idle) = update.await?;
        // Logged by ids alone: never a prompt, a path or a payload.
        let (agent, launch) = (&p.agent_id, &bound);
        match session.lock().unwrap_or_else(|e| e.into_inner()).take() {
            Some(what) => tracing::info!(%agent, %launch, ?event, "agy hook: {what}"),
            None => tracing::debug!(%agent, %launch, ?event, "agy hook"),
        }
        // The turn that ended may have added tokens.
        if turned_idle {
            self.refresh_tokens(&p.agent_id);
        }
        Ok(done)
    }

    /// Removes the hooks folder beside `task`'s work folder, which was just removed or is already
    /// gone. Only the folder that names this task goes (`antigravity::remove_hooks_dir`). A
    /// failure is logged and leaves the rest of the cleanup as it is.
    pub(crate) fn remove_agy_hooks(&self, task: &AgentTask) {
        let Some(workdir) = task.worktree_path.as_deref() else {
            return;
        };
        if let Err(error) = antigravity::remove_hooks_dir(workdir, &task.id) {
            tracing::warn!(agent = %task.id, kind = ?error.kind(), "agent hooks folder not removed");
        }
    }
}
