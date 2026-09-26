use serde_json::{json, Value};
use std::{sync::Arc, time::Instant};

pub struct Routes {
    started: Instant,
    pub generation: String,
    shutdown: tokio::sync::watch::Sender<bool>,
    runtime: Option<Arc<crate::runtime::Runtime>>,
    sessions: std::sync::Mutex<
        std::collections::HashMap<
            (String, String, String),
            (Arc<crate::remote::RemoteSession>, Instant),
        >,
    >,
}
impl Routes {
    pub fn new(generation: String) -> Arc<Self> {
        let (shutdown, _) = tokio::sync::watch::channel(false);
        Arc::new(Self {
            started: Instant::now(),
            generation,
            shutdown,
            runtime: None,
            sessions: Default::default(),
        })
    }
    pub fn with_runtime(generation: String, runtime: Arc<crate::runtime::Runtime>) -> Arc<Self> {
        let (shutdown, _) = tokio::sync::watch::channel(false);
        Arc::new(Self {
            started: Instant::now(),
            generation,
            shutdown,
            runtime: Some(runtime),
            sessions: Default::default(),
        })
    }
    pub async fn shutdown_owners(&self) {
        if let Some(runtime) = &self.runtime {
            runtime.shutdown().await;
        }
    }
    pub fn session(
        &self,
        product: &str,
        installation: &str,
        session: &str,
    ) -> Result<Arc<crate::remote::RemoteSession>, &'static str> {
        let mut sessions = self.sessions.lock().map_err(|_| "busy")?;
        sessions.retain(|_, (guard, _)| Arc::strong_count(guard) > 1 || guard.recent());
        let key = (
            product.to_owned(),
            installation.to_owned(),
            session.to_owned(),
        );
        if let Some((guard, last)) = sessions.get_mut(&key) {
            *last = Instant::now();
            return Ok(guard.clone());
        }
        if sessions.len() >= 64 {
            return Err("session_limit");
        }
        let guard = Arc::new(crate::remote::RemoteSession::new(
            product,
            installation,
            session,
        ));
        sessions.insert(key, (guard.clone(), Instant::now()));
        Ok(guard)
    }
    pub fn accepts(&self, product: &str, component: &str) -> bool {
        (component == "agent.status"
            && ["workspace", "api-studio", "knowledge", "control-center"].contains(&product))
            || (product == "workspace"
                && [
                    "workspace.runtime",
                    "workspace.processes",
                    "workspace.process-actions",
                    "workspace.logs",
                ]
                .contains(&component))
    }
    pub fn shutdown_receiver(&self) -> tokio::sync::watch::Receiver<bool> {
        self.shutdown.subscribe()
    }
    pub fn shutdown(&self) {
        self.shutdown.send_replace(true);
    }
    pub async fn dispatch(
        &self,
        product: &str,
        _session: &str,
        session: Arc<crate::remote::RemoteSession>,
        component: &str,
        request: Value,
    ) -> Value {
        if !self.accepts(product, component) {
            return failure("unauthorized");
        }
        if component.starts_with("workspace.") {
            return match &self.runtime {
                Some(runtime) => {
                    crate::runtime::response(runtime.dispatch(session, component, request).await)
                }
                None => failure("runtime_owner_unavailable"),
            };
        }
        if component != "agent.status"
            || request.get("method").and_then(Value::as_str) != Some("status")
            || request.get("args") != Some(&json!({}))
        {
            return failure("method_unknown");
        }
        if self.runtime.is_some() {
            let Ok(incoming) = serde_json::from_value::<product_ipc::IncomingRequest>(request)
            else {
                return failure("invalid_request");
            };
            let catalog: Value = serde_json::from_str(include_str!("../../../apps/products.json"))
                .expect("embedded product catalog");
            let routes: Vec<&str> = catalog["features"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|feature| feature["owner"].as_str() == Some(product))
                .filter_map(|feature| feature["route"].as_str())
                .collect();
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            if session
                .authorize(&incoming.header, &routes, now, |_| true)
                .is_err()
            {
                return failure("agent_request_rejected");
            }
        }
        json!({"operation":{"outcome":{"state":"succeeded"}},"value": {
            "version": env!("CARGO_PKG_VERSION"), "generation": self.generation,
            "uptimeMs": self.started.elapsed().as_millis() as u64,
            "components": if self.runtime.is_some() { vec!["agent.status","workspace.runtime","workspace.processes","workspace.process-actions","workspace.logs"] } else {vec!["agent.status"]}
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reconnecting_the_same_native_session_keeps_replay_history() {
        let routes = Routes::for_tests();
        let first = routes.session("workspace", "native", "session").unwrap();
        let next = routes.session("workspace", "native", "session").unwrap();
        assert!(Arc::ptr_eq(&first, &next));
        assert!(!Arc::ptr_eq(
            &first,
            &routes
                .session("workspace", "native", "new-session")
                .unwrap()
        ));
    }
    #[test]
    fn runtime_components_accept_only_workspace_peers() {
        let routes = Routes::for_tests();
        for component in [
            "workspace.runtime",
            "workspace.processes",
            "workspace.process-actions",
            "workspace.logs",
        ] {
            assert!(routes.accepts("workspace", component));
            assert!(!routes.accepts("knowledge", component));
            assert!(!routes.accepts("api-studio", component));
        }
        assert!(!routes.accepts("workspace", "workspace.files"));
        assert!(!routes.accepts("workspace", "workspace.terminal"));
        assert!(routes.accepts("control-center", "agent.status"));
    }
}
