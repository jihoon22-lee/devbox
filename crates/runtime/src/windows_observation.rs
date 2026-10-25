//! One-shot private control grants and conservative Windows copy-consumer reconciliation.
use crate::{
    R, RuntimeError, RuntimeService,
    windows_copy_service::{owner_alive, process_owner},
    windows_copy_store::{self as store, Attempt, AttemptPhase, CopyRef, ReaderState},
};
use devbox_protocol::{
    Fail, Internal,
    windows::{ObservationAssigned, ObservationControl, ObservationState, RunIdentity},
};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio_util::sync::CancellationToken;

fn unavailable() -> Fail<RuntimeError> {
    RuntimeError::WindowsCopyUnavailable.into()
}
fn io_fail(_: impl std::fmt::Display) -> Fail<RuntimeError> {
    unavailable()
}
struct PrivatePaths {
    assigned: PathBuf,
    grant: PathBuf,
    closed: PathBuf,
}
fn paths(copy: &store::CopyRecord, control: &ObservationControl) -> R<PrivatePaths> {
    let dir = copy.identity.control_directory();
    let nonce = &control.nonce;
    if !control.valid()
        || control.assigned_path != format!("{dir}\\{nonce}.assigned.json")
        || control.grant_path != format!("{dir}\\{nonce}.grant.json")
        || control.closed_path != format!("{dir}\\{nonce}.closed.json")
    {
        return Err(unavailable());
    }
    let local = copy.identity.mapped_base.join("control");
    crate::windows::reject_links(&local).map_err(io_fail)?;
    let result = PrivatePaths {
        assigned: local.join(format!("{nonce}.assigned.json")),
        grant: local.join(format!("{nonce}.grant.json")),
        closed: local.join(format!("{nonce}.closed.json")),
    };
    for path in [&result.assigned, &result.grant, &result.closed] {
        crate::windows::reject_links(path).map_err(io_fail)?;
    }
    Ok(result)
}
fn read_assigned(path: &Path) -> R<Option<ObservationAssigned>> {
    crate::windows::reject_links(path).map_err(io_fail)?;
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_fail(e)),
    };
    if !meta.is_file() || meta.len() > 16 * 1024 {
        return Err(unavailable());
    }
    serde_json::from_slice(&fs::read(path).map_err(io_fail)?)
        .map(Some)
        .map_err(io_fail)
}
/// Publish immutable complete JSON without replacing another attempt's evidence.
fn publish_once(path: &Path, value: &impl serde::Serialize) -> R<()> {
    use std::os::unix::fs::OpenOptionsExt;
    crate::windows::reject_links(path).map_err(io_fail)?;
    let parent = path.parent().ok_or_else(unavailable)?;
    if !parent.is_dir() {
        return Err(unavailable());
    }
    let temp = parent.join(format!(".control-{}.tmp", uuid::Uuid::new_v4().simple()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)
            .map_err(io_fail)?;
        file.write_all(&serde_json::to_vec(value).map_err(io_fail)?)
            .map_err(io_fail)?;
        file.sync_all().map_err(io_fail)?;
        drop(file);
        fs::hard_link(&temp, path).map_err(io_fail)?;
        File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(io_fail)
    })();
    let _ = fs::remove_file(temp);
    result
}
impl RuntimeService {
    pub(crate) async fn observation_intent(
        &self,
        id: &RunIdentity,
        reference: &CopyRef,
    ) -> R<ObservationControl> {
        let launch = reference.launch_id.clone();
        let db_launch = launch.clone();
        let attempt_id = id.attempt_id.clone();
        if let Some(old) = self
            .db
            .read(move |tx| store::current_attempt(tx, &db_launch, &attempt_id))
            .await
            .internal("read Windows launch fence")?
        {
            if matches!(old.phase, AttemptPhase::Revoked | AttemptPhase::Closed) {
                return Err(unavailable());
            }
            return Ok(old.control);
        }
        let key = reference.key.clone();
        let copy = self
            .db
            .read(move |tx| store::copy(tx, &key))
            .await
            .internal("read Windows copy identity")?
            .ok_or_else(unavailable)?;
        let nonce = uuid::Uuid::new_v4().simple().to_string();
        let dir = copy.identity.control_directory();
        let control = ObservationControl {
            schema: 1,
            launch_id: launch,
            job_name: format!("Local\\Devbox.Observe.{nonce}"),
            assigned_path: format!("{dir}\\{nonce}.assigned.json"),
            grant_path: format!("{dir}\\{nonce}.grant.json"),
            closed_path: format!("{dir}\\{nonce}.closed.json"),
            nonce,
        };
        paths(&copy, &control)?;
        let value = Attempt {
            run_id: id.run_id.clone(),
            attempt_id: id.attempt_id.clone(),
            control,
            phase: AttemptPhase::Intent,
            assigned: None,
            owner: process_owner()?,
        };
        let (value, _) = self
            .db
            .write_checked(move |tx| store::intent(tx, &value))
            .await
            .internal("reserve Windows attempt observation")??;
        Ok(value.control)
    }
    async fn recorded_attempt(&self, id: &RunIdentity) -> R<(store::CopyRecord, Attempt)> {
        let value = self.stored(&id.run_id).await?;
        let reference = value.windows.and_then(|w| w.copy).ok_or_else(unavailable)?;
        let lookup = id.attempt_id.clone();
        self.db
            .read(move |tx| {
                let copy = store::copy(tx, &reference.key)?;
                let attempt = store::current_attempt(tx, &reference.launch_id, &lookup)?;
                Ok(copy.zip(attempt))
            })
            .await
            .internal("read Windows attempt identity")?
            .ok_or_else(unavailable)
    }
    /// The connection registers this owned monitor before returning windows.prepare.
    /// Loss of the caller cannot detach a publisher or turn a committed launch into cancellation.
    pub async fn monitor_windows(
        self: Arc<Self>,
        id: RunIdentity,
        cancel: CancellationToken,
    ) -> R<()> {
        let (copy, attempt) = self.recorded_attempt(&id).await?;
        let service = self.clone();
        self.copy_tasks.spawn(async move {
            if service
                .monitor_attempt(copy, attempt, cancel)
                .await
                .is_err()
            {
                tracing::warn!("Windows observation grant unavailable; copy consumer retained");
            }
        });
        Ok(())
    }
    async fn revoke_attempt(&self, copy: &store::CopyRecord, attempt: &Attempt) -> R<()> {
        let _guard = self
            .run_lock(&format!("copy-attempt:{}", attempt.control.nonce))
            .await;
        let nonce = attempt.control.nonce.clone();
        let (closed, _) = self
            .db
            .write(move |tx| store::revoke(tx, &nonce))
            .await
            .internal("close Windows launch grant")?;
        if closed {
            let private = paths(copy, &attempt.control)?;
            // An existing closed marker also denies execution; never overwrite it.
            if !private.closed.exists() {
                publish_once(&private.closed, &attempt.control)?;
            }
        }
        Ok(())
    }
    async fn monitor_attempt(
        &self,
        copy: store::CopyRecord,
        attempt: Attempt,
        cancel: CancellationToken,
    ) -> R<()> {
        let private = match paths(&copy, &attempt.control) {
            Ok(paths) => paths,
            Err(error) => {
                let _ = self.revoke_attempt(&copy, &attempt).await;
                return Err(error);
            }
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        loop {
            if cancel.is_cancelled()
                || self.stopping.is_cancelled()
                || std::time::Instant::now() >= deadline
            {
                return self.revoke_attempt(&copy, &attempt).await;
            }
            match read_assigned(&private.assigned) {
                Ok(Some(assigned)) => {
                    let _guard = self
                        .run_lock(&format!("copy-attempt:{}", attempt.control.nonce))
                        .await;
                    // Cancellation and commit are serialized with the durable phase CAS.
                    if cancel.is_cancelled() || self.stopping.is_cancelled() {
                        drop(_guard);
                        return self.revoke_attempt(&copy, &attempt).await;
                    }
                    let nonce = attempt.control.nonce.clone();
                    let receipt = assigned.clone();
                    let request_cancel = cancel.clone();
                    let stopping = self.stopping.clone();
                    let committed = self
                        .db
                        .write_checked(move |tx| {
                            commit_if_live(
                                tx,
                                &nonce,
                                &receipt,
                                &request_cancel,
                                &stopping,
                                deadline,
                            )
                        })
                        .await
                        .internal("commit Windows command grant")?;
                    match committed {
                        Ok((true, _)) => publish_once(&private.grant, &assigned)?,
                        Ok((false, _)) => {
                            drop(_guard);
                            if cancel.is_cancelled()
                                || self.stopping.is_cancelled()
                                || std::time::Instant::now() >= deadline
                            {
                                self.revoke_attempt(&copy, &attempt).await?;
                            }
                        }
                        Err(error) => {
                            drop(_guard);
                            self.revoke_attempt(&copy, &attempt).await?;
                            return Err(error.into());
                        }
                    }
                    // No second grant is sent, including committed-but-unpublished recovery.
                    return Ok(());
                }
                Ok(None) => (),
                Err(error) => {
                    self.revoke_attempt(&copy, &attempt).await?;
                    return Err(error);
                }
            }
            tokio::select! {_=cancel.cancelled()=>{},_=self.stopping.cancelled()=>{},_=tokio::time::sleep(Duration::from_millis(25))=>{}}
        }
    }
    pub(crate) async fn reconcile_windows_copies(&self) -> R<()> {
        self.reconcile_windows_selection(None).await
    }
    pub(crate) async fn reconcile_windows_target(&self, key: &str) -> R<()> {
        self.reconcile_windows_selection(Some(key)).await
    }
    async fn reconcile_windows_selection(&self, key: Option<&str>) -> R<()> {
        if self.execution.is_none() {
            return Ok(());
        }
        let execution = self.execution()?;
        let wanted = key.map(str::to_owned);
        let readers = self
            .db
            .read(move |tx| store::unresolved_for(tx, wanted.as_deref()))
            .await
            .internal("read unresolved Windows consumers")?;
        for reader in readers {
            let key = reader.copy.key.clone();
            let owned = self
                .db
                .read(move |tx| store::copy(tx, &key))
                .await
                .internal("read Windows namespace owner")?;
            if owned
                .as_ref()
                .is_none_or(|copy| copy.identity.owner != execution.windows_owner)
            {
                // A copied/moved database must not revoke a different instance/distro's grant.
                continue;
            }
            if reader.state == ReaderState::Reserved {
                // A live publisher owns its guard until actual worker exit; no age-based unlock.
                if owner_alive(&reader.owner) == Some(false) {
                    let launch = reader.copy.launch_id.clone();
                    self.db
                        .write(move |tx| store::cancel_reserved(tx, &launch))
                        .await
                        .internal("recover abandoned copy writer")?;
                }
                continue;
            }
            let launch = reader.copy.launch_id.clone();
            let key = reader.copy.key.clone();
            let (copy, attempts) = self
                .db
                .read(move |tx| Ok((store::copy(tx, &key)?, store::attempts(tx, &launch)?)))
                .await
                .internal("read Windows consumer proofs")?;
            let Some(copy) = copy else {
                continue;
            };
            for attempt in &attempts {
                match attempt.phase {
                    AttemptPhase::Intent | AttemptPhase::Assigned
                        if owner_alive(&attempt.owner) == Some(false) =>
                    {
                        if self.revoke_attempt(&copy, attempt).await.is_err() {
                            tracing::warn!("Windows pre-command revocation marker unavailable");
                        }
                    }
                    AttemptPhase::Revoked => {
                        if self.revoke_attempt(&copy, attempt).await.is_err() {
                            tracing::warn!("Windows closed grant marker unavailable");
                        }
                    }
                    AttemptPhase::Committed => {
                        let Some(assigned) = attempt.assigned.as_ref() else {
                            continue;
                        };
                        if !attempt.control.accepts(
                            &RunIdentity {
                                run_id: attempt.run_id.clone(),
                                attempt_id: attempt.attempt_id.clone(),
                            },
                            assigned,
                        ) {
                            continue;
                        }
                        let state = (execution.windows_observation)(assigned.clone()).await;
                        if matches!(state, ObservationState::Empty | ObservationState::NotFound) {
                            let nonce = attempt.control.nonce.clone();
                            let assigned = assigned.clone();
                            self.db
                                .write_checked(move |tx| {
                                    store::observed_closed(tx, &nonce, &assigned)
                                })
                                .await
                                .internal("confirm Windows consumer exit")??;
                        }
                    }
                    _ => (),
                }
            }
            if reader.state == ReaderState::Submitted {
                let Ok(unit) = devbox_core::systemd::Systemd.show(&reader.unit).await else {
                    continue;
                };
                let can_seal = launch_sealed(reader.state, &unit, !attempts.is_empty());
                if can_seal {
                    self.seal_windows_copy(&reader.copy.launch_id).await?;
                }
            }
        }
        Ok(())
    }
}
/// Service auto-restart gaps are active/activating or auto-restart substates.
/// No recorded attempt plus missing unit is an uncertain submission, not no execution.
pub(crate) fn launch_sealed(
    state: ReaderState,
    unit: &devbox_core::systemd::UnitState,
    has_attempt: bool,
) -> bool {
    if state != ReaderState::Submitted {
        return false;
    }
    if matches!(
        unit.active_state.as_str(),
        "active" | "activating" | "deactivating"
    ) || unit.sub_state.starts_with("auto-restart")
    {
        return false;
    }
    if unit.load_state == "not-found" {
        return has_attempt;
    }
    matches!(unit.active_state.as_str(), "inactive" | "failed")
}

fn commit_if_live(
    tx: &rusqlite::Transaction<'_>,
    nonce: &str,
    receipt: &ObservationAssigned,
    cancel: &CancellationToken,
    stopping: &CancellationToken,
    deadline: std::time::Instant,
) -> store::Decision<bool> {
    // This check runs in the serialized DB writer, not only before awaiting it.
    if cancel.is_cancelled() || stopping.is_cancelled() || std::time::Instant::now() >= deadline {
        store::revoke(tx, nonce)?;
        return Ok(Ok(false));
    }
    store::commit(tx, nonce, receipt)
}
#[cfg(test)]
mod tests;
