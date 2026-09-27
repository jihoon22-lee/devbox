//! Private per-command admission and confirmed session retirement. These are
//! control frames on an already authenticated inherited pipe, not IPC methods.
use crate::{Request, Response};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum ControlInput {
    #[serde(rename = "execution_cancel")]
    Cancel {
        version: u32,
        session_id: String,
        request_id: String,
        sequence: u64,
    },
    #[serde(rename = "execution_admission_reply")]
    AdmissionReply {
        version: u32,
        session_id: String,
        request_id: String,
        sequence: u64,
        admission_id: String,
        approved: bool,
    },
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum ControlOutput {
    #[serde(rename = "execution_admission")]
    Admission {
        version: u32,
        session_id: String,
        request_id: String,
        sequence: u64,
        admission_id: String,
        target_root: String,
    },
    #[serde(rename = "retired")]
    Retired {
        version: u32,
        session_id: String,
        sequence: u64,
    },
}
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Input {
    Request(Request),
    Control(ControlInput),
}
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Output {
    Response(Response),
    Control(ControlOutput),
}

/// The cursor owns partial framing across a cancelled asynchronous read. A
/// retirement reader resumes from the same prefix/body without guessing where
/// the next JSON frame begins. It also drives the synchronous pipe reader.
#[derive(Default)]
pub struct FrameCursor {
    prefix: [u8; 4],
    prefix_read: usize,
    body: Vec<u8>,
    body_read: usize,
}
impl FrameCursor {
    pub fn is_empty(&self) -> bool {
        self.prefix_read == 0
    }
    pub fn buffer(&mut self) -> &mut [u8] {
        if self.prefix_read < 4 {
            &mut self.prefix[self.prefix_read..]
        } else {
            &mut self.body[self.body_read..]
        }
    }
    pub fn advance(&mut self, bytes: usize) -> std::io::Result<Option<Vec<u8>>> {
        if bytes == 0 || bytes > self.buffer().len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid frame cursor",
            ));
        }
        if self.prefix_read < 4 {
            self.prefix_read += bytes;
            if self.prefix_read == 4 {
                let length = u32::from_le_bytes(self.prefix) as usize;
                if length == 0 || length > crate::MAX_FRAME_BYTES {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "invalid frame length",
                    ));
                }
                self.body = vec![0; length];
            }
        } else {
            self.body_read += bytes;
            if self.body_read == self.body.len() {
                let body = std::mem::take(&mut self.body);
                self.prefix_read = 0;
                self.body_read = 0;
                return Ok(Some(body));
            }
        }
        Ok(None)
    }
}

/// Context-bound project calls accepted by the Windows connection. Actual
/// pipe fixtures share this gate so native-only tests cannot bypass it.
pub fn project_method(method: &str) -> bool {
    matches!(
        method,
        "lsp_capture"
            | "lsp_validate"
            | "source_capture"
            | "source_validate"
            | "source_worktree_preview"
            | "agent_usage"
            | "agent_resources"
            | "dependency_inventory"
            | "definitions_attach"
            | "definitions_read"
            | "definitions_validate"
            | "definitions_write"
            | "files_attach"
            | "files_lsp_snapshot"
            | "files_reveal"
            | "files_poll"
            | "files_recover"
            | "files_list"
            | "files_preview"
            | "files_open"
            | "files_open_chunk"
            | "files_save"
            | "files_rename"
            | "files_delete"
            | "files_close"
            | "files_sync_editor"
    )
}

/// Project Git dispatch methods. Creation additionally consumes a retained
/// destination preview; sibling cleanup needs a separate native capability.
pub fn source_method(method: &str) -> bool {
    matches!(
        method,
        "create_worktree"
            | "repo_merge"
            | "repo_conflicts"
            | "repo_conflict_versions"
            | "repo_conflict_resolve"
            | "repo_operation_continue"
            | "repo_operation_abort"
            | "repo_pr_status"
            | "repo_pr_list"
            | "repo_pr_create"
            | "repo_file_hunks"
            | "repo_hunks_apply"
            | "repo_last_commit"
            | "repo_blame"
            | "repo_branches"
            | "repo_branch_create"
            | "repo_switch"
            | "repo_branch_rename"
            | "repo_branch_delete"
            | "repo_stash_list"
            | "repo_stash_push"
            | "repo_stash_apply"
            | "repo_stash_drop"
            | "repo_stash_store"
            | "inspect_agent_worktree"
            | "remove_agent_worktree"
            | "repo_status"
            | "worktrees"
            | "worktree_clean"
            | "repo_preflight"
            | "repo_history"
            | "repo_commit_detail"
            | "repo_diff"
            | "repo_changes"
            | "repo_stage"
            | "repo_unstage"
            | "repo_commit_preview"
            | "repo_commit"
            | "repo_remote_status"
            | "repo_fetch"
            | "repo_pull"
            | "repo_push"
            | "repo_cleanup_preview"
            | "repo_cleanup"
    )
}

/// Fixed feature errors cross both native and helper Source adapters.
pub fn source_operation_issue(issue: &str) -> Option<&'static str> {
    match issue {
        "conflict_path_invalid" => Some("conflict_path_invalid"),
        "conflict_choice_invalid" => Some("conflict_choice_invalid"),
        "conflict_markers_left" => Some("conflict_markers_left"),
        "conflict_unresolved" => Some("conflict_unresolved"),
        "conflict_no_operation" => Some("conflict_no_operation"),
        "conflict_operation_failed" => Some("conflict_operation_failed"),
        "pr_branch_not_pushed" => Some("pr_branch_not_pushed"),
        "pr_exists" => Some("pr_exists"),
        "pr_input_invalid" => Some("pr_input_invalid"),
        "pr_failed" => Some("pr_failed"),

        "hunk_stale" => Some("hunk_stale"),
        "hunk_selection_invalid" => Some("hunk_selection_invalid"),
        "hunk_unsupported" => Some("hunk_unsupported"),
        "hunk_apply_failed" => Some("hunk_apply_failed"),
        "amend_no_commit" => Some("amend_no_commit"),
        "blame_unavailable" => Some("blame_unavailable"),

        "branch_name_invalid" => Some("branch_name_invalid"),
        "branch_exists" => Some("branch_exists"),
        "branch_missing" => Some("branch_missing"),
        "branch_in_use" => Some("branch_in_use"),
        "switch_blocked_by_changes" => Some("switch_blocked_by_changes"),
        "branch_operation_failed" => Some("branch_operation_failed"),
        "stash_empty" => Some("stash_empty"),
        "stash_missing" => Some("stash_missing"),
        "stash_operation_failed" => Some("stash_operation_failed"),

        "source_merge_dirty" => Some("source_merge_dirty"),
        "source_merge_failed" => Some("source_merge_failed"),
        "worktree_not_agent" => Some("worktree_not_agent"),
        "worktree_remove_dirty" => Some("worktree_remove_dirty"),
        "worktree_branch_unmerged" => Some("worktree_branch_unmerged"),
        "worktree_branch_invalid" => Some("worktree_branch_invalid"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hunk_blame_source_methods_and_errors_are_supported() {
        for method in [
            "repo_file_hunks",
            "repo_hunks_apply",
            "repo_last_commit",
            "repo_blame",
        ] {
            assert!(source_method(method), "{method}");
        }
        for code in [
            "hunk_stale",
            "hunk_selection_invalid",
            "hunk_unsupported",
            "hunk_apply_failed",
            "amend_no_commit",
            "blame_unavailable",
        ] {
            assert_eq!(source_operation_issue(code), Some(code));
        }
    }
    #[test]
    fn branch_stash_source_methods_and_fixed_errors_are_supported() {
        for method in [
            "repo_branches",
            "repo_branch_create",
            "repo_switch",
            "repo_branch_rename",
            "repo_branch_delete",
            "repo_stash_list",
            "repo_stash_push",
            "repo_stash_apply",
            "repo_stash_drop",
            "repo_stash_store",
        ] {
            assert!(source_method(method), "{method}");
        }
        for code in [
            "branch_name_invalid",
            "branch_exists",
            "branch_missing",
            "branch_in_use",
            "switch_blocked_by_changes",
            "branch_operation_failed",
            "stash_empty",
            "stash_missing",
            "stash_operation_failed",
        ] {
            assert_eq!(source_operation_issue(code), Some(code));
            assert_eq!(source_operation_issue(&format!("{code} /private")), None);
        }
    }
    #[test]
    fn agent_worktree_source_methods_are_supported() {
        assert!(source_method("inspect_agent_worktree"));
        assert!(project_method("agent_resources") && project_method("agent_usage"));
        assert_eq!(
            source_operation_issue("source_merge_dirty"),
            Some("source_merge_dirty")
        );
        assert_eq!(
            source_operation_issue("source_merge_dirty /private/path"),
            None
        );
        assert!(source_method("repo_merge") && source_method("remove_agent_worktree"));
    }
    #[test]
    fn a_partial_reader_resumes_the_same_frame_before_retirement() {
        let first = br#"{"result":{"Ok":"aborted response"}}"#;
        let second = br#"{"kind":"retired","version":1,"sessionId":"fixture","sequence":1}"#;
        let mut bytes = Vec::new();
        for value in [first.as_slice(), second.as_slice()] {
            bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
            bytes.extend_from_slice(value);
        }
        let mut cursor = FrameCursor::default();
        let mut frames = Vec::new();
        for byte in bytes {
            cursor.buffer()[0] = byte;
            if let Some(frame) = cursor.advance(1).unwrap() {
                frames.push(frame);
            }
        }
        assert_eq!(frames, vec![first.to_vec(), second.to_vec()]);
        assert!(cursor.is_empty());
        for length in [0, (crate::MAX_FRAME_BYTES + 1) as u32] {
            let mut cursor = FrameCursor::default();
            cursor.buffer().copy_from_slice(&length.to_le_bytes());
            assert!(cursor.advance(4).is_err());
        }
    }
}

#[cfg(test)]
mod conflict_pr_tests {
    #[test]
    fn source_method_forwards_conflict_and_pr_contracts() {
        for method in [
            "repo_conflicts",
            "repo_conflict_versions",
            "repo_conflict_resolve",
            "repo_operation_continue",
            "repo_operation_abort",
            "repo_pr_status",
            "repo_pr_list",
            "repo_pr_create",
        ] {
            assert!(super::source_method(method));
        }
        for code in [
            "conflict_path_invalid",
            "conflict_choice_invalid",
            "conflict_markers_left",
            "conflict_unresolved",
            "conflict_no_operation",
            "conflict_operation_failed",
            "pr_branch_not_pushed",
            "pr_exists",
            "pr_input_invalid",
            "pr_failed",
        ] {
            assert_eq!(super::source_operation_issue(code), Some(code));
        }
    }
}
