#![allow(dead_code)]
use super::support::{self, ChildGuard, TestDaemon};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{ChildStdin, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

pub struct Mcp {
    child: ChildGuard,
    pub input: Option<ChildStdin>,
    output: Receiver<String>,
}
impl Mcp {
    pub fn start(d: &TestDaemon) -> Self {
        let mut command = d.environment(env!("CARGO_BIN_EXE_devbox"));
        command
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        let mut child = ChildGuard(command.spawn().expect("owned MCP process"));
        let input = child.0.stdin.take();
        let stdout = child.0.stdout.take().expect("MCP stdout");
        let (tx, output) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if tx.send(line.expect("protocol line")).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            input,
            output,
        }
    }
    pub fn send(&mut self, message: Value) {
        self.raw(&message.to_string());
    }
    pub fn raw(&mut self, message: &str) {
        writeln!(self.input.as_mut().expect("open stdin"), "{message}").expect("write request");
    }
    pub fn read(&self) -> Value {
        let line = self
            .output
            .recv_timeout(Duration::from_secs(8))
            .expect("bounded MCP response");
        serde_json::from_str(&line).expect("stdout is JSON-RPC only")
    }
    pub fn call(&mut self, message: Value) -> Value {
        self.send(message);
        self.read()
    }
    pub fn tool(&mut self, name: &str, arguments: Value) -> Value {
        self.call(json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":name,"arguments":arguments,"_meta":{"owned":"test"}}}))
    }
    pub fn finish(mut self) {
        self.input.take();
        assert!(support::exits(self.child).success());
        assert!(
            self.output.recv_timeout(Duration::from_secs(1)).is_err(),
            "no stray response at EOF"
        );
    }
}
pub fn body(reply: &Value) -> Value {
    assert_ne!(reply["result"]["isError"], true, "{reply}");
    assert_eq!(reply["result"]["content"][0]["type"], "text");
    serde_json::from_str(
        reply["result"]["content"][0]["text"]
            .as_str()
            .expect("text"),
    )
    .expect("JSON text")
}
pub fn names(reply: &Value) -> Vec<String> {
    reply["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|tool| {
            assert_eq!(tool["inputSchema"]["type"], "object");
            assert_eq!(tool["inputSchema"]["additionalProperties"], false);
            tool["name"].as_str().expect("name").to_owned()
        })
        .collect()
}
