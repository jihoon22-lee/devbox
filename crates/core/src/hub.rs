use devbox_protocol::Topic;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

pub const QUEUE: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HubMessage {
    Event { rev: u64, payload: Arc<str> },
    Resync,
}

#[derive(Default)]
struct State {
    events: VecDeque<HubMessage>,
    lagged: bool,
    receiver_closed: bool,
    publisher_closed: bool,
}

#[derive(Default)]
struct Sub {
    state: Mutex<State>,
    notify: Notify,
}

#[derive(Default)]
pub struct Hub {
    subs: Mutex<HashMap<String, Vec<Arc<Sub>>>>,
}

pub struct HubReceiver {
    sub: Arc<Sub>,
}

impl Hub {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn subscribe(&self, topic: &str) -> HubReceiver {
        let sub = Arc::new(Sub::default());
        self.subs
            .lock()
            .expect("hub lock")
            .entry(topic.to_owned())
            .or_default()
            .push(sub.clone());
        HubReceiver { sub }
    }

    pub fn publish<T: Topic>(&self, rev: u64, payload: &T::Payload) {
        let payload: Arc<str> = serde_json::to_string(payload)
            .expect("topic payloads serialize")
            .into();
        let mut subs = self.subs.lock().expect("hub lock");
        if let Some(list) = subs.get_mut(T::NAME) {
            list.retain(|sub| {
                let mut state = sub.state.lock().expect("subscription lock");
                if state.receiver_closed {
                    return false;
                }
                // Pending resync subsumes every publication until it is consumed.
                if !state.lagged {
                    if state.events.len() == QUEUE {
                        state.events.clear();
                        state.lagged = true;
                    } else {
                        state.events.push_back(HubMessage::Event {
                            rev,
                            payload: payload.clone(),
                        });
                    }
                }
                drop(state);
                sub.notify.notify_one();
                true
            });
        }
    }

    pub fn subscriber_count(&self, topic: &str) -> usize {
        let mut subs = self.subs.lock().expect("hub lock");
        subs.get_mut(topic)
            .map(|list| {
                list.retain(|sub| !sub.state.lock().expect("subscription lock").receiver_closed);
                list.len()
            })
            .unwrap_or(0)
    }
}

impl Drop for Hub {
    fn drop(&mut self) {
        for sub in self.subs.get_mut().expect("hub lock").values().flatten() {
            sub.state
                .lock()
                .expect("subscription lock")
                .publisher_closed = true;
            sub.notify.notify_one();
        }
    }
}

impl HubReceiver {
    pub async fn recv(&mut self) -> Option<HubMessage> {
        loop {
            let notified = self.sub.notify.notified();
            {
                let mut state = self.sub.state.lock().expect("subscription lock");
                // The same lock orders overflow, resync consumption and new events.
                if state.lagged {
                    state.lagged = false;
                    return Some(HubMessage::Resync);
                }
                if let Some(event) = state.events.pop_front() {
                    return Some(event);
                }
                if state.publisher_closed {
                    return None;
                }
            }
            // notify_one retains a permit if publication precedes this await.
            notified.await;
        }
    }
}

impl Drop for HubReceiver {
    fn drop(&mut self) {
        let mut state = self.sub.state.lock().expect("subscription lock");
        state.receiver_closed = true;
        state.events.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use devbox_protocol::topic;
    use serde::Serialize;
    use std::future::{Future, poll_fn};
    use std::sync::{Arc, Barrier};
    use std::time::Duration;

    #[derive(Serialize, devbox_protocol::ts_rs::TS)]
    #[ts(crate = "devbox_protocol::ts_rs")]
    struct Ping {
        n: u32,
    }
    topic!(PingTopic = "test.ping", Ping);

    fn event(n: u32) -> Option<HubMessage> {
        Some(HubMessage::Event {
            rev: n.into(),
            payload: format!("{{\"n\":{n}}}").into(),
        })
    }

    async fn receive(receiver: &mut HubReceiver) -> Option<HubMessage> {
        tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await
            .expect("receiver made progress")
    }

    async fn receive_after_waiting(
        mut receiver: HubReceiver,
        ready: tokio::sync::oneshot::Sender<()>,
    ) -> Option<HubMessage> {
        let mut ready = Some(ready);
        let mut receive = std::pin::pin!(receiver.recv());
        tokio::time::timeout(
            Duration::from_secs(5),
            poll_fn(|cx| {
                let result = receive.as_mut().poll(cx);
                if result.is_pending() {
                    if let Some(ready) = ready.take() {
                        ready.send(()).expect("test coordinator alive");
                    }
                }
                result
            }),
        )
        .await
        .expect("waiting receiver woke")
    }

    #[tokio::test]
    async fn delivers_events_in_order_to_matching_topics_only() {
        let hub = Hub::new();
        let mut a = hub.subscribe("test.ping");
        let mut other = hub.subscribe("test.other");
        for n in 1..=2 {
            hub.publish::<PingTopic>(n.into(), &Ping { n });
        }
        assert_eq!(receive(&mut a).await, event(1));
        assert_eq!(receive(&mut a).await, event(2));
        drop(hub);
        assert_eq!(receive(&mut other).await, None);
    }

    #[tokio::test]
    async fn overflow_coalesces_and_fast_subscriber_is_unaffected() {
        let hub = Hub::new();
        let mut slow = hub.subscribe("test.ping");
        let mut fast = hub.subscribe("test.ping");
        for n in 0..(QUEUE as u32 + 50) {
            hub.publish::<PingTopic>(n.into(), &Ping { n });
            assert_eq!(receive(&mut fast).await, event(n));
        }
        assert_eq!(receive(&mut slow).await, Some(HubMessage::Resync));
        hub.publish::<PingTopic>(999, &Ping { n: 999 });
        assert_eq!(receive(&mut slow).await, event(999));
        drop(hub);
        assert_eq!(receive(&mut slow).await, None);
    }

    #[tokio::test]
    async fn exactly_queue_events_do_not_overflow() {
        let hub = Hub::new();
        let mut receiver = hub.subscribe("test.ping");
        for n in 0..QUEUE as u32 {
            hub.publish::<PingTopic>(n.into(), &Ping { n });
        }
        for n in 0..QUEUE as u32 {
            assert_eq!(receive(&mut receiver).await, event(n));
        }
    }

    #[test]
    fn dropped_receivers_are_removed() {
        let hub = Hub::new();
        drop(hub.subscribe("test.ping"));
        hub.publish::<PingTopic>(1, &Ping { n: 1 });
        assert_eq!(hub.subscriber_count("test.ping"), 0);
        drop(hub.subscribe("test.ping"));
        assert_eq!(hub.subscriber_count("test.ping"), 0);
    }

    #[tokio::test]
    async fn hub_drop_drains_pending_events_and_resync_then_closes() {
        let hub = Hub::new();
        let mut receiver = hub.subscribe("test.ping");
        hub.publish::<PingTopic>(1, &Ping { n: 1 });
        drop(hub);
        assert_eq!(receive(&mut receiver).await, event(1));
        assert_eq!(receive(&mut receiver).await, None);
        let hub = Hub::new();
        let mut receiver = hub.subscribe("test.ping");
        for n in 0..=QUEUE as u32 {
            hub.publish::<PingTopic>(n.into(), &Ping { n });
        }
        drop(hub);
        assert_eq!(receive(&mut receiver).await, Some(HubMessage::Resync));
        assert_eq!(receive(&mut receiver).await, None);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn racing_publish_recv_preserves_resync_boundary_and_wakeups() {
        for _ in 0..64 {
            let hub = Arc::new(Hub::new());
            let mut receiver = hub.subscribe("test.ping");
            for n in 0..=QUEUE as u32 {
                hub.publish::<PingTopic>(n.into(), &Ping { n });
            }
            let barrier = Arc::new(Barrier::new(2));
            let publisher = {
                let hub = hub.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    hub.publish::<PingTopic>(1000, &Ping { n: 1000 });
                })
            };
            barrier.wait();
            assert_eq!(receive(&mut receiver).await, Some(HubMessage::Resync));
            publisher.join().expect("publisher completed");
            hub.publish::<PingTopic>(1001, &Ping { n: 1001 });
            let next = receive(&mut receiver).await;
            if next == event(1000) {
                assert_eq!(receive(&mut receiver).await, event(1001));
            } else {
                assert_eq!(next, event(1001));
            }
            // Exercise a receiver waiting before publication and before hub shutdown.
            let waiting = hub.subscribe("test.ping");
            let (ready, registered) = tokio::sync::oneshot::channel();
            let waiter = tokio::spawn(receive_after_waiting(waiting, ready));
            registered.await.expect("receiver registered its waker");
            hub.publish::<PingTopic>(1002, &Ping { n: 1002 });
            assert_eq!(waiter.await.expect("waiter completed"), event(1002));
            let closing = hub.subscribe("test.other");
            let (ready, registered) = tokio::sync::oneshot::channel();
            let waiter = tokio::spawn(receive_after_waiting(closing, ready));
            registered.await.expect("receiver registered its waker");
            drop(hub);
            assert_eq!(waiter.await.expect("close waiter completed"), None);
        }
    }
}
