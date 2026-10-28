//! One-shot private control grants and conservative Windows copy-consumer reconciliation.
use crate::{
    R, RuntimeError, RuntimeService,
    windows_copy_service::{owner_alive, process_owner},
    windows_copy_store::{self as store, Attempt, AttemptPhase, CopyRef, ReaderState},
};
use devbox_protocol::{
    Fail, Internal,
    windows::{
        ControlShape, HolderIdentity, ObservationAssigned, ObservationControl, ObservationState,
        RunIdentity,
    },
};
use std::{
    fs::{self, File, OpenOptions},
    future::Future,
    io::Write,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    time::Duration,
};
use tokio_util::sync::CancellationToken;

/// Holder liveness for a vanished or session-hidden job (Rulings 34/35): the holder's recorded
/// identity, and whether the job was judged `IdentityMismatch`. Only `NotFound` proves it gone.
pub type WindowsHolder = Arc<
    dyn Fn(HolderIdentity, bool) -> Pin<Box<dyn Future<Output = ObservationState> + Send>>
        + Send
        + Sync,
>;
pub(crate) fn unavailable_holder() -> WindowsHolder {
    Arc::new(|_, _| Box::pin(async { ObservationState::Unavailable }))
}
pub(crate) fn unavailable_observation() -> crate::WindowsObservation {
    Arc::new(|_| Box::pin(async { ObservationState::Unavailable }))
}
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
    /// The holder's ready marker and emptied receipt; an rc.1 control has neither.
    holder: Option<HolderPaths>,
}
struct HolderPaths {
    ready: PathBuf,
    emptied: PathBuf,
}
/// The private files of a stored control of either shape, derived from its copy and nonce.
fn paths(copy: &store::CopyRecord, control: &ObservationControl) -> R<PrivatePaths> {
    let dir = copy.identity.control_directory();
    let nonce = &control.nonce;
    let shape = control.shape().ok_or_else(unavailable)?;
    let holder = shape == ControlShape::Holder;
    if control.assigned_path != format!("{dir}\\{nonce}.assigned.json")
        || control.grant_path != format!("{dir}\\{nonce}.grant.json")
        || control.closed_path != format!("{dir}\\{nonce}.closed.json")
        || (holder
            && (control.holder_ready_path != Some(format!("{dir}\\{nonce}.holder-ready"))
                || control.emptied_path != Some(format!("{dir}\\{nonce}.emptied.json"))))
    {
        return Err(unavailable());
    }
    let local = copy.identity.mapped_base.join("control");
    crate::windows::reject_links(&local).map_err(io_fail)?;
    let result = PrivatePaths {
        assigned: local.join(format!("{nonce}.assigned.json")),
        grant: local.join(format!("{nonce}.grant.json")),
        closed: local.join(format!("{nonce}.closed.json")),
        holder: holder.then(|| HolderPaths {
            ready: local.join(format!("{nonce}.holder-ready")),
            emptied: local.join(format!("{nonce}.emptied.json")),
        }),
    };
    let mut owned = vec![&result.assigned, &result.grant, &result.closed];
    if let Some(holder) = &result.holder {
        owned.extend([&holder.ready, &holder.emptied]);
    }
    for path in owned {
        crate::windows::reject_links(path).map_err(io_fail)?;
    }
    Ok(result)
}
fn read_json<T: serde::de::DeserializeOwned>(path: &Path, limit: u64) -> R<Option<T>> {
    crate::windows::reject_links(path).map_err(io_fail)?;
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_fail(e)),
    };
    if !meta.is_file() || meta.len() > limit {
        return Err(unavailable());
    }
    serde_json::from_slice(&fs::read(path).map_err(io_fail)?)
        .map(Some)
        .map_err(io_fail)
}
fn read_assigned(path: &Path) -> R<Option<ObservationAssigned>> {
    read_json(path, 16 * 1024)
}
/// The Windows holder's `{"nonce":…}` receipt that the job emptied after a process ran in it.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Emptied {
    nonce: String,
}
/// Only this attempt's own receipt, at the path derived from its stored control, is proof.
/// Missing, linked, oversized, malformed or another nonce's receipt is no proof.
fn emptied(copy: &store::CopyRecord, control: &ObservationControl) -> bool {
    let Some(holder) = paths(copy, control).ok().and_then(|private| private.holder) else {
        return false;
    };
    read_json::<Emptied>(&holder.emptied, 1024)
        .is_ok_and(|receipt| receipt.is_some_and(|r| r.nonce == control.nonce))
}
/// The identity this attempt's own holder recorded in its ready marker, read like the receipt
/// (derived path, no link, bounded, exact shape). Another nonce, user or session is none.
fn holder_identity(
    copy: &store::CopyRecord,
    control: &ObservationControl,
    assigned: &ObservationAssigned,
) -> Option<HolderIdentity> {
    let holder = paths(copy, control).ok()?.holder?;
    read_json::<HolderIdentity>(&holder.ready, 1024)
        .ok()
        .flatten()
        .filter(|identity| identity.belongs_to(assigned))
}
/// The bootstrap's own process from its assigned receipt, which `observes` validated.
fn bootstrap_identity(assigned: &ObservationAssigned) -> HolderIdentity {
    HolderIdentity {
        nonce: assigned.nonce.clone(),
        pid: assigned.pid,
        creation_time: assigned.creation_time.clone(),
        sid: assigned.sid.clone(),
        session_id: assigned.session_id,
    }
}
/// I1 under Rulings 34-36 for a holder control. The receipt alone closes, before any probe.
/// Otherwise `Empty` closes; a vanished job (`NotFound`) or one hidden by another session
/// (`IdentityMismatch`) closes only once this attempt's own holder is provably gone, and for
/// another session the bootstrap too.
async fn holder_attempt_ended(
    execution: &crate::ExecutionConfig,
    copy: &store::CopyRecord,
    control: &ObservationControl,
    assigned: &ObservationAssigned,
) -> bool {
    if emptied(copy, control) {
        return true;
    }
    let cross_session = match (execution.windows_observation)(assigned.clone()).await {
        ObservationState::Empty => return true,
        ObservationState::NotFound => false,
        ObservationState::IdentityMismatch => true,
        _ => return false,
    };
    // Without a recorded holder there is nothing to prove gone: the reader stays held.
    let Some(holder) = holder_identity(copy, control, assigned) else {
        return false;
    };
    if (execution.windows_holder)(holder, cross_session).await != ObservationState::NotFound {
        return false;
    }
    // In the same session Observe's NotFound already proved the bootstrap gone. Another
    // session's Observe stopped at its session gate, so the bootstrap is proven gone the same
    // way: its command may still run there if the holder alone died.
    !cross_session
        || (execution.windows_holder)(bootstrap_identity(assigned), true).await
            == ObservationState::NotFound
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
        // Only the constructor builds a new control, so it always carries the holder paths;
        // the rc.1 rule stays reachable only for rows stored before them.
        let control = ObservationControl::new(launch, nonce, &copy.identity.control_directory());
        if !control.valid() {
            return Err(unavailable());
        }
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
                        let run = RunIdentity {
                            run_id: attempt.run_id.clone(),
                            attempt_id: attempt.attempt_id.clone(),
                        };
                        let ended = match attempt.control.observes(&run, assigned) {
                            // The name vanishes with the last handle even while orphaned
                            // descendants run (I1): the holder and its receipt decide.
                            Some(ControlShape::Holder) => {
                                holder_attempt_ended(execution, &copy, &attempt.control, assigned)
                                    .await
                            }
                            // rc.1 rule for an rc.1 row: no holder ever ran for it.
                            Some(ControlShape::Legacy) => matches!(
                                (execution.windows_observation)(assigned.clone()).await,
                                ObservationState::Empty | ObservationState::NotFound
                            ),
                            None => continue,
                        };
                        if ended {
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
