use serde_json::{json, Value};
use std::{sync::Arc, time::Instant};

pub struct Routes {
    started: Instant,
    pub generation: String,
    shutdown: tokio::sync::watch::Sender<bool>,
}
impl Routes {
    pub fn new(generation: String) -> Arc<Self> {
        let (shutdown, _) = tokio::sync::watch::channel(false);
        Arc::new(Self {
            started: Instant::now(),
            generation,
            shutdown,
        })
    }
    pub fn shutdown_receiver(&self) -> tokio::sync::watch::Receiver<bool> {
        self.shutdown.subscribe()
    }
    pub fn shutdown(&self) {
        self.shutdown.send_replace(true);
    }
    pub async fn dispatch(
        &self,
        _product: &str,
        _session: &str,
        component: &str,
        request: Value,
    ) -> Value {
        if component != "agent.status"
            || request.get("method").and_then(Value::as_str) != Some("status")
            || request.get("args") != Some(&json!({}))
        {
            return failure("method_unknown");
        }
        json!({"operation":{"outcome":{"state":"succeeded"}},"value": {
            "version": env!("CARGO_PKG_VERSION"), "generation": self.generation,
            "uptimeMs": self.started.elapsed().as_millis() as u64,
            "components": ["agent.status"]
        }})
    }
    #[cfg(test)]
    pub fn for_tests() -> Arc<Self> {
        Self::new("fixture".into())
    }
}
pub fn failure(code: &str) -> Value {
    json!({"operation":{"outcome":{"state":"failed","code":"unavailable"}}, "value":{"issue":code}})
}
