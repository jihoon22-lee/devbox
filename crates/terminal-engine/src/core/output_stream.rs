//! Push terminal output with one batch in flight. Idle sessions wait on the
//! buffer's change notification, so they cost no IPC.
use crate::core::terminal_output::OutputBatch;
use std::time::Duration;
use tokio::sync::{mpsc, watch};

#[derive(Debug, PartialEq, Eq)]
pub enum PumpEnd {
    Closed,
    Stopped,
    SendFailed,
    AckTimeout,
    ReadFailed,
}

pub async fn pump<R, S>(
    read: R,
    mut changes: watch::Receiver<u64>,
    mut send: S,
    mut acks: mpsc::Receiver<u64>,
    mut stop: watch::Receiver<bool>,
    start: u64,
    ack_timeout: Duration,
) -> PumpEnd
where
    R: Fn(u64) -> Result<OutputBatch, &'static str>,
    S: FnMut(&OutputBatch) -> bool,
{
    let mut cursor = start;
    loop {
        if *stop.borrow() {
            return PumpEnd::Stopped;
        }
        changes.borrow_and_update();
        let Ok(batch) = read(cursor) else {
            return PumpEnd::ReadFailed;
        };
        if !batch.frames.is_empty() || batch.truncated || batch.closed {
            if !send(&batch) {
                return PumpEnd::SendFailed;
            }
            if batch.closed && !batch.more {
                return PumpEnd::Closed;
            }
            let expected = batch.cursor;
            let deadline = tokio::time::Instant::now() + ack_timeout;
            loop {
                tokio::select! {
                    _ = stop.changed() => return PumpEnd::Stopped,
                    ack = tokio::time::timeout_at(deadline, acks.recv()) => match ack {
                        Ok(Some(acked)) if acked == expected => break,
                        Ok(Some(_)) => continue,
                        Ok(None) => return PumpEnd::Stopped,
                        Err(_) => return PumpEnd::AckTimeout,
                    },
                }
            }
            cursor = batch.cursor;
            if batch.more {
                continue;
            }
        } else {
            cursor = batch.cursor;
        }
        tokio::select! {
            _ = stop.changed() => return PumpEnd::Stopped,
            changed = changes.changed() => if changed.is_err() { return PumpEnd::Closed; },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::terminal_output::OutputBuffer;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::sync::{mpsc, watch};

    fn setup() -> (Arc<Mutex<OutputBuffer>>, watch::Receiver<u64>) {
        let buffer = Arc::new(Mutex::new(OutputBuffer::default()));
        let changes = buffer.lock().unwrap().subscribe();
        (buffer, changes)
    }

    #[tokio::test(start_paused = true)]
    async fn idle_sessions_send_nothing_and_output_is_acked_one_batch_at_a_time() {
        let (buffer, changes) = setup();
        let (ack_tx, ack_rx) = mpsc::channel(4);
        let (stop_tx, stop_rx) = watch::channel(false);
        let sent = Arc::new(Mutex::new(Vec::<u64>::new()));
        let reader = buffer.clone();
        let log = sent.clone();
        let task = tokio::spawn(pump(
            move |after| reader.lock().unwrap().read(after),
            changes,
            move |batch| {
                log.lock().unwrap().push(batch.cursor);
                true
            },
            ack_rx,
            stop_rx,
            0,
            Duration::from_secs(10),
        ));
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert!(sent.lock().unwrap().is_empty(), "idle pane must not send");
        buffer.lock().unwrap().append("hello");
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(*sent.lock().unwrap(), vec![1]);
        buffer.lock().unwrap().append("world");
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(
            sent.lock().unwrap().len(),
            1,
            "waits for the ack before sending more"
        );
        ack_tx.send(1).await.unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(*sent.lock().unwrap(), vec![1, 2]);
        stop_tx.send(true).unwrap();
        assert_eq!(task.await.unwrap(), PumpEnd::Stopped);
    }

    #[tokio::test(start_paused = true)]
    async fn closing_delivers_the_last_batch_and_ends() {
        let (buffer, changes) = setup();
        let (ack_tx, ack_rx) = mpsc::channel(4);
        let (_stop_tx, stop_rx) = watch::channel(false);
        buffer.lock().unwrap().append("bye");
        buffer.lock().unwrap().close();
        let reader = buffer.clone();
        let task = tokio::spawn(pump(
            move |after| reader.lock().unwrap().read(after),
            changes,
            |batch| batch.closed || !batch.frames.is_empty(),
            ack_rx,
            stop_rx,
            0,
            Duration::from_secs(10),
        ));
        ack_tx.send(1).await.unwrap();
        assert_eq!(task.await.unwrap(), PumpEnd::Closed);
    }

    #[tokio::test(start_paused = true)]
    async fn a_silent_renderer_times_out_and_a_failed_send_ends_the_stream() {
        let (buffer, changes) = setup();
        let (_ack_tx, ack_rx) = mpsc::channel(4);
        let (_stop_tx, stop_rx) = watch::channel(false);
        buffer.lock().unwrap().append("x");
        let reader = buffer.clone();
        let end = pump(
            move |after| reader.lock().unwrap().read(after),
            changes,
            |_| true,
            ack_rx,
            stop_rx,
            0,
            Duration::from_secs(10),
        )
        .await;
        assert_eq!(end, PumpEnd::AckTimeout);

        let (buffer, changes) = setup();
        let (_ack_tx, ack_rx) = mpsc::channel(4);
        let (_stop_tx, stop_rx) = watch::channel(false);
        buffer.lock().unwrap().append("x");
        let reader = buffer.clone();
        let end = pump(
            move |after| reader.lock().unwrap().read(after),
            changes,
            |_| false,
            ack_rx,
            stop_rx,
            0,
            Duration::from_secs(10),
        )
        .await;
        assert_eq!(end, PumpEnd::SendFailed);
    }
}

#[cfg(test)]
mod boundaries {
    use super::*;
    use crate::core::terminal_output::{OutputBuffer, MAX_BATCH_BYTES, MAX_REPLAY_BYTES};
    use std::sync::{Arc, Mutex};

    #[tokio::test(start_paused = true)]
    async fn closing_drains_every_retained_batch_and_preserves_gap() {
        let mut buffer = OutputBuffer::default();
        buffer.append(&"x".repeat(MAX_REPLAY_BYTES + MAX_BATCH_BYTES));
        buffer.close();
        let changes = buffer.subscribe();
        let (ack_tx, ack_rx) = tokio::sync::mpsc::channel(1);
        let (_stop_tx, stop_rx) = tokio::sync::watch::channel(false);
        let batches = Arc::new(Mutex::new(Vec::new()));
        let sent = batches.clone();
        let end = pump(
            move |after| buffer.read(after),
            changes,
            move |batch| {
                sent.lock().unwrap().push(batch.clone());
                ack_tx.try_send(batch.cursor).unwrap();
                true
            },
            ack_rx,
            stop_rx,
            0,
            std::time::Duration::from_secs(10),
        )
        .await;
        assert_eq!(end, PumpEnd::Closed);
        let batches = batches.lock().unwrap();
        assert!(batches.len() > 1);
        assert!(batches[0].truncated);
        assert!(batches.last().unwrap().closed);
        assert_eq!(
            batches
                .iter()
                .flat_map(|b| &b.frames)
                .map(|f| f.data.len())
                .sum::<usize>(),
            MAX_REPLAY_BYTES
        );
        assert!(batches
            .iter()
            .all(|b| b.frames.iter().map(|f| f.data.len()).sum::<usize>() <= MAX_BATCH_BYTES));
    }

    #[tokio::test(start_paused = true)]
    async fn invalid_acks_do_not_advance_or_extend_the_deadline() {
        let buffer = Arc::new(Mutex::new(OutputBuffer::default()));
        buffer.lock().unwrap().append("first");
        let changes = buffer.lock().unwrap().subscribe();
        let (ack_tx, ack_rx) = tokio::sync::mpsc::channel(4);
        let (_stop_tx, stop_rx) = tokio::sync::watch::channel(false);
        let sent = Arc::new(Mutex::new(Vec::new()));
        let log = sent.clone();
        let read = buffer.clone();
        let task = tokio::spawn(pump(
            move |after| read.lock().unwrap().read(after),
            changes,
            move |batch| {
                log.lock().unwrap().push(batch.cursor);
                true
            },
            ack_rx,
            stop_rx,
            0,
            std::time::Duration::from_secs(10),
        ));
        tokio::task::yield_now().await;
        buffer.lock().unwrap().append("second");
        for ack in [0, 999, 0] {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            ack_tx.send(ack).await.unwrap();
            tokio::task::yield_now().await;
            assert_eq!(*sent.lock().unwrap(), vec![1]);
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let result = tokio::time::timeout(std::time::Duration::from_millis(1), task)
            .await
            .expect("stale acknowledgements must not renew the deadline");
        assert_eq!(result.unwrap(), PumpEnd::AckTimeout);
    }

    #[tokio::test(start_paused = true)]
    async fn invalid_cursor_fails_and_idle_stop_wakes_without_output() {
        let buffer = OutputBuffer::default();
        let changes = buffer.subscribe();
        let (_ack_tx, ack_rx) = tokio::sync::mpsc::channel(1);
        let (_stop_tx, stop_rx) = tokio::sync::watch::channel(false);
        assert_eq!(
            pump(
                |after| buffer.read(after),
                changes,
                |_| panic!("must not send"),
                ack_rx,
                stop_rx,
                1,
                std::time::Duration::from_secs(10)
            )
            .await,
            PumpEnd::ReadFailed
        );
        let changes = buffer.subscribe();
        let (_ack_tx, ack_rx) = tokio::sync::mpsc::channel(1);
        let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
        let task = tokio::spawn(pump(
            move |after| buffer.read(after),
            changes,
            |_| panic!("idle must not send"),
            ack_rx,
            stop_rx,
            0,
            std::time::Duration::from_secs(10),
        ));
        tokio::task::yield_now().await;
        stop_tx.send(true).unwrap();
        assert_eq!(task.await.unwrap(), PumpEnd::Stopped);
    }
}
