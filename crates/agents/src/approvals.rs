//! An approval wait ends only when the tool call it was for ends.
//!
//! Claude's PermissionRequest records a digest of its tool call. PostToolUse,
//! PostToolUseFailure and PermissionDenied for the same call remove it, and the wait
//! resumes when none is left. Digests live only in this process and are never stored
//! or logged. A wait that no recorded request began (a notification-only wait, a
//! dropped request, or records a daemon restart lost) may hold a dialog this process
//! never saw. It fails closed, even if requests follow: only a prompt, a finished turn
//! or a lifecycle event can end it.
use crate::{
    model::AgentTask,
    service::{apply_event, now_ms},
    state::{AgentEvent, AgentState},
};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard, PoisonError},
};

/// SHA-256 over (Claude `agent_id` or empty, `tool_name`, canonical `tool_input`).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ApprovalKey([u8; 32]);
impl std::fmt::Debug for ApprovalKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApprovalKey(..)")
    }
}

/// What a Claude hook says about an approval wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approval {
    /// PermissionRequest: the wait is for this tool call.
    Requested(ApprovalKey),
    /// PostToolUse, PostToolUseFailure or PermissionDenied: this tool call is over.
    Finished(ApprovalKey),
}

/// Key order and whitespace in `tool_input` do not matter.
pub fn key(agent: &str, tool: &str, input: &serde_json::Value) -> ApprovalKey {
    let mut canonical = Vec::new();
    write_canonical(input, &mut canonical);
    let mut hash = Sha256::new();
    for part in [agent.as_bytes(), tool.as_bytes(), &canonical] {
        hash.update((part.len() as u64).to_le_bytes());
        hash.update(part);
    }
    ApprovalKey(hash.finalize().into())
}

fn write_canonical(value: &serde_json::Value, out: &mut Vec<u8>) {
    use serde_json::Value;
    match value {
        Value::Object(map) => {
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_unstable_by(|a, b| a.0.cmp(b.0));
            out.push(b'{');
            for (index, (name, item)) in entries.into_iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                write_json(name, out);
                out.push(b':');
                write_canonical(item, out);
            }
            out.push(b'}');
        }
        Value::Array(items) => {
            out.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                write_canonical(item, out);
            }
            out.push(b']');
        }
        scalar => write_json(scalar, out),
    }
}

/// Strings and scalars keep serde_json's escaping and number format.
fn write_json(value: &impl serde::Serialize, out: &mut Vec<u8>) {
    serde_json::to_writer(out, value).expect("JSON values serialize into memory");
}

struct Pending {
    launch: String,
    /// The activity sequence the hook path last saw. Another launch, or any other
    /// change to the task, makes this record stale.
    seq: u64,
    /// A count, so identical concurrent requests each need their own ending.
    keys: HashMap<ApprovalKey, u32>,
}

/// Per-task pending approvals, held by the agents service.
#[derive(Default)]
pub(crate) struct Approvals(Mutex<HashMap<String, Pending>>);
impl Approvals {
    fn pending(&self) -> MutexGuard<'_, HashMap<String, Pending>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
    /// Applies one Claude hook inside its write transaction.
    pub(crate) fn apply(
        &self,
        task: &mut AgentTask,
        launch: &str,
        approval: Option<Approval>,
        events: Vec<AgentEvent>,
    ) -> bool {
        let mut pending = self.pending();
        if pending.get(&task.id).is_some_and(|wait| {
            wait.launch != launch
                || wait.seq != task.activity_seq
                || task.state != AgentState::Waiting
        }) {
            pending.remove(&task.id);
        }
        // A request records its approval only if it begins the wait or joins a wait
        // whose approvals are all recorded.
        let tracked = task.state != AgentState::Waiting || pending.contains_key(&task.id);
        let mut changed = false;
        for event in events {
            // A finished tool call resumes only a wait whose last approval it was.
            if event == AgentEvent::OutputResumed {
                let Some(Approval::Finished(key)) = approval else {
                    continue;
                };
                if !finish(&mut pending, &task.id, &key) {
                    continue;
                }
            }
            changed |= apply_event(task, &event, now_ms());
        }
        if task.state != AgentState::Waiting {
            pending.remove(&task.id);
            return changed;
        }
        if let (Some(Approval::Requested(key)), true) = (approval, tracked) {
            let wait = pending.entry(task.id.clone()).or_insert_with(|| Pending {
                launch: launch.into(),
                seq: task.activity_seq,
                keys: HashMap::new(),
            });
            *wait.keys.entry(key).or_default() += 1;
        }
        if let Some(wait) = pending.get_mut(&task.id) {
            wait.seq = task.activity_seq;
        }
        changed
    }
    /// Leaving Waiting by any other path ends the wait's approvals.
    pub(crate) fn forget(&self, id: &str) {
        self.pending().remove(id);
    }
    #[cfg(test)]
    pub(crate) fn has_pending(&self, id: &str) -> bool {
        self.pending().contains_key(id)
    }
}

fn finish(pending: &mut HashMap<String, Pending>, id: &str, key: &ApprovalKey) -> bool {
    let Some(wait) = pending.get_mut(id) else {
        return false;
    };
    let Some(count) = wait.keys.get_mut(key) else {
        return false;
    };
    *count -= 1;
    if *count == 0 {
        wait.keys.remove(key);
    }
    let last = wait.keys.is_empty();
    if last {
        pending.remove(id);
    }
    last
}

#[cfg(test)]
#[path = "approvals_tests.rs"]
mod tests;
