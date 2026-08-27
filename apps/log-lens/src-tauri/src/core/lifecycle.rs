use super::model::CoreError;
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

const MAX_TRACKED_OPERATIONS: usize = 32;

#[derive(Clone, Debug)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    pub fn check(&self) -> Result<(), CoreError> {
        if self.is_cancelled() {
            Err(CoreError::OperationCancelled)
        } else {
            Ok(())
        }
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
struct OperationEntry {
    generation: u64,
    token: CancellationToken,
}

#[derive(Debug, Default)]
struct RegistryInner {
    current_generation: u64,
    operations: HashMap<String, OperationEntry>,
}

/// Single-flight operation registry. Starting a newer read cancels every
/// older operation; an opaque caller-owned id is required for cancellation so
/// a stale WebView callback cannot cancel a later operation by index.
#[derive(Debug, Default)]
pub struct OperationRegistry {
    inner: Mutex<RegistryInner>,
}

impl OperationRegistry {
    pub fn begin(
        &self,
        operation_id: &str,
        generation: u64,
    ) -> Result<CancellationToken, CoreError> {
        validate_operation_id(operation_id)?;
        let mut inner = self.inner.lock().map_err(|_| CoreError::Io)?;
        if generation < inner.current_generation {
            return Err(CoreError::StaleOperation);
        }
        if inner.operations.contains_key(operation_id) {
            return Err(CoreError::InvalidInput);
        }
        for operation in inner.operations.values() {
            operation.token.cancel();
        }
        inner.current_generation = generation;
        let token = CancellationToken::new();
        inner.operations.insert(
            operation_id.to_string(),
            OperationEntry {
                generation,
                token: token.clone(),
            },
        );
        if inner.operations.len() > MAX_TRACKED_OPERATIONS {
            // Generation is caller supplied and may stay unchanged while a
            // view repeatedly refreshes. A generation-based retain alone can
            // therefore grow forever. Evict cancelled/non-current entries
            // until the registry is bounded, keeping the operation just
            // created so its worker can still pass the current check.
            let overflow = inner.operations.len() - MAX_TRACKED_OPERATIONS;
            let evict = inner
                .operations
                .keys()
                .filter(|id| id.as_str() != operation_id)
                .take(overflow)
                .cloned()
                .collect::<Vec<_>>();
            for id in evict {
                inner.operations.remove(&id);
            }
        }
        Ok(token)
    }

    pub fn cancel(&self, operation_id: &str) -> Result<bool, CoreError> {
        validate_operation_id(operation_id)?;
        let inner = self.inner.lock().map_err(|_| CoreError::Io)?;
        let Some(operation) = inner.operations.get(operation_id) else {
            return Ok(false);
        };
        operation.token.cancel();
        Ok(true)
    }

    pub fn check_current(
        &self,
        operation_id: &str,
        generation: u64,
        token: &CancellationToken,
    ) -> Result<(), CoreError> {
        token.check()?;
        let inner = self.inner.lock().map_err(|_| CoreError::Io)?;
        let Some(operation) = inner.operations.get(operation_id) else {
            return Err(CoreError::StaleOperation);
        };
        if operation.generation != generation || !Arc::ptr_eq(&operation.token.0, &token.0) {
            return Err(CoreError::StaleOperation);
        }
        Ok(())
    }

    pub fn finish(&self, operation_id: &str, generation: u64) {
        if let Ok(mut inner) = self.inner.lock() {
            if inner
                .operations
                .get(operation_id)
                .is_some_and(|operation| operation.generation == generation)
            {
                inner.operations.remove(operation_id);
            }
        }
    }
}

fn validate_operation_id(operation_id: &str) -> Result<(), CoreError> {
    if operation_id.is_empty()
        || operation_id.len() > 128
        || operation_id
            .bytes()
            .any(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')))
    {
        return Err(CoreError::InvalidInput);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_operation_cancels_previous_single_flight() {
        let registry = OperationRegistry::default();
        let old = registry.begin("old", 1).unwrap();
        let new = registry.begin("new", 2).unwrap();
        assert!(old.is_cancelled());
        assert!(!new.is_cancelled());
        assert_eq!(
            registry.check_current("old", 1, &old),
            Err(CoreError::OperationCancelled)
        );
        assert!(registry.check_current("new", 2, &new).is_ok());
    }

    #[test]
    fn stale_generation_and_bad_id_are_rejected() {
        let registry = OperationRegistry::default();
        let _ = registry.begin("current", 3).unwrap();
        assert!(matches!(
            registry.begin("old", 2),
            Err(CoreError::StaleOperation)
        ));
        assert!(matches!(
            registry.begin("../bad", 4),
            Err(CoreError::InvalidInput)
        ));
    }

    #[test]
    fn explicit_cancel_lookup_is_stable() {
        let registry = OperationRegistry::default();
        let token = registry.begin("opaque-1", 1).unwrap();
        assert!(registry.cancel("opaque-1").unwrap());
        assert!(token.is_cancelled());
        assert!(!registry.cancel("missing").unwrap());
    }

    #[test]
    fn repeated_same_generation_stays_within_tracking_limit() {
        let registry = OperationRegistry::default();
        let mut tokens = Vec::new();
        for index in 0..(MAX_TRACKED_OPERATIONS + 8) {
            tokens.push(registry.begin(&format!("operation-{index}"), 1).unwrap());
        }

        assert_eq!(
            registry.inner.lock().unwrap().operations.len(),
            MAX_TRACKED_OPERATIONS
        );
        assert!(tokens.last().is_some_and(|token| !token.is_cancelled()));
    }
}
