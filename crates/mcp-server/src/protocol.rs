use crate::{
    tools::{self, ToolCall},
    ToolHost,
};
use serde_json::{json, Value};
pub const MODERN_VERSION: &str = "2026-07-28";
pub const LEGACY_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
pub const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;
pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const SERVER_DISABLED: i64 = -32001;
const INTERNAL_ERROR: i64 = -32603;
const VERSION_KEY: &str = "io.modelcontextprotocol/protocolVersion";
const INSTRUCTIONS: &str = "Devbox exposes the user's registered projects, task runs, logs and notes. Prefer devbox_search before reading notes.";
const DISABLED: &str =
    "Devbox MCP server is turned off. Turn it on in Devbox Control Center > Settings > MCP.";
pub struct Server<H: ToolHost> {
    host: H,
    version: String,
}
impl<H: ToolHost> Server<H> {
    pub fn new(host: H, server_version: &str) -> Self {
        Self {
            host,
            version: server_version.chars().take(128).collect(),
        }
    }
    pub fn host_mut(&mut self) -> &mut H {
        &mut self.host
    }
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        if line.len() > MAX_LINE_BYTES {
            return Some(error(Value::Null, INVALID_REQUEST, "Request exceeds 4 MiB"));
        }
        let request: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(_) => return Some(error(Value::Null, PARSE_ERROR, "Invalid JSON")),
        };
        let valid_id = request.get("id").is_none_or(|id| {
            id.is_null() || id.is_number() || id.as_str().is_some_and(|id| id.len() <= 128)
        });
        if !request.is_object()
            || request["jsonrpc"] != "2.0"
            || !request["method"].is_string()
            || !valid_id
        {
            return Some(error(
                Value::Null,
                INVALID_REQUEST,
                "Invalid JSON-RPC request",
            ));
        }
        // Notifications never trigger tool effects or produce a reply.
        let id = request.get("id")?.clone();
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
        if !params.is_object() {
            return Some(error(id, INVALID_PARAMS, "params must be an object"));
        }
        let modern = params["_meta"][VERSION_KEY] == MODERN_VERSION;
        let method = request["method"].as_str().unwrap_or_default();
        let result = self.dispatch(method, &params);
        Some(match result {
            Ok(mut value) => {
                if modern {
                    value["resultType"] = json!("complete");
                }
                let response = json!({"jsonrpc":"2.0","id":id,"result":value}).to_string();
                if response.len() > MAX_LINE_BYTES {
                    error(id, INTERNAL_ERROR, "Tool result exceeds the response limit")
                } else {
                    response
                }
            }
            Err((code, message)) => error(id, code, &message),
        })
    }
    fn dispatch(&mut self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        if method == "ping" {
            return Ok(json!({}));
        }
        if !matches!(
            method,
            "initialize" | "server/discover" | "tools/list" | "tools/call"
        ) {
            return Err((METHOD_NOT_FOUND, "Method not found".into()));
        }
        let settings = self
            .host
            .settings()
            .map_err(|_| (INTERNAL_ERROR, "Could not read Devbox MCP settings".into()))?;
        match method {
            "initialize" => {
                if !settings.enabled {
                    return Err((SERVER_DISABLED, DISABLED.into()));
                }
                let requested = params["protocolVersion"].as_str().unwrap_or_default();
                let version = if LEGACY_VERSIONS.contains(&requested) {
                    requested
                } else {
                    LEGACY_VERSIONS[0]
                };
                Ok(
                    json!({"protocolVersion":version,"capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"devbox","version":self.version},"instructions":INSTRUCTIONS}),
                )
            }
            "server/discover" => {
                if !settings.enabled {
                    return Err((SERVER_DISABLED, DISABLED.into()));
                }
                Ok(
                    json!({"resultType":"complete","supportedVersions":[MODERN_VERSION,LEGACY_VERSIONS[0]],"capabilities":{"tools":{}},"instructions":INSTRUCTIONS,"ttlMs":0,"cacheScope":"private","_meta":{"io.modelcontextprotocol/serverInfo":{"name":"devbox","version":self.version}}}),
                )
            }
            "tools/list" => {
                Ok(json!({"tools":tools::catalog(&settings),"ttlMs":0,"cacheScope":"private"}))
            }
            "tools/call" => {
                let name = params["name"]
                    .as_str()
                    .ok_or((INVALID_PARAMS, "name must be a string".into()))?;
                let arguments = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let tool = ToolCall::parse(name, &arguments)
                    .map_err(|message| (INVALID_PARAMS, message))?;
                if !tool.allowed(&settings) {
                    return Ok(tool_error(if settings.enabled {
                        "This write tool is not enabled in Devbox Control Center."
                    } else {
                        DISABLED
                    }));
                }
                Ok(match self.host.call(&tool) {
                    Ok(value) => {
                        json!({"content":[{"type":"text","text":serde_json::to_string_pretty(&value).unwrap_or_default()}],"structuredContent":value,"isError":false})
                    }
                    Err(error) => tool_error(&error.message),
                })
            }
            _ => unreachable!(),
        }
    }
}
fn tool_error(message: &str) -> Value {
    json!({"content":[{"type":"text","text":message.chars().take(4096).collect::<String>()}],"isError":true})
}
fn error(id: Value, code: i64, message: &str) -> String {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}}).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HostError, McpSettings};
    use serde_json::{json, Value};

    struct FakeHost {
        settings: McpSettings,
        calls: Vec<String>,
    }
    impl ToolHost for FakeHost {
        fn settings(&mut self) -> Result<McpSettings, HostError> {
            Ok(self.settings.clone())
        }
        fn call(&mut self, tool: &ToolCall) -> Result<Value, HostError> {
            self.calls.push(tool.name().to_string());
            match tool {
                ToolCall::Projects => Ok(json!({"projects": [{"id": "p1", "name": "devbox"}]})),
                ToolCall::NoteRead { path } if path == "missing.md" => Err(HostError {
                    code: "note_missing".into(),
                    message: "Note not found".into(),
                }),
                _ => Ok(json!({"ok": true})),
            }
        }
    }
    fn server(enabled: bool) -> Server<FakeHost> {
        Server::new(
            FakeHost {
                settings: McpSettings {
                    enabled,
                    ..McpSettings::default()
                },
                calls: vec![],
            },
            "0.9.0",
        )
    }
    fn reply(server: &mut Server<FakeHost>, message: Value) -> Value {
        serde_json::from_str(
            &server
                .handle_line(&message.to_string())
                .expect("a response"),
        )
        .unwrap()
    }

    #[test]
    fn oversized_request_ids_cannot_escape_the_response_line_limit() {
        let mut s = server(true);
        let response = reply(
            &mut s,
            json!({"jsonrpc":"2.0","id":"x".repeat(1024),"method":"ping"}),
        );
        assert_eq!(response["error"]["code"], INVALID_REQUEST);
        assert!(response["id"].is_null());
    }

    #[test]
    fn notifications_and_invalid_ids_never_execute_tools() {
        let mut s = server(true);
        assert!(s
            .handle_line(
                r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"devbox_projects"}}"#
            )
            .is_none());
        let r = reply(
            &mut s,
            json!({"jsonrpc":"2.0","id":true,"method":"tools/call","params":{"name":"devbox_projects"}}),
        );
        assert_eq!(r["error"]["code"], INVALID_REQUEST);
        assert!(s.host_mut().calls.is_empty());
    }
    #[test]
    fn disabling_a_live_session_hides_tools_and_blocks_calls() {
        let mut s = server(true);
        s.host_mut().settings.enabled = false;
        assert_eq!(
            reply(
                &mut s,
                json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})
            )["result"]["tools"],
            json!([])
        );
        assert_eq!(
            reply(
                &mut s,
                json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"devbox_projects"}})
            )["result"]["isError"],
            true
        );
        assert!(s.host_mut().calls.is_empty());
    }

    #[test]
    fn legacy_initialize_negotiates_a_supported_version() {
        let mut s = server(true);
        let r = reply(
            &mut s,
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"claude-code","version":"2"}}}),
        );
        assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(r["result"]["serverInfo"]["name"], "devbox");
        assert!(r["result"]["capabilities"]["tools"].is_object());
        let r = reply(
            &mut s,
            json!({"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"2023-01-01","capabilities":{}}}),
        );
        assert_eq!(r["result"]["protocolVersion"], "2025-11-25");
        assert!(s
            .handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .is_none());
    }

    #[test]
    fn a_disabled_server_explains_itself_at_the_handshake() {
        let mut s = server(false);
        let r = reply(
            &mut s,
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{}}}),
        );
        assert_eq!(r["error"]["code"], SERVER_DISABLED);
        assert!(r["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Control Center"));
        let r = reply(
            &mut s,
            json!({"jsonrpc":"2.0","id":2,"method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion": MODERN_VERSION}}}),
        );
        assert_eq!(r["error"]["code"], SERVER_DISABLED);
    }

    #[test]
    fn modern_requests_carry_their_version_and_get_complete_results() {
        let mut s = server(true);
        let meta = json!({"io.modelcontextprotocol/protocolVersion": MODERN_VERSION});
        let r = reply(
            &mut s,
            json!({"jsonrpc":"2.0","id":"d","method":"server/discover","params":{"_meta": meta}}),
        );
        assert_eq!(r["result"]["resultType"], "complete");
        assert_eq!(
            r["result"]["supportedVersions"],
            json!([MODERN_VERSION, "2025-11-25"])
        );
        assert_eq!(r["result"]["cacheScope"], "private");
        assert_eq!(
            r["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
            "devbox"
        );
        let r = reply(
            &mut s,
            json!({"jsonrpc":"2.0","id":"l","method":"tools/list","params":{"_meta": meta}}),
        );
        assert_eq!(r["result"]["resultType"], "complete");
        assert_eq!(r["result"]["ttlMs"], 0);
        assert!(r["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "devbox_projects"));
    }

    #[test]
    fn write_tools_are_listed_only_when_allowed() {
        let mut s = server(true);
        let names = |r: &Value| {
            r["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| t["name"].as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        };
        let r = reply(
            &mut s,
            json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}),
        );
        assert!(!names(&r).contains(&"devbox_note_capture".to_string()));
        assert!(!names(&r).contains(&"devbox_task_run".to_string()));
        s.host_mut().settings.allow_task_run = true;
        let r = reply(
            &mut s,
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
        );
        assert!(names(&r).contains(&"devbox_task_run".to_string()));
    }

    #[test]
    fn tool_results_and_tool_failures_use_content_blocks() {
        let mut s = server(true);
        let r = reply(
            &mut s,
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"devbox_projects","arguments":{}}}),
        );
        assert_eq!(r["result"]["isError"], false);
        assert_eq!(
            r["result"]["structuredContent"]["projects"][0]["name"],
            "devbox"
        );
        assert!(r["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("devbox"));
        let r = reply(
            &mut s,
            json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"devbox_note_read","arguments":{"path":"missing.md"}}}),
        );
        assert_eq!(r["result"]["isError"], true);
        assert!(r["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Note not found"));
        let r = reply(
            &mut s,
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"devbox_task_run","arguments":{"jobId":"j1"}}}),
        );
        assert_eq!(
            r["result"]["isError"], true,
            "a disallowed write tool is a tool error, not a call"
        );
        assert!(!s.host_mut().calls.contains(&"devbox_task_run".to_string()));
    }

    #[test]
    fn protocol_errors_keep_the_session_alive() {
        let mut s = server(true);
        let r: Value = serde_json::from_str(&s.handle_line("{not json").unwrap()).unwrap();
        assert_eq!(
            (r["error"]["code"].as_i64(), r["id"].is_null()),
            (Some(PARSE_ERROR), true)
        );
        let r = reply(&mut s, json!([{"jsonrpc":"2.0","id":1,"method":"ping"}]));
        assert_eq!(r["error"]["code"], INVALID_REQUEST);
        let r = reply(
            &mut s,
            json!({"jsonrpc":"2.0","id":2,"method":"resources/list","params":{}}),
        );
        assert_eq!(r["error"]["code"], METHOD_NOT_FOUND);
        let r = reply(
            &mut s,
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"nope","arguments":{}}}),
        );
        assert_eq!(r["error"]["code"], INVALID_PARAMS);
        let r = reply(
            &mut s,
            json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"devbox_run_log","arguments":{"runId":5}}}),
        );
        assert_eq!(r["error"]["code"], INVALID_PARAMS);
        let long = format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"ping\",\"pad\":\"{}\"}}",
            "x".repeat(MAX_LINE_BYTES)
        );
        let r: Value = serde_json::from_str(&s.handle_line(&long).unwrap()).unwrap();
        assert_eq!(r["error"]["code"], INVALID_REQUEST);
        assert_eq!(
            reply(&mut s, json!({"jsonrpc":"2.0","id":6,"method":"ping"}))["result"],
            json!({})
        );
    }
}
