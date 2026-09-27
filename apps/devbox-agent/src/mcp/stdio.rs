use mcp_server::{protocol::MAX_LINE_BYTES, Server, ToolHost};
use std::io::{self, BufRead, Write};
fn write_line(output: &mut impl Write, line: &str) -> io::Result<()> {
    writeln!(output, "{line}")?;
    output.flush()
}
fn protocol_error(output: &mut impl Write, code: i64, message: &str) -> io::Result<()> {
    write_line(
        output,
        &serde_json::json!({"jsonrpc":"2.0","id":null,"error":{"code":code,"message":message}})
            .to_string(),
    )
}
pub fn run(host: impl ToolHost, mut input: impl BufRead, mut output: impl Write) -> io::Result<()> {
    let mut server = Server::new(host, env!("CARGO_PKG_VERSION"));
    loop {
        let mut line = Vec::new();
        let mut oversized = false;
        let mut eof = false;
        loop {
            let buffer = input.fill_buf()?;
            if buffer.is_empty() {
                eof = true;
                break;
            }
            let newline = buffer.iter().position(|b| *b == b'\n');
            let count = newline.map_or(buffer.len(), |n| n + 1);
            if !oversized {
                if line.len().saturating_add(count) > MAX_LINE_BYTES + 1 {
                    oversized = true;
                    line.clear();
                } else {
                    line.extend_from_slice(&buffer[..count]);
                }
            }
            input.consume(count);
            if newline.is_some() {
                break;
            }
        }
        if line.last() == Some(&b'\n') {
            line.pop();
        }
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        if oversized || line.len() > MAX_LINE_BYTES {
            protocol_error(&mut output, -32600, "Request exceeds 4 MiB")?;
        } else {
            match std::str::from_utf8(&line) {
                Err(_) => protocol_error(&mut output, -32700, "Invalid UTF-8 JSON")?,
                Ok(text) if !text.trim().is_empty() => {
                    if let Some(reply) = server.handle_line(text) {
                        write_line(&mut output, &reply)?;
                    }
                }
                _ => {}
            }
        }
        if eof {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_server::{HostError, McpSettings, ToolCall, ToolHost};
    struct Host;
    impl ToolHost for Host {
        fn settings(&mut self) -> Result<McpSettings, HostError> {
            Ok(McpSettings {
                enabled: true,
                ..Default::default()
            })
        }
        fn call(&mut self, _: &ToolCall) -> Result<serde_json::Value, HostError> {
            Ok(serde_json::json!({}))
        }
    }
    #[test]
    fn stdio_flushes_only_json_responses_and_ignores_notifications() {
        let input = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n";
        let mut output = Vec::new();
        run(Host, std::io::Cursor::new(input), &mut output).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert_eq!(text.lines().count(), 2);
        for line in text.lines() {
            assert!(serde_json::from_str::<serde_json::Value>(line).is_ok());
        }
    }
    #[test]
    fn oversized_and_invalid_utf8_lines_are_drained_before_the_next_request() {
        let mut input = vec![b'x'; mcp_server::protocol::MAX_LINE_BYTES + 1];
        input.extend_from_slice(b"\n\xff\n{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"ping\"}\n");
        let mut output = Vec::new();
        run(Host, std::io::Cursor::new(input), &mut output).unwrap();
        let values: Vec<serde_json::Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(values.len(), 3);
        assert_eq!(values[0]["error"]["code"], -32600);
        assert_eq!(values[1]["error"]["code"], -32700);
        assert_eq!(values[2]["result"], serde_json::json!({}));
    }
}
