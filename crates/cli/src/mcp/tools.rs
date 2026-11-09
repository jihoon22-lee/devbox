use super::{Server, protocol::*};
use crate::client::ClientError;
use devbox_agents::{AgentsListMethod, AgentsSend, ListAgents, SendToAgent, state::AgentState};
use devbox_projects::{ListProjects, ProjectsList};
use devbox_protocol::Method;
use devbox_protocol::message::ClientKind;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::time::Duration;

pub(super) const RPC_DEADLINE: Duration = Duration::from_secs(5);

pub fn list(write: bool) -> Vec<Tool> {
    let mut tools = vec![
        tool("devbox_projects", "등록된 프로젝트 목록을 조회합니다.", &[]),
        tool(
            "devbox_agents",
            "열린 에이전트 작업과 상태·주의 메시지를 조회합니다.",
            &[],
        ),
    ];
    tools[1]
        .input_schema
        .properties
        .insert("afterId", PropertySchema::String);
    tools.extend(super::runs::tools());
    tools.extend(super::notes::tools(write));
    if write {
        tools.push(tool(
            "devbox_agent_send",
            "에이전트 작업에 지시를 보냅니다.",
            &["id", "text"],
        ));
    }
    if write {
        tools.push(tool(
            "devbox_run_task",
            "승인된 프로젝트 작업을 실행합니다.",
            &["projectId", "id"],
        ));
    }
    tools
}
pub(super) fn tool(name: &'static str, description: &'static str, fields: &[&'static str]) -> Tool {
    Tool {
        name,
        description,
        input_schema: InputSchema {
            kind: "object",
            properties: fields
                .iter()
                .map(|field| (*field, PropertySchema::String))
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
                #[derive(Default, serde::Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct AgentPage {
                    after_id: Option<String>,
                }
                let page: AgentPage = call
                    .arguments
                    .as_deref()
                    .map(|raw| params(Some(raw)))
                    .transpose()?
                    .unwrap_or_default();
                match self
                    .rpc::<AgentsListMethod>(&ListAgents {
                        include_closed_hours: Some(0),
                        after_id: page.after_id,
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
                // No pane check can see this: the screen is a plain prompt. Every task gets it,
                // whatever its tool; the app's own sends skip it.
                if runs_as_command(&args.text) {
                    return Ok(ToolResult::error(
                        "보낼 글이 `!`나 `/`로 시작하면 도구가 셸 명령이나 도구 명령으로 실행할 수 있어 보내지 않았습니다. 앞에 다른 말을 붙여 다시 보내세요.",
                    ));
                }
                match self.rpc::<AgentsSend>(&args).await {
                    Ok(result) => ToolResult::json(result),
                    Err(error) => Ok(error),
                }
            }
            "devbox_runs" | "devbox_logs" | "devbox_problems" | "devbox_run_task" => {
                self.call_run_tool(call).await
            }
            "devbox_notes_search" | "devbox_note_read" | "devbox_note_add" => {
                self.call_note_tool(call).await
            }
            _ => Err(ErrorKind::InvalidParams.into()),
        }
    }
    pub(super) async fn rpc<M: Method>(&mut self, args: &M::Params) -> Result<M::Output, ToolResult>
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
                Err(ToolResult::error(&message))
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
/// Codex, Claude Code and pi run a text that starts with "!" as a shell command, and the tools
/// run one that starts with "/" as their own command, after any blanks. A blank is whatever Rust
/// or JavaScript trims: Codex is Rust, and Claude Code, opencode and pi are JavaScript, whose
/// trimStart also drops U+FEFF.
fn runs_as_command(text: &str) -> bool {
    text.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
        .starts_with(['!', '/'])
}
fn message_for(error: &ClientError) -> Cow<'static, str> {
    Cow::Borrowed(match error {
        ClientError::Rpc {
            code,
            diagnostic_id,
            ..
        } => match code.as_str() {
            "agents.not_found" => "에이전트 작업을 찾을 수 없습니다.",
            "agents.bad_state" => "현재 상태에서는 실행할 수 없습니다.",
            "agents.session_missing" => {
                "에이전트 실행이 끊겼습니다. 에이전트 작업을 재개해 주세요."
            }
            "agents.awaiting_input" => {
                "에이전트가 선택을 기다리고 있습니다. 터미널에서 승인이나 선택을 마친 뒤 다시 보내세요."
            }
            "agents.tool_starting" => {
                "에이전트가 시작하는 중입니다. 터미널에 에이전트 화면이 나타나면 다시 보내세요."
            }
            "agents.control_characters" => {
                "보낼 글에 제어 문자가 있어 보내지 않았습니다. 제어 문자는 터미널에서 키 입력으로 처리됩니다. 탭·줄바꿈 말고 다른 제어 문자(터미널 출력의 색 코드 등)를 지운 뒤 다시 보내세요."
            }
            "agents.delivery_unknown" => {
                "전달 결과를 확인하지 못했습니다. 이미 전달됐을 수 있으니 같은 지시를 다시 보내지 말고 devbox_agents로 작업 상태와 터미널을 확인하세요."
            }
            "agents.invalid_input" => "에이전트 작업의 입력값이 올바르지 않습니다.",
            "invalid_params" => "요청 값이 올바르지 않습니다. 도구 인자를 확인하세요.",
            "projects.trust_required" | "projects.trust_preview_stale" => {
                "projects.trust_required: 프로젝트 실행 명령의 신뢰 승인이 필요합니다. 앱에서 변경 내용을 확인하고 승인해 주세요."
            }
            "runtime.run_not_found" | "observe.run_not_found" => "실행을 찾을 수 없습니다.",
            "runtime.project_not_found" | "observe.project_not_found" => {
                "프로젝트를 찾을 수 없습니다."
            }
            "cancelled" => "요청이 취소되었습니다.",
            "daemon_restarting" => {
                "다시 시작하는 중이라 실행하지 않았습니다. 잠시 뒤 다시 시도해 주세요."
            }
            "method_not_found" => "이 기능을 사용할 수 없습니다.",
            _ => {
                return Cow::Owned(match diagnostic_id {
                    Some(id) => format!("{code}: 예상하지 못한 문제가 생겼습니다. 진단 ID: {id}"),
                    None => format!("{code}: 예상하지 못한 문제가 생겼습니다."),
                });
            }
        },
        ClientError::Io(_) => "데몬에 연결할 수 없습니다. 데몬 상태를 확인해 주세요.",
        _ => {
            "데몬 연결이 끊겨 실행 결과를 확인하지 못했습니다. 작업 상태를 확인한 뒤 재시도해 주세요."
        }
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn rpc(code: &str, diagnostic_id: Option<&str>) -> ClientError {
        rpc_with_detail(code, diagnostic_id, None)
    }
    fn rpc_with_detail(
        code: &str,
        diagnostic_id: Option<&str>,
        detail: Option<&str>,
    ) -> ClientError {
        ClientError::Rpc {
            code: code.into(),
            detail: detail.map(|d| {
                serde_json::value::RawValue::from_string(d.into())
                    .expect("test detail is valid JSON")
            }),
            diagnostic_id: diagnostic_id.map(Into::into),
        }
    }
    #[test]
    fn mcp_delivery_unknown_preserves_code_and_warns_against_resend() {
        let message = message_for(&rpc("agents.delivery_unknown", None));
        assert!(message.contains("다시 보내지"), "{message}");
        assert!(!message.contains("예상하지 못한"), "{message}");
        assert_eq!(
            message,
            "전달 결과를 확인하지 못했습니다. 이미 전달됐을 수 있으니 같은 지시를 다시 보내지 말고 devbox_agents로 작업 상태와 터미널을 확인하세요."
        );
    }
    #[test]
    fn mcp_unknown_daemon_code_names_the_code() {
        let message = message_for(&rpc("notes.conflict", None));
        assert!(message.starts_with("notes.conflict"), "{message}");
        assert!(message.contains("예상하지 못한 문제"), "{message}");
        let with_id = message_for(&rpc("notes.conflict", Some("diag-7")));
        assert!(with_id.starts_with("notes.conflict"), "{with_id}");
        assert!(with_id.contains("diag-7"), "{with_id}");
        let with_detail = message_for(&rpc_with_detail(
            "notes.conflict",
            None,
            Some(r#"{"path":"/home/me/private"}"#),
        ));
        assert!(with_detail.starts_with("notes.conflict"), "{with_detail}");
        assert!(!with_detail.contains("/home/me"), "{with_detail}");
    }
    #[test]
    fn mcp_invalid_params_is_neutral() {
        let message = message_for(&rpc("invalid_params", None));
        assert_eq!(
            message,
            "요청 값이 올바르지 않습니다. 도구 인자를 확인하세요."
        );
    }
    #[test]
    fn a_send_before_the_agent_reads_its_input_says_when_to_send_again() {
        let error = ClientError::Rpc {
            code: "agents.tool_starting".into(),
            detail: None,
            diagnostic_id: None,
        };
        assert_eq!(
            message_for(&error),
            "에이전트가 시작하는 중입니다. 터미널에 에이전트 화면이 나타나면 다시 보내세요."
        );
    }
    #[test]
    fn a_send_with_a_control_character_says_what_to_remove() {
        let error = ClientError::Rpc {
            code: "agents.control_characters".into(),
            detail: None,
            diagnostic_id: None,
        };
        assert_eq!(
            message_for(&error),
            "보낼 글에 제어 문자가 있어 보내지 않았습니다. 제어 문자는 터미널에서 키 입력으로 처리됩니다. 탭·줄바꿈 말고 다른 제어 문자(터미널 출력의 색 코드 등)를 지운 뒤 다시 보내세요."
        );
    }
    #[test]
    fn a_command_prefix_counts_after_any_blank_that_rust_or_javascript_trims() {
        // Every code point Node 24's String.prototype.trimStart drops, and every one Rust's
        // char::is_whitespace matches: the two lists differ only in U+FEFF and U+0085.
        let javascript = "\t\n\u{b}\u{c}\r \u{a0}\u{1680}\u{2000}\u{2001}\u{2002}\u{2003}\u{2004}\u{2005}\u{2006}\u{2007}\u{2008}\u{2009}\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\u{feff}";
        let rust = (char::MIN..=char::MAX).filter(|c| c.is_whitespace());
        for blank in javascript.chars().chain(rust) {
            assert!(runs_as_command(&format!("{blank}!ls")), "{blank:?}");
            assert!(
                runs_as_command(&format!("{blank}{blank}/clear")),
                "{blank:?}"
            );
        }
        assert!(!runs_as_command("hello !ls"));
        assert!(!runs_as_command("fix /tmp/x"));
    }
}
