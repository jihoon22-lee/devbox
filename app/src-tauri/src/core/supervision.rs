//! One worker owns each attempt until cancellation cleanup has finished.
use std::future::Future;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
pub async fn run<F, Fut>(
    mut retries: watch::Receiver<Option<String>>,
    stop: CancellationToken,
    mut attempt: F,
) where
    F: FnMut(u64, Option<String>, CancellationToken) -> Fut,
    Fut: Future<Output = ()>,
{
    let mut generation = 0u64;
    loop {
        if stop.is_cancelled() {
            return;
        }
        generation = generation.checked_add(1).expect("generation exhausted");
        let forced = retries.borrow_and_update().clone();
        let cancel = stop.child_token();
        let work = attempt(generation, forced, cancel.clone());
        tokio::pin!(work);
        tokio::select! {
            _=stop.cancelled()=>{cancel.cancel();work.await;return;}
            changed=retries.changed()=>{cancel.cancel();work.await;if changed.is_err(){return;}}
            _=&mut work=>{tokio::select!{_=stop.cancelled()=>return,changed=retries.changed()=>{if changed.is_err(){return;}}}}
        }
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
        let task = tokio::spawn(run(rx, cancel, move |_, _, token| {
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
}
