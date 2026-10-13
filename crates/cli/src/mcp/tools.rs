use super::{Server, protocol::*};
use crate::client::ClientError;
use devbox_agents::{AgentsListMethod, AgentsSend, ListAgents, SendToAgent, state::AgentState};
use devbox_projects::{ListProjects, ProjectsList};
use devbox_protocol::Method;
use devbox_protocol::message::ClientKind;
use std::collections::BTreeMap;
use std::time::Duration;

const RPC_DEADLINE: Duration = Duration::from_secs(5);

pub fn list(write: bool) -> Vec<Tool> {
    let mut tools = vec![
        tool("devbox_projects", "등록된 프로젝트 목록을 조회합니다.", &[]),
        tool(
            "devbox_agents",
            "열린 에이전트 작업과 상태·주의 메시지를 조회합니다.",
            &[],
        ),
    ];
    if write {
        tools.push(tool(
            "devbox_agent_send",
            "에이전트 작업에 지시를 보냅니다.",
            &["id", "text"],
        ));
    }
    tools
}
fn tool(name: &'static str, description: &'static str, fields: &[&'static str]) -> Tool {
    Tool {
        name,
        description,
        input_schema: InputSchema {
            kind: "object",
            properties: fields
                .iter()
                .map(|field| (*field, StringSchema { kind: "string" }))
                .collect::<BTreeMap<_, _>>(),
            required: fields.to_vec(),
            additional_properties: false,
        },
    }
}
impl Server {
    pub(super) async fn call_tool(&mut self, call: CallParams) -> Result<ToolResult, RpcError> {
        match call.name.as_str() {
            "devbox_projects" => {
                let _: Empty = call
                    .arguments
                    .as_deref()
                    .map(|raw| params(Some(raw)))
                    .transpose()?
                    .unwrap_or_default();
                match self.rpc::<ProjectsList>(&ListProjects {}).await {
                    Ok(result) => ToolResult::json(result),
                    Err(error) => Ok(error),
                }
            }
            "devbox_agents" => {
                let _: Empty = call
                    .arguments
                    .as_deref()
                    .map(|raw| params(Some(raw)))
                    .transpose()?
                    .unwrap_or_default();
                match self
                    .rpc::<AgentsListMethod>(&ListAgents {
                        include_closed_hours: Some(0),
                    })
                    .await
                {
                    Ok(mut result) => {
                        // Closed tasks can still meet the daemon's inclusive time cutoff.
                        result.items.retain(|task| task.state != AgentState::Closed);
                        ToolResult::json(result)
                    }
                    Err(error) => Ok(error),
                }
            }
            "devbox_agent_send" => {
                let args: SendToAgent = params(call.arguments.as_deref())?;
                // Enforce even if a caller invokes a tool hidden from tools/list.
                if !self.write {
                    return Ok(ToolResult::error(
                        "MCP 쓰기 도구가 꺼져 있습니다. [mcp] write = true 설정이 필요합니다.",
                    ));
                }
                match self.rpc::<AgentsSend>(&args).await {
                    Ok(result) => ToolResult::json(result),
                    Err(error) => Ok(error),
                }
            }
            _ => Err(ErrorKind::InvalidParams.into()),
        }
    }
    async fn rpc<M: Method>(&mut self, args: &M::Params) -> Result<M::Output, ToolResult>
    where
        M::Output: serde::de::DeserializeOwned,
        M::Params: serde::Serialize,
    {
        // One bounded attempt. Never reconnect/retry a mutation whose acceptance is unknown.
        let operation = async {
            if self.client.is_none() {
                self.client = Some(
                    crate::client::Client::connect(
                        &self.paths.socket,
                        ClientKind::Mcp,
                        self.paths.instance.as_str(),
                    )
                    .await?,
                );
            }
            self.client
                .as_ref()
                .expect("connected MCP client")
                .call::<M>(args)
                .await
        };
        match tokio::time::timeout(RPC_DEADLINE, operation).await {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(error)) => {
                let message = message_for(&error);
                if !matches!(error, ClientError::Rpc { .. }) {
                    self.client = None;
                }
                Err(ToolResult::error(message))
            }
            Err(_) => {
                self.client = None;
                Err(ToolResult::error(
                    "데몬 응답 시간이 초과되어 실행 결과를 확인하지 못했습니다. 작업 상태를 확인한 뒤 재시도해 주세요.",
                ))
            }
        }
    }
}
fn message_for(error: &ClientError) -> &'static str {
    match error {
        ClientError::Rpc { code, .. } => match code.as_str() {
            "agents.not_found" => "에이전트 작업을 찾을 수 없습니다.",
            "agents.bad_state" => "현재 상태에서는 실행할 수 없습니다.",
            "agents.session_missing" => {
                "에이전트 실행이 끊겼습니다. 에이전트 작업을 재개해 주세요."
            }
            "agents.invalid_input" | "invalid_params" => {
                "에이전트 작업의 입력값이 올바르지 않습니다."
            }
            "cancelled" => "요청이 취소되었습니다.",
            "daemon_restarting" => {
                "다시 시작하는 중이라 실행하지 않았습니다. 잠시 뒤 다시 시도해 주세요."
            }
            "method_not_found" => "이 기능을 사용할 수 없습니다.",
            _ => "예상하지 못한 문제가 생겼습니다.",
        },
        ClientError::Io(_) => "데몬에 연결할 수 없습니다. 데몬 상태를 확인해 주세요.",
        _ => {
            "데몬 연결이 끊겨 실행 결과를 확인하지 못했습니다. 작업 상태를 확인한 뒤 재시도해 주세요."
        }
    }
}
