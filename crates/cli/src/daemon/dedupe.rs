use super::state::Services;
use devbox_protocol::{CommonError, Ctx, DispatchError, MethodKind, Router};
use serde_json::value::RawValue;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

fn fingerprint(text: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(text.as_bytes()).into()
}

type ResultValue = Result<Box<RawValue>, DispatchError>;
struct Entry {
    params: [u8; 32],
    result: watch::Sender<Option<ResultValue>>,
    completed: Mutex<Option<Instant>>,
}
#[derive(Default)]
pub(super) struct Dedupe {
    entries: Mutex<HashMap<(String, String), Arc<Entry>>>,
    pub tracker: tokio_util::task::TaskTracker,
}

impl Dedupe {
    pub async fn dispatch(
        &self,
        router: &Router<Services>,
        method: &str,
        ctx: Ctx,
        params: Box<RawValue>,
        crid: Option<String>,
    ) -> ResultValue {
        if router.kind_of(method) != Some(MethodKind::Mutation) {
            return router.dispatch(method, ctx, params).await;
        }
        if ctx.cancel.is_cancelled() {
            return Err(DispatchError::common(CommonError::Cancelled));
        }
        let (entry, execute) = if let Some(crid) = crid {
            let mut map = self.entries.lock().expect("dedupe");
            map.retain(|_, entry| {
                entry
                    .completed
                    .lock()
                    .expect("completion")
                    .is_none_or(|at| at.elapsed() < Duration::from_secs(600))
            });
            let key = (method.to_owned(), crid);
            if let Some(entry) = map.get(&key) {
                if entry.params != fingerprint(params.get()) {
                    return Err(DispatchError::common(CommonError::InvalidParams {
                        reason: "clientRequestId was already used with different params".into(),
                    }));
                }
                (entry.clone(), false)
            } else {
                let entry = Arc::new(Entry {
                    params: fingerprint(params.get()),
                    result: watch::channel(None).0,
                    completed: Mutex::new(None),
                });
                map.insert(key, entry.clone());
                (entry, true)
            }
        } else {
            (
                Arc::new(Entry {
                    params: fingerprint(params.get()),
                    result: watch::channel(None).0,
                    completed: Mutex::new(None),
                }),
                true,
            )
        };
        let mut receiver = entry.result.subscribe();
        if execute {
            // An accepted mutation owns its execution. Cancelling a waiter must not
            // discard a DB job's eventual committed result or permit a retry effect.
            let execution = router.dispatch(
                method,
                Ctx {
                    channel: ctx.channel,
                    streams: ctx.streams.clone(),
                    cancel: CancellationToken::new(),
                },
                params,
            );
            self.tracker.spawn(async move {
                let result = execution.await;
                *entry.completed.lock().expect("completion") = Some(Instant::now());
                entry.result.send_replace(Some(result));
            });
        }
        loop {
            if let Some(result) = receiver.borrow_and_update().clone() {
                return result;
            }
            tokio::select! {
                _ = ctx.cancel.cancelled() => return Err(DispatchError::common(CommonError::Cancelled)),
                changed = receiver.changed() => if changed.is_err() { return Err(DispatchError::common(CommonError::Internal)); },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use devbox_projects::ProjectsRemove;
    use devbox_protocol::Done;
    use std::future::{Future, poll_fn};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn fingerprints_preserve_retry_identity_without_retaining_plaintext() {
        let temp = tempfile::tempdir().expect("owned fixture");
        let services = super::super::build_services_for_test(temp.path());
        let count = Arc::new(AtomicUsize::new(0));
        let set_count = count.clone();
        let delete_count = count.clone();
        let router = Router::new(services)
            .register::<devbox_secrets::Set, _, _>(move |_, _, _| {
                set_count.fetch_add(1, Ordering::SeqCst);
                async { Ok(Done { rev: "1".into() }) }
            })
            .register::<devbox_secrets::Delete, _, _>(move |_, _, _| {
                delete_count.fetch_add(1, Ordering::SeqCst);
                async { Ok(Done { rev: "2".into() }) }
            });
        let dedupe = Dedupe::default();
        let ctx = || Ctx {
            channel: 1,
            streams: None,
            cancel: CancellationToken::new(),
        };
        let raw = || {
            serde_json::value::to_raw_value(&devbox_secrets::SetSecret {
                name: "TOKEN".into(),
                value: "owned-private-value".into(),
            })
            .expect("params")
        };
        for _ in 0..2 {
            dedupe
                .dispatch(&router, "secrets.set", ctx(), raw(), Some("same".into()))
                .await
                .expect("same mutation");
        }
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(
            dedupe.entries.lock().expect("entries")[&("secrets.set".into(), "same".into())].params,
            fingerprint(raw().get())
        );
        let different = serde_json::value::to_raw_value(&devbox_secrets::SetSecret {
            name: "TOKEN".into(),
            value: "different-private-value".into(),
        })
        .expect("params");
        assert_eq!(
            dedupe
                .dispatch(
                    &router,
                    "secrets.set",
                    ctx(),
                    different,
                    Some("same".into())
                )
                .await
                .expect_err("different params")
                .error
                .code,
            "invalid_params"
        );
        dedupe
            .dispatch(
                &router,
                "secrets.delete",
                ctx(),
                serde_json::value::to_raw_value(&devbox_secrets::Name {
                    name: "TOKEN".into(),
                })
                .expect("delete"),
                Some("same".into()),
            )
            .await
            .expect("different method identity");
        assert_eq!(count.load(Ordering::SeqCst), 2);
        let bad = RawValue::from_string(r#"{"name":"TOKEN","value":123456789}"#.into())
            .expect("bad input");
        let error = router
            .dispatch("secrets.set", ctx(), bad)
            .await
            .expect_err("invalid private input");
        assert!(
            !serde_json::to_string(&error.error)
                .expect("error wire")
                .contains("123456789")
        );
        dedupe.tracker.close();
        dedupe.tracker.wait().await;
    }
    #[tokio::test]
    async fn inflight_retry_coalesces_and_cancelled_waiter_retains_committed_result() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let service = super::super::build_services_for_test(temp.path());
        let started = Arc::new(tokio::sync::Notify::new());
        let finish = Arc::new(tokio::sync::Notify::new());
        let count = Arc::new(AtomicUsize::new(0));
        let router = Arc::new(Router::new(service).register::<ProjectsRemove, _, _>({
            let (started, finish, count) = (started.clone(), finish.clone(), count.clone());
            move |_, _, _| {
                let (started, finish, count) = (started.clone(), finish.clone(), count.clone());
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    started.notify_one();
                    finish.notified().await;
                    Ok(Done { rev: "1".into() })
                }
            }
        }));
        let dedupe = Arc::new(Dedupe::default());
        let raw = || RawValue::from_string("{\"id\":\"one\"}".into()).expect("encode fixture JSON");
        let cancel = CancellationToken::new();
        let first = tokio::spawn({
            let (dedupe, router, cancel) = (dedupe.clone(), router.clone(), cancel.clone());
            async move {
                dedupe
                    .dispatch(
                        &router,
                        "projects.remove",
                        Ctx {
                            channel: 1,
                            cancel,
                            streams: None,
                        },
                        raw(),
                        Some("same".into()),
                    )
                    .await
            }
        });
        tokio::time::timeout(Duration::from_secs(5), started.notified())
            .await
            .expect("fixture completes before deadline");
        let mut second = std::pin::pin!(dedupe.dispatch(
            &router,
            "projects.remove",
            Ctx {
                channel: 2,
                streams: None,
                cancel: CancellationToken::new()
            },
            raw(),
            Some("same".into())
        ));
        poll_fn(|cx| {
            assert!(
                second.as_mut().poll(cx).is_pending(),
                "retry waits on the first mutation"
            );
            std::task::Poll::Ready(())
        })
        .await;
        cancel.cancel();
        assert_eq!(
            first
                .await
                .expect("join cancelled fixture request")
                .expect_err("fixture operation must fail")
                .error
                .code,
            "cancelled"
        );
        assert_eq!(count.load(Ordering::SeqCst), 1);
        finish.notify_one();
        let result = tokio::time::timeout(Duration::from_secs(5), second)
            .await
            .expect("fixture completes before deadline")
            .expect("fixture completes before deadline");
        let retry = dedupe
            .dispatch(
                &router,
                "projects.remove",
                Ctx {
                    channel: 3,
                    streams: None,
                    cancel: CancellationToken::new(),
                },
                raw(),
                Some("same".into()),
            )
            .await
            .expect("dispatch fixture request");
        assert_eq!(result.get(), retry.get());
        assert_eq!(count.load(Ordering::SeqCst), 1);
        dedupe.tracker.close();
        dedupe.tracker.wait().await;
    }
}
