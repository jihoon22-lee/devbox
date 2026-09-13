//! Read-only owner projections. A row is never an approval or cancellation token.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Review,
    Running,
    CancelRequested,
    Cancelled,
    Uncancellable,
    Succeeded,
    Failed,
    Unknown,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Row {
    pub id: String,
    pub owner: String,
    pub component: String,
    pub route: String,
    pub label: String,
    pub phase: Phase,
    pub revision: String,
}
impl Row {
    pub fn new(
        owner: &str,
        component: &str,
        route: &str,
        id: &str,
        label: &str,
        phase: Phase,
        generation: &impl Serialize,
    ) -> Result<Self, &'static str> {
        if !crate::commands::opaque_id(id)
            || label.len() > 256
            || label.chars().any(char::is_control)
            || !crate::installation::PRODUCTS.contains(&owner)
        {
            return Err("operation_invalid");
        }
        let revision = Sha256::digest(
            serde_json::to_vec(&(owner, component, route, id, phase, generation))
                .map_err(|_| "operation_invalid")?,
        )
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
        Ok(Self {
            id: id.into(),
            owner: owner.into(),
            component: component.into(),
            route: route.into(),
            label: label.into(),
            phase,
            revision,
        })
    }
    pub fn review(
        &self,
        operation_id: &str,
    ) -> (crate::commands::Descriptor, crate::commands::Request) {
        use crate::commands::*;
        let descriptor = Descriptor {
            id: format!("{}.operation-{}", self.owner, self.id),
            owner: self.owner.clone(),
            component: self.component.clone(),
            label: self.label.clone(),
            revision: self.revision.clone(),
            target: Target::Route {
                route: self.route.clone(),
            },
            review_route: Some(self.route.clone()),
            required_context: ContextRequirement::None,
            context: None,
            destructive: false,
            requires_review: true,
            disabled_reason: None,
        };
        let request = Request {
            operation_id: operation_id.into(),
            command_id: descriptor.id.clone(),
            revision: self.revision.clone(),
            context: None,
            selection_id: None,
        };
        (descriptor, request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_intent_and_owner_review_are_not_execution_completion() {
        let requested = Row::new(
            "workspace",
            "workspace.terminal",
            "terminal",
            "session-one",
            "개발 세션",
            Phase::CancelRequested,
            &1,
        )
        .unwrap();
        let completed = Row::new(
            "workspace",
            "workspace.terminal",
            "terminal",
            "session-one",
            "개발 세션",
            Phase::Cancelled,
            &2,
        )
        .unwrap();
        assert_ne!(requested.revision, completed.revision);
        let (descriptor, request) = requested.review("operation-one");
        assert!(descriptor.requires_review);
        assert!(!descriptor.destructive);
        assert!(descriptor.validate_request(&request).is_ok());
        assert!(Row::new(
            "other",
            "workspace.terminal",
            "terminal",
            "session-one",
            "safe",
            Phase::Running,
            &1
        )
        .is_err());
    }
}
