//! Pure validation only. Caller and installation identity must come from native
//! state, never from an assertion made by a renderer or an unverified peer.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod commands;
pub mod context;
pub mod installation;
pub mod operation;
pub mod references;
pub mod session_summary;
pub mod transport;
pub use context::{ExecutionTarget, ProjectContext};
pub use operation::{Operation, OperationState, Problem, ProblemCode};

pub const MAX_REQUEST_BYTES: usize = 4096;
pub const MAX_DEADLINE_MS: u64 = 30_000;
pub const MAX_PENDING_IDS: usize = 1024;

/// Development navigation accepts only registered route slugs, never a URL or
/// a path. This selects a view and does not grant that view command authority.
pub fn development_route(
    args: impl IntoIterator<Item = String>,
    allowed_routes: &[&str],
) -> Result<Option<String>, &'static str> {
    let mut route = None;
    for argument in args {
        if let Some(value) = argument.strip_prefix("--route=") {
            if route.is_some() || !allowed_routes.contains(&value) {
                return Err("invalid or repeated development route");
            }
            route = Some(value.to_owned());
        } else if argument == "--route" {
            return Err("development route requires --route=<registered-route>");
        }
    }
    Ok(route)
}

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
    pub operation: Operation,
}

/// This is a per-native-session replay cache. Unexpired entries are never
/// evicted to admit a new request; overload fails closed until expiry.
pub struct SessionGuard {
    caller_window: String,
    handshake: Handshake,
    seen: HashMap<String, u64>,
    context: Option<ProjectContext>,
}

impl SessionGuard {
    pub fn new(handshake: Handshake) -> Self {
        Self {
            caller_window: "main".into(),
            handshake,
            seen: HashMap::new(),
            context: None,
        }
    }

    /// The native companion factory supplies this immutable label before creating
    /// the window. This constructor is not exposed through a renderer request.
    pub fn for_window(handshake: Handshake, native_label: &str) -> Result<Self, &'static str> {
        if native_label.is_empty()
            || native_label.len() > 96
            || !native_label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err("invalid native window");
        }
        Ok(Self {
            caller_window: native_label.into(),
            ..Self::new(handshake)
        })
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
    ) -> Result<(), ProblemCode> {
        if caller_window != self.caller_window || !local_origin {
            return Err(ProblemCode::Unauthorized);
        }
        if let Some(context) = &request.context {
            context
                .validate()
                .map_err(|_| ProblemCode::InvalidRequest)?;
        }
        if request.context != self.context {
            return Err(ProblemCode::StaleContext);
        }
        if request.installation_id.len() > 128
            || request.session_id.len() > 128
            || request.route.len() > 96
            || request.request_id.len() > 64
        {
            return Err(ProblemCode::InvalidRequest);
        }
        let bytes = serde_json::to_vec(request).map_err(|_| ProblemCode::InvalidRequest)?;
        if bytes.len() > MAX_REQUEST_BYTES {
            return Err(ProblemCode::InvalidRequest);
        }
        if request.protocol_version != 1
            || request.protocol_version != self.handshake.protocol_version
            || request.session_id != self.handshake.session_id
            || request.installation_id != self.handshake.installation_id
        {
            return Err(ProblemCode::Unauthorized);
        }
        if request.request_id.is_empty()
            || request.request_id.len() > 64
            || !request
                .request_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || !allowed_routes.contains(&request.route.as_str())
        {
            return Err(ProblemCode::InvalidRequest);
        }
        if request.deadline_ms <= now_ms || request.deadline_ms - now_ms > MAX_DEADLINE_MS {
            return Err(ProblemCode::Expired);
        }
        self.seen.retain(|_, expiry| *expiry > now_ms);
        if self.seen.contains_key(&request.request_id) {
            return Err(ProblemCode::Replayed);
        }
        if self.seen.len() >= MAX_PENDING_IDS {
            return Err(ProblemCode::Overloaded);
        }
        self.seen
            .insert(request.request_id.clone(), request.deadline_ms);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn development_route_cannot_navigate_to_foreign_origins_or_unregistered_views() {
        let parse = |args: &[&str]| {
            development_route(args.iter().map(|s| s.to_string()), &["overview", "files"])
        };
        assert_eq!(parse(&["--route=files"]), Ok(Some("files".into())));
        assert_eq!(parse(&[]), Ok(None));
        for args in [
            vec!["--route=https://remote"],
            vec!["--route=../private"],
            vec!["--route=notes"],
            vec!["--route"],
            vec!["--route=files", "--route=overview"],
        ] {
            assert!(parse(&args).is_err());
        }
    }
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
    fn native_companion_binding_cannot_borrow_the_main_or_another_window() {
        let handshake = guard().handshake().clone();
        let mut companion = SessionGuard::for_window(handshake, "terminal-fixture").unwrap();
        for window in ["main", "terminal-other"] {
            assert_eq!(
                companion.authorize(window, true, &request(), 1000, &["overview"]),
                Err(ProblemCode::Unauthorized)
            );
        }
        assert_eq!(
            companion.authorize("terminal-fixture", false, &request(), 1000, &["overview"]),
            Err(ProblemCode::Unauthorized)
        );
        assert!(companion
            .authorize("terminal-fixture", true, &request(), 1000, &["overview"])
            .is_ok());
        assert_eq!(
            guard().authorize("terminal-fixture", true, &request(), 1000, &["overview"]),
            Err(ProblemCode::Unauthorized)
        );
    }
    #[test]
    fn shared_fixture_replay_expiry_and_capacity() {
        let mut g = guard();
        assert!(g
            .authorize("main", true, &request(), 1000, &["overview"])
            .is_ok());
        assert_eq!(
            g.authorize("main", true, &request(), 1001, &["overview"]),
            Err(ProblemCode::Replayed)
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
            Err(ProblemCode::Overloaded)
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

pub mod command_index;

pub mod navigation;

pub mod launcher_preferences;

pub mod query;

pub mod shortcuts;

pub mod project_provider;
