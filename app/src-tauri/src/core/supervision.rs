//! One worker owns each attempt until cancellation cleanup has finished. The first attempt is
//! the automatic start; every later one answers a user's retry (`retried`).
use std::future::Future;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
pub async fn run<F, Fut>(
    mut retries: watch::Receiver<Option<String>>,
    stop: CancellationToken,
    mut attempt: F,
) where
    F: FnMut(u64, Option<String>, bool, CancellationToken) -> Fut,
    Fut: Future<Output = ()>,
{
    let mut generation = 0u64;
    let mut retried = false;
    loop {
        if stop.is_cancelled() {
            return;
        }
        generation = generation.checked_add(1).expect("generation exhausted");
        let forced = retries.borrow_and_update().clone();
        let cancel = stop.child_token();
        let work = attempt(generation, forced, retried, cancel.clone());
        tokio::pin!(work);
        tokio::select! {
            _=stop.cancelled()=>{cancel.cancel();work.await;return;}
            changed=retries.changed()=>{cancel.cancel();work.await;if changed.is_err(){return;}}
            _=&mut work=>{tokio::select!{_=stop.cancelled()=>return,changed=retries.changed()=>{if changed.is_err(){return;}}}}
        }
        // Only a retry request (`gate_retry`) gets here.
        retried = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    #[tokio::test]
    async fn retries_are_coalesced_and_cleanup_precedes_next_generation() {
        let (tx, rx) = watch::channel(None);
        let stop = CancellationToken::new();
        let active = Arc::new(AtomicUsize::new(0));
        let count = active.clone();
        let cancel = stop.clone();
        let task = tokio::spawn(run(rx, cancel, move |_, _, _, token| {
            let active = count.clone();
            async move {
                assert_eq!(active.fetch_add(1, Ordering::SeqCst), 0);
                token.cancelled().await;
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                assert_eq!(active.fetch_sub(1, Ordering::SeqCst), 1);
            }
        }));
        tokio::task::yield_now().await;
        for i in 0..50 {
            tx.send_replace(Some(format!("distro-{i}")));
            tokio::task::yield_now().await;
        }
        stop.cancel();
        task.await.unwrap();
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }
    #[tokio::test]
    async fn only_runs_after_a_user_retry_are_marked_as_retries() {
        let (tx, rx) = watch::channel(None);
        let stop = CancellationToken::new();
        let (seen, mut runs) = tokio::sync::mpsc::unbounded_channel();
        let task = tokio::spawn(run(rx, stop.clone(), move |generation, _, retried, _| {
            seen.send((generation, retried)).unwrap();
            async {}
        }));
        // The automatic start, then [다시 시도] twice (a distro choice is a retry too).
        assert_eq!(runs.recv().await, Some((1, false)));
        tx.send_replace(Some("distro".into()));
        assert_eq!(runs.recv().await, Some((2, true)));
        tx.send_replace(None);
        assert_eq!(runs.recv().await, Some((3, true)));
        stop.cancel();
        task.await.unwrap();
    }
}
