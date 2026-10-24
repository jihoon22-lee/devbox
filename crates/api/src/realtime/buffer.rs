//! 데몬 → 화면 이벤트 큐. 메시지는 1,000개·4MiB까지 담고 넘치면 새 메시지를 버리며 수를 센다.
//! 열림·닫힘·재연결 같은 제어 이벤트는 버리지 않는다(세션당 몇 개뿐이다).
use super::RealtimeEvent;
use std::collections::VecDeque;
use std::sync::Mutex;
use tokio::sync::Notify;

pub(crate) const MAX_MESSAGES: usize = 1_000;
pub(crate) const MAX_BYTES: usize = 4 * 1024 * 1024;

pub(crate) struct Next {
    /// 이 이벤트 앞에서 알릴 버린 메시지 수(꺼낼 때 0으로 돌아간다).
    pub(crate) dropped: u64,
    pub(crate) event: RealtimeEvent,
}

#[derive(Default)]
struct State {
    items: VecDeque<(RealtimeEvent, usize)>,
    messages: usize,
    bytes: usize,
    dropped: u64,
    ended: bool,
}

#[derive(Default)]
pub(crate) struct Buffer {
    state: Mutex<State>,
    notify: Notify,
}

impl Buffer {
    /// 메시지 하나(`size`는 원본 바이트). 넘치면 버리고 거짓.
    pub(crate) fn message(&self, event: RealtimeEvent, size: usize) -> bool {
        let mut state = self.state.lock().expect("realtime buffer");
        if state.ended {
            return false;
        }
        if state.messages >= MAX_MESSAGES || state.bytes.saturating_add(size) > MAX_BYTES {
            state.dropped = state.dropped.saturating_add(1);
            return false;
        }
        state.messages += 1;
        state.bytes += size;
        state.items.push_back((event, size));
        drop(state);
        self.notify.notify_one();
        true
    }

    /// 버리지 않는 제어 이벤트.
    pub(crate) fn control(&self, event: RealtimeEvent) {
        let mut state = self.state.lock().expect("realtime buffer");
        if state.ended {
            return;
        }
        state.items.push_back((event, usize::MAX));
        drop(state);
        self.notify.notify_one();
    }

    /// 더 넣지 않는다. 쓰는 쪽은 남은 이벤트를 다 쓴 뒤 멈춘다.
    pub(crate) fn end(&self) {
        self.state.lock().expect("realtime buffer").ended = true;
        self.notify.notify_one();
    }

    pub(crate) async fn next(&self) -> Option<Next> {
        loop {
            let notified = self.notify.notified();
            {
                let mut state = self.state.lock().expect("realtime buffer");
                if let Some((event, size)) = state.items.pop_front() {
                    if size != usize::MAX {
                        state.messages -= 1;
                        state.bytes -= size;
                    }
                    let dropped = std::mem::take(&mut state.dropped);
                    return Some(Next { dropped, event });
                }
                if state.ended {
                    let dropped = std::mem::take(&mut state.dropped);
                    return (dropped > 0).then_some(Next {
                        dropped: 0,
                        event: RealtimeEvent::Dropped { count: dropped },
                    });
                }
            }
            notified.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(n: usize) -> RealtimeEvent {
        RealtimeEvent::Message {
            direction: crate::Direction::Received,
            kind: crate::MessageKind::Text,
            data: Some(n.to_string()),
            data_base64: None,
            at_ms: 0,
        }
    }

    #[tokio::test]
    async fn count_limit_drops_new_messages_and_reports_before_the_next_event() {
        let buffer = Buffer::default();
        buffer.control(RealtimeEvent::Open { protocol: None });
        for n in 0..MAX_MESSAGES + 5 {
            buffer.message(text(n), 1);
        }
        buffer.control(RealtimeEvent::Closed {
            code: Some(1000),
            reason: None,
        });
        buffer.end();
        let first = buffer.next().await.expect("open");
        assert_eq!(first.dropped, 5);
        let mut count = 0;
        while let Some(next) = buffer.next().await {
            assert_eq!(next.dropped, 0);
            if matches!(next.event, RealtimeEvent::Message { .. }) {
                count += 1;
            }
        }
        assert_eq!(count, MAX_MESSAGES);
    }

    #[tokio::test]
    async fn byte_limit_counts_payload_bytes_and_frees_on_take() {
        let buffer = Buffer::default();
        assert!(buffer.message(text(0), MAX_BYTES));
        assert!(!buffer.message(text(1), 1));
        // 버린 수는 다음으로 꺼내는 이벤트보다 먼저 알린다(쓰기가 재개되면 맨 먼저).
        assert_eq!(buffer.next().await.expect("message").dropped, 1);
        assert!(buffer.message(text(2), 1));
        assert_eq!(buffer.next().await.expect("after").dropped, 0);
    }

    #[tokio::test]
    async fn drops_without_a_following_event_are_still_reported() {
        let buffer = Buffer::default();
        assert!(
            !buffer.message(text(0), MAX_BYTES + 1),
            "one message over the byte bound"
        );
        buffer.end();
        let tail = buffer.next().await.expect("dropped tail");
        assert!(matches!(tail.event, RealtimeEvent::Dropped { count: 1 }));
        assert!(buffer.next().await.is_none());
        assert!(!buffer.message(text(1), 1), "nothing is queued after end");
    }
}
