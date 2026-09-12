//! Bounded destination-owned route review. Queueing a command is not opening it.
use crate::commands::{Descriptor, Request, Target};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
type Result<T> = std::result::Result<T, &'static str>;
const MAX_PENDING: usize = 32;
const MAX_RECEIPTS: usize = 256;
const REVIEW_MS: u64 = 120_000;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    AwaitingReview,
    Opening,
    Opened,
    Rejected,
    Expired,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
    pub operation_id: String,
    pub phase: Phase,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub operation_id: String,
    pub revision: String,
    pub label: String,
    pub route: String,
}
struct Entry {
    receipt: Receipt,
    revision: String,
    label: String,
    route: String,
    expires: u64,
}
#[derive(Default)]
pub struct Queue {
    entries: BTreeMap<String, Entry>,
}
impl Queue {
    fn expire(&mut self, now: u64) {
        for entry in self.entries.values_mut() {
            if entry.expires <= now
                && matches!(entry.receipt.phase, Phase::AwaitingReview | Phase::Opening)
            {
                entry.receipt.phase = Phase::Expired;
            }
        }
    }
    pub fn enqueue(
        &mut self,
        descriptor: &Descriptor,
        request: &Request,
        now: u64,
    ) -> Result<Receipt> {
        descriptor.validate_request(request)?;
        let Target::Route { route } = &descriptor.target else {
            return Err("navigation_requires_entity_owner");
        };
        // A route switch grants no project selection. Contextful/entity commands
        // must use their actual owner's reviewed navigation adapter separately.
        if request.context.is_some() || request.selection_id.is_some() || descriptor.destructive {
            return Err("navigation_requires_owner_review");
        }
        let revision = Sha256::digest(
            serde_json::to_vec(&(descriptor, request)).map_err(|_| "navigation_invalid")?,
        )
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
        self.expire(now);
        if let Some(entry) = self.entries.get(&request.operation_id) {
            return if entry.revision == revision {
                Ok(entry.receipt.clone())
            } else {
                Err("navigation_operation_conflict")
            };
        }
        if self
            .entries
            .values()
            .filter(|entry| matches!(entry.receipt.phase, Phase::AwaitingReview | Phase::Opening))
            .count()
            >= MAX_PENDING
        {
            return Err("navigation_pending_limit");
        }
        // Keep terminal receipts for the connection lifetime. Eviction would
        // let an old operation ID create another navigation side effect.
        if self.entries.len() >= MAX_RECEIPTS {
            return Err("navigation_receipt_limit");
        }
        let receipt = Receipt {
            operation_id: request.operation_id.clone(),
            phase: Phase::AwaitingReview,
        };
        self.entries.insert(
            request.operation_id.clone(),
            Entry {
                receipt: receipt.clone(),
                revision,
                label: descriptor.label.clone(),
                route: route.clone(),
                expires: now.saturating_add(REVIEW_MS),
            },
        );
        Ok(receipt)
    }
    pub fn pending(&mut self, now: u64) -> Vec<Review> {
        self.expire(now);
        self.entries
            .values()
            .filter(|entry| entry.receipt.phase == Phase::AwaitingReview)
            .map(|entry| Review {
                operation_id: entry.receipt.operation_id.clone(),
                revision: entry.revision.clone(),
                label: entry.label.clone(),
                route: entry.route.clone(),
            })
            .collect()
    }
    pub fn decide(
        &mut self,
        id: &str,
        revision: &str,
        accept: bool,
        now: u64,
    ) -> Result<Option<String>> {
        self.expire(now);
        let entry = self.entries.get_mut(id).ok_or("navigation_missing")?;
        if entry.revision != revision || entry.receipt.phase != Phase::AwaitingReview {
            return Err("navigation_review_stale");
        }
        entry.receipt.phase = if accept {
            Phase::Opening
        } else {
            Phase::Rejected
        };
        Ok(accept.then(|| entry.route.clone()))
    }
    pub fn acknowledge(&mut self, id: &str, route: &str, now: u64) -> Result<Receipt> {
        self.expire(now);
        let entry = self.entries.get_mut(id).ok_or("navigation_missing")?;
        if entry.route != route || entry.receipt.phase != Phase::Opening {
            return Err("navigation_delivery_stale");
        }
        entry.receipt.phase = Phase::Opened;
        Ok(entry.receipt.clone())
    }
    pub fn status(&mut self, id: &str, now: u64) -> Result<Receipt> {
        self.expire(now);
        self.entries
            .get(id)
            .map(|entry| entry.receipt.clone())
            .ok_or("navigation_missing")
    }
    pub fn revoke(&mut self) {
        for entry in self.entries.values_mut() {
            if matches!(entry.receipt.phase, Phase::AwaitingReview | Phase::Opening) {
                entry.receipt.phase = Phase::Rejected;
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::ContextRequirement;
    fn command() -> (Descriptor, Request) {
        let descriptor = Descriptor {
            id: "knowledge.open-notes".into(),
            owner: "knowledge".into(),
            component: "knowledge.shell".into(),
            label: "Notes".into(),
            revision: "a".repeat(64),
            target: Target::Route {
                route: "notes".into(),
            },
            required_context: ContextRequirement::None,
            context: None,
            destructive: false,
            requires_review: false,
            disabled_reason: None,
        };
        let request = Request {
            operation_id: "fixture-operation".into(),
            command_id: descriptor.id.clone(),
            revision: descriptor.revision.clone(),
            context: None,
            selection_id: None,
        };
        (descriptor, request)
    }
    #[test]
    fn enqueue_is_not_delivery_and_duplicate_does_not_reopen() {
        let (descriptor, request) = command();
        let mut queue = Queue::default();
        assert_eq!(
            queue.enqueue(&descriptor, &request, 1000).unwrap().phase,
            Phase::AwaitingReview
        );
        let review = queue.pending(1001).pop().unwrap();
        assert!(queue
            .acknowledge(&review.operation_id, "notes", 1001)
            .is_err());
        assert_eq!(
            queue
                .decide(&review.operation_id, &review.revision, true, 1002)
                .unwrap()
                .as_deref(),
            Some("notes")
        );
        assert!(queue
            .acknowledge(&review.operation_id, "daily", 1003)
            .is_err());
        assert_eq!(
            queue
                .acknowledge(&review.operation_id, "notes", 1003)
                .unwrap()
                .phase,
            Phase::Opened
        );
        assert_eq!(
            queue.enqueue(&descriptor, &request, 1004).unwrap().phase,
            Phase::Opened
        );
        assert!(queue.pending(1005).is_empty());
    }
    #[test]
    fn reject_expire_conflict_and_revocation_keep_receipts() {
        let (descriptor, mut request) = command();
        let mut queue = Queue::default();
        queue.enqueue(&descriptor, &request, 0).unwrap();
        let review = queue.pending(1).pop().unwrap();
        assert!(queue
            .decide(&review.operation_id, "stale", false, 1)
            .is_err());
        assert_eq!(
            queue
                .decide(&review.operation_id, &review.revision, false, 1)
                .unwrap(),
            None
        );
        assert_eq!(
            queue.enqueue(&descriptor, &request, 2).unwrap().phase,
            Phase::Rejected
        );
        let mut changed = descriptor.clone();
        changed.label = "changed".into();
        assert!(queue.enqueue(&changed, &request, 2).is_err());
        request.operation_id = "expires".into();
        queue.enqueue(&descriptor, &request, 0).unwrap();
        assert_eq!(
            queue.status("expires", REVIEW_MS).unwrap().phase,
            Phase::Expired
        );
        request.operation_id = "revoked".into();
        queue.enqueue(&descriptor, &request, REVIEW_MS).unwrap();
        queue.revoke();
        assert_eq!(
            queue.status("revoked", REVIEW_MS).unwrap().phase,
            Phase::Rejected
        );
    }
}
