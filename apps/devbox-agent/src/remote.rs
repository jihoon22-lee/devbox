use product_contract::{Handshake, ProjectContext, RouteRequest, SessionGuard};
use std::sync::Mutex;

pub struct RemoteSession(Mutex<(SessionGuard, std::time::Instant)>);
impl RemoteSession {
    pub fn new(product: &str, installation: &str, session: &str) -> Self {
        Self(Mutex::new((
            SessionGuard::new(Handshake {
                protocol_version: 1,
                product: product.into(),
                installation_id: installation.into(),
                session_id: session.into(),
            }),
            std::time::Instant::now(),
        )))
    }
    pub fn recent(&self) -> bool {
        self.0
            .lock()
            .map(|state| state.1.elapsed().as_secs() < 60)
            .unwrap_or(true)
    }
    pub fn authorize(
        &self,
        request: &RouteRequest,
        routes: &[&str],
        now: u64,
        context: impl FnOnce(&ProjectContext) -> bool,
    ) -> Result<(), &'static str> {
        if request
            .context
            .as_ref()
            .is_some_and(|value| !context(value))
        {
            return Err("stale_context");
        }
        let mut guard = self.0.lock().map_err(|_| "busy")?;
        guard.1 = std::time::Instant::now();
        guard
            .0
            .bind_context(request.context.clone())
            .map_err(|_| "invalid_request")?;
        guard
            .0
            .authorize("main", true, request, now, routes)
            .map_err(|_| "agent_request_rejected")
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> RouteRequest {
        serde_json::from_value(serde_json::json!({"protocolVersion":1,"installationId":"native-install","sessionId":"s","requestId":"one","deadlineMs":2000,"route":"tasks"})).unwrap()
    }
    #[test]
    fn remote_admission_binds_native_identity_routes_deadlines_and_replay() {
        let session = RemoteSession::new("workspace", "native-install", "s");
        let request = request();
        assert!(session
            .authorize(&request, &["tasks"], 1000, |_| true)
            .is_ok());
        assert!(session
            .authorize(&request, &["tasks"], 1000, |_| true)
            .is_err());
        for field in ["installation", "session", "route", "deadline"] {
            let session = RemoteSession::new("workspace", "native-install", "s");
            let mut request = request.clone();
            match field {
                "installation" => request.installation_id = "foreign".into(),
                "session" => request.session_id = "foreign".into(),
                "route" => request.route = "files".into(),
                _ => request.deadline_ms = 1000,
            }
            assert!(session
                .authorize(&request, &["tasks"], 1000, |_| true)
                .is_err());
        }
    }
}
