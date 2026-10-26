//! Read-only App0 probes have their own waiter, so four running children cannot starve reconciliation.
use super::{Unavailable, WindowsBroker};
use devbox_protocol::windows::*;
use std::{collections::HashMap, sync::Mutex, time::Duration};
use tokio::sync::{Notify, Semaphore, oneshot};
use tokio_util::sync::CancellationToken;

pub(super) struct Probes {
    state: Mutex<State>,
    ready: Notify,
    slots: Semaphore,
}
impl Default for Probes {
    fn default() -> Self {
        Self {
            state: Mutex::default(),
            ready: Notify::new(),
            slots: Semaphore::new(4),
        }
    }
}
#[derive(Default)]
struct State {
    waiter: Option<Waiter>,
    active: HashMap<String, Active>,
}
struct Waiter {
    generation: u64,
    cancel: CancellationToken,
    sender: oneshot::Sender<WindowsProbeRequest>,
}
struct Active {
    generation: u64,
    cancel: CancellationToken,
    probe: WindowsProbe,
    sender: oneshot::Sender<WindowsProbeResult>,
}
impl WindowsBroker {
    pub(in crate::daemon) async fn probe_next(
        &self,
        generation: u64,
        cancel: CancellationToken,
    ) -> Result<WindowsProbeRequest, Unavailable> {
        let (sender, receiver) = oneshot::channel();
        {
            let mut state = self.probes.state.lock().expect("Windows probes");
            state
                .active
                .retain(|_, p| !p.cancel.is_cancelled() && !p.sender.is_closed());
            if state
                .waiter
                .as_ref()
                .is_some_and(|w| !w.cancel.is_cancelled() && !w.sender.is_closed())
            {
                return Err(Unavailable);
            }
            state.waiter = Some(Waiter {
                generation,
                cancel: cancel.clone(),
                sender,
            });
        }
        self.probes.ready.notify_waiters();
        tokio::select! {_=cancel.cancelled()=>Err(Unavailable),request=receiver=>request.map_err(|_|Unavailable)}
    }
    pub(in crate::daemon) async fn probe(
        &self,
        probe: WindowsProbe,
        cancel: CancellationToken,
    ) -> Result<WindowsProbeResult, Unavailable> {
        if !probe.valid()
            || serde_json::to_vec(&probe).map_err(|_| Unavailable)?.len() > MAX_WINDOWS_PROBE_BYTES
        {
            return Err(Unavailable);
        }
        let _permit = self.probes.slots.try_acquire().map_err(|_| Unavailable)?;
        let id = uuid::Uuid::new_v4().simple().to_string();
        let waiting = async {
            loop {
                let notified = self.probes.ready.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                let available = self
                    .probes
                    .state
                    .lock()
                    .expect("Windows probes")
                    .waiter
                    .as_ref()
                    .is_some_and(|w| !w.cancel.is_cancelled() && !w.sender.is_closed());
                if available {
                    return self.submit_probe(&id, probe);
                }
                tokio::select! {_=cancel.cancelled()=>return Err(Unavailable),_=notified=>()}
            }
        };
        let (receiver, native_cancel) = tokio::time::timeout(Duration::from_secs(2), waiting)
            .await
            .map_err(|_| Unavailable)??;
        let result = tokio::select! {
            biased;
            _=cancel.cancelled()=>Err(Unavailable),
            _=native_cancel.cancelled()=>Err(Unavailable),
            result=tokio::time::timeout(Duration::from_secs(6),receiver)=>match result {Ok(Ok(result))=>Ok(result),_=>Err(Unavailable)},
        };
        self.probes
            .state
            .lock()
            .expect("Windows probes")
            .active
            .remove(&id);
        if cancel.is_cancelled() || native_cancel.is_cancelled() {
            Err(Unavailable)
        } else {
            result
        }
    }
    fn submit_probe(
        &self,
        id: &str,
        probe: WindowsProbe,
    ) -> Result<(oneshot::Receiver<WindowsProbeResult>, CancellationToken), Unavailable> {
        let mut state = self.probes.state.lock().expect("Windows probes");
        state
            .active
            .retain(|_, p| !p.cancel.is_cancelled() && !p.sender.is_closed());
        if state.active.len() >= 4 {
            return Err(Unavailable);
        }
        let waiter = state
            .waiter
            .take()
            .filter(|w| !w.cancel.is_cancelled() && !w.sender.is_closed())
            .ok_or(Unavailable)?;
        let (sender, receiver) = oneshot::channel();
        state.active.insert(
            id.into(),
            Active {
                generation: waiter.generation,
                cancel: waiter.cancel.clone(),
                probe: probe.clone(),
                sender,
            },
        );
        if waiter
            .sender
            .send(WindowsProbeRequest {
                probe_id: id.into(),
                probe,
            })
            .is_err()
        {
            state.active.remove(id);
            return Err(Unavailable);
        }
        Ok((receiver, waiter.cancel))
    }
    pub(in crate::daemon) fn probe_output(
        &self,
        generation: u64,
        reply: WindowsProbeReply,
    ) -> Result<(), Unavailable> {
        let mut state = self.probes.state.lock().expect("Windows probes");
        let active = state
            .active
            .get(&reply.probe_id)
            .filter(|p| p.generation == generation && !p.cancel.is_cancelled())
            .ok_or(Unavailable)?;
        if !reply.result.valid_for(&active.probe) {
            return Err(Unavailable);
        }
        let active = state
            .active
            .remove(&reply.probe_id)
            .expect("validated probe");
        active.sender.send(reply.result).map_err(|_| Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    fn request() -> WindowsProbe {
        WindowsProbe::Paths {
            paths: vec!["C:\\owned".into()],
        }
    }
    async fn waiting(
        broker: &Arc<WindowsBroker>,
        generation: u64,
        cancel: CancellationToken,
    ) -> tokio::task::JoinHandle<Result<WindowsProbeRequest, Unavailable>> {
        let b = broker.clone();
        let task = tokio::spawn(async move { b.probe_next(generation, cancel).await });
        while broker.probes.state.lock().expect("state").waiter.is_none() {
            tokio::task::yield_now().await;
        }
        task
    }
    #[tokio::test]
    async fn windows_probe_reply_is_generation_and_request_bound_and_single_use() {
        let b = Arc::new(WindowsBroker::default());
        let native = waiting(&b, 7, CancellationToken::new()).await;
        let owner = b.clone();
        let caller =
            tokio::spawn(async move { owner.probe(request(), CancellationToken::new()).await });
        let request = native.await.expect("native").expect("probe");
        let reply = WindowsProbeReply {
            probe_id: request.probe_id,
            result: WindowsProbeResult::Paths {
                state: PathObservation::Safe,
            },
        };
        assert!(b.probe_output(8, reply.clone()).is_err());
        let mut wrong = reply.clone();
        wrong.probe_id.push('x');
        assert!(b.probe_output(7, wrong).is_err());
        let mut wrong = reply.clone();
        wrong.result = WindowsProbeResult::Observation {
            state: ObservationState::Empty,
        };
        assert!(b.probe_output(7, wrong).is_err());
        b.probe_output(7, reply.clone()).expect("exact owner");
        assert_eq!(caller.await.expect("caller").expect("reply"), reply.result);
        assert!(b.probe_output(7, reply).is_err());
    }
    #[tokio::test]
    async fn windows_probe_cancel_or_native_disconnect_is_unavailable_never_empty() {
        for disconnect in [false, true] {
            let b = Arc::new(WindowsBroker::default());
            let native_cancel = CancellationToken::new();
            let caller_cancel = CancellationToken::new();
            let native = waiting(&b, 1, native_cancel.clone()).await;
            let owner = b.clone();
            let token = caller_cancel.clone();
            let caller = tokio::spawn(async move { owner.probe(request(), token).await });
            let request = native.await.expect("native").expect("request");
            if disconnect {
                native_cancel.cancel();
            } else {
                caller_cancel.cancel();
            }
            assert!(caller.await.expect("caller").is_err());
            assert!(
                b.probe_output(
                    1,
                    WindowsProbeReply {
                        probe_id: request.probe_id,
                        result: WindowsProbeResult::Paths {
                            state: PathObservation::Safe
                        }
                    }
                )
                .is_err()
            );
        }
    }
    #[tokio::test]
    async fn windows_probe_is_not_starved_by_four_active_run_consumers() {
        let b = Arc::new(WindowsBroker::default());
        let mut readers = vec![];
        for n in 0..4 {
            let owner = b.clone();
            let native = tokio::spawn(async move { owner.next(7, CancellationToken::new()).await });
            let nonce = format!("{n:032x}");
            let run = RunWindows {
                run_id: format!("run-{n}"),
                attempt_id: "attempt".into(),
                cwd: "C:\\owned".into(),
                command: "fixture".into(),
                env: Default::default(),
                ports: vec![],
                observation: ObservationControl {
                    schema: 1,
                    launch_id: format!("launch-{n}"),
                    job_name: format!("Local\\Devbox.Observe.{nonce}"),
                    nonce,
                    assigned_path: format!("C:\\owned\\{n}.assigned"),
                    grant_path: format!("C:\\owned\\{n}.grant"),
                    closed_path: format!("C:\\owned\\{n}.closed"),
                },
            };
            readers.push(
                b.submit(run, CancellationToken::new())
                    .await
                    .expect("run transport"),
            );
            native.await.expect("native").expect("run dispatched");
        }
        assert_eq!(b.state.lock().expect("runs").active.len(), 4);
        let native = waiting(&b, 7, CancellationToken::new()).await;
        let owner = b.clone();
        let caller =
            tokio::spawn(async move { owner.probe(request(), CancellationToken::new()).await });
        let probe = native
            .await
            .expect("native probe")
            .expect("probe independent of run slots");
        b.probe_output(
            7,
            WindowsProbeReply {
                probe_id: probe.probe_id,
                result: WindowsProbeResult::Paths {
                    state: PathObservation::Safe,
                },
            },
        )
        .expect("observation reply");
        assert_eq!(
            caller.await.expect("caller").expect("observed"),
            WindowsProbeResult::Paths {
                state: PathObservation::Safe
            }
        );
        assert_eq!(b.state.lock().expect("runs").active.len(), 4);
        assert_eq!(readers.len(), 4);
    }
}
