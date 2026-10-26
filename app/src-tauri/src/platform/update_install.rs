//! Native installation orchestration; irreversible authorization stays under Rust locks.
use super::{
    native_activity::Activity,
    quit::{self, QuitState},
    update::Updates,
    update_runner::{self, Prepared},
};
use crate::core::update::{InstallSummary, Snapshot, UpdateError, runner::Binding};
use tauri::{AppHandle, Manager, State};
#[tauri::command]
pub fn update_install_summary(
    app: AppHandle,
    state: State<'_, Updates>,
    generation: u64,
) -> InstallSummary {
    let lifecycle = app.state::<QuitState>();
    let quit = lifecycle.state.lock().expect("quit state");
    let (native, epoch) = app.state::<Activity>().refresh();
    state
        .inner()
        .0
        .install_summary(generation, native, epoch, quit.epoch())
}
#[tauri::command]
pub fn update_install_confirm(
    app: AppHandle,
    state: State<'_, Updates>,
    generation: u64,
    confirmation_id: u64,
) -> Result<Snapshot, UpdateError> {
    let lifecycle = app.state::<QuitState>();
    let quit = lifecycle.state.lock().expect("quit state");
    if quit.sealed() {
        return Err(UpdateError::StaleOperation);
    }
    let (native, epoch) = app.state::<Activity>().refresh();
    let work =
        state
            .inner()
            .0
            .begin_install(generation, confirmation_id, native, epoch, quit.epoch())?;
    let snapshot = state.inner().0.snapshot();
    drop(quit);
    let service = state.inner().0.clone();
    tauri::async_runtime::spawn(async move {
        let generation = work.generation;
        let round = match quit::begin(&app) {
            Ok(round) => round,
            Err(_) => {
                service.install_aborted(generation, UpdateError::QuitRejected);
                return;
            }
        };
        let source = match work.proof.sealed_source() {
            Ok(source) => source,
            Err(error) => {
                quit::abort(&app, round);
                service.install_aborted(generation, error);
                return;
            }
        };
        let binding = Binding {
            operation: generation,
            update_generation: generation,
            quit_generation: round,
            target_version: work.version,
            fixture_install: work.fixture_install,
        };
        let preparation = update_runner::prepare(&app, binding, source, work.token.clone());
        tokio::pin!(preparation);
        let result = tokio::select! {
            result = &mut preparation => result,
            _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {
                work.token.cancel();
                service.install_quarantined(generation);
                app.state::<QuitState>().state.lock().expect("quit state").hold();
                // A blocked filesystem call cannot be killed safely. Retain this one worker,
                // expose unknown cleanup, and admit no replacement until its result is known.
                preparation.await
            }
        };
        let prepared = match result {
            Ok(prepared) => prepared,
            Err(failed) => {
                if let Some(mut quarantine) = failed.quarantine {
                    service.install_quarantined(generation);
                    disarm(&app, round, &mut quarantine).await;
                }
                quit::abort(&app, round);
                service.install_aborted(
                    generation,
                    if work.token.is_cancelled()
                        || failed.error == crate::core::update::runner::RunnerError::Cancelled
                    {
                        UpdateError::Cancelled
                    } else {
                        UpdateError::Runner
                    },
                );
                return;
            }
        };
        // The runner has its frozen copy. Keep only Service's retry proof until commit,
        // so successful commit can clean the download before Tauri's process::exit.
        drop(work.proof);
        let mut prepared = prepared;
        if quit::prepare_round(&app, round, &work.token).await.is_err() {
            service.install_quarantined(generation);
            disarm(&app, round, &mut prepared).await;
            quit::abort(&app, round);
            service.install_aborted(
                generation,
                if work.token.is_cancelled() {
                    UpdateError::Cancelled
                } else {
                    UpdateError::QuitRejected
                },
            );
            return;
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        // Retain abort authority even when UI scheduling or delivery fails.
        let prepared = std::sync::Arc::new(std::sync::Mutex::new(Some(prepared)));
        let ui_prepared = prepared.clone();
        let ui = app.clone();
        let ui_service = service.clone();
        let scheduled = app.run_on_main_thread(move || {
            let lifecycle = ui.state::<QuitState>();
            let mut quit = lifecycle.state.lock().expect("quit state");
            let native = ui.state::<Activity>().refresh();
            let result = ui_service.commit_after_quit(
                &mut quit,
                crate::core::update::CommitFence {
                    generation,
                    round,
                    labels: &quit::labels(&ui),
                    native,
                    expected_native_epoch: epoch,
                },
                || {
                    ui_prepared
                        .lock()
                        .expect("install runner")
                        .as_mut()
                        .ok_or(UpdateError::Runner)?
                        .commit()
                        .map_err(|_| UpdateError::Runner)
                },
            );
            drop(quit);
            if result.is_ok() {
                ui.exit(0);
            }
            let _ = tx.send(result);
        });
        let result = if scheduled.is_ok() {
            rx.await.unwrap_or(Err(UpdateError::Runner))
        } else {
            Err(UpdateError::Runner)
        };
        if let Err(error) = result {
            if service.installation_committed(generation) {
                return;
            }
            let runner = prepared.lock().expect("install runner").take();
            if let Some(mut runner) = runner {
                service.install_quarantined(generation);
                disarm(&app, round, &mut runner).await;
            }
            quit::abort(&app, round);
            service.install_aborted(generation, error);
        }
        // Service retains the original retry proof until commit or a later explicit operation.
    });
    Ok(snapshot)
}
async fn disarm(app: &AppHandle, round: u64, runner: &mut Prepared) {
    app.state::<QuitState>()
        .state
        .lock()
        .expect("quit state")
        .hold();
    // No commit was issued. Existing documents can resume while admission remains held.
    quit::resume_documents(app, round);
    loop {
        if matches!(runner.abort().await, Ok(true)) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}
