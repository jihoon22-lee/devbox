//! MCP fixture: 소유 임시 폴더의 python3 stdio 서버 스크립트와, 127.0.0.1:0 hyper streamable HTTP MCP +
//! OAuth 2.1 인가 서버(메타데이터·동적 등록·authorize·token, PKCE S256 확인). 외부 네트워크를 쓰지 않는다.
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use sha2::{Digest, Sha256};
use std::convert::Infallible;
use std::sync::{Arc, Mutex};

pub const SCRIPT: &str = r#"
import json, os, subprocess, sys
marker = os.environ.get("FIXTURE_MARKER")
if marker:
    with open(marker, "w") as f:
        f.write(str(os.getpid()))
grand = os.environ.get("FIXTURE_GRANDCHILD")
if grand:
    p = subprocess.Popen(["sleep", "60"])
    with open(grand, "w") as f:
        f.write(str(p.pid))
def out(m):
    sys.stdout.write(json.dumps(m) + "\n")
    sys.stdout.flush()
while True:
    line = sys.stdin.readline()
    if not line:
        break
    line = line.strip()
    if not line:
        continue
    msg = json.loads(line)
    mid = msg.get("id")
    method = msg.get("method")
    if mid is None:
        continue
    params = msg.get("params") or {}
    if method == "initialize":
        res = {"protocolVersion": os.environ.get("FIXTURE_PROTOCOL", "2025-06-18"), "capabilities": {"tools": {}},
               "serverInfo": {"name": os.environ.get("FIXTURE_NAME", "fixture"), "version": "1.0"}}
    elif method == "tools/list":
        if params.get("cursor") == "p2":
            res = {"tools": [{"name": "env", "inputSchema": {"type": "object"}}]}
        else:
            res = {"tools": [{"name": "echo", "inputSchema": {"type": "object"}}], "nextCursor": "p2"}
    elif method == "tools/call":
        name = params.get("name")
        if name == "echo":
            res = {"content": [{"type": "text", "text": json.dumps(params.get("arguments"))}]}
        elif name == "env":
            res = {"env": dict(os.environ), "cwd": os.getcwd()}
        elif name == "fail-secret":
            out({"jsonrpc": "2.0", "id": mid, "error": {"code": -32000, "message": "bad " + os.environ.get("TOKEN", "")}})
            continue
        elif name == "big":
            sys.stdout.write('{"jsonrpc":"2.0","id":%s,"result":{"x":"%s"}}\n' % (json.dumps(mid), "x" * (4 * 1024 * 1024 + 16)))
            sys.stdout.flush()
            continue
        elif name == "ping-me":
            out({"jsonrpc": "2.0", "id": "s1", "method": "ping"})
            reply = json.loads(sys.stdin.readline())
            res = {"pong": reply.get("id") == "s1" and reply.get("result") == {}}
        else:
            out({"jsonrpc": "2.0", "id": mid, "error": {"code": -32602, "message": "unknown tool"}})
            continue
    else:
        res = {}
    out({"jsonrpc": "2.0", "id": mid, "result": res})
"#;

/// 스크립트를 소유 폴더에 쓰고 경로를 돌려준다(`python3 -I <경로>`로 실행).
pub fn script(dir: &std::path::Path) -> String {
    let path = dir.join("mcp_fixture.py");
    std::fs::write(&path, SCRIPT).expect("write fixture script");
    path.display().to_string()
}

/// pid가 끝났는지(없거나 좀비).
pub fn gone(pid: u32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Err(_) => true,
        Ok(stat) => stat
            .rsplit_once(')')
            .is_some_and(|(_, rest)| rest.trim_start().starts_with('Z')),
    }
}

#[derive(Clone, Copy)]
pub struct Options {
    pub oauth: bool,
    pub s256: bool,
    pub token_bytes: usize,
    pub sse: bool,
    /// 인가 서버 메타데이터의 `issuer`를 다른 값으로(RFC 8414 §3.3 위반).
    pub bad_issuer: bool,
    /// 보호 자원 메타데이터의 `resource`를 다른 값으로(RFC 9728 §3.3 위반).
    pub bad_resource: bool,
    /// 소유 CA로 서명한 인증서의 https로 듣는다.
    pub tls: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            oauth: false,
            s256: true,
            token_bytes: 48,
            sse: false,
            bad_issuer: false,
            bad_resource: false,
            tls: false,
        }
    }
}

/// 등록 프로젝트 폴더 하나를 만들고 경로를 돌려준다(stdio 초안의 cwd).
pub async fn workdir(h: &super::api::Harness) -> String {
    let dir = h.dir.path().join("work");
    std::fs::create_dir_all(&dir).expect("work dir");
    h.projects
        .add(devbox_projects::AddProject {
            path: dir.display().to_string(),
        })
        .await
        .expect("register work project");
    std::fs::canonicalize(&dir)
        .expect("real work dir")
        .display()
        .to_string()
}

/// (메서드, 경로, authorization, mcp-session-id, mcp-protocol-version, 본문)
pub type SeenRequest = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
);

#[derive(Default)]
pub struct Seen {
    pub requests: Vec<SeenRequest>,
    pub authorize_query: Vec<(String, String)>,
    pub token_form: Vec<(String, String)>,
    pub registered: Option<String>,
}

pub struct McpHttp {
    pub port: u16,
    pub seen: Arc<Mutex<Seen>>,
    pub token: String,
    tls: bool,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for McpHttp {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn reply(status: u16, kind: &str, body: String) -> hyper::Response<Full<Bytes>> {
    let mut r = hyper::Response::new(Full::new(Bytes::from(body)));
    *r.status_mut() = hyper::StatusCode::from_u16(status).expect("status");
    if !kind.is_empty() {
        r.headers_mut()
            .insert(hyper::header::CONTENT_TYPE, kind.parse().expect("type"));
    }
    r
}

fn header(req: &hyper::Request<hyper::body::Incoming>, name: &str) -> Option<String> {
    req.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

struct Ctx {
    base: String,
    options: Options,
    token: String,
    seen: Arc<Mutex<Seen>>,
}

async fn handle(
    ctx: Arc<Ctx>,
    req: hyper::Request<hyper::body::Incoming>,
) -> hyper::Response<Full<Bytes>> {
    let method = req.method().to_string();
    let path = req.uri().path().to_owned();
    let query = req.uri().query().unwrap_or("").to_owned();
    let auth = header(&req, "authorization");
    let session = header(&req, "mcp-session-id");
    let protocol = header(&req, "mcp-protocol-version");
    let body = req
        .into_body()
        .collect()
        .await
        .map(|b| b.to_bytes())
        .unwrap_or_default();
    let body = String::from_utf8_lossy(&body).into_owned();
    ctx.seen.lock().expect("seen").requests.push((
        method.clone(),
        path.clone(),
        auth.clone(),
        session,
        protocol,
        body.clone(),
    ));
    let pairs = |text: &str| -> Vec<(String, String)> {
        url::form_urlencoded::parse(text.as_bytes())
            .into_owned()
            .collect()
    };
    let get = |pairs: &[(String, String)], k: &str| {
        pairs
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    let base = &ctx.base;
    match (method.as_str(), path.as_str()) {
        ("GET", "/.well-known/oauth-protected-resource") => reply(
            200,
            "application/json",
            serde_json::json!({
                "resource": if ctx.options.bad_resource { format!("{base}/other") } else { format!("{base}/mcp") },
                "authorization_servers": [base],
            })
            .to_string(),
        ),
        ("GET", "/.well-known/oauth-authorization-server") => reply(
            200,
            "application/json",
            serde_json::json!({
                "issuer": if ctx.options.bad_issuer { format!("{base}/evil") } else { base.clone() },
                "authorization_endpoint": format!("{base}/authorize"),
                "token_endpoint": format!("{base}/token"),
                "registration_endpoint": format!("{base}/register"),
                "code_challenge_methods_supported": if ctx.options.s256 { vec!["S256"] } else { vec!["plain"] },
            })
            .to_string(),
        ),
        ("POST", "/register") => {
            ctx.seen.lock().expect("seen").registered = Some(body);
            reply(201, "application/json", r#"{"client_id":"client-1"}"#.into())
        }
        ("GET", "/authorize") => {
            let q = pairs(&query);
            let location = format!(
                "{}?code=code-1&state={}",
                get(&q, "redirect_uri"),
                get(&q, "state")
            );
            ctx.seen.lock().expect("seen").authorize_query = q;
            let mut r = reply(302, "", String::new());
            r.headers_mut().insert(hyper::header::LOCATION, location.parse().expect("location"));
            r
        }
        ("POST", "/token") => {
            let form = pairs(&body);
            let challenge = get(&ctx.seen.lock().expect("seen").authorize_query, "code_challenge");
            let verifier = get(&form, "code_verifier");
            let computed = data_encoding::BASE64URL_NOPAD.encode(&Sha256::digest(verifier.as_bytes()));
            let ok = get(&form, "grant_type") == "authorization_code"
                && get(&form, "code") == "code-1"
                && get(&form, "client_id") == "client-1"
                && !get(&form, "resource").is_empty()
                && computed == challenge;
            ctx.seen.lock().expect("seen").token_form = form;
            if ok {
                reply(
                    200,
                    "application/json",
                    serde_json::json!({"access_token": ctx.token, "token_type": "Bearer", "expires_in": 3600}).to_string(),
                )
            } else {
                reply(400, "application/json", r#"{"error":"invalid_grant"}"#.into())
            }
        }
        ("DELETE", "/mcp") => reply(200, "", String::new()),
        ("POST", "/mcp") => {
            if ctx.options.oauth && auth.as_deref() != Some(&format!("Bearer {}", ctx.token)) {
                let mut r = reply(401, "", String::new());
                let value = format!(
                    "Bearer resource_metadata=\"{base}/.well-known/oauth-protected-resource\", scope=\"mcp:tools\""
                );
                r.headers_mut().insert(hyper::header::WWW_AUTHENTICATE, value.parse().expect("challenge"));
                return r;
            }
            let msg: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
            let Some(id) = msg.get("id").cloned() else {
                return reply(202, "", String::new());
            };
            let result = match msg["method"].as_str() {
                Some("initialize") => serde_json::json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "http-fixture", "version": "2.0"},
                }),
                Some("tools/list") => serde_json::json!({"tools": [{"name": "remote"}]}),
                // `big`: 인자 `bytes`만큼의 문자열을 돌려준다(SSE 크기 시험).
                Some("tools/call") if msg["params"]["name"] == "big" => {
                    let n = msg["params"]["arguments"]["bytes"].as_u64().unwrap_or(0) as usize;
                    serde_json::json!({"x": "x".repeat(n)})
                }
                // 받은 Authorization을 되돌려 준다(토큰이 이벤트에서 가려지는지 본다).
                _ => serde_json::json!({"echo": msg["params"], "authorization": auth.clone()}),
            };
            let text = serde_json::json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string();
            let mut r = if ctx.options.sse && msg["method"] != "initialize" {
                reply(200, "text/event-stream", format!("event: message\ndata: {text}\n\n"))
            } else {
                reply(200, "application/json", text)
            };
            if msg["method"] == "initialize" {
                r.headers_mut().insert("mcp-session-id", "sess-1".parse().expect("sid"));
            }
            r
        }
        _ => reply(404, "", String::new()),
    }
}

impl McpHttp {
    pub async fn start(options: Options) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind owned fixture");
        let port = listener.local_addr().expect("addr").port();
        let seen = Arc::new(Mutex::new(Seen::default()));
        let token = format!("tok-{}", "t".repeat(options.token_bytes.saturating_sub(4)));
        let scheme = if options.tls { "https" } else { "http" };
        let acceptor = options
            .tls
            .then(|| super::tls::acceptor(super::tls::Pki::new().server(false)));
        let ctx = Arc::new(Ctx {
            base: format!("{scheme}://127.0.0.1:{port}"),
            options,
            token: token.clone(),
            seen: seen.clone(),
        });
        let task = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let (ctx, acceptor) = (ctx.clone(), acceptor.clone());
                tokio::spawn(async move {
                    let service = hyper::service::service_fn(move |req| {
                        let ctx = ctx.clone();
                        async move { Ok::<_, Infallible>(handle(ctx, req).await) }
                    });
                    let http = hyper::server::conn::http1::Builder::new();
                    let _ = match acceptor {
                        Some(acceptor) => match acceptor.accept(stream).await {
                            Ok(tls) => {
                                http.serve_connection(hyper_util::rt::TokioIo::new(tls), service)
                                    .await
                            }
                            Err(_) => return,
                        },
                        None => {
                            http.serve_connection(hyper_util::rt::TokioIo::new(stream), service)
                                .await
                        }
                    };
                });
            }
        });
        Self {
            port,
            seen,
            token,
            tls: options.tls,
            task,
        }
    }
    pub fn url(&self) -> String {
        let scheme = if self.tls { "https" } else { "http" };
        format!("{scheme}://127.0.0.1:{}/mcp", self.port)
    }
}
