use super::SubEvent;
use std::collections::VecDeque;
use std::sync::Mutex;
use tokio::sync::Notify;

#[derive(Default)]
struct State {
    events: VecDeque<SubEvent>,
    resync: bool,
    closed: bool,
}
#[derive(Default)]
pub(super) struct Queue {
    state: Mutex<State>,
    notify: Notify,
}
impl Queue {
    pub fn push(&self, event: SubEvent) {
        let mut state = self.state.lock().expect("subscription");
        if state.closed {
            return;
        }
        if event.rev.is_empty() || state.events.len() == 256 {
            state.events.clear();
            state.resync = true;
        } else if !state.resync {
            state.events.push_back(event);
        }
        drop(state);
        self.notify.notify_one();
    }
    pub fn close(&self) {
        let mut state = self.state.lock().expect("subscription");
        state.closed = true;
        state.events.clear();
        state.resync = false;
        drop(state);
        self.notify.notify_one();
    }
    pub async fn recv(&self) -> Option<SubEvent> {
        loop {
            let notified = self.notify.notified();
            {
                let mut state = self.state.lock().expect("subscription");
                if state.closed {
                    return None;
                }
                if state.resync {
                    state.resync = false;
                    return Some(SubEvent {
                        rev: String::new(),
                        payload: String::new(),
                    });
                }
                if let Some(event) = state.events.pop_front() {
                    return Some(event);
                }
            }
            notified.await;
        }
    }
}
