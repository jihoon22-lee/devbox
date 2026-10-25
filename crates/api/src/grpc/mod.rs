//! gRPC(`api.grpc_describe`·`api.grpc_call`): reflection(v1, 없으면 v1alpha) 또는 protox로 서술자를 얻고
//! `prost-reflect` 동적 메시지로 unary·server streaming을 부른다. 입력 검사(변수·비밀·메타데이터·TLS)는
//! 연결 전에 끝내고, 메시지·메타데이터·상태 문장은 이 호출에서 푼 비밀을 가린다. 호출은 기록하지 않는다.
mod call;
mod codec;
mod connect;
mod describe;
mod metadata;

use crate::service::ApiService;
use crate::{
    ApiError, Auth, Body, EnvSnapshot, HeaderRow, Pair, R, RequestOrigin, RequestSnapshot,
};
use crate::{TlsOptions, validate};
use devbox_protocol::message::WireError;
use devbox_protocol::{Ctx, StreamOpened};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 메시지 하나의 상한(보내기·받기, tonic 기본 4MiB와 같음).
pub(crate) const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
/// server streaming 호출 하나에서 받을 메시지 수.
pub(crate) const MAX_STREAM_MESSAGES: usize = 10_000;
/// 연결·서술자 조회 시한.
pub(crate) const DESCRIBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GrpcTarget {
    /// `http://host:port`(평문 h2c) 또는 `https://host:port`(요청 TLS 설정). 변수·비밀 참조 가능.
    pub target: String,
    pub metadata: Vec<HeaderRow>,
    pub tls: TlsOptions,
    pub env: EnvSnapshot,
    /// 있으면 reflection 대신 이 `.proto` 파일들(WSL 절대 경로)을 protox로 해석한다.
    pub proto_files: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum CatalogSource {
    Reflection,
    ProtoFiles,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GrpcMethod {
    pub name: String,
    /// `package.Service/Method`.
    pub full_name: String,
    pub client_streaming: bool,
    pub server_streaming: bool,
    /// 입력 메시지 기본값(JSON 텍스트).
    pub input_default: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GrpcService {
    pub name: String,
    pub methods: Vec<GrpcMethod>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GrpcCatalog {
    pub source: CatalogSource,
    pub services: Vec<GrpcService>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GrpcCall {
    pub target: String,
    /// `package.Service/Method`(목록의 `fullName`).
    pub method: String,
    /// 요청 메시지 JSON 텍스트(변수·비밀 참조 가능).
    pub json: String,
    pub metadata: Vec<HeaderRow>,
    pub tls: TlsOptions,
    pub env: EnvSnapshot,
    pub timeout_ms: i64,
    pub proto_files: Option<Vec<String>>,
    pub origin: Option<RequestOrigin>,
}

/// `api.grpc_call` 스트림의 NDJSON 한 줄.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GrpcEvent {
    Headers {
        metadata: Vec<Pair>,
    },
    Message {
        json: String,
    },
    /// 서버가 끝낸 상태(`code` 0 = OK). 서버가 보낸 실패 상태도 여기로 온다.
    Trailers {
        code: i32,
        message: String,
        metadata: Vec<Pair>,
    },
    /// 연결·전송 실패나 상한 초과(서버 상태가 아님).
    Failed {
        error: WireError,
    },
}

/// 치환·비밀 해석에 넣을 모양: 대상 = URL, 메타데이터 = 헤더, 요청 JSON = 활성 본문, TLS 경로.
fn snapshot(
    target: &str,
    metadata: &[HeaderRow],
    json: Option<&str>,
    tls: &TlsOptions,
) -> RequestSnapshot {
    RequestSnapshot {
        method: "POST".into(),
        url: target.into(),
        query: vec![],
        headers: metadata.to_vec(),
        cookies: vec![],
        body: match json {
            Some(text) => Body::Json { text: text.into() },
            None => Body::None,
        },
        auth: Auth::None,
        tls: tls.clone(),
        timeout_ms: DESCRIBE_TIMEOUT.as_millis() as i64,
        follow_redirects: false,
    }
}

/// 대상 URL은 scheme·host·port만(경로·query·fragment 없음).
pub(crate) fn target_url(text: &str) -> Result<url::Url, ApiError> {
    let url = validate::url(text)?;
    if !matches!(url.path(), "" | "/") || url.query().is_some() {
        return Err(ApiError::url("grpc_target"));
    }
    Ok(url)
}

/// 연결 전 검사를 끝낸 대상.
pub(crate) struct Prepared {
    pub(crate) url: url::Url,
    pub(crate) metadata: tonic::metadata::MetadataMap,
    pub(crate) tls: crate::tls::PreparedTls,
    pub(crate) proto_files: Option<Vec<String>>,
    pub(crate) redaction: crate::redact::Redaction,
}

impl ApiService {
    async fn grpc_prepare(
        &self,
        target: &str,
        metadata: &[HeaderRow],
        json: Option<&str>,
        tls: &TlsOptions,
        env: &EnvSnapshot,
        proto_files: Option<Vec<String>>,
    ) -> R<(Prepared, Option<String>)> {
        let (filled, values) = self
            .resolve(&snapshot(target, metadata, json, tls), env)
            .await?;
        let url = target_url(&filled.url)?;
        let metadata = metadata::prepare(&filled.headers)?;
        let mut redaction = crate::redact::Redaction::new(&values)?;
        redaction.request_headers(metadata::headers(&metadata))?;
        let tls = crate::tls::prepare(&filled.tls)?;
        if let Some(files) = &proto_files {
            describe::check_proto_paths(files)?;
        }
        let json = match filled.body {
            Body::Json { text } => Some(text),
            _ => None,
        };
        Ok((
            Prepared {
                url,
                metadata,
                tls,
                proto_files,
                redaction,
            },
            json,
        ))
    }

    pub async fn grpc_describe(&self, p: GrpcTarget) -> R<GrpcCatalog> {
        let (mut prepared, _) = self
            .grpc_prepare(&p.target, &p.metadata, None, &p.tls, &p.env, p.proto_files)
            .await?;
        let started = std::time::Instant::now();
        let result = tokio::time::timeout(DESCRIBE_TIMEOUT, describe::catalog(&mut prepared))
            .await
            .unwrap_or_else(|_| Err(ApiError::network("timeout")));
        tracing::debug!(
            method = "api.grpc_describe",
            elapsed_ms = started.elapsed().as_millis() as u64,
            outcome = if result.is_ok() { "ok" } else { "failed" }
        );
        Ok(result?)
    }

    /// unary·server streaming 호출. 검사·서술자 해석·요청 JSON 해석은 스트림을 열기 전에 끝낸다.
    pub async fn grpc_call(&self, ctx: Ctx, p: GrpcCall) -> R<StreamOpened> {
        let timeout = validate::timeout(p.timeout_ms)?;
        let (prepared, json) = self
            .grpc_prepare(
                &p.target,
                &p.metadata,
                Some(&p.json),
                &p.tls,
                &p.env,
                p.proto_files,
            )
            .await?;
        let job =
            call::prepare(prepared, &p.method, json.as_deref().unwrap_or(""), timeout).await?;
        let permit = self.session_slot()?;
        let ends = ctx.open_stream().map_err(|_| ApiError::StreamUnsupported)?;
        let stream_id = ends.id;
        drop(ends.input);
        tokio::spawn(call::run(job, ends.tx, permit));
        Ok(StreamOpened { stream_id })
    }
}
