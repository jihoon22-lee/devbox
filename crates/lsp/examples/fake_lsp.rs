//! Owned stdio fixture. Never detects, installs or invokes a real language server.
use serde_json::{Value, json};
use std::io::{self, BufRead, Read, Write};
fn send(value: Value) {
    let body = value.to_string();
    let mut out = io::stdout().lock();
    write!(out, "Content-Length: {}\r\n\r\n{}", body.len(), body).expect("fixture stdout");
    out.flush().expect("flush");
}
fn main() {
    let mut input = io::BufReader::new(io::stdin().lock());
    let mut initialized = 0;
    let mut open = 0;
    loop {
        let mut length = None;
        loop {
            let mut line = String::new();
            if input.read_line(&mut line).expect("header") == 0 {
                return;
            }
            if line == "\r\n" {
                break;
            }
            if let Some(n) = line.strip_prefix("Content-Length:") {
                length = Some(n.trim().parse::<usize>().expect("length"));
            }
        }
        let mut body = vec![0; length.expect("content length")];
        input.read_exact(&mut body).expect("body");
        let v: Value = serde_json::from_slice(&body).expect("json");
        let id = v.get("id").cloned();
        let method = v["method"].as_str().unwrap_or("");
        let result = match method {
            "initialize" => {
                initialized += 1;
                json!({"capabilities":{"positionEncoding":"utf-16","textDocumentSync":1,"hoverProvider":true},"initializeCount":initialized,"pid":std::process::id()})
            }
            "textDocument/didOpen" => {
                open += 1;
                continue;
            }
            "textDocument/didClose" => {
                open -= 1;
                continue;
            }
            "textDocument/didChange" => {
                send(
                    json!({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":v["params"]["textDocument"]["uri"],"version":v["params"]["textDocument"]["version"],"diagnostics":[{"range":{"start":{"line":0,"character":3},"end":{"line":0,"character":4}},"severity":1,"message":"fixture error"}]}}),
                );
                continue;
            }
            "test/state" => json!({"opens":open,"pid":std::process::id()}),
            "test/large" => json!({"data":"x".repeat(300*1024)}),
            "test/crash" => std::process::exit(2),
            "test/malformed" => {
                print!("Content-Length: -1\r\n\r\n");
                io::stdout().flush().expect("flush");
                continue;
            }
            _ => v.get("params").cloned().unwrap_or(Value::Null),
        };
        if let Some(id) = id {
            send(json!({"jsonrpc":"2.0","id":id,"result":result}));
        }
    }
}
