//! MCP 클라이언트(`api.mcp_connect`): stdio(자식 프로세스) 또는 streamable HTTP(+OAuth 2.1)로 연결해
//! `initialize` 뒤 화면 입력(NDJSON `McpInput`)을 JSON-RPC 요청으로 보내고 결과를 이벤트로 돌려준다.
//! 저장소 파일이 정의한 stdio 명령은 데몬이 그 파일의 digest·검토·명령 일치를 확인한 뒤에만 실행한다(07b 34행).
mod http;
mod oauth;
mod profile;
mod session;
mod stdio;

use crate::service::ApiService;
use crate::{ApiError, Auth, Body, CollectionRoot, CollectionsService, EnvSnapshot, HeaderRow};
use crate::{Pair, R, RequestSnapshot, TlsOptions};
use devbox_protocol::message::WireError;
use devbox_protocol::{Ctx, StreamOpened};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// JSON-RPC 메시지 하나(줄·HTTP 본문·SSE data)의 상한.
pub(crate) const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
/// 답을 기다리는 화면 요청 수.
pub(crate) const MAX_PENDING: usize = 64;
/// `initialize`와 요청 하나의 응답 시한.
pub(crate) const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
pub(crate) const PROTOCOL_VERSION: &str = "2025-06-18";
/// 서버가 고른 버전이 이 목록에 없으면 연결하지 않는다(MCP 수명 주기: 버전 협상 실패면 끊음).
/// v0.9.0은 2026-07-28(+2025-11-25)을 썼다. v1은 S1 devbox MCP 서버와 같은 2025-06-18만 구현한다.
pub(crate) const SUPPORTED_VERSIONS: [&str; 1] = [PROTOCOL_VERSION];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum McpTransport {
    /// 셸 없이 `command`를 `args`로 실행한다(치환하지 않음). `env` 값만 변수·비밀 참조를 푼다.
    Stdio {
        command: String,
        args: Vec<String>,
        cwd: Option<String>,
        env: Vec<Pair>,
    },
    Http {
        url: String,
        headers: Vec<HeaderRow>,
        tls: TlsOptions,
    },
}

/// 연결 정의의 출처. `File`이면 데몬이 그 파일을 읽어 digest·검토·정의 일치를 확인한다.
/// `Draft`(저장하지 않은 입력)는 화면이 사용자 확인을 거친다(데몬이 확인할 파일이 없다).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum McpSource {
    Draft,
    File {
        root: CollectionRoot,
        path: String,
        digest: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpConnect {
    pub transport: McpTransport,
    pub env: EnvSnapshot,
    pub source: McpSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum McpList {
    Tools,
    Resources,
    Prompts,
}

/// 화면 → 데몬 입력(NDJSON 한 줄씩). `id`는 화면이 정하고 결과 이벤트가 같은 `id`를 단다.
/// `argumentsJson`은 JSON 객체 텍스트이며 치환하지 않고 그대로 보낸다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum McpInput {
    List {
        id: String,
        what: McpList,
        cursor: Option<String>,
    },
    Call {
        id: String,
        name: String,
        arguments_json: String,
    },
    Read {
        id: String,
        uri: String,
    },
    GetPrompt {
        id: String,
        name: String,
        arguments_json: String,
    },
}

/// `api.mcp_connect` 스트림의 NDJSON 한 줄. JSON 텍스트는 이 연결에서 푼 비밀·토큰을 가린 것이다.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum McpEvent {
    /// OAuth 2.1: 이 URL을 브라우저로 연다(콜백은 127.0.0.1, 인증 중에만).
    Authorize {
        url: String,
    },
    Ready {
        server_name: String,
        server_version: String,
        protocol_version: String,
        capabilities_json: String,
    },
    Result {
        id: String,
        json: String,
    },
    /// 서버가 돌려준 JSON-RPC 오류.
    Error {
        id: String,
        code: i64,
        message: String,
    },
    Failed {
        error: WireError,
    },
    Closed,
}

/// stdio env·HTTP 헤더·TLS 경로를 같은 치환·비밀 규칙(`resolve`)에 넣는 모양.
fn snapshot(transport: &McpTransport) -> RequestSnapshot {
    let (url, headers, tls) = match transport {
        McpTransport::Stdio { env, .. } => (
            "http://stdio.invalid/".to_owned(),
            env.iter()
                .enumerate()
                .map(|(i, p)| HeaderRow {
                    row: i as u32,
                    name: p.name.clone(),
                    value: p.value.clone(),
                })
                .collect(),
            TlsOptions::default(),
        ),
        McpTransport::Http { url, headers, tls } => (url.clone(), headers.clone(), tls.clone()),
    };
    RequestSnapshot {
        method: "POST".into(),
        url,
        query: vec![],
        headers,
        cookies: vec![],
        body: Body::None,
        auth: Auth::None,
        tls,
        timeout_ms: REQUEST_TIMEOUT.as_millis() as i64,
        follow_redirects: false,
    }
}

impl ApiService {
    /// 검사 순서(모두 스트림을 열기 전): 출처 파일 확인(digest·검토·정의 일치) → 모양 검사 →
    /// 치환·비밀 해석 → (stdio) cwd가 등록 프로젝트 안인지 → 자리 확보. 실행·연결은 스트림을 연 뒤다.
    pub async fn mcp_connect(
        &self,
        ctx: Ctx,
        p: McpConnect,
        collections: &CollectionsService,
    ) -> R<StreamOpened> {
        if let McpSource::File { root, path, digest } = &p.source {
            let bytes = collections.verified(root, path, digest).await?;
            if !profile::matches(&bytes, &p.transport) {
                return Err(ApiError::input("source", "mismatch").into());
            }
        }
        if let McpTransport::Stdio {
            command, args, env, ..
        } = &p.transport
        {
            stdio::check(command, args, env)?;
        }
        let (filled, values) = self.resolve(&snapshot(&p.transport), &p.env).await?;
        let transport = match &p.transport {
            McpTransport::Stdio {
                command, args, cwd, ..
            } => {
                let projects = self
                    .projects
                    .list(devbox_projects::ListProjects {})
                    .await
                    .map_err(|_| devbox_protocol::Fail::Internal("list projects".into()))?;
                // 프로젝트 파일의 빈 cwd는 그 프로젝트 루트다. 초안·개인 파일은 cwd를 적어야 한다.
                let default = match &p.source {
                    McpSource::File {
                        root: CollectionRoot::Project { project_id },
                        ..
                    } => projects
                        .items
                        .iter()
                        .find(|item| &item.id == project_id)
                        .map(|item| item.path.clone()),
                    _ => None,
                };
                let roots: Vec<String> = projects.items.into_iter().map(|p| p.path).collect();
                let cwd = stdio::cwd(cwd.as_deref(), default.as_deref(), &roots)?;
                let env: Vec<(String, String)> = filled
                    .headers
                    .iter()
                    .map(|h| (h.name.clone(), h.value.clone()))
                    .collect();
                stdio::check_values(&env)?;
                session::Prepared::Stdio(stdio::Plan {
                    program: command.clone(),
                    args: args.clone(),
                    cwd,
                    env: stdio::child_env(|k| std::env::var(k).ok(), &env),
                })
            }
            McpTransport::Http { .. } => {
                session::Prepared::Http(http::prepare(&filled, self.inner.resolver.as_ref())?)
            }
        };
        let permit = self.session_slot()?;
        let ends = ctx.open_stream().map_err(|_| ApiError::StreamUnsupported)?;
        let stream_id = ends.id;
        tokio::spawn(session::run(session::Session {
            transport,
            tx: ends.tx,
            input: ends.input,
            values,
            _permit: permit,
        }));
        Ok(StreamOpened { stream_id })
    }
}
