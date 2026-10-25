//! One native waiter and four active Windows children. A lost generation is never replayed.
use devbox_protocol::windows::{RunWindows, WindowsEvent, WindowsOutput};
use std::{collections::HashMap, sync::Mutex};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
mod probes;
#[derive(Default)]
pub(super) struct WindowsBroker {
    state: Mutex<State>,
    ready: tokio::sync::Notify,
    probes: probes::Probes,
}
#[derive(Default)]
struct State {
    waiter: Option<Waiter>,
    active: HashMap<(String, String), Active>,
}
struct Waiter {
    generation: u64,
    cancel: CancellationToken,
    sender: oneshot::Sender<RunWindows>,
}
struct Active {
    generation: u64,
    cancel: CancellationToken,
    sender: mpsc::Sender<WindowsEvent>,
}
#[derive(Debug, thiserror::Error)]
#[error("Windows native execution is unavailable or its result is unknown")]
pub(super) struct Unavailable;
impl WindowsBroker {
    pub async fn next(
        &self,
        generation: u64,
        cancel: CancellationToken,
    ) -> Result<RunWindows, Unavailable> {
        let (sender, receiver) = oneshot::channel();
        {
            let mut state = self.state.lock().expect("Windows broker");
            state
                .active
                .retain(|_, v| !v.cancel.is_cancelled() && !v.sender.is_closed());
            if state
                .waiter
                .as_ref()
                .is_some_and(|w| !w.cancel.is_cancelled())
            {
                return Err(Unavailable);
            }
            state.waiter = Some(Waiter {
                generation,
                cancel: cancel.clone(),
                sender,
            });
        }
        self.ready.notify_waiters();
        tokio::select! { _=cancel.cancelled()=>Err(Unavailable), request=receiver=>request.map_err(|_|Unavailable) }
    }
    pub async fn submit(
        &self,
        request: RunWindows,
        cancel: CancellationToken,
    ) -> Result<(mpsc::Receiver<WindowsEvent>, CancellationToken), Unavailable> {
        if !request.tracked() {
            return Err(Unavailable);
        }
        let waiting = async {
            loop {
                let notified = self.ready.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                let available = self
                    .state
                    .lock()
                    .expect("Windows broker")
                    .waiter
                    .as_ref()
                    .is_some_and(|w| !w.cancel.is_cancelled());
                if available {
                    return self.submit_now(request);
                }
                tokio::select! {_=cancel.cancelled()=>return Err(Unavailable),_=notified=>()}
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(2), waiting)
            .await
            .map_err(|_| Unavailable)?
    }
    fn submit_now(
        &self,
        request: RunWindows,
    ) -> Result<(mpsc::Receiver<WindowsEvent>, CancellationToken), Unavailable> {
        let key = (request.run_id.clone(), request.attempt_id.clone());
        let mut state = self.state.lock().expect("Windows broker");
        state
            .active
            .retain(|_, v| !v.cancel.is_cancelled() && !v.sender.is_closed());
        if state.active.contains_key(&key) || state.active.len() >= 4 {
            return Err(Unavailable);
        }
        let waiter = state
            .waiter
            .take()
            .filter(|w| !w.cancel.is_cancelled())
            .ok_or(Unavailable)?;
        let (sender, receiver) = mpsc::channel(16);
        state.active.insert(
            key.clone(),
            Active {
                generation: waiter.generation,
                cancel: waiter.cancel.clone(),
                sender,
            },
        );
        if waiter.sender.send(request).is_err() {
            state.active.remove(&key);
            return Err(Unavailable);
        }
        Ok((receiver, waiter.cancel))
    }
    pub async fn output(&self, generation: u64, event: WindowsEvent) -> Result<(), Unavailable> {
        let key = (event.run_id.clone(), event.attempt_id.clone());
        if matches!(&event.event,WindowsOutput::Stdout{data}|WindowsOutput::Stderr{data} if data.len()>8192)
        {
            return Err(Unavailable);
        }
        let finished = matches!(
            event.event,
            WindowsOutput::Finished { .. } | WindowsOutput::Failed
        );
        let (sender, cancel) = {
            let state = self.state.lock().expect("Windows broker");
            let active = state
                .active
                .get(&key)
                .filter(|v| v.generation == generation && !v.cancel.is_cancelled())
                .ok_or(Unavailable)?;
            (active.sender.clone(), active.cancel.clone())
        };
        let sent =
            tokio::select! {_=cancel.cancelled()=>false,result=sender.send(event)=>result.is_ok()};
        if finished || !sent {
            self.state
                .lock()
                .expect("Windows broker")
                .active
                .remove(&key);
        }
        if sent { Ok(()) } else { Err(Unavailable) }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    fn request() -> RunWindows {
        RunWindows {
            run_id: "run".into(),
            attempt_id: "attempt".into(),
            cwd: "C:\\owned".into(),
            command: "owned".into(),
            env: Default::default(),
            ports: vec![],
            observation: devbox_protocol::windows::ObservationControl {
                schema: 1,
                launch_id: "owned-launch".into(),
                nonce: "0123456789abcdef0123456789abcdef".into(),
                job_name: "Local\\Devbox.Observe.0123456789abcdef0123456789abcdef".into(),
                assigned_path: "C:\\owned\\control\\assigned.json".into(),
                grant_path: "C:\\owned\\control\\grant.json".into(),
                closed_path: "C:\\owned\\control\\closed.json".into(),
            },
        }
    }
    fn output(n: u8) -> WindowsEvent {
        WindowsEvent {
            run_id: "run".into(),
            attempt_id: "attempt".into(),
            event: WindowsOutput::Stdout { data: vec![n] },
        }
    }
    async fn waiting(
        broker: &Arc<WindowsBroker>,
        generation: u64,
        cancel: CancellationToken,
    ) -> tokio::task::JoinHandle<Result<RunWindows, Unavailable>> {
        let owner = broker.clone();
        let task = tokio::spawn(async move { owner.next(generation, cancel).await });
        loop {
            if broker
                .state
                .lock()
                .expect("fixture result")
                .waiter
                .is_some()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        task
    }
    #[tokio::test]
    async fn windows_broker_preserves_identity_stream_exit_and_backpressure() {
        let broker = Arc::new(WindowsBroker::default());
        let cancel = CancellationToken::new();
        let native = waiting(&broker, 7, cancel).await;
        let (mut events, _) = broker
            .submit(request(), CancellationToken::new())
            .await
            .expect("fixture result");
        let request = native
            .await
            .expect("fixture result")
            .expect("fixture result");
        assert_eq!(request.attempt_id, "attempt");
        for n in 0..16 {
            broker.output(7, output(n)).await.expect("fixture result");
        }
        let b = broker.clone();
        let blocked = tokio::spawn(async move { b.output(7, output(16)).await });
        tokio::task::yield_now().await;
        assert!(!blocked.is_finished());
        for n in 0..17 {
            let e = events.recv().await.expect("fixture value");
            assert!(matches!(e.event,WindowsOutput::Stdout{data} if data==[n]));
        }
        blocked
            .await
            .expect("fixture result")
            .expect("fixture result");
        broker
            .output(
                7,
                WindowsEvent {
                    run_id: "run".into(),
                    attempt_id: "attempt".into(),
                    event: WindowsOutput::Stderr {
                        data: b"err".to_vec(),
                    },
                },
            )
            .await
            .expect("fixture result");
        broker
            .output(
                7,
                WindowsEvent {
                    run_id: "run".into(),
                    attempt_id: "attempt".into(),
                    event: WindowsOutput::Finished { code: 78 },
                },
            )
            .await
            .expect("fixture result");
        assert!(matches!(
            events.recv().await.expect("fixture value").event,
            WindowsOutput::Stderr { .. }
        ));
        assert!(matches!(
            events.recv().await.expect("fixture value").event,
            WindowsOutput::Finished { code: 78 }
        ));
        assert!(events.recv().await.is_none());
    }
    #[tokio::test]
    async fn windows_broker_disconnect_and_late_other_generation_output_fail() {
        let broker = Arc::new(WindowsBroker::default());
        let cancel = CancellationToken::new();
        let native = waiting(&broker, 1, cancel.clone()).await;
        let (_, lost) = broker
            .submit(request(), CancellationToken::new())
            .await
            .expect("fixture result");
        native
            .await
            .expect("fixture result")
            .expect("fixture result");
        assert!(broker.output(2, output(1)).await.is_err());
        cancel.cancel();
        assert!(lost.is_cancelled());
        assert!(broker.output(1, output(2)).await.is_err());
        assert!(
            broker
                .submit(request(), CancellationToken::new())
                .await
                .is_err()
        );
    }
}
