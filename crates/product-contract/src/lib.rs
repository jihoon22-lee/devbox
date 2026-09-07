//! Pure validation only. Caller and installation identity must come from native
//! state, never from an assertion made by a renderer or an unverified peer.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod context;
pub use context::{ExecutionTarget, ProjectContext};

pub const MAX_REQUEST_BYTES: usize = 4096;
pub const MAX_DEADLINE_MS: u64 = 30_000;
pub const MAX_PENDING_IDS: usize = 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Handshake {
    pub protocol_version: u32,
    pub product: String,
    pub installation_id: String,
    pub session_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RouteRequest {
    pub protocol_version: u32,
    pub installation_id: String,
    pub session_id: String,
    pub request_id: String,
    pub deadline_ms: u64,
    pub route: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<ProjectContext>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Provenance {
    pub product: String,
    pub component: String,
    pub request_id: String,
    pub revision: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RouteStatus {
    pub route: String,
    pub availability: String,
    pub provenance: Provenance,
}

/// This is a per-native-session replay cache. Unexpired entries are never
/// evicted to admit a new request; overload fails closed until expiry.
pub struct SessionGuard {
    handshake: Handshake,
    seen: HashMap<String, u64>,
    context: Option<ProjectContext>,
}

impl SessionGuard {
    pub fn new(handshake: Handshake) -> Self {
        Self {
            handshake,
            seen: HashMap::new(),
            context: None,
        }
    }

    pub fn handshake(&self) -> &Handshake {
        &self.handshake
    }

    pub fn context(&self) -> Option<&ProjectContext> {
        self.context.as_ref()
    }

    /// Called only by a native registry owner after resolving filesystem/distro
    /// identity. This does not approve a renderer assertion or grant trust.
    pub fn bind_context(&mut self, context: Option<ProjectContext>) -> Result<(), &'static str> {
        if let Some(context) = &context {
            context.validate()?;
        }
        self.context = context;
        Ok(())
    }

    pub fn authorize(
        &mut self,
        caller_window: &str,
        local_origin: bool,
        request: &RouteRequest,
        now_ms: u64,
        allowed_routes: &[&str],
    ) -> Result<(), &'static str> {
        if caller_window != "main" || !local_origin {
            return Err("unauthorized caller");
        }
        if let Some(context) = &request.context {
            context.validate()?;
        }
        if request.context != self.context {
            return Err("stale or unregistered project context");
        }
        if request.installation_id.len() > 128
            || request.session_id.len() > 128
            || request.route.len() > 96
            || request.request_id.len() > 64
        {
            return Err("request exceeds limit");
        }
        let bytes = serde_json::to_vec(request).map_err(|_| "invalid request")?;
        if bytes.len() > MAX_REQUEST_BYTES {
            return Err("request exceeds limit");
        }
        if request.protocol_version != 1
            || request.protocol_version != self.handshake.protocol_version
            || request.session_id != self.handshake.session_id
            || request.installation_id != self.handshake.installation_id
        {
            return Err("session or installation mismatch");
        }
        if request.request_id.is_empty()
            || request.request_id.len() > 64
            || !request
                .request_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || !allowed_routes.contains(&request.route.as_str())
        {
            return Err("invalid request identity or route");
        }
        if request.deadline_ms <= now_ms || request.deadline_ms - now_ms > MAX_DEADLINE_MS {
            return Err("expired or excessive deadline");
        }
        self.seen.retain(|_, expiry| *expiry > now_ms);
        if self.seen.contains_key(&request.request_id) {
            return Err("replayed request");
        }
        if self.seen.len() >= MAX_PENDING_IDS {
            return Err("request capacity reached");
        }
        self.seen
            .insert(request.request_id.clone(), request.deadline_ms);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn guard() -> SessionGuard {
        SessionGuard::new(Handshake {
            protocol_version: 1,
            product: "workspace".into(),
            installation_id: "fixture-install".into(),
            session_id: "fixture-session".into(),
        })
    }
    fn request() -> RouteRequest {
        serde_json::from_str(include_str!(
            "../../../packages/product-shell/fixtures/route-request.json"
        ))
        .unwrap()
    }
    #[test]
    fn shared_fixture_replay_expiry_and_capacity() {
        let mut g = guard();
        assert!(g
            .authorize("main", true, &request(), 1000, &["overview"])
            .is_ok());
        assert_eq!(
            g.authorize("main", true, &request(), 1001, &["overview"]),
            Err("replayed request")
        );
        for i in 1..MAX_PENDING_IDS {
            let mut r = request();
            r.request_id = format!("req-{i}");
            g.authorize("main", true, &r, 1001, &["overview"]).unwrap();
        }
        let mut r = request();
        r.request_id = "overflow".into();
        assert_eq!(
            g.authorize("main", true, &r, 1001, &["overview"]),
            Err("request capacity reached")
        );
        r.deadline_ms = 7000;
        assert!(g.authorize("main", true, &r, 6000, &["overview"]).is_ok());
    }
    #[test]
    fn caller_session_owner_version_bounds_and_deadline_are_native_checks() {
        for (window, local) in [("other", true), ("main", false)] {
            assert!(guard()
                .authorize(window, local, &request(), 1000, &["overview"])
                .is_err());
        }
        for field in ["sessionId", "installationId", "route", "requestId"] {
            let mut v = serde_json::to_value(request()).unwrap();
            v[field] = "invalid".into();
            if field == "requestId" {
                v[field] = "../invalid".into();
            }
            assert!(guard()
                .authorize(
                    "main",
                    true,
                    &serde_json::from_value(v).unwrap(),
                    1000,
                    &["overview"]
                )
                .is_err());
        }
        for deadline in [0, 1000, 31001, u64::MAX] {
            let mut r = request();
            r.deadline_ms = deadline;
            assert!(guard()
                .authorize("main", true, &r, 1000, &["overview"])
                .is_err());
        }
        let mut r = request();
        r.protocol_version = 2;
        assert!(guard()
            .authorize("main", true, &r, 1000, &["overview"])
            .is_err());
        r = request();
        r.route = "x".repeat(MAX_REQUEST_BYTES);
        assert!(guard()
            .authorize("main", true, &r, 1000, &["overview"])
            .is_err());
    }

    #[test]
    fn native_context_binding_rejects_other_worktrees_and_stale_revisions() {
        let context: ProjectContext = serde_json::from_str(include_str!(
            "../../../packages/product-shell/fixtures/project-context.json"
        ))
        .unwrap();
        let mut g = guard();
        let mut r = request();
        r.context = Some(context.clone());
        assert!(g.authorize("main", true, &r, 1000, &["overview"]).is_err());
        g.bind_context(Some(context.clone())).unwrap();
        g.authorize("main", true, &r, 1000, &["overview"]).unwrap();
        let mut revised = context;
        revised.revision += 1;
        g.bind_context(Some(revised)).unwrap();
        r.request_id = "fresh-request-stale-context".into();
        assert!(g.authorize("main", true, &r, 1000, &["overview"]).is_err());
        r.context = g.context().cloned();
        r.context.as_mut().unwrap().worktree_id = "another-worktree".into();
        assert!(g.authorize("main", true, &r, 1000, &["overview"]).is_err());
    }
}
