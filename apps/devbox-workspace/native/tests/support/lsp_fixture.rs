//! Synthetic native LSP server, built only by the test-fixtures feature.
//! It is never included in the packaged helper resources.
#[cfg(target_os = "linux")]
fn main() {
    use serde_json::{json, Value};
    use std::{
        collections::BTreeMap,
        fs,
        io::{BufRead, Read, Write},
        os::unix::process::CommandExt,
        path::PathBuf,
        process::{Command, Stdio},
        time::Duration,
    };
    fn output(value: Value) {
        let bytes = serde_json::to_vec(&value).unwrap();
        let mut out = std::io::stdout().lock();
        write!(out, "Content-Length: {}\r\n\r\n", bytes.len()).unwrap();
        out.write_all(&bytes).unwrap();
        out.flush().unwrap();
    }
    fn diagnostic(uri: &str, version: i64) -> Value {
        json!({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":uri,"version":version,"diagnostics":[{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},"severity":2,"message":"synthetic fixture diagnostic"}]}})
    }
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--version") {
        println!("v22.0.0");
        return;
    }
    let state = PathBuf::from(
        args.windows(2)
            .find(|pair| pair[0] == "--state")
            .expect("owned fixture state")[1]
            .clone(),
    );
    if args.iter().any(|arg| arg == "--detached") {
        fs::write(state.join("detached.pid"), std::process::id().to_string()).unwrap();
        loop {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    fs::write(state.join("server.pid"), std::process::id().to_string()).unwrap();
    let mut starts = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(state.join("starts"))
        .unwrap();
    writeln!(starts, "{}", std::process::id()).unwrap();
    drop(starts);
    if args.iter().any(|arg| arg == "--detach") {
        let mut child = Command::new(std::env::current_exe().unwrap());
        child
            .args(["--detached", "--state"])
            .arg(&state)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        unsafe {
            child.pre_exec(|| {
                unsafe extern "C" {
                    fn setsid() -> i32;
                }
                if setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        // This fixture deliberately abandons a setsid child. The production
        // native subreaper must adopt and reap it when startup is cancelled.
        #[allow(clippy::zombie_processes)]
        let _child = child.spawn().unwrap();
    }
    let mut input = std::io::BufReader::new(std::io::stdin().lock());
    let mut documents = BTreeMap::<String, (i64, String)>::new();
    loop {
        let mut length = None;
        let mut line = String::new();
        loop {
            line.clear();
            if input.read_line(&mut line).unwrap() == 0 {
                return;
            }
            if line == "\r\n" {
                break;
            }
            if let Some(value) = line.strip_prefix("Content-Length:") {
                length = Some(value.trim().parse::<usize>().unwrap());
            }
        }
        let length = length.expect("frame length");
        assert!(length <= 32 * 1024 * 1024);
        let mut bytes = vec![0; length];
        input.read_exact(&mut bytes).unwrap();
        let message: Value = serde_json::from_slice(&bytes).unwrap();
        let method = message["method"].as_str().unwrap_or("");
        let params = &message["params"];
        let id = message.get("id");
        let result = match method {
            "initialize" => {
                fs::write(state.join("initializing"), b"ready").unwrap();
                if args.iter().any(|arg| arg == "--slow") {
                    loop {
                        std::thread::sleep(Duration::from_secs(1));
                    }
                }
                json!({"capabilities":{"positionEncoding":"utf-16","textDocumentSync":{"openClose":true,"change":1,"save":true},"completionProvider":{},"hoverProvider":true,"definitionProvider":true,"referencesProvider":true,"renameProvider":true,"documentFormattingProvider":true,"diagnosticProvider":{"interFileDependencies":false,"workspaceDiagnostics":false}}})
            }
            "textDocument/didOpen" => {
                let doc = &params["textDocument"];
                let uri = doc["uri"].as_str().unwrap();
                let version = doc["version"].as_i64().unwrap();
                let text = doc["text"].as_str().unwrap();
                documents.insert(uri.into(), (version, text.into()));
                fs::write(state.join("opened"), text).unwrap();
                output(diagnostic(uri, version));
                if args.iter().any(|arg| arg == "--crash-once")
                    && fs::OpenOptions::new()
                        .create_new(true)
                        .write(true)
                        .open(state.join("crashed-once"))
                        .is_ok()
                {
                    std::process::exit(7);
                }
                Value::Null
            }
            "textDocument/didChange" => {
                let doc = &params["textDocument"];
                let uri = doc["uri"].as_str().unwrap();
                let version = doc["version"].as_i64().unwrap();
                let text = params["contentChanges"][0]["text"].as_str().unwrap();
                documents.insert(uri.into(), (version, text.into()));
                fs::write(state.join("changed"), text).unwrap();
                output(diagnostic(uri, version));
                Value::Null
            }
            "textDocument/didClose" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap();
                if let Some((version, _)) = documents.remove(uri) {
                    output(diagnostic(uri, version));
                }
                fs::write(state.join("closed"), b"closed").unwrap();
                Value::Null
            }
            "textDocument/completion" => {
                json!({"isIncomplete":false,"items":[{"label":"fixtureComplete","kind":6,"insertText":"fixtureComplete"}]})
            }
            "textDocument/hover" => {
                json!({"contents":{"kind":"markdown","value":"fixture hover **safe**"}})
            }
            "textDocument/definition" | "textDocument/references" => {
                let uri = if args.iter().any(|arg| arg == "--foreign-uri") {
                    "file:///outside-fixture/denied.rs"
                } else {
                    params["textDocument"]["uri"].as_str().unwrap()
                };
                json!([{"uri":uri,"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}}}])
            }
            "textDocument/formatting" => {
                json!([{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},"newText":"// formatted\n"}])
            }
            "textDocument/diagnostic" => {
                json!({"kind":"full","items":[{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},"severity":2,"message":"synthetic pull diagnostic"}]})
            }
            "textDocument/rename" => {
                fs::write(state.join("rename-requested"), b"unexpected").unwrap();
                json!({"changes":{}})
            }
            "exit" => return,
            _ => Value::Null,
        };
        if let Some(id) = id {
            output(json!({"jsonrpc":"2.0","id":id,"result":result}));
        }
    }
}
#[cfg(not(target_os = "linux"))]
fn main() {
    panic!("the synthetic LSP fixture requires Linux");
}
