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
            | "repo_commit"
            | "repo_remote_status"
            | "repo_fetch"
            | "repo_pull"
            | "repo_push"
            | "repo_cleanup_preview"
            | "repo_cleanup"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
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
