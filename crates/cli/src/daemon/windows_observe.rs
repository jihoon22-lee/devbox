//! Daemon-owned read-only probes. No renderer request chooses a receipt or probe path.
use super::windows_broker::WindowsBroker;
use devbox_protocol::windows::*;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub(super) fn configure(
    execution: &mut devbox_runtime::ExecutionConfig,
    broker: Arc<WindowsBroker>,
    cancel: CancellationToken,
) {
    let executable = execution.windows_interop.clone();
    let native = broker.clone();
    let stopping = cancel.clone();
    execution.windows_observation = Arc::new(move |assigned| {
        let executable = executable.clone();
        let native = native.clone();
        let cancel = stopping.child_token();
        Box::pin(async move {
            let observed =
                crate::windows_exec::observe(&executable, &assigned, cancel.clone()).await;
            if !matches!(
                observed,
                ObservationState::Unavailable | ObservationState::IdentityMismatch
            ) || cancel.is_cancelled()
            {
                return observed;
            }
            match native
                .probe(WindowsProbe::Observation { assigned }, cancel)
                .await
            {
                Ok(WindowsProbeResult::Observation { state }) => state,
                _ => ObservationState::Unavailable,
            }
        })
    });
    let executable = execution.windows_interop.clone();
    let native = broker.clone();
    let stopping = cancel.clone();
    execution.windows_listeners = Arc::new(move |assigned, ports| {
        let executable = executable.clone();
        let native = native.clone();
        let cancel = stopping.child_token();
        Box::pin(async move {
            let observed =
                crate::windows_exec::listeners(&executable, &assigned, &ports, cancel.clone())
                    .await;
            // Direct interop may run in another session; only the native broker can retry it.
            if !matches!(
                observed,
                ListenerObservation::Unavailable | ListenerObservation::IdentityMismatch
            ) || cancel.is_cancelled()
            {
                return observed;
            }
            match native
                .probe(WindowsProbe::Listeners { assigned, ports }, cancel)
                .await
            {
                Ok(WindowsProbeResult::Listeners { state }) => state,
                _ => ListenerObservation::Unavailable,
            }
        })
    });
    let executable = execution.windows_interop.clone();
    execution.windows_paths = Arc::new(move |paths| {
        let executable = executable.clone();
        let native = broker.clone();
        let cancel = cancel.child_token();
        Box::pin(async move {
            let observed =
                crate::windows_exec::check_paths(&executable, &paths, cancel.clone()).await;
            if observed != PathObservation::Unavailable || cancel.is_cancelled() {
                return observed;
            }
            match native.probe(WindowsProbe::Paths { paths }, cancel).await {
                Ok(WindowsProbeResult::Paths { state }) => state,
                _ => PathObservation::Unavailable,
            }
        })
    });
}

#[cfg(test)]
#[path = "windows_observe_tests.rs"]
mod tests;
