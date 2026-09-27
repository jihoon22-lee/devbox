//! Bounded native-only preparation and operation receipts. No execution data is serialized.
use product_contract::ProjectContext;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
type Result<T> = std::result::Result<T, &'static str>;
pub const PREPARE_TTL_MS: u64 = 180_000;
const MAX_PREPARED: usize = 64 * 16;
const MAX_OPERATIONS: usize = 512;
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Scope {
    pub session: String,
    pub context: ProjectContext,
}
impl Scope {
    pub fn validate(&self) -> Result<()> {
        if !product_contract::commands::opaque_id(&self.session) {
            return Err("session_runtime_invalid");
        }
        self.context
            .validate()
            .map_err(|_| "session_runtime_invalid")
    }
}
struct Preparation<P> {
    scope: Scope,
    value: P,
    expires: u64,
}
pub struct Receipt<W> {
    pub reference: String,
    pub witness: Option<Arc<W>>,
    pub finished: bool,
    pub cancelled: bool,
    pub issue: Option<&'static str>,
}
impl<W> Clone for Receipt<W> {
    fn clone(&self) -> Self {
        Self {
            reference: self.reference.clone(),
            witness: self.witness.clone(),
            finished: self.finished,
            cancelled: self.cancelled,
            issue: self.issue,
        }
    }
}
struct Operation<W> {
    scope: Scope,
    preparation: String,
    receipt: Receipt<W>,
}
pub enum Begin<P, W> {
    New {
        reference: String,
        witness: Arc<W>,
        prepared: P,
    },
    Existing {
        reference: String,
        witness: Arc<W>,
        finished: bool,
    },
}
pub struct Registry<P, W> {
    prepared: HashMap<String, Preparation<P>>,
    operations: HashMap<String, Operation<W>>,
}
impl<P, W> Default for Registry<P, W> {
    fn default() -> Self {
        Self {
            prepared: HashMap::new(),
            operations: HashMap::new(),
        }
    }
}
impl<P: Clone, W> Registry<P, W> {
    pub fn prepare(&mut self, scope: Scope, value: P, now: u64) -> Result<String> {
        scope.validate()?;
        self.prepared.retain(|_, entry| entry.expires > now);
        if self.prepared.len() >= MAX_PREPARED {
            return Err("session_runtime_limit");
        }
        let reference = uuid::Uuid::new_v4().to_string();
        self.prepared.insert(
            reference.clone(),
            Preparation {
                scope,
                value,
                expires: now.saturating_add(PREPARE_TTL_MS),
            },
        );
        Ok(reference)
    }
    pub fn prepared(&self, scope: &Scope, reference: &str, now: u64) -> Result<P> {
        self.prepared
            .get(reference)
            .filter(|entry| entry.scope == *scope && entry.expires > now)
            .map(|entry| entry.value.clone())
            .ok_or("session_runtime_stale")
    }
    pub fn begin(
        &mut self,
        scope: &Scope,
        operation: &str,
        preparation: &str,
        witness: Arc<W>,
        now: u64,
    ) -> Result<Begin<P, W>> {
        scope.validate()?;
        validate_operation(operation)?;
        if let Some(entry) = self.operations.get(operation) {
            if entry.scope != *scope {
                return Err("session_runtime_stale");
            }
            if entry.receipt.cancelled {
                return Err("session_runtime_cancelled");
            }
            if entry.preparation != preparation {
                return Err("session_runtime_conflict");
            }
            return Ok(Begin::Existing {
                reference: entry.receipt.reference.clone(),
                witness: entry
                    .receipt
                    .witness
                    .clone()
                    .ok_or("session_runtime_stale")?,
                finished: entry.receipt.finished,
            });
        }
        if self.operations.len() >= MAX_OPERATIONS {
            return Err("session_runtime_limit");
        }
        let prepared = self.prepared(scope, preparation, now)?;
        let reference = uuid::Uuid::new_v4().to_string();
        self.operations.insert(
            operation.into(),
            Operation {
                scope: scope.clone(),
                preparation: preparation.into(),
                receipt: Receipt {
                    reference: reference.clone(),
                    witness: Some(witness.clone()),
                    finished: false,
                    cancelled: false,
                    issue: None,
                },
            },
        );
        Ok(Begin::New {
            reference,
            witness,
            prepared,
        })
    }
    pub fn receipt(&self, scope: &Scope, operation: &str) -> Result<Receipt<W>> {
        self.operations
            .get(operation)
            .filter(|entry| entry.scope == *scope)
            .map(|entry| entry.receipt.clone())
            .ok_or("session_runtime_stale")
    }
    pub fn by_reference(&self, scope: &Scope, reference: &str) -> Result<Receipt<W>> {
        self.operations
            .values()
            .find(|entry| entry.scope == *scope && entry.receipt.reference == reference)
            .map(|entry| entry.receipt.clone())
            .ok_or("session_runtime_stale")
    }
    pub fn finish(
        &mut self,
        scope: &Scope,
        operation: &str,
        issue: Option<&'static str>,
    ) -> Result<()> {
        let entry = self
            .operations
            .get_mut(operation)
            .filter(|entry| entry.scope == *scope)
            .ok_or("session_runtime_stale")?;
        if !entry.receipt.finished {
            entry.receipt.finished = true;
            entry.receipt.issue = issue;
        }
        Ok(())
    }
    pub fn cancel(&mut self, scope: &Scope, operation: &str) -> Result<Option<Arc<W>>> {
        scope.validate()?;
        validate_operation(operation)?;
        if !self.operations.contains_key(operation) {
            if self.operations.len() >= MAX_OPERATIONS {
                return Err("session_runtime_limit");
            }
            self.operations.insert(
                operation.into(),
                Operation {
                    scope: scope.clone(),
                    preparation: String::new(),
                    receipt: Receipt {
                        reference: uuid::Uuid::new_v4().to_string(),
                        witness: None,
                        finished: true,
                        cancelled: true,
                        issue: Some("session_runtime_cancelled"),
                    },
                },
            );
        }
        let entry = self
            .operations
            .get_mut(operation)
            .filter(|entry| entry.scope == *scope)
            .ok_or("session_runtime_stale")?;
        entry.receipt.cancelled = true;
        Ok(entry.receipt.witness.clone())
    }
}
fn validate_operation(operation: &str) -> Result<()> {
    if product_contract::commands::opaque_id(operation) {
        Ok(())
    } else {
        Err("session_runtime_invalid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope(id: &str) -> Scope {
        Scope {
            session: id.into(),
            context: product_contract::ProjectContext {
                project_id: "p".into(),
                worktree_id: "w".into(),
                target: product_contract::ExecutionTarget::Windows,
                revision: 1,
            },
        }
    }
    #[test]
    fn a_lost_reply_keeps_the_same_witness_and_never_starts_twice() {
        let mut registry = Registry::<u32, u32>::default();
        let owner = scope("session");
        let prepared = registry.prepare(owner.clone(), 7, 100).unwrap();
        let Begin::New {
            reference,
            witness,
            prepared: value,
        } = registry
            .begin(&owner, "operation", &prepared, Arc::new(9), 100)
            .unwrap()
        else {
            panic!("first admission must start")
        };
        assert_eq!(value, 7);
        let Begin::Existing {
            reference: again,
            witness: same,
            ..
        } = registry
            .begin(&owner, "operation", &prepared, Arc::new(99), 101)
            .unwrap()
        else {
            panic!("duplicate must not start")
        };
        assert_eq!(reference, again);
        assert!(Arc::ptr_eq(&witness, &same));
        registry
            .finish(&owner, "operation", Some("session_runtime_unavailable"))
            .unwrap();
        let receipt = registry.receipt(&owner, "operation").unwrap();
        assert!(receipt.finished);
        assert_eq!(*receipt.witness.unwrap(), 9);
        assert!(registry
            .by_reference(&scope("foreign"), &reference)
            .is_err());
    }
    #[test]
    fn cancellation_before_start_blocks_late_start_and_expired_preparations_do_not_mutate() {
        let mut registry = Registry::<u32, u32>::default();
        let owner = scope("one");
        let reference = registry.prepare(owner.clone(), 1, 100).unwrap();
        assert!(registry.cancel(&owner, "operation").unwrap().is_none());
        assert!(matches!(
            registry.begin(&owner, "operation", &reference, Arc::new(1), 101),
            Err("session_runtime_cancelled")
        ));
        assert!(matches!(
            registry.begin(
                &owner,
                "late",
                &reference,
                Arc::new(1),
                100 + PREPARE_TTL_MS
            ),
            Err("session_runtime_stale")
        ));
    }
    #[test]
    fn an_operation_cannot_be_rebound_to_another_preparation_or_context() {
        let mut registry = Registry::<u32, u32>::default();
        let owner = scope("one");
        let first = registry.prepare(owner.clone(), 1, 0).unwrap();
        let second = registry.prepare(owner.clone(), 2, 0).unwrap();
        registry
            .begin(&owner, "op", &first, Arc::new(1), 0)
            .unwrap();
        assert!(matches!(
            registry.begin(&owner, "op", &second, Arc::new(2), 0),
            Err("session_runtime_conflict")
        ));
        let mut changed = owner.clone();
        changed.context.revision += 1;
        assert!(registry.receipt(&changed, "op").is_err());
    }
}
