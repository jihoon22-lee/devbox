use devbox_applink::{TaskControlAction, TaskControlRequest};
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum WorkspaceTaskControlReceiptStatus {
    Accepted,
    Rejected,
    Started,
    Stopped,
    Failed,
}

impl WorkspaceTaskControlReceiptStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::Started => "started",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTaskControlReceipt {
    pub schema_version: u32,
    pub request_id: String,
    pub task_id: String,
    pub action: TaskControlAction,
    pub status: WorkspaceTaskControlReceiptStatus,
    pub operation_id: Option<String>,
    pub failure_code: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTaskControlPreview {
    pub request_id: String,
    pub task_id: String,
    pub action: TaskControlAction,
    pub expected_revision: String,
    pub label: String,
    pub task_kind: String,
}

impl WorkspaceTaskControlPreview {
    pub fn from_request(request: &TaskControlRequest, label: String, task_kind: String) -> Self {
        Self {
            request_id: request.request_id.clone(),
            task_id: request.task_id.clone(),
            action: request.action,
            expected_revision: request.expected_revision.clone(),
            label,
            task_kind,
        }
    }
}
