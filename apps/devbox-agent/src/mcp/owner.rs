use super::{notes, settings};
use crate::{collectors::Collectors, remote::RemoteSession, runtime::Runtime};
use mcp_server::{McpSettings, ToolCall};
use product_contract::RouteRequest;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc};
use workspace_core::lanes::{Lane, Lanes};
type Result<T> = std::result::Result<T, &'static str>;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct McpRequest {
    header: RouteRequest,
    generation: String,
    method: String,
    args: Value,
}
enum Call {
    Settings,
    Tool(ToolCall),
}
fn admit(
    product: &str,
    generation: &str,
    session: &RemoteSession,
    request: &McpRequest,
    now: u64,
) -> Result<Call> {
    if product != "mcp" || request.generation != generation {
        return Err("unauthorized");
    }
    session.authorize(&request.header, &["mcp"], now, |_| false)?;
    if request.method == "settings" && request.args == json!({}) {
        return Ok(Call::Settings);
    }
    ToolCall::parse(&format!("devbox_{}", request.method), &request.args)
        .map(Call::Tool)
        .map_err(|_| "invalid_request")
}
fn now() -> Result<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|time| time.as_millis() as u64)
        .map_err(|_| "request_expired")
}
pub struct Paths {
    pub installation: PathBuf,
    pub data: PathBuf,
    pub knowledge: PathBuf,
    pub generation: String,
}
pub struct Owner {
    paths: Paths,
    runtime: Arc<Runtime>,
    collectors: Arc<Collectors>,
    verify: Arc<dyn Fn() -> Result<()> + Send + Sync>,
    serial: tokio::sync::Mutex<bool>,
    lanes: Lanes,
}
impl Owner {
    pub fn new(
        paths: Paths,
        runtime: Arc<Runtime>,
        collectors: Arc<Collectors>,
        verify: Arc<dyn Fn() -> Result<()> + Send + Sync>,
    ) -> Arc<Self> {
        Arc::new(Self {
            paths,
            runtime,
            collectors,
            verify,
            serial: Default::default(),
            lanes: Default::default(),
        })
    }
    pub async fn shutdown(&self) {
        *self.serial.lock().await = true;
    }
    pub async fn dispatch(
        self: &Arc<Self>,
        product: &str,
        session: Arc<RemoteSession>,
        request: Value,
    ) -> Value {
        let product = product.to_owned();
        let owner = self.clone();
        let result = tauri::async_runtime::spawn(async move {
            let _permit = owner.lanes.try_enter(Lane::Engine)?;
            let request: McpRequest =
                serde_json::from_value(request).map_err(|_| "invalid_request")?;
            let call = admit(
                &product,
                &owner.paths.generation,
                &session,
                &request,
                now()?,
            )?;
            let stopped = owner.serial.lock().await;
            if *stopped {
                return Err("agent_unavailable");
            }
            (owner.verify)()?;
            workspace_core::current_deadline(request.header.deadline_ms)?;
            let data = owner.paths.data.clone();
            let selected = tauri::async_runtime::spawn_blocking(move || settings::load(&data))
                .await
                .map_err(|_| "worker_unavailable")?;
            match call {
                Call::Settings => Ok(json!(selected)),
                Call::Tool(tool) => {
                    if !selected.enabled {
                        return Err("mcp_disabled");
                    }
                    if !tool.allowed(&selected) {
                        return Err("tool_not_allowed");
                    }
                    let result = match tool {
                        ToolCall::Search { query, limit } => {
                            owner
                                .collectors
                                .mcp_search(query, limit, request.header.deadline_ms)
                                .await
                        }
                        ToolCall::NoteRead { path } => {
                            let data = owner.paths.knowledge.clone();
                            tauri::async_runtime::spawn_blocking(move || {
                                workspace_core::current_deadline(request.header.deadline_ms)?;
                                let vault = knowledge_stores::vault_root(&data)
                                    .map_err(|_| "vault_unavailable")?
                                    .ok_or("vault_unavailable")?;
                                notes::read(&vault, &path)
                                    .map(|note| json!(note))
                                    .map_err(|error| note_issue(&error.code))
                            })
                            .await
                            .map_err(|_| "worker_unavailable")?
                        }
                        ToolCall::NoteCapture { title, body } => {
                            let data = owner.paths.knowledge.clone();
                            tauri::async_runtime::spawn_blocking(move || {
                                workspace_core::current_deadline(request.header.deadline_ms)?;
                                let vault = knowledge_stores::vault_root(&data)
                                    .map_err(|_| "vault_unavailable")?
                                    .ok_or("vault_unavailable")?;
                                notes::capture(&vault, &title, &body, now()?)
                                    .map(|path| json!({"path":path}))
                                    .map_err(|error| note_issue(&error.code))
                            })
                            .await
                            .map_err(|_| "worker_unavailable")?
                        }
                        tool => {
                            owner
                                .runtime
                                .mcp(tool, request.header.request_id, request.header.deadline_ms)
                                .await
                        }
                    }?;
                    (owner.verify)()?;
                    Ok(result)
                }
            }
        })
        .await
        .unwrap_or(Err("worker_unavailable"));
        crate::runtime::response(result)
    }
    pub async fn settings(
        self: &Arc<Self>,
        product: &str,
        session: Arc<RemoteSession>,
        request: Value,
    ) -> Value {
        if product != "control-center" {
            return crate::routes::failure("unauthorized");
        }
        let owner = self.clone();
        let result = tauri::async_runtime::spawn(async move {
            let _permit = owner.lanes.try_enter(Lane::Metadata)?;
            let incoming: product_ipc::IncomingRequest = serde_json::from_value(request).map_err(|_| "invalid_request")?;
            session.authorize(&incoming.header, &["environment"], now()?, |_| false)?;
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Input { settings: McpSettings }
            let update = match incoming.method.as_str() {
                "mcp_settings" if incoming.args == json!({}) => None,
                "set_mcp_settings" => Some(serde_json::from_value::<Input>(incoming.args).map_err(|_| "invalid_request")?.settings),
                _ => return Err("method_unknown"),
            };
            let stopped = owner.serial.lock().await;
            if *stopped { return Err("agent_unavailable"); }
            workspace_core::current_deadline(incoming.header.deadline_ms)?;
            (owner.verify)()?;
            let data = owner.paths.data.clone();
            let installation = owner.paths.installation.clone();
            let selected = tauri::async_runtime::spawn_blocking(move || {
                if let Some(update) = update {
                    let image = std::env::current_exe().map_err(|_| "mcp_settings_unavailable")?;
                    settings::save_ready(&data, &installation, &image, &update, incoming.header.deadline_ms).map_err(|_| "mcp_settings_unavailable")?;
                }
                Ok::<_, &'static str>(settings::load(&data))
            }).await.map_err(|_| "worker_unavailable")??;
            (owner.verify)()?;
            Ok(json!({"settings":selected,"launcherPath":super::launcher::stable_path(&owner.paths.installation)}))
        }).await.unwrap_or(Err("worker_unavailable"));
        crate::runtime::response(result)
    }
}
fn note_issue(code: &str) -> &'static str {
    match code {
        "note_missing" => "note_missing",
        "note_path_invalid" => "note_path_invalid",
        "note_capture_limit" => "note_capture_limit",
        "vault_unavailable" => "vault_unavailable",
        _ => "note_unavailable",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::RemoteSession;
    use serde_json::json;
    fn request() -> McpRequest {
        serde_json::from_value(json!({"header":{"protocolVersion":1,"installationId":"native-root","sessionId":"s","requestId":"r","deadlineMs":2000,"route":"mcp"},"generation":"g1","method":"projects","args":{}})).unwrap()
    }
    #[test]
    fn native_mcp_admission_binds_role_generation_deadline_and_replay() {
        let session = RemoteSession::new("mcp", "native-root", "s");
        let request = request();
        assert!(admit("workspace", "g1", &session, &request, 1000).is_err());
        assert!(admit("mcp", "g2", &session, &request, 1000).is_err());
        assert!(admit("mcp", "g1", &session, &request, 1000).is_ok());
        assert!(admit("mcp", "g1", &session, &request, 1000).is_err());
        let session = RemoteSession::new("mcp", "other-root", "s");
        assert!(admit("mcp", "g1", &session, &request, 1000).is_err());
        let session = RemoteSession::new("mcp", "native-root", "s");
        assert!(admit("mcp", "g1", &session, &request, 2001).is_err());
        let mut request = request;
        request.header.context = Some(product_contract::ProjectContext {
            project_id: "p".into(),
            worktree_id: "w".into(),
            revision: 1,
            target: product_contract::ExecutionTarget::Windows,
        });
        assert!(admit(
            "mcp",
            "g1",
            &RemoteSession::new("mcp", "native-root", "s"),
            &request,
            1000
        )
        .is_err());
    }
}
