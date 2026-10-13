//! Minimal newline-framed MCP stdio server, with write access disabled by default.
mod input;
mod protocol;
mod tools;
use crate::client::Client;
use devbox_core::{Config, Paths};
use input::{Input, Line};
use protocol::*;
use tokio::io::AsyncWriteExt;

pub async fn run() -> anyhow::Result<()> {
    let paths = crate::paths()?;
    let config = Config::load(&paths)?;
    let mut server = Server {
        paths,
        write: config.mcp.write,
        client: None,
    };
    let mut input = Input::stdin()?;
    let mut stdout = tokio::io::stdout();
    while let Some(line) = input.next().await? {
        let reply = match line {
            Line::Oversized => Some(Reply::error(None, ErrorKind::Parse.into())),
            Line::Message(line) => server.handle(&line).await,
        };
        if let Some(reply) = reply {
            let mut bytes = serde_json::to_vec(&reply)?;
            bytes.push(b'\n');
            stdout.write_all(&bytes).await?;
            stdout.flush().await?;
        }
    }
    Ok(())
}
struct Server {
    paths: Paths,
    write: bool,
    client: Option<Client>,
}
impl Server {
    async fn handle(&mut self, line: &[u8]) -> Option<Reply> {
        // Parse syntax separately so malformed JSON is distinct from an invalid envelope.
        let raw = match serde_json::from_slice::<Box<serde_json::value::RawValue>>(line) {
            Ok(raw) => raw,
            Err(_) => return Some(Reply::error(None, ErrorKind::Parse.into())),
        };
        let request = if raw.get().trim_start().starts_with('{') {
            serde_json::from_str::<Request>(raw.get()).ok()
        } else {
            None
        };
        let Some(request) = request.filter(|r| r.jsonrpc == "2.0") else {
            return Some(Reply::error(None, ErrorKind::InvalidRequest.into()));
        };
        // MCP tool calls are requests, never notifications. Ignore ID-less
        // messages before dispatch so they cannot trigger a mutation.
        let RequestId::Present(id) = request.id else {
            return None;
        };
        let result = self
            .dispatch(&request.method, request.params.as_deref())
            .await;
        Some(Reply::result(id, result))
    }
    async fn dispatch(
        &mut self,
        method: &str,
        raw: Option<&serde_json::value::RawValue>,
    ) -> Result<Box<serde_json::value::RawValue>, RpcError> {
        match method {
            "initialize" => {
                let Initialize {
                    protocol_version,
                    capabilities: _capabilities,
                    client_info,
                } = params(raw)?;
                let ClientInfo {
                    name: _name,
                    version: _version,
                } = client_info;
                encode(Initialized {
                    protocol_version,
                    capabilities: Capabilities { tools: Empty {} },
                    server_info: ServerInfo {
                        name: "devbox",
                        version: crate::VERSION,
                    },
                })
            }
            "tools/list" => {
                if raw.is_some() {
                    let _: ListParams = params(raw)?;
                }
                encode(ToolList {
                    tools: tools::list(self.write),
                })
            }
            "tools/call" => {
                let call: CallParams = params(raw)?;
                let result = self.call_tool(call).await?;
                encode(result)
            }
            _ => Err(ErrorKind::MethodNotFound.into()),
        }
    }
}
