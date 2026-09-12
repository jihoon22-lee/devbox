//! Development-session transitions and exact Runtime generation references.
//! Persist a changed Store before carrying out any returned native action.
//! Deserialized references are history, never native process authority.
use product_contract::ProjectContext;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

type Result<T> = std::result::Result<T, &'static str>;
const MAX_SESSIONS: usize = 64;
const MAX_RESOURCES: usize = 128;
const MAX_PER_SESSION: usize = 32;
const MAX_OPERATIONS: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Preflight,
    Review,
    Restoring,
    Preparing,
    Starting,
    Readiness,
    Active,
    Stopping,
    Stopped,
    Degraded,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    RestoreOnly,
    StartReviewed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ResourceKind {
    Job,
    Service,
    TaskOperation,
    Terminal,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceIdentity {
    pub kind: ResourceKind,
    pub owner_id: String,
    /// Exact run/operation/companion identity, never a PID or friendly task name.
    pub generation: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Resource {
    pub identity: ResourceIdentity,
    pub context: ProjectContext,
    pub shared: bool,
    /// None means the resource predated these sessions and must never be stopped.
    pub created_by: Option<String>,
    pub holders: BTreeSet<String>,
    pub stop_requested: bool,
    pub stop_operation: Option<String>,
    #[serde(default)]
    pub failed_stop_operations: BTreeSet<String>,
    pub stopped: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Session {
    pub id: String,
    pub context: ProjectContext,
    pub revision: u64,
    pub plan_revision: String,
    pub phase: Phase,
    pub mode: Option<Mode>,
    pub cancel_requested: bool,
    pub deadline_ms: Option<u64>,
    pub resources: BTreeSet<String>,
    pub pending: BTreeSet<String>,
    pub issue: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Reservation {
    pub session_id: String,
    pub owner_id: String,
    pub kind: ResourceKind,
    pub plan_revision: String,
    pub resource_key: Option<String>,
    pub interrupted: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Store {
    pub schema_version: u32,
    pub sessions: BTreeMap<String, Session>,
    pub resources: BTreeMap<String, Resource>,
    pub operations: BTreeMap<String, Reservation>,
}
impl Default for Store {
    fn default() -> Self {
        Self {
            schema_version: 1,
            sessions: BTreeMap::new(),
            resources: BTreeMap::new(),
            operations: BTreeMap::new(),
        }
    }
}
fn opaque(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}
fn revision(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}
fn operation_id(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
impl Store {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1
            || self.sessions.len() > MAX_SESSIONS
            || self.resources.len() > MAX_RESOURCES
            || self.operations.len() > MAX_OPERATIONS
            || self
                .resources
                .values()
                .map(|resource| resource.failed_stop_operations.len())
                .sum::<usize>()
                > MAX_OPERATIONS
        {
            return Err("session_store_invalid");
        }
        for (key, session) in &self.sessions {
            if key != &session.id
                || !operation_id(key)
                || session.context.validate().is_err()
                || session.revision == 0
                || !revision(&session.plan_revision)
                || session.resources.len() > MAX_PER_SESSION
                || session.pending.len() > MAX_PER_SESSION
                || session.issue.as_ref().is_some_and(|issue| !opaque(issue))
            {
                return Err("session_store_invalid");
            }
            for resource in &session.resources {
                if !self
                    .resources
                    .get(resource)
                    .is_some_and(|resource| resource.holders.contains(key))
                {
                    return Err("session_store_invalid");
                }
            }
            for operation in &session.pending {
                if !self.operations.get(operation).is_some_and(|operation| {
                    operation.session_id == *key && operation.resource_key.is_none()
                }) {
                    return Err("session_store_invalid");
                }
            }
        }
        for (key, resource) in &self.resources {
            if !opaque(key)
                || !opaque(&resource.identity.owner_id)
                || !opaque(&resource.identity.generation)
                || resource.context.validate().is_err()
                || resource.holders.len() > MAX_SESSIONS
                || resource
                    .created_by
                    .as_ref()
                    .is_some_and(|owner| !self.sessions.contains_key(owner))
                || resource
                    .failed_stop_operations
                    .iter()
                    .any(|id| !operation_id(id))
                || resource.stopped && !resource.holders.is_empty()
                || resource.shared && resource.identity.kind != ResourceKind::Service
                || resource
                    .stop_operation
                    .as_ref()
                    .is_some_and(|id| !operation_id(id) || !resource.stop_requested)
            {
                return Err("session_store_invalid");
            }
            for holder in &resource.holders {
                if !self
                    .sessions
                    .get(holder)
                    .is_some_and(|session| session.resources.contains(key))
                {
                    return Err("session_store_invalid");
                }
            }
        }
        for (id, operation) in &self.operations {
            if !operation_id(id)
                || !opaque(&operation.owner_id)
                || !revision(&operation.plan_revision)
                || !self.sessions.contains_key(&operation.session_id)
                || operation
                    .resource_key
                    .as_ref()
                    .is_some_and(|key| !self.resources.contains_key(key))
            {
                return Err("session_store_invalid");
            }
        }
        Ok(())
    }
    pub fn create(
        &mut self,
        id: String,
        context: ProjectContext,
        plan_revision: String,
        now: u64,
    ) -> Result<Session> {
        if !operation_id(&id) || context.validate().is_err() || !revision(&plan_revision) {
            return Err("session_plan_invalid");
        }
        if let Some(existing) = self.sessions.get(&id) {
            return if existing.context == context && existing.plan_revision == plan_revision {
                Ok(existing.clone())
            } else {
                Err("session_identity_conflict")
            };
        }
        if self.sessions.len() >= MAX_SESSIONS {
            return Err("session_limit");
        }
        let session = Session {
            id: id.clone(),
            context,
            revision: 1,
            plan_revision,
            phase: Phase::Preflight,
            mode: None,
            cancel_requested: false,
            deadline_ms: Some(now.saturating_add(30_000)),
            resources: BTreeSet::new(),
            pending: BTreeSet::new(),
            issue: None,
        };
        self.sessions.insert(id, session.clone());
        Ok(session)
    }
    fn selected(&mut self, id: &str, expected: u64) -> Result<&mut Session> {
        let session = self.sessions.get_mut(id).ok_or("session_missing")?;
        if session.revision != expected {
            return Err("session_changed");
        }
        Ok(session)
    }
    pub fn reviewed(
        &mut self,
        id: &str,
        expected: u64,
        plan: &str,
        blocked: bool,
        now: u64,
    ) -> Result<()> {
        let session = self.selected(id, expected)?;
        if session.phase != Phase::Preflight
            || session.plan_revision != plan
            || session.deadline_ms.is_some_and(|deadline| now >= deadline)
        {
            return Err("session_plan_changed");
        }
        session.phase = if blocked {
            Phase::Degraded
        } else {
            Phase::Review
        };
        session.deadline_ms = (!blocked).then(|| now.saturating_add(120_000));
        session.issue = blocked.then(|| "session_preflight_blocked".into());
        session.revision += 1;
        Ok(())
    }
    pub fn approve(
        &mut self,
        id: &str,
        expected: u64,
        plan: &str,
        mode: Mode,
        now: u64,
    ) -> Result<()> {
        let session = self.selected(id, expected)?;
        if session.phase != Phase::Review
            || session.plan_revision != plan
            || session.cancel_requested
            || session.deadline_ms.is_some_and(|deadline| now >= deadline)
        {
            return Err("session_plan_changed");
        }
        session.mode = Some(mode);
        session.phase = Phase::Restoring;
        session.deadline_ms = Some(now.saturating_add(30_000));
        session.revision += 1;
        Ok(())
    }
    pub fn advance(&mut self, id: &str, expected: u64, from: Phase, now: u64) -> Result<()> {
        let session = self.selected(id, expected)?;
        if session.phase != from || session.cancel_requested {
            return Err("session_phase_changed");
        }
        if session.deadline_ms.is_some_and(|deadline| now >= deadline) {
            return Err("session_phase_expired");
        }
        session.phase = match from {
            Phase::Restoring if session.mode == Some(Mode::RestoreOnly) => Phase::Active,
            Phase::Restoring => Phase::Preparing,
            Phase::Preparing => Phase::Starting,
            Phase::Starting if session.pending.is_empty() => Phase::Readiness,
            Phase::Readiness => Phase::Active,
            _ => return Err("session_phase_changed"),
        };
        session.deadline_ms = match session.phase {
            Phase::Active => None,
            Phase::Readiness => Some(now.saturating_add(60_000)),
            _ => Some(now.saturating_add(30_000)),
        };
        session.revision += 1;
        Ok(())
    }
    /// Return false for an identical existing operation. The native controller
    /// must inspect its receipt/resource; it must never submit that effect again.
    pub fn reserve(
        &mut self,
        session_id: &str,
        operation: String,
        owner: String,
        kind: ResourceKind,
        plan: &str,
        now: u64,
    ) -> Result<bool> {
        if !operation_id(&operation) || !opaque(&owner) {
            return Err("session_operation_invalid");
        }
        if self.resources.values().any(|resource| {
            resource.stop_operation.as_deref() == Some(&operation)
                || resource.failed_stop_operations.contains(&operation)
        }) {
            return Err("session_operation_conflict");
        }
        if let Some(existing) = self.operations.get(&operation) {
            return if existing.session_id == session_id
                && existing.owner_id == owner
                && existing.kind == kind
                && existing.plan_revision == plan
            {
                Ok(false)
            } else {
                Err("session_operation_conflict")
            };
        }
        let session = self.sessions.get_mut(session_id).ok_or("session_missing")?;
        if session.phase != Phase::Starting
            || session.mode != Some(Mode::StartReviewed)
            || session.cancel_requested
            || session.plan_revision != plan
            || session.deadline_ms.is_some_and(|deadline| now >= deadline)
        {
            return Err("session_not_approved");
        }
        if self.operations.len() >= MAX_OPERATIONS
            || session.pending.len() + session.resources.len() >= MAX_PER_SESSION
        {
            return Err("session_resource_limit");
        }
        session.pending.insert(operation.clone());
        session.revision += 1;
        self.operations.insert(
            operation,
            Reservation {
                session_id: session_id.into(),
                owner_id: owner,
                kind,
                plan_revision: plan.into(),
                resource_key: None,
                interrupted: false,
            },
        );
        Ok(true)
    }
    /// Bind only after native Runtime has proved this exact generation and whether
    /// it created the resource. This method does not infer provenance from a PID.
    pub fn acquired(
        &mut self,
        operation: &str,
        key: String,
        identity: ResourceIdentity,
        created: bool,
        shared: bool,
    ) -> Result<()> {
        if !opaque(&key)
            || !opaque(&identity.owner_id)
            || !opaque(&identity.generation)
            || shared && identity.kind != ResourceKind::Service
        {
            return Err("session_resource_invalid");
        }
        let reservation = self
            .operations
            .get(operation)
            .ok_or("session_operation_missing")?
            .clone();
        if reservation.owner_id != identity.owner_id
            || reservation.kind != identity.kind
            || reservation.interrupted
        {
            return Err("session_resource_changed");
        }
        if let Some(existing) = &reservation.resource_key {
            return if existing == &key
                && self
                    .resources
                    .get(existing)
                    .is_some_and(|resource| resource.identity == identity)
            {
                Ok(())
            } else {
                Err("session_resource_changed")
            };
        }
        let context = self
            .sessions
            .get(&reservation.session_id)
            .ok_or("session_missing")?
            .context
            .clone();
        if let Some(existing) = self.resources.get(&key) {
            if existing.identity != identity
                || existing.stopped
                || created
                || existing.stop_requested
                || (!existing.shared && !existing.holders.contains(&reservation.session_id))
                || existing.shared != shared
            {
                return Err("session_resource_conflict");
            }
        } else {
            if self.resources.len() >= MAX_RESOURCES {
                return Err("session_resource_limit");
            }
            self.resources.insert(
                key.clone(),
                Resource {
                    identity,
                    context,
                    shared,
                    created_by: created.then(|| reservation.session_id.clone()),
                    holders: BTreeSet::new(),
                    stop_requested: false,
                    stop_operation: None,
                    failed_stop_operations: BTreeSet::new(),
                    stopped: false,
                },
            );
        }
        self.resources
            .get_mut(&key)
            .ok_or("session_resource_missing")?
            .holders
            .insert(reservation.session_id.clone());
        let session = self
            .sessions
            .get_mut(&reservation.session_id)
            .ok_or("session_missing")?;
        session.pending.remove(operation);
        session.resources.insert(key.clone());
        session.revision += 1;
        self.operations
            .get_mut(operation)
            .ok_or("session_operation_missing")?
            .resource_key = Some(key);
        Ok(())
    }
    /// Used only when a native worker/receipt proves no process owner remains.
    /// An interrupted operation stays recorded and cannot be resubmitted.
    pub fn settle_without_resource(&mut self, operation: &str) -> Result<()> {
        let reservation = self
            .operations
            .get_mut(operation)
            .ok_or("session_operation_missing")?;
        if reservation.resource_key.is_some() {
            return Err("session_resource_exists");
        }
        reservation.interrupted = true;
        let session = self
            .sessions
            .get_mut(&reservation.session_id)
            .ok_or("session_missing")?;
        session.pending.remove(operation);
        if session.phase != Phase::Stopping {
            session.phase = Phase::Degraded;
            session.issue = Some("session_resource_start_failed".into());
        }
        session.revision += 1;
        Ok(())
    }
    pub fn begin_stop(&mut self, id: &str) -> Result<()> {
        let session = self.sessions.get_mut(id).ok_or("session_missing")?;
        if session.phase == Phase::Stopped {
            return Ok(());
        }
        session.cancel_requested = true;
        session.phase = Phase::Stopping;
        session.deadline_ms = None;
        session.revision += 1;
        Ok(())
    }
    /// Release references only after pending native workers settle. An external
    /// resource is never a stop candidate. A shared created resource is deferred
    /// until its last holder releases and its creator has requested stopping.
    pub fn release(&mut self, id: &str) -> Result<Vec<(String, ResourceIdentity)>> {
        let session = self.sessions.get(id).ok_or("session_missing")?;
        if session.phase != Phase::Stopping || !session.pending.is_empty() {
            return Err("session_cleanup_pending");
        }
        let keys = session.resources.clone();
        for key in &keys {
            let resource = self
                .resources
                .get_mut(key)
                .ok_or("session_resource_missing")?;
            if resource.created_by.as_deref() == Some(id) {
                resource.stop_requested = true;
            }
            resource.holders.remove(id);
        }
        let session = self.sessions.get_mut(id).ok_or("session_missing")?;
        session.resources.clear();
        session.revision += 1;
        Ok(self
            .resources
            .iter()
            .filter(|(_, resource)| {
                resource.created_by.is_some()
                    && resource.stop_requested
                    && resource.holders.is_empty()
                    && !resource.stopped
            })
            .map(|(key, resource)| (key.clone(), resource.identity.clone()))
            .collect())
    }
    pub fn reserve_stop(&mut self, key: &str, operation: String) -> Result<String> {
        if !operation_id(&operation)
            || self.operations.contains_key(&operation)
            || self
                .resources
                .values()
                .any(|resource| resource.failed_stop_operations.contains(&operation))
            || self.resources.iter().any(|(other, resource)| {
                other != key && resource.stop_operation.as_deref() == Some(&operation)
            })
        {
            return Err("session_operation_conflict");
        }
        let resource = self
            .resources
            .get_mut(key)
            .ok_or("session_resource_missing")?;
        if resource.created_by.is_none()
            || !resource.stop_requested
            || !resource.holders.is_empty()
            || resource.stopped
        {
            return Err("session_resource_changed");
        }
        if let Some(existing) = &resource.stop_operation {
            if !resource.failed_stop_operations.contains(existing) {
                return Ok(existing.clone());
            }
        }
        resource.stop_operation = Some(operation.clone());
        Ok(operation)
    }
    /// Call only after the exact retained native stop worker settled in failure.
    /// A timeout while that worker still owns cleanup is not retry admission.
    pub fn stop_failed(
        &mut self,
        key: &str,
        identity: &ResourceIdentity,
        operation: &str,
    ) -> Result<()> {
        if self
            .resources
            .values()
            .map(|resource| resource.failed_stop_operations.len())
            .sum::<usize>()
            >= MAX_OPERATIONS
        {
            return Err("session_resource_limit");
        }
        let resource = self
            .resources
            .get_mut(key)
            .ok_or("session_resource_missing")?;
        if &resource.identity != identity
            || resource.stopped
            || resource.stop_operation.as_deref() != Some(operation)
        {
            return Err("session_resource_changed");
        }
        resource.failed_stop_operations.insert(operation.into());
        Ok(())
    }
    pub fn retired(
        &mut self,
        key: &str,
        identity: &ResourceIdentity,
        operation: &str,
    ) -> Result<()> {
        let resource = self
            .resources
            .get_mut(key)
            .ok_or("session_resource_missing")?;
        if &resource.identity != identity
            || !resource.holders.is_empty()
            || !resource.stop_requested
            || resource.created_by.is_none()
            || resource.stop_operation.as_deref() != Some(operation)
            || resource.failed_stop_operations.contains(operation)
        {
            return Err("session_resource_changed");
        }
        resource.stopped = true;
        Ok(())
    }
    pub fn finish_stop(&mut self, id: &str) -> Result<()> {
        let session = self.sessions.get_mut(id).ok_or("session_missing")?;
        if session.phase != Phase::Stopping
            || !session.pending.is_empty()
            || !session.resources.is_empty()
            || self.resources.values().any(|resource| {
                resource.created_by.as_deref() == Some(id)
                    && resource.holders.is_empty()
                    && !resource.stopped
            })
        {
            return Err("session_cleanup_pending");
        }
        session.phase = Phase::Stopped;
        session.revision += 1;
        Ok(())
    }
    pub fn recover(&mut self) -> Result<()> {
        self.validate()?;
        for operation in self.operations.values_mut() {
            if operation.resource_key.is_none() {
                operation.interrupted = true;
            }
        }
        for session in self.sessions.values_mut() {
            if session.phase != Phase::Stopped {
                session.phase = Phase::Degraded;
                session.issue = Some("session_native_owner_lost".into());
                session.deadline_ms = None;
                session.revision += 1;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn context(worktree: &str) -> ProjectContext {
        ProjectContext {
            project_id: "project".into(),
            worktree_id: worktree.into(),
            target: product_contract::ExecutionTarget::Windows,
            revision: 1,
        }
    }
    fn ready(store: &mut Store, worktree: &str) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let plan = "a".repeat(64);
        store
            .create(id.clone(), context(worktree), plan.clone(), 100)
            .unwrap();
        store.reviewed(&id, 1, &plan, false, 101).unwrap();
        store
            .approve(&id, 2, &plan, Mode::StartReviewed, 102)
            .unwrap();
        store.advance(&id, 3, Phase::Restoring, 103).unwrap();
        store.advance(&id, 4, Phase::Preparing, 104).unwrap();
        id
    }
    fn reserve(store: &mut Store, session: &str) -> String {
        let operation = uuid::Uuid::new_v4().to_string();
        assert!(store
            .reserve(
                session,
                operation.clone(),
                "service".into(),
                ResourceKind::Service,
                &"a".repeat(64),
                105
            )
            .unwrap());
        operation
    }
    fn identity() -> ResourceIdentity {
        ResourceIdentity {
            kind: ResourceKind::Service,
            owner_id: "service".into(),
            generation: "exact-run-generation".into(),
        }
    }

    #[test]
    fn two_worktrees_keep_a_created_shared_service_until_the_last_release() {
        let mut store = Store::default();
        let first = ready(&mut store, "one");
        let second = ready(&mut store, "two");
        let a = reserve(&mut store, &first);
        store
            .acquired(&a, "resource".into(), identity(), true, true)
            .unwrap();
        let b = reserve(&mut store, &second);
        store
            .acquired(&b, "resource".into(), identity(), false, true)
            .unwrap();
        store.begin_stop(&first).unwrap();
        assert!(store.release(&first).unwrap().is_empty());
        store.finish_stop(&first).unwrap();
        assert_eq!(
            store.resources["resource"].holders,
            BTreeSet::from([second.clone()])
        );
        store.begin_stop(&second).unwrap();
        assert_eq!(
            store.release(&second).unwrap(),
            vec![("resource".into(), identity())]
        );
        let stop = uuid::Uuid::new_v4().to_string();
        assert_eq!(store.reserve_stop("resource", stop.clone()).unwrap(), stop);
        assert_eq!(
            store
                .reserve_stop("resource", uuid::Uuid::new_v4().to_string())
                .unwrap(),
            stop
        );
        store.retired("resource", &identity(), &stop).unwrap();
        store.finish_stop(&second).unwrap();
        store.validate().unwrap();
    }
    #[test]
    fn failed_stop_requires_a_new_receipt_and_rejects_late_results() {
        let mut store = Store::default();
        let session = ready(&mut store, "one");
        let start = reserve(&mut store, &session);
        store
            .acquired(&start, "resource".into(), identity(), true, false)
            .unwrap();
        store.begin_stop(&session).unwrap();
        store.release(&session).unwrap();
        let first = store
            .reserve_stop("resource", uuid::Uuid::new_v4().to_string())
            .unwrap();
        store.stop_failed("resource", &identity(), &first).unwrap();
        assert!(store.reserve_stop("resource", first.clone()).is_err());
        let second = store
            .reserve_stop("resource", uuid::Uuid::new_v4().to_string())
            .unwrap();
        assert_ne!(first, second);
        assert!(store.retired("resource", &identity(), &first).is_err());
        assert!(store.stop_failed("resource", &identity(), &first).is_err());
        store.retired("resource", &identity(), &second).unwrap();
        store.finish_stop(&session).unwrap();
        store.validate().unwrap();
    }
    #[test]
    fn borrowed_external_service_and_stale_generation_never_become_stop_authority() {
        let mut store = Store::default();
        let first = ready(&mut store, "one");
        let operation = reserve(&mut store, &first);
        store
            .acquired(&operation, "external".into(), identity(), false, true)
            .unwrap();
        store.begin_stop(&first).unwrap();
        assert!(store.release(&first).unwrap().is_empty());
        assert!(store
            .reserve_stop("external", uuid::Uuid::new_v4().to_string())
            .is_err());
        assert!(store
            .retired("external", &identity(), &uuid::Uuid::new_v4().to_string())
            .is_err());
        store.finish_stop(&first).unwrap();
        let second = ready(&mut store, "two");
        let operation = reserve(&mut store, &second);
        let mut replacement = identity();
        replacement.generation = "replacement-run".into();
        assert!(store
            .acquired(&operation, "external".into(), replacement, false, true)
            .is_err());
        assert_eq!(store.resources["external"].identity, identity());
        store.validate().unwrap();
    }
    #[test]
    fn cancellation_keeps_a_late_native_start_owned_until_compensation_finishes() {
        let mut store = Store::default();
        let session = ready(&mut store, "one");
        let operation = reserve(&mut store, &session);
        store.begin_stop(&session).unwrap();
        assert!(store.release(&session).is_err());
        assert!(store.finish_stop(&session).is_err());
        store
            .acquired(&operation, "late".into(), identity(), true, false)
            .unwrap();
        assert_eq!(store.release(&session).unwrap().len(), 1);
        assert!(store.finish_stop(&session).is_err());
        store
            .reserve_stop("late", uuid::Uuid::new_v4().to_string())
            .unwrap();
        let stop = store.resources["late"].stop_operation.clone().unwrap();
        store.retired("late", &identity(), &stop).unwrap();
        store.finish_stop(&session).unwrap();
        store.validate().unwrap();
    }
    #[test]
    fn crash_history_and_repeated_identity_never_authorize_reexecution() {
        let mut store = Store::default();
        let session = ready(&mut store, "one");
        let operation = reserve(&mut store, &session);
        store.recover().unwrap();
        assert!(!store
            .reserve(
                &session,
                operation.clone(),
                "service".into(),
                ResourceKind::Service,
                &"a".repeat(64),
                200
            )
            .unwrap());
        assert!(store
            .acquired(&operation, "unknown".into(), identity(), true, true)
            .is_err());
        store.begin_stop(&session).unwrap();
        assert!(store.release(&session).is_err());
        store.settle_without_resource(&operation).unwrap();
        assert!(store.release(&session).unwrap().is_empty());
        store.finish_stop(&session).unwrap();
        store.validate().unwrap();
    }
    #[test]
    fn restore_only_and_expired_plans_cannot_start_resources() {
        let mut store = Store::default();
        let id = uuid::Uuid::new_v4().to_string();
        let plan = "a".repeat(64);
        store
            .create(id.clone(), context("one"), plan.clone(), 0)
            .unwrap();
        store.reviewed(&id, 1, &plan, false, 1).unwrap();
        assert!(store
            .approve(&id, 2, &plan, Mode::StartReviewed, 120_001)
            .is_err());
        store.approve(&id, 2, &plan, Mode::RestoreOnly, 2).unwrap();
        store.advance(&id, 3, Phase::Restoring, 3).unwrap();
        assert_eq!(store.sessions[&id].phase, Phase::Active);
        assert!(store
            .reserve(
                &id,
                uuid::Uuid::new_v4().to_string(),
                "service".into(),
                ResourceKind::Service,
                &plan,
                4
            )
            .is_err());
        store.validate().unwrap();
    }
}
