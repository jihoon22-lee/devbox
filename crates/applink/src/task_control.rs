//! Typed Workbench -> Run Manager task-control handoff contract.

use crate::HandoffClaim;
use serde::{Deserialize, Serialize};

pub const TASK_CONTROL_HANDOFF_KIND: &str = "task-control/v1";
pub const TASK_CONTROL_SCHEMA_VERSION: u32 = 1;
pub const TASK_CONTROL_SOURCE_APP: &str = "workbench";
pub const TASK_CONTROL_TARGET_APP: &str = "run-manager";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TaskControlAction {
    Start,
    Stop,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskControlRequest {
    pub schema_version: u32,
    pub request_id: String,
    pub task_id: String,
    pub action: TaskControlAction,
    pub expected_revision: String,
}

impl TaskControlRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != TASK_CONTROL_SCHEMA_VERSION
            || !valid_hex(&self.request_id, 32)
            || !valid_task_id(&self.task_id)
            || !valid_hex(&self.expected_revision, 64)
        {
            return Err("task-control-invalid");
        }
        Ok(())
    }

    pub fn to_payload(&self) -> Result<serde_json::Value, &'static str> {
        self.validate()?;
        serde_json::to_value(self).map_err(|_| "task-control-invalid")
    }

    pub fn from_claim(claim: &HandoffClaim) -> Result<Self, &'static str> {
        if claim.envelope.kind != TASK_CONTROL_HANDOFF_KIND
            || claim.envelope.source_app != TASK_CONTROL_SOURCE_APP
            || claim.envelope.target_app.as_deref() != Some(TASK_CONTROL_TARGET_APP)
        {
            return Err("task-control-invalid");
        }
        let request: Self = serde_json::from_value(claim.envelope.payload.clone())
            .map_err(|_| "task-control-invalid")?;
        request.validate()?;
        Ok(request)
    }
}

fn valid_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_task_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HandoffEnvelope, PROTOCOL_VERSION};

    fn request() -> TaskControlRequest {
        TaskControlRequest {
            schema_version: TASK_CONTROL_SCHEMA_VERSION,
            request_id: "a".repeat(32),
            task_id: "task-1".to_owned(),
            action: TaskControlAction::Start,
            expected_revision: "b".repeat(64),
        }
    }

    #[test]
    fn payload_contains_only_typed_opaque_values() {
        let payload = request().to_payload().unwrap();
        assert_eq!(
            payload
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            vec![
                "action",
                "expectedRevision",
                "requestId",
                "schemaVersion",
                "taskId"
            ]
        );
        let encoded = payload.to_string();
        assert!(!encoded.contains("command"));
        assert!(!encoded.contains("path"));
        assert!(!encoded.contains("environment"));
    }

    #[test]
    fn claim_requires_exact_producer_consumer_and_bounded_ids() {
        let claim = HandoffClaim {
            envelope: HandoffEnvelope {
                protocol_version: PROTOCOL_VERSION,
                id: "c".repeat(32),
                kind: TASK_CONTROL_HANDOFF_KIND.to_owned(),
                source_app: TASK_CONTROL_SOURCE_APP.to_owned(),
                target_app: Some(TASK_CONTROL_TARGET_APP.to_owned()),
                created_at_ms: 1,
                expires_at_ms: 10,
                payload: request().to_payload().unwrap(),
            },
            claim_token: "d".repeat(64),
            lease_until_ms: 9,
        };
        assert_eq!(TaskControlRequest::from_claim(&claim), Ok(request()));
        let mut wrong = claim;
        wrong.envelope.source_app = "renamed-producer".to_owned();
        assert_eq!(
            TaskControlRequest::from_claim(&wrong),
            Err("task-control-invalid")
        );
    }
}
