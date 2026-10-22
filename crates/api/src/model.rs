//! `api.*` RPC 경계 타입. 사용자가 쓴 JSON은 텍스트 `String`으로만 오간다.
use devbox_protocol::message::WireError;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Pair {
    pub name: String,
    pub value: String,
}

/// 활성 헤더·쿠키 한 줄. `row`는 화면 행 번호(비활성 행 포함 순번)로 오류 위치에 쓴다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HeaderRow {
    pub row: u32,
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum GraphqlMethod {
    Post,
    Get,
}

/// 활성 본문 하나. 화면의 다른 종류 초안은 snapshot에 들어오지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Body {
    None,
    Raw {
        text: String,
        content_type: Option<String>,
    },
    Json {
        text: String,
    },
    Form {
        fields: Vec<Pair>,
    },
    Multipart {
        parts: Vec<Part>,
    },
    Binary {
        path: String,
    },
    Graphql {
        query: String,
        variables: String,
        operation_name: Option<String>,
        method: GraphqlMethod,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Part {
    Text {
        name: String,
        value: String,
        content_type: Option<String>,
    },
    File {
        name: String,
        path: String,
        file_name: Option<String>,
        content_type: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum KeyPlacement {
    Header,
    Query,
}

/// 활성 인증 하나.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Auth {
    None,
    Basic {
        username: String,
        password: String,
    },
    Bearer {
        token: String,
    },
    ApiKey {
        name: String,
        value: String,
        placement: KeyPlacement,
    },
    Oauth2ClientCredentials {
        token_url: String,
        client_id: String,
        client_secret: String,
        scope: Option<String>,
    },
    Oauth2AuthCode {
        authorization_url: String,
        token_url: String,
        client_id: String,
        client_secret: Option<String>,
        scope: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TlsOptions {
    pub verify: bool,
    pub ca_path: Option<String>,
    pub client_cert_path: Option<String>,
    pub client_key_path: Option<String>,
}

impl Default for TlsOptions {
    fn default() -> Self {
        Self {
            verify: true,
            ca_path: None,
            client_cert_path: None,
            client_key_path: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestSnapshot {
    pub method: String,
    pub url: String,
    pub query: Vec<Pair>,
    pub headers: Vec<HeaderRow>,
    pub cookies: Vec<HeaderRow>,
    pub body: Body,
    pub auth: Auth,
    pub tls: TlsOptions,
    pub timeout_ms: i64,
    pub follow_redirects: bool,
}

/// 화면이 환경과 세션을 합친 값. 값 안의 `{{secret:X}}`는 그대로 온다.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnvSnapshot {
    pub variables: Vec<EnvVar>,
}

/// 변수 값의 출처. `Capture`(응답에서 캡처한 값)는 글자 그대로 넣고 `{{secret:…}}` 참조로 풀지 않는다.
/// 악성 서버가 응답에 참조를 넣어 다음 요청에서 비밀을 끌어내지 못하게 한다(07b S5-3c).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum VarOrigin {
    Environment,
    Capture,
}

/// `sensitive`: 캡처 값·민감 이름·알려진 토큰 패턴이면 화면이 참으로 보낸다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnvVar {
    pub name: String,
    pub value: String,
    pub sensitive: bool,
    pub origin: VarOrigin,
}

impl EnvVar {
    /// 기록·응답에서 가릴 값인지. 캡처 값은 화면 표시와 상관없이 늘 가린다.
    pub fn masked(&self) -> bool {
        self.sensitive || self.origin == VarOrigin::Capture
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CollectionRoot {
    Personal,
    Project { project_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestOrigin {
    pub root: CollectionRoot,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SendRequest {
    pub snapshot: RequestSnapshot,
    pub env: EnvSnapshot,
    pub run_id: Option<String>,
    pub origin: Option<RequestOrigin>,
    pub cookie_jar: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Redirect {
    pub status: u16,
    pub location: String,
}

/// `api.send` 스트림의 NDJSON 한 줄.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SendEvent {
    Started {
        history_id: String,
        sent_at_ms: i64,
    },
    Authorize {
        url: String,
    },
    Head {
        status: u16,
        reason: String,
        http_version: String,
        headers: Vec<Pair>,
        elapsed_ms: i64,
        redirects: Vec<Redirect>,
    },
    /// 원본 최대 48KiB씩.
    Body {
        data_base64: String,
    },
    End {
        size_bytes: i64,
        elapsed_ms: i64,
        binary: bool,
        history_truncated: bool,
    },
    Failed {
        error: WireError,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum HistoryOutcome {
    Ok,
    Failed,
    Cancelled,
}

impl HistoryOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoryQuery {
    pub before: Option<String>,
    pub limit: u32,
    pub origin: Option<RequestOrigin>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HistorySummary {
    pub id: String,
    pub sent_at_ms: i64,
    pub method: String,
    pub url: String,
    pub status: Option<u16>,
    pub elapsed_ms: i64,
    pub size_bytes: i64,
    pub outcome: HistoryOutcome,
    pub run_id: Option<String>,
    pub origin: Option<RequestOrigin>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPage {
    pub items: Vec<HistorySummary>,
    pub next: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoryId {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HistoryResponse {
    pub status: u16,
    pub reason: String,
    pub http_version: String,
    pub headers: Vec<Pair>,
    pub redirects: Vec<Redirect>,
    pub elapsed_ms: i64,
    pub size_bytes: i64,
    pub binary: bool,
}

/// 기록 한 건. `snapshot`은 치환 전 요청을 정화한 것, `method`·`url`은 치환 뒤 값을 가린 것이다.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: String,
    pub sent_at_ms: i64,
    pub method: String,
    pub url: String,
    pub snapshot: RequestSnapshot,
    pub tls_verify: bool,
    pub run_id: Option<String>,
    pub origin: Option<RequestOrigin>,
    pub outcome: HistoryOutcome,
    pub error: Option<WireError>,
    pub response: Option<HistoryResponse>,
    pub body_base64: String,
    pub body_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CookieJarRef {
    pub jar: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CookieInfo {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    pub expires_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CookieList {
    pub cookies: Vec<CookieInfo>,
}
