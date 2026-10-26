//! Internal, staged updater handoff. Renderer commands never provide paths or kernel handles.
mod execute;
mod native;
mod recovery;
use crate::core::{
    channel::CHANNEL,
    update::{
        SealedSource,
        runner::{
            Binding, ProcessIdentity, Result, RunnerError,
            files::{self, Outcome, SpawnRecord, Stage},
        },
    },
};
pub use recovery::{Recovery, RecoveryOutcome, acknowledge};
use std::{
    os::windows::io::OwnedHandle,
    path::PathBuf,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};
use tokio_util::sync::CancellationToken;
use windows::Win32::{
    Storage::FileSystem::FILE_GENERIC_READ,
    System::Threading::{
        EVENT_MODIFY_STATE, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    },
};

const PREPARE_TIME: Duration = Duration::from_secs(30);
pub fn inspect(app: &AppHandle) -> Result<Recovery> {
    recovery::inspect(app)
}
pub struct PrepareFailure {
    pub error: RunnerError,
    pub quarantine: Option<Prepared>,
}
impl From<RunnerError> for PrepareFailure {
    fn from(error: RunnerError) -> Self {
        Self {
            error,
            quarantine: None,
        }
    }
}
pub struct Prepared {
    stage: Option<Stage>,
    child: OwnedHandle,
    commit: OwnedHandle,
    disarm: OwnedHandle,
    armed: OwnedHandle,
    parent: ProcessIdentity,
    committed: bool,
    finished: bool,
}
impl Prepared {
    /// The successful SetEvent is irreversible. Call only under the shared final quit/admission gate.
    pub fn commit(&mut self) -> Result<()> {
        if self.committed || self.finished || native::exited(native::raw(&self.child))? {
            return Err(RunnerError::InvalidBinding);
        }
        let stage = self.stage.as_mut().ok_or(RunnerError::InvalidBinding)?;
        let current = native::current_process()?;
        if native::identity(native::raw(&current))? != self.parent {
            return Err(RunnerError::Identity);
        }
        stage.revalidate(&CancellationToken::new())?;
        if native::exited(native::raw(&self.child))? {
            return Err(RunnerError::Timeout);
        }
        // Durable startup guard precedes authority. A record alone can never authorize installation.
        files::record(
            &stage.manifest,
            native::identity(native::raw(&self.child))?,
            Outcome::Committed,
            None,
            None,
        )?;
        native::signal(&self.commit)?;
        self.committed = true;
        // The runner owns the immutable copies from this point. Parent shutdown cannot delete them.
        stage.keep();
        Ok(())
    }
    /// False/Err retains this Prepared and its files in the coordinator's quarantine.
    pub async fn abort(&mut self) -> Result<bool> {
        if self.committed {
            return Err(RunnerError::InvalidBinding);
        }
        if self.finished {
            return Ok(true);
        }
        let _ = native::signal(&self.disarm);
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if native::exited(native::raw(&self.child))? {
                self.finish_abort();
                return Ok(true);
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        // Exact handle to our own uncommitted runner, never an installer or a caller PID.
        native::terminate_prepared(native::raw(&self.child))?;
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if native::exited(native::raw(&self.child))? {
                self.finish_abort();
                return Ok(true);
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        Ok(false)
    }
    fn finish_abort(&mut self) {
        self.finished = true;
        self.stage.take();
    }
}
impl Drop for Prepared {
    fn drop(&mut self) {
        if !self.committed
            && !self.finished
            && !matches!(
                native::abort_prepared(native::raw(&self.child), &self.disarm),
                Ok(true)
            )
            && let Some(stage) = &mut self.stage
        {
            // Unknown lifetime is retained on disk; the normal owner keeps the whole Prepared too.
            stage.keep();
        }
    }
}
struct CancelOnDrop(CancellationToken);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

pub async fn prepare(
    app: &AppHandle,
    binding: Binding,
    source: SealedSource,
    cancel: CancellationToken,
) -> std::result::Result<Prepared, PrepareFailure> {
    let root = app
        .path()
        .app_local_data_dir()
        .map_err(|_| RunnerError::Storage)?
        .join("updates");
    // Verify both startup inspection and Tauri use exactly the same channel directory.
    if std::fs::canonicalize(&root).map_err(|_| RunnerError::Storage)?
        != std::fs::canonicalize(native::local_root()?).map_err(|_| RunnerError::Storage)?
    {
        return Err(RunnerError::Identity.into());
    }
    let stop = cancel.child_token();
    let _cancel_on_drop = CancelOnDrop(stop.clone());
    let work_stop = stop.clone();
    let mut task =
        tokio::task::spawn_blocking(move || prepare_blocking(root, binding, source, work_stop));
    let (result, timed_out) = tokio::select! {
        result = &mut task => (result, false),
        _ = tokio::time::sleep(PREPARE_TIME) => { stop.cancel(); (task.await, true) },
    };
    let result = result.map_err(|_| PrepareFailure::from(RunnerError::Unknown))?;
    if timed_out {
        match result {
            Ok(mut ready) => {
                let exited = ready.abort().await.unwrap_or(false);
                Err(PrepareFailure {
                    error: RunnerError::Timeout,
                    quarantine: (!exited).then_some(ready),
                })
            }
            Err(mut failed) => {
                failed.error = RunnerError::Timeout;
                Err(failed)
            }
        }
    } else {
        result
    }
}
fn prepare_blocking(
    root: PathBuf,
    binding: Binding,
    source: SealedSource,
    cancel: CancellationToken,
) -> std::result::Result<Prepared, PrepareFailure> {
    let process = native::current_process()?;
    let parent = native::identity(native::raw(&process))?;
    let parent_exe = native::image(native::raw(&process))?;
    let stage = Stage::create(
        &root,
        &parent_exe,
        source,
        binding,
        CHANNEL,
        parent,
        &cancel,
    )?;
    let commit = native::event()?;
    let disarm = native::event()?;
    let armed = native::event()?;
    // The runner can wait for commit/disarm but cannot signal either authorization event.
    let inherited = [
        native::duplicate(
            native::raw(&process),
            (PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE).0,
            true,
        )?,
        native::duplicate(native::raw(&stage.installer), FILE_GENERIC_READ.0, true)?,
        native::duplicate(native::raw(&stage.payload), FILE_GENERIC_READ.0, true)?,
        native::duplicate(native::raw(&commit), PROCESS_SYNCHRONIZE.0, true)?,
        native::duplicate(native::raw(&disarm), PROCESS_SYNCHRONIZE.0, true)?,
        native::duplicate(native::raw(&armed), EVENT_MODIFY_STATE.0, true)?,
    ];
    if cancel.is_cancelled() {
        return Err(RunnerError::Cancelled.into());
    }
    let child = native::spawn_runner(&stage.manifest.runner_exe, &inherited)?;
    drop(inherited);
    let mut ready = Prepared {
        stage: Some(stage),
        child,
        commit,
        disarm,
        armed,
        parent,
        committed: false,
        finished: false,
    };
    let validation = (|| {
        if !native::outside_job(native::raw(&ready.child))? {
            return Err(RunnerError::Spawn);
        }
        let runner = native::identity(native::raw(&ready.child))?;
        let stage = ready.stage.as_ref().ok_or(RunnerError::InvalidBinding)?;
        files::write_new(
            &stage
                .manifest
                .runner_exe
                .parent()
                .ok_or(RunnerError::Storage)?
                .join("spawn.json"),
            &SpawnRecord {
                nonce: stage.manifest.nonce.clone(),
                runner,
            },
        )?;
        let deadline = Instant::now() + PREPARE_TIME;
        loop {
            if cancel.is_cancelled() {
                return Err(RunnerError::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(RunnerError::Timeout);
            }
            match native::wait_any(
                &[native::raw(&ready.armed), native::raw(&ready.child)],
                Duration::from_millis(25),
            )? {
                Some(0) if !native::exited(native::raw(&ready.child))? => {
                    let record = files::read_record(
                        &stage
                            .manifest
                            .runner_exe
                            .parent()
                            .ok_or(RunnerError::Identity)?
                            .join("outcome.json"),
                    )?;
                    if record.schema != 1
                        || record.nonce != stage.manifest.nonce
                        || record.binding != stage.manifest.binding
                        || record.channel != stage.manifest.channel
                        || record.runner != runner
                        || record.outcome != Outcome::Armed
                    {
                        return Err(RunnerError::Identity);
                    }
                    return Ok(());
                }
                Some(_) => return Err(RunnerError::Spawn),
                None => {}
            }
        }
    })();
    match validation {
        Ok(()) => Ok(ready),
        Err(error) => {
            let exited =
                native::abort_prepared(native::raw(&ready.child), &ready.disarm).unwrap_or(false);
            if exited {
                ready.finish_abort();
            }
            Err(PrepareFailure {
                error,
                quarantine: (!exited).then_some(ready),
            })
        }
    }
}
pub fn entry() -> Option<i32> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.iter().any(|arg| arg == "--update-runner") {
        if arguments.first().is_none_or(|arg| arg != "--update-runner") {
            return Some(2);
        }
        return Some(if execute::run(&arguments[1..]).is_ok() {
            0
        } else {
            2
        });
    }
    if arguments.iter().any(|arg| arg == "--self-check") {
        return None;
    }
    if recovery::startup_blocked(native::local_root().and_then(|root| recovery::early_busy(&root)))
    {
        eprintln!(
            "업데이트가 진행 중이거나 설치 종료를 확인하지 못했습니다. 설치 상태를 확인한 뒤 앱을 다시 시작하세요."
        );
        Some(3)
    } else {
        None
    }
}
