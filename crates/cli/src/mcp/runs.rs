use super::{
    Server,
    protocol::*,
    tools::{RPC_DEADLINE, tool},
};
use devbox_observe::{LogLine, LogSource, LogsSearch, ObserveProblems, ProblemsQuery, SearchLogs};
use devbox_runtime::{RunsQuery, RuntimeRuns, RuntimeStart, StartRun};
use serde::{Deserialize, Serialize};

const MAX_LINES: u32 = 500;
const MAX_BYTES: usize = 768 * 1024;
const MAX_PAGES: usize = 64;

pub(super) fn tools() -> Vec<Tool> {
    let mut runs = tool("devbox_runs", "최근 실행을 최대 100개 조회합니다.", &[]);
    runs.input_schema
        .properties
        .insert("projectId", PropertySchema::String);
    let mut logs = tool(
        "devbox_logs",
        "비밀 값이 가려진 최근 실행 로그를 시간순으로 조회합니다. 기본 200줄, 최대 500줄·768 KiB입니다.",
        &["runId"],
    );
    logs.input_schema.properties.insert(
        "lines",
        PropertySchema::Integer {
            minimum: 1,
            maximum: MAX_LINES,
        },
    );
    vec![
        runs,
        logs,
        tool(
            "devbox_problems",
            "프로젝트 문제를 최대 100개 조회합니다.",
            &["projectId"],
        ),
    ]
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProjectFilter {
    project_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Project {
    project_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Task {
    project_id: String,
    id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Logs {
    run_id: String,
    #[serde(default = "default_lines")]
    lines: u32,
}
fn default_lines() -> u32 {
    200
}
#[derive(Serialize)]
struct LogResult {
    items: Vec<LogLine>,
    truncated: bool,
}
impl Server {
    pub(super) async fn call_run_tool(&mut self, call: CallParams) -> Result<ToolResult, RpcError> {
        match call.name.as_str() {
            "devbox_runs" => {
                let args: ProjectFilter = call
                    .arguments
                    .as_deref()
                    .map(|r| params(Some(r)))
                    .transpose()?
                    .unwrap_or_default();
                match self
                    .rpc::<RuntimeRuns>(&RunsQuery {
                        project_id: args.project_id,
                        limit: 100,
                    })
                    .await
                {
                    Ok(result) => ToolResult::json(result),
                    Err(error) => Ok(error),
                }
            }
            "devbox_problems" => {
                let args: Project = params(call.arguments.as_deref())?;
                match self
                    .rpc::<ObserveProblems>(&ProblemsQuery {
                        project_id: args.project_id,
                        limit: Some(100),
                        cursor: None,
                    })
                    .await
                {
                    Ok(result) => ToolResult::json(result),
                    Err(error) => Ok(error),
                }
            }
            "devbox_run_task" => {
                let args: Task = params(call.arguments.as_deref())?;
                if !self.write {
                    return Ok(ToolResult::error(
                        "MCP 쓰기 도구가 꺼져 있습니다. [mcp] write = true 설정이 필요합니다.",
                    ));
                }
                match self
                    .rpc::<RuntimeStart>(&StartRun {
                        project_id: args.project_id,
                        id: args.id,
                        worktree: None,
                    })
                    .await
                {
                    Ok(result) => ToolResult::json(result),
                    Err(error) => Ok(error),
                }
            }
            "devbox_logs" => {
                let args: Logs = params(call.arguments.as_deref())?;
                if args.lines == 0 || args.lines > MAX_LINES {
                    return Err(ErrorKind::InvalidParams.into());
                }
                // Bound the whole traversal as well as each individual RPC. Dropping
                // the client closes the socket and cancels the daemon's pending scan.
                match tokio::time::timeout(RPC_DEADLINE, self.logs(args)).await {
                    Ok(result) => result,
                    Err(_) => {
                        self.client = None;
                        Ok(ToolResult::error(
                            "로그 조회 시간이 초과되었습니다. 잠시 뒤 다시 시도해 주세요.",
                        ))
                    }
                }
            }
            _ => Err(ErrorKind::InvalidParams.into()),
        }
    }
    async fn logs(&mut self, args: Logs) -> Result<ToolResult, RpcError> {
        let mut before = None;
        let mut items = Vec::new();
        let mut bytes = 64;
        let mut truncated = true;
        'pages: for _ in 0..MAX_PAGES {
            let page = match self
                .rpc::<LogsSearch>(&SearchLogs {
                    sources: vec![LogSource::Run {
                        run_id: args.run_id.clone(),
                    }],
                    query: String::new(),
                    regex: false,
                    before,
                    limit: (args.lines - items.len() as u32).min(256),
                })
                .await
            {
                Ok(page) => page,
                Err(error) => return Ok(error),
            };
            for item in page.items {
                let size = serde_json::to_vec(&item)
                    .map_err(|_| RpcError::from(ErrorKind::Internal))?
                    .len()
                    + 1;
                if bytes + size > MAX_BYTES {
                    break 'pages;
                }
                bytes += size;
                items.push(item);
            }
            before = page.next_cursor;
            truncated = before.is_some();
            if before.is_none() || items.len() == args.lines as usize {
                break;
            }
            // Empty pages can still carry a cursor when a scan budget was reached.
        }
        items.reverse();
        ToolResult::json(LogResult { items, truncated })
    }
}
