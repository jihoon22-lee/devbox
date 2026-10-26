//! Durable Windows copy readers. Safety rows never cascade with projects or presentation runs.
use crate::windows_copy::CopyIdentity;
use devbox_protocol::windows::{ObservationAssigned, ObservationControl};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

pub(crate) const MIGRATION_4: &str = "
CREATE TABLE runtime_windows_copies (key TEXT PRIMARY KEY, body TEXT NOT NULL);
CREATE TABLE runtime_windows_readers (
 launch_id TEXT PRIMARY KEY, copy_key TEXT NOT NULL, project_id TEXT NOT NULL,
 run_id TEXT NOT NULL, state TEXT NOT NULL, body TEXT NOT NULL
);
CREATE INDEX runtime_windows_readers_copy ON runtime_windows_readers(copy_key,state);
CREATE INDEX runtime_windows_readers_project ON runtime_windows_readers(project_id,state);
CREATE TABLE runtime_windows_attempts (
 nonce TEXT PRIMARY KEY, launch_id TEXT NOT NULL, attempt_id TEXT NOT NULL,
 phase TEXT NOT NULL, body TEXT NOT NULL, UNIQUE(launch_id,attempt_id)
);
";
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CopyRef {
    pub key: String,
    pub revision: String,
    pub launch_id: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProcessOwner {
    pub pid: u32,
    pub start: String,
    pub boot: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CopyRecord {
    pub identity: CopyIdentity,
    pub revision: Option<String>,
    pub writer: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReaderState {
    Reserved,
    Dispatching,
    Submitted,
    Sealed,
    Closed,
}
impl ReaderState {
    fn name(self) -> &'static str {
        match self {
            Self::Reserved => "reserved",
            Self::Dispatching => "dispatching",
            Self::Submitted => "submitted",
            Self::Sealed => "sealed",
            Self::Closed => "closed",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Reader {
    pub copy: CopyRef,
    pub project_id: String,
    pub run_id: String,
    pub unit: String,
    pub state: ReaderState,
    pub owner: ProcessOwner,
    pub previous_attempt: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AttemptPhase {
    Intent,
    Assigned,
    Committed,
    Revoked,
    Closed,
}
impl AttemptPhase {
    fn name(self) -> &'static str {
        match self {
            Self::Intent => "intent",
            Self::Assigned => "assigned",
            Self::Committed => "committed",
            Self::Revoked => "revoked",
            Self::Closed => "closed",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Attempt {
    pub run_id: String,
    pub attempt_id: String,
    pub control: ObservationControl,
    pub phase: AttemptPhase,
    pub assigned: Option<ObservationAssigned>,
    pub owner: ProcessOwner,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Admission {
    Writer,
    Shared,
}
pub(crate) type Decision<T> = rusqlite::Result<Result<T, crate::RuntimeError>>;
fn encode(value: &impl Serialize) -> rusqlite::Result<String> {
    serde_json::to_string(value).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}
fn decode<T: serde::de::DeserializeOwned>(text: String) -> rusqlite::Result<T> {
    serde_json::from_str(&text).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}
fn invalid() -> crate::RuntimeError {
    crate::RuntimeError::WindowsCopyUnavailable
}
pub(crate) fn copy(tx: &Transaction<'_>, key: &str) -> rusqlite::Result<Option<CopyRecord>> {
    tx.query_row(
        "SELECT body FROM runtime_windows_copies WHERE key=?1",
        [key],
        |r| decode(r.get(0)?),
    )
    .optional()
}
fn put_copy(tx: &Transaction<'_>, value: &CopyRecord) -> rusqlite::Result<()> {
    tx.execute("INSERT INTO runtime_windows_copies(key,body) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET body=excluded.body",params![value.identity.key,encode(value)?])?;
    Ok(())
}
pub(crate) fn reader(tx: &Transaction<'_>, launch: &str) -> rusqlite::Result<Option<Reader>> {
    tx.query_row(
        "SELECT body FROM runtime_windows_readers WHERE launch_id=?1",
        [launch],
        |r| decode(r.get(0)?),
    )
    .optional()
}
fn put_reader(tx: &Transaction<'_>, value: &Reader) -> rusqlite::Result<()> {
    tx.execute("INSERT INTO runtime_windows_readers(launch_id,copy_key,project_id,run_id,state,body) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(launch_id) DO UPDATE SET state=excluded.state,body=excluded.body",params![value.copy.launch_id,value.copy.key,value.project_id,value.run_id,value.state.name(),encode(value)?])?;
    Ok(())
}
pub(crate) fn needs_reconcile(tx: &Transaction<'_>) -> rusqlite::Result<bool> {
    tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM runtime_windows_readers WHERE state!='closed')",
        [],
        |r| r.get(0),
    )
}
#[cfg(test)]
pub(crate) fn unresolved(tx: &Transaction<'_>) -> rusqlite::Result<Vec<Reader>> {
    unresolved_for(tx, None)
}
pub(crate) fn unresolved_for(
    tx: &Transaction<'_>,
    key: Option<&str>,
) -> rusqlite::Result<Vec<Reader>> {
    tx.prepare("SELECT body FROM runtime_windows_readers WHERE state!='closed' AND (?1 IS NULL OR copy_key=?1) ORDER BY launch_id")?.query_map([key],|r|decode(r.get(0)?))?.collect()
}

pub(crate) fn reserve(
    tx: &Transaction<'_>,
    identity: &CopyIdentity,
    reader: &Reader,
) -> Decision<Admission> {
    if !tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM projects_projects WHERE id=?1)",
        [&reader.project_id],
        |r| r.get::<_, bool>(0),
    )? {
        return Ok(Err(crate::RuntimeError::ProjectNotFound {
            id: reader.project_id.clone(),
        }));
    }
    if reader.copy.key != identity.key
        || reader.project_id != identity.project_id
        || self::reader(tx, &reader.copy.launch_id)?.is_some()
    {
        return Ok(Err(invalid()));
    }
    let prior = copy(tx, &identity.key)?;
    if prior.as_ref().is_some_and(|c| c.identity != *identity) {
        return Ok(Err(invalid()));
    }
    if prior.as_ref().is_some_and(|c| c.writer.is_some()) {
        return Ok(Err(crate::RuntimeError::WindowsCopyBusy));
    }
    let active=tx.query_row("SELECT EXISTS(SELECT 1 FROM runtime_windows_readers WHERE copy_key=?1 AND state!='closed')",[&identity.key],|r|r.get::<_,bool>(0))?;
    let admission = if active {
        if prior.as_ref().and_then(|c| c.revision.as_ref()) != Some(&reader.copy.revision) {
            return Ok(Err(crate::RuntimeError::WindowsCopyBusy));
        }
        Admission::Shared
    } else {
        Admission::Writer
    };
    if admission == Admission::Writer {
        put_copy(
            tx,
            &CopyRecord {
                identity: identity.clone(),
                revision: None,
                writer: Some(reader.copy.launch_id.clone()),
            },
        )?;
    }
    put_reader(tx, reader)?;
    Ok(Ok(admission))
}
pub(crate) fn publish(tx: &Transaction<'_>, reference: &CopyRef) -> Decision<()> {
    let Some(mut value) = copy(tx, &reference.key)? else {
        return Ok(Err(invalid()));
    };
    let Some(reader) = reader(tx, &reference.launch_id)? else {
        return Ok(Err(invalid()));
    };
    if value.writer.as_deref() != Some(&reference.launch_id)
        || reader.state != ReaderState::Reserved
        || reader.copy != *reference
    {
        return Ok(Err(invalid()));
    }
    value.writer = None;
    value.revision = Some(reference.revision.clone());
    put_copy(tx, &value)?;
    Ok(Ok(()))
}
/// Safe only before any unit submission. The coordinator invokes this after its worker exits.
pub(crate) fn cancel_reserved(tx: &Transaction<'_>, launch: &str) -> rusqlite::Result<()> {
    let Some(mut value) = reader(tx, launch)? else {
        return Ok(());
    };
    if value.state != ReaderState::Reserved {
        return Ok(());
    }
    if let Some(mut copy) = copy(tx, &value.copy.key)?
        && copy.writer.as_deref() == Some(launch)
    {
        copy.writer = None;
        copy.revision = None;
        put_copy(tx, &copy)?;
    }
    value.state = ReaderState::Closed;
    put_reader(tx, &value)
}
pub(crate) fn submit(tx: &Transaction<'_>, reference: &CopyRef) -> Decision<()> {
    let Some(mut value) = reader(tx, &reference.launch_id)? else {
        return Ok(Err(invalid()));
    };
    let ready = copy(tx, &reference.key)?
        .is_some_and(|c| c.writer.is_none() && c.revision.as_ref() == Some(&reference.revision));
    if value.copy != *reference || value.state != ReaderState::Reserved || !ready {
        return Ok(Err(invalid()));
    }
    value.state = ReaderState::Dispatching;
    put_reader(tx, &value)?;
    Ok(Ok(()))
}
pub(crate) fn launched(tx: &Transaction<'_>, reference: &CopyRef) -> Decision<()> {
    let Some(mut value) = reader(tx, &reference.launch_id)? else {
        return Ok(Err(invalid()));
    };
    if value.copy != *reference
        || !matches!(
            value.state,
            ReaderState::Dispatching | ReaderState::Submitted
        )
    {
        return Ok(Err(invalid()));
    }
    value.state = ReaderState::Submitted;
    put_reader(tx, &value)?;
    Ok(Ok(()))
}
pub(crate) fn seal(tx: &Transaction<'_>, launch: &str) -> rusqlite::Result<()> {
    if let Some(mut value) = reader(tx, launch)?
        && matches!(
            value.state,
            ReaderState::Dispatching | ReaderState::Submitted
        )
    {
        value.state = ReaderState::Sealed;
        put_reader(tx, &value)?;
        for mut attempt in attempts(tx, launch)? {
            if matches!(attempt.phase, AttemptPhase::Intent | AttemptPhase::Assigned) {
                attempt.phase = AttemptPhase::Revoked;
                put_attempt(tx, &attempt)?;
            }
        }
    }
    close_if_finished(tx, launch)
}
fn close_if_finished(tx: &Transaction<'_>, launch: &str) -> rusqlite::Result<()> {
    let Some(mut value) = reader(tx, launch)? else {
        return Ok(());
    };
    if value.state != ReaderState::Sealed {
        return Ok(());
    }
    let unresolved=tx.query_row("SELECT EXISTS(SELECT 1 FROM runtime_windows_attempts WHERE launch_id=?1 AND phase NOT IN ('closed','revoked'))",[launch],|r|r.get::<_,bool>(0))?;
    if !unresolved {
        value.state = ReaderState::Closed;
        put_reader(tx, &value)?;
    }
    Ok(())
}
pub(crate) fn attempts(tx: &Transaction<'_>, launch: &str) -> rusqlite::Result<Vec<Attempt>> {
    tx.prepare("SELECT body FROM runtime_windows_attempts WHERE launch_id=?1 ORDER BY attempt_id")?
        .query_map([launch], |r| decode(r.get(0)?))?
        .collect()
}
pub(crate) fn attempt(tx: &Transaction<'_>, nonce: &str) -> rusqlite::Result<Option<Attempt>> {
    tx.query_row(
        "SELECT body FROM runtime_windows_attempts WHERE nonce=?1",
        [nonce],
        |r| decode(r.get(0)?),
    )
    .optional()
}
pub(crate) fn current_attempt(
    tx: &Transaction<'_>,
    launch: &str,
    id: &str,
) -> rusqlite::Result<Option<Attempt>> {
    tx.query_row(
        "SELECT body FROM runtime_windows_attempts WHERE launch_id=?1 AND attempt_id=?2",
        params![launch, id],
        |r| decode(r.get(0)?),
    )
    .optional()
}
fn put_attempt(tx: &Transaction<'_>, value: &Attempt) -> rusqlite::Result<()> {
    tx.execute("INSERT INTO runtime_windows_attempts(nonce,launch_id,attempt_id,phase,body) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(nonce) DO UPDATE SET phase=excluded.phase,body=excluded.body",params![value.control.nonce,value.control.launch_id,value.attempt_id,value.phase.name(),encode(value)?])?;
    Ok(())
}
pub(crate) fn intent(tx: &Transaction<'_>, value: &Attempt) -> Decision<Attempt> {
    let Some(mut reader) = reader(tx, &value.control.launch_id)? else {
        return Ok(Err(invalid()));
    };
    if reader.run_id != value.run_id
        || !matches!(
            reader.state,
            ReaderState::Dispatching | ReaderState::Submitted
        )
        || reader.previous_attempt.as_deref() == Some(&value.attempt_id)
    {
        return Ok(Err(invalid()));
    }
    if let Some(old) = current_attempt(tx, &value.control.launch_id, &value.attempt_id)? {
        return Ok(Ok(old));
    }
    put_attempt(tx, value)?;
    // A new wrapper attempt is positive submission evidence, even if the caller lost the systemd ACK.
    if reader.state == ReaderState::Dispatching {
        reader.state = ReaderState::Submitted;
        put_reader(tx, &reader)?;
    }
    Ok(Ok(value.clone()))
}
pub(crate) fn commit(
    tx: &Transaction<'_>,
    nonce: &str,
    assigned: &ObservationAssigned,
) -> Decision<bool> {
    let Some(mut value) = attempt(tx, nonce)? else {
        return Ok(Err(invalid()));
    };
    let Some(reader) = reader(tx, &value.control.launch_id)? else {
        return Ok(Err(invalid()));
    };
    if reader.state != ReaderState::Submitted || reader.run_id != value.run_id {
        return Ok(Err(invalid()));
    }
    if !matches!(value.phase, AttemptPhase::Intent | AttemptPhase::Assigned) {
        return Ok(Ok(false));
    }
    if !value.control.accepts(
        &devbox_protocol::windows::RunIdentity {
            run_id: value.run_id.clone(),
            attempt_id: value.attempt_id.clone(),
        },
        assigned,
    ) {
        return Ok(Err(invalid()));
    }
    value.assigned = Some(assigned.clone());
    value.phase = AttemptPhase::Committed;
    put_attempt(tx, &value)?;
    Ok(Ok(true))
}
/// A committed attempt is never reclassified as a pre-command cancellation.
pub(crate) fn revoke(tx: &Transaction<'_>, nonce: &str) -> rusqlite::Result<bool> {
    let Some(mut value) = attempt(tx, nonce)? else {
        return Ok(false);
    };
    if value.phase == AttemptPhase::Revoked {
        return Ok(true);
    }
    if !matches!(value.phase, AttemptPhase::Intent | AttemptPhase::Assigned) {
        return Ok(false);
    }
    value.phase = AttemptPhase::Revoked;
    put_attempt(tx, &value)?;
    close_if_finished(tx, &value.control.launch_id)?;
    Ok(true)
}
pub(crate) fn observed_closed(
    tx: &Transaction<'_>,
    nonce: &str,
    assigned: &ObservationAssigned,
) -> Decision<()> {
    let Some(mut value) = attempt(tx, nonce)? else {
        return Ok(Err(invalid()));
    };
    if value.phase != AttemptPhase::Committed || value.assigned.as_ref() != Some(assigned) {
        return Ok(Err(invalid()));
    }
    value.phase = AttemptPhase::Closed;
    put_attempt(tx, &value)?;
    close_if_finished(tx, &value.control.launch_id)?;
    Ok(Ok(()))
}
