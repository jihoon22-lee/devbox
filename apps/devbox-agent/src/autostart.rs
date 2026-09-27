//! The shared login preference is owned by the verified background service.
use product_contract::agent_settings::{AgentAutostartStatus, AgentSettingsCall};
use serde_json::{json, Value};
use std::sync::Arc;

pub fn login_launch(args: &[std::ffi::OsString]) -> Result<bool, &'static str> {
    match args {
        [] => Ok(false),
        [flag] if flag == "--autostart" => Ok(true),
        _ => Err("agent_arguments_invalid"),
    }
}

pub struct Settings {
    #[cfg(windows)]
    scope: Arc<suite_runtime::platform::component_scope::CapturedScope>,
    #[cfg(windows)]
    owner: suite_runtime::agent_autostart::Owner,
    serial: std::sync::Mutex<bool>,
}
impl Settings {
    #[cfg(windows)]
    pub fn new(
        scope: Arc<suite_runtime::platform::component_scope::CapturedScope>,
        namespace: &str,
    ) -> Result<Arc<Self>, &'static str> {
        let owner = suite_runtime::agent_autostart::Owner::new(
            &scope.review_root(),
            namespace,
            scope
                .agent_image()?
                .to_str()
                .ok_or("agent_autostart_invalid")?,
        )?;
        Ok(Arc::new(Self {
            scope,
            owner,
            serial: Default::default(),
        }))
    }
    #[cfg(windows)]
    pub fn reconcile(&self) -> Result<(), &'static str> {
        use suite_runtime::{agent_autostart as policy, platform::agent_autostart::Key};
        let serial = self
            .serial
            .lock()
            .map_err(|_| "agent_autostart_unavailable")?;
        if *serial {
            return Err("agent_autostart_unavailable");
        }
        self.scope.revalidate()?;
        suite_runtime::platform::agent_autostart::retire_workspace(&self.owner, true)?;
        let mut key = Key::open()?;
        if let Some(change) = policy::reconcile(&self.owner, policy::snapshot(&key, &self.owner)?)?
        {
            policy::apply(&mut key, &self.owner, &change)?;
        }
        Ok(())
    }
    fn execute(&self, call: AgentSettingsCall) -> Result<AgentAutostartStatus, &'static str> {
        let serial = self
            .serial
            .lock()
            .map_err(|_| "agent_autostart_unavailable")?;
        if *serial {
            return Err("agent_autostart_unavailable");
        }
        #[cfg(windows)]
        {
            use suite_runtime::{agent_autostart as policy, platform::agent_autostart::Key};
            self.scope.revalidate()?;
            let mut key = Key::open()?;
            if let AgentSettingsCall::SetAutostart { enabled } = call {
                suite_runtime::platform::agent_autostart::retire_workspace(&self.owner, enabled)?;
                let change =
                    policy::setting(&self.owner, policy::snapshot(&key, &self.owner)?, enabled)?;
                policy::apply(&mut key, &self.owner, &change)?;
            }
            Ok(AgentAutostartStatus {
                supported: true,
                enabled: self.owner.enabled(&policy::snapshot(&key, &self.owner)?),
            })
        }
        #[cfg(not(windows))]
        {
            let _ = call;
            Ok(AgentAutostartStatus {
                supported: false,
                enabled: false,
            })
        }
    }
    pub async fn shutdown(self: &Arc<Self>) {
        let owner = self.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            if let Ok(mut stopped) = owner.serial.lock() {
                *stopped = true;
            }
        })
        .await;
    }
    pub async fn dispatch(
        self: &Arc<Self>,
        product: &str,
        session: Arc<crate::remote::RemoteSession>,
        request: Value,
    ) -> Value {
        let result = async {
            let incoming: product_ipc::IncomingRequest =
                serde_json::from_value(request).map_err(|_| "invalid_request")?;
            let route = match product {
                "knowledge" => "activity",
                "control-center" => "environment",
                _ => return Err("unauthorized"),
            };
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| "agent_autostart_unavailable")?
                .as_millis() as u64;
            session.authorize(&incoming.header, &[route], now, |_| false)?;
            let call: AgentSettingsCall =
                serde_json::from_value(json!({"method":incoming.method,"args":incoming.args}))
                    .map_err(|_| "method_unknown")?;
            let owner = self.clone();
            tauri::async_runtime::spawn_blocking(move || owner.execute(call))
                .await
                .map_err(|_| "agent_autostart_unavailable")?
        }
        .await;
        match result {
            Ok(value) => json!({"operation":{"outcome":{"state":"succeeded"}},"value":value}),
            Err(issue) => crate::routes::failure(issue),
        }
    }
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;
    #[test]
    fn only_the_single_login_flag_is_accepted() {
        assert_eq!(login_launch(&[]), Ok(false));
        assert_eq!(login_launch(&["--autostart".into()]), Ok(true));
        assert!(login_launch(&["--background".into()]).is_err());
        assert!(login_launch(&["--autostart".into(), "--open".into()]).is_err());
    }
    #[tokio::test]
    async fn settings_enforce_native_session_route_and_closed_arguments() {
        let owner = Arc::new(Settings {
            serial: Default::default(),
        });
        for (route, args, expected) in [
            ("activity", json!({}), "succeeded"),
            ("search", json!({}), "failed"),
            ("activity", json!({"path":"foreign"}), "failed"),
        ] {
            let session = Arc::new(crate::remote::RemoteSession::new(
                "knowledge",
                "native",
                "session",
            ));
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            let reply = owner.dispatch("knowledge", session, json!({"header":{"protocolVersion":1,"installationId":"native","sessionId":"session","requestId":"request","deadlineMs":now+1000,"route":route},"method":"autostart_status","args":args})).await;
            assert_eq!(
                reply
                    .pointer("/operation/outcome/state")
                    .and_then(Value::as_str),
                Some(expected)
            );
        }
        owner.shutdown().await;
        assert!(owner
            .execute(AgentSettingsCall::SetAutostart { enabled: true })
            .is_err());
        let routes = crate::routes::Routes::for_tests();
        assert!(routes.accepts("knowledge", "agent.settings"));
        assert!(routes.accepts("control-center", "agent.settings"));
        assert!(!routes.accepts("workspace", "agent.settings"));
        assert!(!routes.accepts("api-studio", "agent.settings"));
    }
}
