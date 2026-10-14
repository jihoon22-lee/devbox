use crate::error::{CommonError, Fail, to_wire};
use crate::message::WireError;
use crate::method::{Method, MethodKind};
use serde_json::value::RawValue;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

#[derive(Clone)]
pub struct Ctx {
    pub channel: u32,
    pub cancel: CancellationToken,
    pub streams: Option<Arc<dyn crate::stream::StreamOpener>>,
}
impl std::fmt::Debug for Ctx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ctx")
            .field("channel", &self.channel)
            .finish()
    }
}
impl Ctx {
    pub fn open_stream(&self) -> Result<crate::stream::StreamEnds, crate::stream::StreamError> {
        self.streams
            .as_ref()
            .ok_or(crate::stream::StreamError::Unsupported)?
            .open()
    }
}

#[derive(Debug, Clone)]
pub struct DispatchError {
    pub error: WireError,
    pub diagnostic_id: Option<String>,
}

impl DispatchError {
    pub fn common(e: CommonError) -> Self {
        Self {
            error: to_wire(&e),
            diagnostic_id: None,
        }
    }
}

type Handler<S> = Arc<
    dyn Fn(Arc<S>, Ctx, Box<RawValue>) -> BoxFuture<Result<Box<RawValue>, DispatchError>>
        + Send
        + Sync,
>;

pub struct Router<S> {
    state: Arc<S>,
    handlers: HashMap<&'static str, (MethodKind, Handler<S>)>,
}

impl<S: Send + Sync + 'static> Router<S> {
    pub fn new(state: Arc<S>) -> Self {
        Self {
            state,
            handlers: HashMap::new(),
        }
    }

    pub fn register<M, F, Fut>(mut self, f: F) -> Self
    where
        M: Method,
        F: Fn(Arc<S>, Ctx, M::Params) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<M::Output, Fail<M::Error>>> + Send + 'static,
    {
        assert!(
            !self.handlers.contains_key(M::NAME),
            "duplicate method {}",
            M::NAME
        );
        let f = Arc::new(f);
        let handler: Handler<S> = Arc::new(move |state, ctx, raw| {
            let f = f.clone();
            Box::pin(async move {
                let params: M::Params = serde_json::from_str(raw.get()).map_err(|e| {
                    DispatchError::common(CommonError::InvalidParams {
                        reason: e.to_string(),
                    })
                })?;
                match f(state, ctx, params).await {
                    Ok(out) => serde_json::value::to_raw_value(&out)
                        .map_err(|e| internal(M::NAME, &e.to_string())),
                    Err(Fail::Domain(e)) => Err(DispatchError {
                        error: to_wire(&e),
                        diagnostic_id: None,
                    }),
                    Err(Fail::Internal(msg)) => Err(internal(M::NAME, &msg)),
                }
            })
        });
        self.handlers.insert(M::NAME, (M::KIND, handler));
        self
    }

    pub fn kind_of(&self, method: &str) -> Option<MethodKind> {
        self.handlers.get(method).map(|(k, _)| *k)
    }

    pub fn names(&self) -> Vec<&'static str> {
        let mut v: Vec<_> = self.handlers.keys().copied().collect();
        v.sort_unstable();
        v
    }

    pub fn dispatch(
        &self,
        method: &str,
        ctx: Ctx,
        params: Box<RawValue>,
    ) -> BoxFuture<Result<Box<RawValue>, DispatchError>> {
        let Some((_, h)) = self.handlers.get(method) else {
            let e = DispatchError::common(CommonError::MethodNotFound {
                method: method.to_owned(),
            });
            return Box::pin(async move { Err(e) });
        };
        let cancel = ctx.cancel.clone();
        let fut = h(self.state.clone(), ctx, params);
        Box::pin(async move {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => Err(DispatchError::common(CommonError::Cancelled)),
                r = fut => r,
            }
        })
    }
}

fn internal(method: &'static str, msg: &str) -> DispatchError {
    let id = uuid::Uuid::new_v4().simple().to_string()[..8].to_owned();
    tracing::error!(diagnostic_id = %id, method, error = %msg, "internal error");
    DispatchError {
        error: to_wire(&CommonError::Internal),
        diagnostic_id: Some(id),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Confirm, DomainError, domain_error, method};
    use serde::{Deserialize, Serialize};
    use std::sync::Arc;

    #[derive(Deserialize, ts_rs::TS)]
    pub struct EchoParams {
        pub text: String,
    }
    #[derive(Serialize, ts_rs::TS)]
    pub struct EchoResult {
        pub text: String,
    }
    domain_error! {
        pub enum EchoError {
            Empty = "echo.empty",
            TooLong { limit: u32 } = "echo.too_long",
        }
    }
    method!(pub Echo = "echo.say", query, EchoParams => EchoResult, EchoError);
    method!(pub EchoSet = "echo.set", mutation(always), EchoParams => EchoResult, EchoError);

    struct State;
    fn router() -> Router<State> {
        Router::new(Arc::new(State)).register::<Echo, _, _>(|_, _, p| async move {
            if p.text.is_empty() {
                return Err(EchoError::Empty.into());
            }
            if p.text == "boom" {
                return Err(Fail::Internal("disk on fire".into()));
            }
            if p.text.len() > 5 {
                return Err(EchoError::TooLong { limit: 5 }.into());
            }
            Ok(EchoResult { text: p.text })
        })
    }
    fn raw(s: &str) -> Box<serde_json::value::RawValue> {
        serde_json::value::RawValue::from_string(s.into()).expect("encode fixture JSON")
    }
    fn ctx() -> Ctx {
        Ctx {
            channel: 0,
            streams: None,
            cancel: tokio_util::sync::CancellationToken::new(),
        }
    }

    #[tokio::test]
    async fn dispatches_ok_domain_internal_and_bad_params() {
        let r = router();
        let ok = r
            .dispatch("echo.say", ctx(), raw(r#"{"text":"hi"}"#))
            .await
            .expect("dispatch fixture request");
        assert_eq!(ok.get(), r#"{"text":"hi"}"#);

        let e = r
            .dispatch("echo.say", ctx(), raw(r#"{"text":"toolong"}"#))
            .await
            .expect_err("fixture operation must fail");
        assert_eq!(e.error.code, "echo.too_long");
        assert_eq!(
            e.error.detail.as_ref().expect("fixture child exists").get(),
            r#"{"limit":5}"#
        );
        assert!(e.diagnostic_id.is_none());

        let e = r
            .dispatch("echo.say", ctx(), raw(r#"{"text":"boom"}"#))
            .await
            .expect_err("fixture operation must fail");
        assert_eq!(e.error.code, "internal");
        assert!(e.diagnostic_id.is_some());

        let e = r
            .dispatch("echo.say", ctx(), raw(r#"{"nope":1}"#))
            .await
            .expect_err("fixture operation must fail");
        assert_eq!(e.error.code, "invalid_params");

        let e = r
            .dispatch("echo.unknown", ctx(), raw("{}"))
            .await
            .expect_err("fixture operation must fail");
        assert_eq!(e.error.code, "method_not_found");
    }

    #[tokio::test]
    async fn cancellation_wins_over_a_slow_handler() {
        let r = Router::new(Arc::new(State)).register::<Echo, _, _>(|_, _, _| async move {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            Ok(EchoResult {
                text: String::new(),
            })
        });
        let c = ctx();
        let cancel = c.cancel.clone();
        let fut = r.dispatch("echo.say", c, raw(r#"{"text":"x"}"#));
        cancel.cancel();
        assert_eq!(
            fut.await
                .expect_err("fixture operation must fail")
                .error
                .code,
            "cancelled"
        );
    }

    #[test]
    fn metadata_and_codes() {
        assert_eq!(<EchoSet as Method>::KIND, MethodKind::Mutation);
        assert_eq!(<EchoSet as Method>::CONFIRM, Confirm::Always);
        assert_eq!(<Echo as Method>::CONFIRM, Confirm::None);
        assert_eq!(
            <EchoError as DomainError>::CODES,
            &["echo.empty", "echo.too_long"]
        );
        assert_eq!(router().kind_of("echo.say"), Some(MethodKind::Query));
    }

    #[test]
    #[should_panic(expected = "duplicate method echo.say")]
    fn duplicate_registration_panics() {
        let _ = router()
            .register::<Echo, _, _>(|_, _, p| async move { Ok(EchoResult { text: p.text }) });
    }
    #[tokio::test]
    async fn precancelled_dispatch_never_invokes_handler() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let invoked = Arc::new(AtomicBool::new(false));
        let flag = invoked.clone();
        let r = Router::new(Arc::new(State)).register::<Echo, _, _>(move |_, _, _| {
            flag.store(true, Ordering::SeqCst);
            async {
                Ok(EchoResult {
                    text: String::new(),
                })
            }
        });
        let c = ctx();
        c.cancel.cancel();
        let error = r
            .dispatch("echo.say", c, raw(r#"{"text":"x"}"#))
            .await
            .expect_err("fixture operation must fail");
        assert_eq!(error.error.code, "cancelled");
        assert!(!invoked.load(Ordering::SeqCst));
        assert!(error.diagnostic_id.is_none());
    }

    #[tokio::test]
    async fn cancellation_interrupts_an_executing_handler() {
        let started = Arc::new(tokio::sync::Notify::new());
        let notify = started.clone();
        let r = Router::new(Arc::new(State)).register::<Echo, _, _>(move |_, _, _| {
            let notify = notify.clone();
            async move {
                notify.notify_one();
                std::future::pending::<Result<EchoResult, Fail<EchoError>>>().await
            }
        });
        let c = ctx();
        let token = c.cancel.clone();
        let request = r.dispatch("echo.say", c, raw(r#"{"text":"x"}"#));
        let cancel = async {
            started.notified().await;
            token.cancel();
        };
        let (result, ()) = tokio::join!(request, cancel);
        assert_eq!(
            result.expect_err("fixture operation must fail").error.code,
            "cancelled"
        );
    }

    #[tokio::test]
    async fn common_and_internal_errors_keep_private_causes_off_wire() {
        let r = router();
        let unit = r
            .dispatch("echo.say", ctx(), raw(r#"{"text":""}"#))
            .await
            .expect_err("fixture operation must fail");
        assert_eq!(unit.error.code, "echo.empty");
        assert!(unit.error.detail.is_none());
        let failure = r
            .dispatch("echo.say", ctx(), raw(r#"{"text":"boom"}"#))
            .await
            .expect_err("fixture operation must fail");
        assert_eq!(
            serde_json::to_string(&failure.error).expect("serialize fixture JSON"),
            r#"{"code":"internal"}"#
        );
        assert!(
            failure
                .diagnostic_id
                .expect("internal failure has diagnostic ID")
                .chars()
                .all(|c| c.is_ascii_hexdigit())
        );
        let missing = r
            .dispatch("unknown", ctx(), raw("{}"))
            .await
            .expect_err("fixture operation must fail");
        assert_eq!(
            missing
                .error
                .detail
                .expect("fixture error includes details")
                .get(),
            r#"{"method":"unknown"}"#
        );
        assert!(missing.diagnostic_id.is_none());
        let invalid = r
            .dispatch("echo.say", ctx(), raw("{}"))
            .await
            .expect_err("fixture operation must fail");
        assert!(
            invalid
                .error
                .detail
                .expect("fixture error includes details")
                .get()
                .contains("reason")
        );
        assert!(invalid.diagnostic_id.is_none());
        assert_eq!(r.names(), vec!["echo.say"]);
        assert_eq!(r.kind_of("unknown"), None);
    }
}
