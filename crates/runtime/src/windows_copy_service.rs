//! Copy coordination outlives a cancelled caller until its publisher actually exits.
use crate::{
    R, RuntimeError, RuntimeService,
    windows::PreparedCopy,
    windows_copy,
    windows_copy_store::{self as store, Admission, CopyRef, ProcessOwner, Reader, ReaderState},
};
use devbox_core::Db;
use devbox_protocol::{Fail, Internal, windows::PathObservation};

use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

pub(crate) fn process_owner() -> R<ProcessOwner> {
    let pid = std::process::id();
    let text =
        std::fs::read_to_string("/proc/self/stat").internal("read copy coordinator identity")?;
    let start = text
        .rsplit_once(") ")
        .and_then(|(_, fields)| fields.split_whitespace().nth(19))
        .ok_or(Fail::Domain(RuntimeError::WindowsCopyUnavailable))?
        .to_owned();
    let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .internal("read copy coordinator boot identity")?
        .trim()
        .to_owned();
    Ok(ProcessOwner { pid, start, boot })
}
pub(crate) fn owner_alive(owner: &ProcessOwner) -> Option<bool> {
    if owner.pid == 0
        || !owner.start.parse::<u64>().is_ok_and(|value| value > 0)
        || uuid::Uuid::parse_str(&owner.boot).is_err()
    {
        return None;
    }
    let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?;
    if boot.trim() != owner.boot {
        return Some(false);
    }
    match std::fs::read_to_string(format!("/proc/{}/stat", owner.pid)) {
        Ok(text) => text
            .rsplit_once(") ")
            .and_then(|(_, f)| f.split_whitespace().nth(19))
            .map(|start| start == owner.start),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(false),
        Err(_) => None,
    }
}
pub(crate) struct CopyPermit {
    pub reference: CopyRef,
    db: Db,
}
impl CopyPermit {
    pub async fn submitted(&self) -> R<()> {
        let reference = self.reference.clone();
        self.db
            .write_checked(move |tx| store::submit(tx, &reference))
            .await
            .internal("commit Windows copy consumer")??;
        Ok(())
    }
    pub async fn launched(&self) -> R<()> {
        let reference = self.reference.clone();
        self.db
            .write_checked(move |tx| store::launched(tx, &reference))
            .await
            .internal("acknowledge Windows unit submission")??;
        Ok(())
    }
}
impl Drop for CopyPermit {
    fn drop(&mut self) {
        let db = self.db.clone();
        let launch = self.reference.launch_id.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _ = db
                    .write(move |tx| store::cancel_reserved(tx, &launch))
                    .await;
            });
        }
    }
}
fn copy_failure(_: crate::windows::WindowsError) -> Fail<RuntimeError> {
    RuntimeError::WindowsCopyUnavailable.into()
}
pub(crate) async fn safe_paths(execution: &crate::ExecutionConfig, paths: Vec<String>) -> R<()> {
    use devbox_protocol::windows::{MAX_WINDOWS_PROBE_PATH_BYTES, MAX_WINDOWS_PROBE_PATHS};
    let mut batch = Vec::new();
    let mut bytes = 0;
    for path in paths {
        if path.len() > MAX_WINDOWS_PROBE_PATH_BYTES {
            return Err(RuntimeError::WindowsCopyUnavailable.into());
        }
        if batch.len() == MAX_WINDOWS_PROBE_PATHS
            || bytes + path.len() > MAX_WINDOWS_PROBE_PATH_BYTES
        {
            if (execution.windows_paths)(std::mem::take(&mut batch)).await != PathObservation::Safe
            {
                return Err(RuntimeError::WindowsCopyUnavailable.into());
            }
            bytes = 0;
        }
        bytes += path.len();
        batch.push(path);
    }
    if !batch.is_empty() && (execution.windows_paths)(batch).await != PathObservation::Safe {
        return Err(RuntimeError::WindowsCopyUnavailable.into());
    }
    Ok(())
}

async fn coordinate(
    db: &Db,
    execution: &crate::ExecutionConfig,
    copy: &PreparedCopy,
    reader: &Reader,
    cancel: &CancellationToken,
) -> R<()> {
    if cancel.is_cancelled() {
        return Err(RuntimeError::WindowsCopyUnavailable.into());
    }
    copy.identity.check_source().map_err(copy_failure)?;
    safe_paths(execution, copy.identity.metadata_paths()).await?;
    let owner = copy.identity.clone();
    tokio::task::spawn_blocking(move || owner.ensure_owned())
        .await
        .internal("Windows copy owner worker")?
        .map_err(copy_failure)?;
    let manifest = windows_copy::capture(&copy.source, cancel.clone())
        .await
        .map_err(copy_failure)?;
    if manifest.revision != reader.copy.revision {
        return Err(RuntimeError::WindowsCopyUnavailable.into());
    }
    let identity = copy.identity.clone();
    let reservation = reader.clone();
    let (admission, _) = db
        .write_checked(move |tx| store::reserve(tx, &identity, &reservation))
        .await
        .internal("reserve Windows copy consumer")??;
    if admission == Admission::Writer {
        let paths = manifest
            .files
            .iter()
            .map(|f| {
                format!(
                    "{}\\{}",
                    copy.identity.directory(),
                    f.path.to_string_lossy().replace('/', "\\")
                )
            })
            .chain(std::iter::once(copy.cwd.clone()))
            .collect();
        safe_paths(execution, paths).await?;
        #[cfg(test)]
        let hook = execution.windows_publish_hook.clone();
        #[cfg(not(test))]
        let hook = None;
        windows_copy::publish_with_hook(
            &copy.source,
            &copy.target,
            &manifest,
            cancel.clone(),
            hook,
        )
        .await
        .map_err(copy_failure)?;
        let cwd = copy.target.join(&copy.relative_cwd);
        tokio::task::spawn_blocking(move || windows_copy::safe_dir(&cwd))
            .await
            .internal("Windows cwd owner worker")?
            .map_err(copy_failure)?;
        copy.identity.check_source().map_err(copy_failure)?;
        if windows_copy::capture(&copy.source, cancel.clone())
            .await
            .map_err(copy_failure)?
            .revision
            != manifest.revision
            || cancel.is_cancelled()
        {
            return Err(RuntimeError::WindowsCopyUnavailable.into());
        }
        let reference = reader.copy.clone();
        db.write_checked(move |tx| store::publish(tx, &reference))
            .await
            .internal("publish Windows copy revision")??;
    }
    Ok(())
}
impl RuntimeService {
    pub(crate) async fn reserve_windows_copy(
        &self,
        copy: PreparedCopy,
        run_id: String,
        unit: String,
    ) -> R<CopyPermit> {
        self.reconcile_windows_target(&copy.identity.key).await?;
        let guard = self.run_lock(&format!("copy:{}", copy.identity.key)).await;
        let execution = self.execution()?.clone();
        let db = self.db.clone();
        let cancel = self.stopping.child_token();
        let _cancel = cancel.clone().drop_guard();
        let (send, receive) = oneshot::channel();
        let owner = process_owner()?;
        let previous_attempt =
            crate::exit_record::ExitStore::new(&execution.paths.data_dir.join("runs"), &run_id)
                .internal("resolve previous Windows attempt")?
                .current_attempt()
                .internal("read previous Windows attempt")?;
        self.copy_tasks.spawn(async move {
            let _guard = guard;
            let launch_id = uuid::Uuid::new_v4().simple().to_string();
            // Capture happens here, after WSL prerequisites, not graph authorization.
            let manifest = windows_copy::capture(&copy.source, cancel.clone())
                .await
                .map_err(copy_failure);
            let result = match manifest {
                Ok(manifest) => {
                    let reader = Reader {
                        copy: CopyRef {
                            key: copy.identity.key.clone(),
                            revision: manifest.revision,
                            launch_id: launch_id.clone(),
                        },
                        project_id: copy.identity.project_id.clone(),
                        run_id,
                        unit,
                        state: ReaderState::Reserved,
                        owner,
                        previous_attempt,
                    };
                    coordinate(&db, &execution, &copy, &reader, &cancel)
                        .await
                        .map(|()| reader.copy)
                }
                Err(error) => Err(error),
            };
            if result.is_err() || cancel.is_cancelled() || send.is_closed() {
                // No publisher is detached: it completed before this reservation can be released.
                let id = launch_id.clone();
                let _ = db.write(move |tx| store::cancel_reserved(tx, &id)).await;
                let error = result
                    .err()
                    .unwrap_or_else(|| RuntimeError::WindowsCopyUnavailable.into());
                let _ = send.send(Err(error));
            } else if send
                .send(result.map(|reference| CopyPermit {
                    reference,
                    db: db.clone(),
                }))
                .is_err()
            {
                let _ = db
                    .write(move |tx| store::cancel_reserved(tx, &launch_id))
                    .await;
            }
        });
        receive
            .await
            .internal("Windows copy coordinator completion")?
    }
    pub(crate) async fn seal_windows_copy(&self, launch: &str) -> R<()> {
        let launch = launch.to_owned();
        self.db
            .write(move |tx| store::seal(tx, &launch))
            .await
            .internal("seal Windows copy consumer")?;
        Ok(())
    }
}
