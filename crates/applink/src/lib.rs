//! 이미 떠 있는(또는 콜드 스타트하는) devbox 앱에게 "이 상태로 가라"고 말하는 인바운드
//! 계약.
//!
//! `crates/launch`가 다른 앱을 **실행**하고, `crates/integration`이 상태를 **남기면**,
//! 이 크레이트는 그 사이 — 실행된 앱이 "무엇을 열어야 하는지"를 argv로 주고받는 타입과
//! 파서를 정의한다. 수신 앱 13개는 설치 경로 해석(`crates/launch`) 없이 이 계약에만
//! 의존해야 하므로 별도 크레이트로 둔다
//! (`docs/superpowers/specs/2026-08-17-app-interop-design.md` §1.1).
//!
//! ## argv 색인 규약 — `parse_argv` vs `build_argv`
//!
//! `parse_argv`는 `std::env::args().collect()`를 그대로 받는 것을 전제로 한다. 즉
//! **인자 0번은 실행 파일 경로**이고 `parse_argv`가 내부에서 건너뛴다 — 호출자가 미리
//! 잘라낼 필요가 없다. `tauri-plugin-single-instance`가 넘기는 두 번째 인스턴스의
//! argv도 같은 모양(0번이 exe 경로)이므로 콜드 스타트와 동일한 코드 경로를 쓸 수 있다
//! (설계 §3).
//!
//! `build_argv`는 반대로 **실행 파일 경로를 포함하지 않는다.** 반환값은
//! `std::process::Command::args()`에 그대로 넘기는 인자 목록이다
//! (`crates/launch::launch`의 `args: &[&str]`와 같은 모양). 그래서 두 함수를 직접
//! 연결해 왕복 검증을 하려면 `build_argv`가 만든 목록 앞에 더미 exe 인자를 하나
//! 붙여야 한다 — 아래 `round_trip_*` 테스트가 그 패턴을 보여준다.
//!
//! 이 비대칭 때문에 "맨 앞 위치 인자를 `Path`로 해석한다"는 하위 호환 규칙
//! (§1.3)이 실행 파일 경로 자체를 타깃으로 오인할 위험이 있다 — `parse_argv`가 0번을
//! 먼저 건너뛰므로 이 위험은 없다. `exe 경로만 있는 argv → Ok(None)` 테스트가 이를
//! 명시적으로 확인한다.
//!
//! ## 전방 호환 규칙 (§1.3) — 반드시 지켜야 하는 계약
//!
//! devbox-manager는 앱을 개별적으로 업데이트하므로, 새 발신자가 구버전 수신자에게
//! 말을 거는 상황이 항상 생긴다. 그래서:
//!
//! - **모르는 플래그는 무시한다.** 오류가 아니다.
//! - 알려진 타깃 플래그가 하나도 없으면 `Ok(None)` — 앱은 평소대로 뜬다.
//! - 알려진 플래그인데 값이 없거나 형식이 잘못됐으면 `Err(ParseError)`.
//! - 맨 앞 위치 인자(플래그 아님)는 `Path`로 해석한다 — 이미 배포된 repo-manager가
//!   그 형태로 보낸다.

mod handoff;
mod task_control;

pub use handoff::{
    handoff_root_in, validate_handoff_text, CreateHandoff, HandoffClaim, HandoffDescriptor,
    HandoffEnvelope, HandoffError, HandoffPublication, HandoffStatus, HandoffStatusRecord,
    HandoffStore, RecordHandoffStatus, DEFAULT_CLAIM_LEASE_MS, DEFAULT_HANDOFF_TTL_MS,
    MAX_HANDOFF_BYTES,
};
pub use task_control::{
    TaskControlAction, TaskControlRequest, TASK_CONTROL_HANDOFF_KIND, TASK_CONTROL_SCHEMA_VERSION,
    TASK_CONTROL_SOURCE_APP, TASK_CONTROL_TARGET_APP,
};

use serde::{Deserialize, Serialize};

/// Handoff envelope의 프로토콜 버전. `OpenRequest` argv에 새 forward-compatible
/// target flag를 추가하는 것만으로는 one-time handoff envelope의 모양이 바뀌지 않는다.
pub const PROTOCOL_VERSION: u32 = 2;
/// Keep cross-app command lines below the smallest supported Windows command
/// line budget. This bounds both parser work and the values that launchers can
/// forward through AppLink, including unknown forward-compatible flags.
pub const MAX_ARGV_ARGS: usize = 64;
pub const MAX_ARGV_BYTES: usize = 32 * 1024;
const MAX_TASK_ID_BYTES: usize = 128;
const MAX_INSTALL_APP_ID_BYTES: usize = 64;

/// Bounded, app-neutral query filter carried by the `query` applink target.
///
/// This is intentionally a small value object rather than an Everything+
/// database type.  Older receivers ignore the optional `--query-filter-v1`
/// flag and still receive the query text; receivers that understand v1 validate
/// and apply the filter before searching their own local index.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueryFilter {
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_after: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_before: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_size: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_size: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_root_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_status: Option<String>,
}

/// Conservative detector for values that must never be persisted in a query
/// or handoff. It intentionally prefers a false positive over copying a
/// credential into an integration snapshot or argv. This is syntax-oriented,
/// not a claim that arbitrary secrets can be identified.
pub fn contains_sensitive_value(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    if lower.contains("-----begin private key-----")
        || lower.contains("-----begin rsa private key-----")
        || lower.contains("-----begin openssh private key-----")
        || lower.starts_with("bearer ")
        || lower.starts_with("basic ")
        || lower.split_whitespace().any(|token| {
            token == "bearer"
                || token == "basic"
                || token.starts_with("bearer ")
                || token.starts_with("basic ")
        })
    {
        return true;
    }

    for key in [
        "authorization",
        "password",
        "passwd",
        "secret",
        "api_key",
        "api-key",
        "access_token",
        "access-token",
        "refresh_token",
        "refresh-token",
        "client_secret",
        "client-secret",
        "cookie",
        "set-cookie",
        "x-api-key",
        "token",
    ] {
        let mut offset = 0;
        while let Some(found) = lower[offset..].find(key) {
            let start = offset + found;
            let suffix = lower[start + key.len()..].trim_start();
            let Some(rest) = suffix
                .strip_prefix(':')
                .or_else(|| suffix.strip_prefix('='))
            else {
                offset = start + key.len();
                continue;
            };
            if !rest.trim_start().is_empty() {
                return true;
            }
            offset = start + key.len();
        }
    }

    // URL userinfo (`scheme://user:password@host`) is a credential even when
    // it is not written as a named query parameter.
    for marker in ["://", "//"] {
        let mut offset = 0;
        while let Some(found) = lower[offset..].find(marker) {
            let authority_start = offset + found + marker.len();
            let authority = lower[authority_start..]
                .split(|character: char| character.is_whitespace() || "/?#".contains(character))
                .next()
                .unwrap_or_default();
            // Treat any URL userinfo as sensitive. A username-only authority
            // and percent-encoded `user%3Apass@` are still unsafe to copy into
            // a snapshot, and rejecting them avoids trying to decode URL
            // escapes in this intentionally conservative detector.
            if authority.contains('@') {
                return true;
            }
            offset = authority_start;
        }
    }

    let token_is_jwt = |token: &str| {
        let mut parts = token.split('.');
        let Some(header) = parts.next() else {
            return false;
        };
        let Some(payload) = parts.next() else {
            return false;
        };
        let Some(signature) = parts.next() else {
            return false;
        };
        parts.next().is_none()
            && header.len() >= 8
            && payload.len() >= 8
            && signature.len() >= 8
            && [header, payload, signature].iter().all(|part| {
                part.bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            })
    };
    if lower
        .split(|character: char| {
            character.is_whitespace()
                || matches!(
                    character,
                    '"' | '\''
                        | '='
                        | ':'
                        | ','
                        | ';'
                        | '&'
                        | '?'
                        | '/'
                        | '\\'
                        | '('
                        | '['
                        | '{'
                        | ')'
                )
        })
        .any(|token| {
            token_is_jwt(token)
                || [
                    "sk-",
                    "ghp_",
                    "github_pat_",
                    "xoxb-",
                    "xoxp-",
                    "glpat-",
                    "npm_",
                    "gho_",
                    "akia",
                    "eyj",
                ]
                .iter()
                .any(|prefix| token.starts_with(prefix) && token.len() > prefix.len())
        })
    {
        return true;
    }
    false
}

impl QueryFilter {
    pub const MAX_EXTENSIONS: usize = 64;
    pub const MAX_EXTENSION_BYTES: usize = 16;

    pub fn normalized(&self) -> Result<Self, ParseError> {
        if self.extensions.len() > Self::MAX_EXTENSIONS {
            return Err(ParseError("query filter extension bound exceeded".into()));
        }
        let mut extensions = Vec::with_capacity(self.extensions.len());
        for extension in &self.extensions {
            let normalized = extension
                .trim()
                .trim_start_matches('.')
                .to_ascii_lowercase();
            if normalized.is_empty()
                || normalized.len() > Self::MAX_EXTENSION_BYTES
                || !normalized.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'_' | b'-' | b'+')
                })
            {
                return Err(ParseError("query filter extension is invalid".into()));
            }
            extensions.push(normalized);
        }
        extensions.sort_unstable();
        extensions.dedup();

        if self
            .modified_after
            .zip(self.modified_before)
            .is_some_and(|(after, before)| after > before)
            || self.modified_after.is_some_and(|timestamp| timestamp < 0)
            || self.modified_before.is_some_and(|timestamp| timestamp < 0)
            || self.min_size.is_some_and(|size| size < 0)
            || self.max_size.is_some_and(|size| size < 0)
            || self
                .min_size
                .zip(self.max_size)
                .is_some_and(|(minimum, maximum)| minimum > maximum)
            || self.source_root_id.is_some_and(|root_id| root_id <= 0)
        {
            return Err(ParseError("query filter range is invalid".into()));
        }

        let content_status = self
            .content_status
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_ascii_lowercase);
        if content_status.as_deref().is_some_and(|status| {
            !matches!(
                status,
                "indexed"
                    | "truncated"
                    | "partial"
                    | "failed"
                    | "not_indexed"
                    | "too_large"
                    | "unsupported_encoding"
                    | "read_error"
                    | "timeout"
                    | "changed_during_read"
                    | "skipped_sensitive"
                    | "no_text"
                    | "unsupported_encrypted"
                    | "extract_error"
            )
        }) {
            return Err(ParseError("query filter status is invalid".into()));
        }

        Ok(Self {
            extensions,
            modified_after: self.modified_after,
            modified_before: self.modified_before,
            min_size: self.min_size,
            max_size: self.max_size,
            source_root_id: self.source_root_id,
            content_status,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.extensions.is_empty()
            && self.modified_after.is_none()
            && self.modified_before.is_none()
            && self.min_size.is_none()
            && self.max_size.is_none()
            && self.source_root_id.is_none()
            && self.content_status.is_none()
    }
}

/// "어디를 열지"를 나타내는 타깃.
///
/// 새 variant를 추가할 때는 §1.3 표를 다시 확인한다 — 구버전 수신 앱은 새
/// variant의 태그를 모르므로 (모르는 플래그로 취급돼) 무시하고 `Ok(None)`으로
/// degrade해야 한다.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum OpenTarget {
    Path {
        path: String,
        line: Option<u32>,
        column: Option<u32>,
    },
    Profile {
        id: String,
    },
    Workspace {
        path: String,
    },
    Query {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filter: Option<QueryFilter>,
    },
    /// Run Manager의 저장된 job/service 하나를 연다. id만 전달하며 실제 job
    /// 명령·환경변수는 수신 앱이 자기 저장소에서 재검증한다.
    Task {
        id: String,
    },
    /// 대상 앱이 설치되어 있지 않을 때 Manager의 해당 앱 설치 화면을 연다.
    Install {
        #[serde(rename = "appId")]
        app_id: String,
    },
    Handoff {
        #[serde(rename = "handoffKind")]
        kind: String,
        id: String,
    },
}

impl From<HandoffDescriptor> for OpenTarget {
    fn from(descriptor: HandoffDescriptor) -> Self {
        Self::Handoff {
            kind: descriptor.kind,
            id: descriptor.id,
        }
    }
}

/// 파싱된 인바운드 요청. Tauri 이벤트(`devbox://open`) payload로 그대로 직렬화된다.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpenRequest {
    pub target: OpenTarget,
    /// 보낸 앱의 카탈로그 id — 로깅·"되돌아가기" 버튼용. 모르면 `None`.
    pub from: Option<String>,
}

/// `parse_argv` 실패 사유. §1.3에 따라 "알려진 플래그인데 값이 없거나 형식이 잘못됨"
/// 상황에서만 만들어진다 — 모르는 플래그는 절대 이 오류를 내지 않는다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "applink: {}", self.0)
    }
}

impl std::error::Error for ParseError {}

/// argv를 `OpenRequest`로 해석한다.
///
/// `args`는 `std::env::args().collect()` 그대로 넘긴다 — **0번 원소는 실행 파일
/// 경로로 취급해 건너뛴다** (모듈 문서의 "argv 색인 규약" 참고). `args`가 비어 있으면
/// 건너뛸 것도 없이 빈 목록으로 취급한다.
///
/// 반환:
/// - `Ok(Some(req))` — 알려진 타깃 플래그(또는 맨 앞 위치 인자)를 찾았다.
/// - `Ok(None)` — 알려진 타깃이 전혀 없다. 모르는 플래그만 있어도 여기 해당한다.
/// - `Err(ParseError)` — 알려진 플래그인데 값이 없거나 형식이 잘못됐다.
pub fn parse_argv(args: &[String]) -> Result<Option<OpenRequest>, ParseError> {
    validate_argv(args)?;
    let rest = if args.is_empty() { args } else { &args[1..] };
    parse_rest(rest)
}

/// Validate the bounded command-line shape used by AppLink. The OS has its
/// own process command-line limit, but checking here keeps each receiver's
/// parser and IPC payload bounded before any known value is cloned.
pub fn validate_argv(args: &[String]) -> Result<(), ParseError> {
    if args.len() > MAX_ARGV_ARGS {
        return Err(ParseError("applink argv가 너무 큼".into()));
    }
    let total = args
        .iter()
        .try_fold(0usize, |total, value| {
            total
                .checked_add(value.len())
                .and_then(|next| next.checked_add(1))
                .ok_or(())
        })
        .map_err(|_| ParseError("applink argv가 너무 큼".into()))?;
    if total > MAX_ARGV_BYTES {
        return Err(ParseError("applink argv가 너무 큼".into()));
    }
    Ok(())
}

fn parse_rest(rest: &[String]) -> Result<Option<OpenRequest>, ParseError> {
    let mut path: Option<String> = None;
    let mut line: Option<u32> = None;
    let mut column: Option<u32> = None;
    let mut profile: Option<String> = None;
    let mut workspace: Option<String> = None;
    let mut query: Option<String> = None;
    let mut query_filter: Option<QueryFilter> = None;
    let mut task: Option<String> = None;
    let mut install_app: Option<String> = None;
    let mut handoff_kind: Option<String> = None;
    let mut handoff_id: Option<String> = None;
    let mut from: Option<String> = None;

    let mut i = 0;

    // 하위 호환: 맨 앞이 플래그가 아니면 위치 인자를 Path로 해석한다 (§1.3).
    if let Some(first) = rest.first() {
        if !first.starts_with("--") {
            path = Some(first.clone());
            i = 1;
        }
    }

    while i < rest.len() {
        let flag = rest[i].as_str();
        match flag {
            "--path" => path = Some(take_value(rest, &mut i, flag)?),
            "--line" => line = Some(take_u32(rest, &mut i, flag)?),
            "--column" => column = Some(take_u32(rest, &mut i, flag)?),
            "--profile" => profile = Some(take_value(rest, &mut i, flag)?),
            "--workspace" => workspace = Some(take_value(rest, &mut i, flag)?),
            "--query" => query = Some(take_value(rest, &mut i, flag)?),
            "--query-filter-v1" => {
                let raw = take_value(rest, &mut i, flag)?;
                let filter = serde_json::from_str::<QueryFilter>(&raw)
                    .map_err(|_| ParseError("query filter is invalid".into()))?
                    .normalized()?;
                query_filter = Some(filter);
            }
            "--task" => {
                let value = take_value(rest, &mut i, flag)?;
                if !valid_opaque_id(&value, MAX_TASK_ID_BYTES, true) {
                    return Err(ParseError("--task 값이 올바르지 않음".into()));
                }
                task = Some(value);
            }
            "--install-app" => {
                let value = take_value(rest, &mut i, flag)?;
                if !valid_opaque_id(&value, MAX_INSTALL_APP_ID_BYTES, false) {
                    return Err(ParseError("--install-app 값이 올바르지 않음".into()));
                }
                install_app = Some(value);
            }
            "--handoff-kind" => handoff_kind = Some(take_value(rest, &mut i, flag)?),
            "--handoff-id" => handoff_id = Some(take_value(rest, &mut i, flag)?),
            "--from" => from = Some(take_value(rest, &mut i, flag)?),
            _ => {
                // 모르는 플래그: 토큰 자체만 건너뛴다. 값의 arity를 모르므로 다음
                // 토큰을 소비하지 않는다 (§1.3 — 오류가 아니라 무시).
                i += 1;
            }
        }
    }

    let handoff = match (handoff_kind, handoff_id) {
        (Some(kind), Some(id)) if !kind.is_empty() && !id.is_empty() => {
            Some(OpenTarget::Handoff { kind, id })
        }
        (None, None) => None,
        _ => return Err(ParseError("handoff kind/id가 모두 필요함".into())),
    };
    if query.is_none() && query_filter.is_some() {
        return Err(ParseError("query filter에는 query가 필요함".into()));
    }

    let target = path
        .map(|path| OpenTarget::Path { path, line, column })
        .or_else(|| profile.map(|id| OpenTarget::Profile { id }))
        .or_else(|| workspace.map(|path| OpenTarget::Workspace { path }))
        .or_else(|| {
            query.map(|text| OpenTarget::Query {
                text,
                filter: query_filter,
            })
        })
        .or_else(|| task.map(|id| OpenTarget::Task { id }))
        .or_else(|| install_app.map(|app_id| OpenTarget::Install { app_id }))
        .or(handoff);

    Ok(target.map(|target| OpenRequest { target, from }))
}

/// `rest[i]`가 알려진 플래그일 때 그 값을 가져오고 `i`를 값 뒤로 옮긴다. 값이 없으면
/// (플래그가 마지막 원소면) `Err`.
fn take_value(rest: &[String], i: &mut usize, flag: &str) -> Result<String, ParseError> {
    let value = rest
        .get(*i + 1)
        .ok_or_else(|| ParseError(format!("{flag} 값 누락")))?
        .clone();
    *i += 2;
    Ok(value)
}

fn valid_opaque_id(value: &str, max_bytes: usize, extended: bool) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'-' | b'_')
                || (extended && (byte.is_ascii_uppercase() || matches!(byte, b'.' | b':')))
        })
}

/// `take_value` + u32 파싱. 값이 숫자가 아니면 `Err`.
fn take_u32(rest: &[String], i: &mut usize, flag: &str) -> Result<u32, ParseError> {
    let raw = take_value(rest, i, flag)?;
    raw.parse::<u32>()
        .map_err(|_| ParseError(format!("{flag} 값이 숫자가 아님")))
}

/// `OpenRequest`를 argv로 인코딩한다. 값이 계약의 범위/형식을 벗어나면 부분 argv를
/// 만들거나 filter를 버리지 않고 오류를 반환한다.
///
/// 반환값은 **실행 파일 경로를 포함하지 않는다** — `std::process::Command::args()`나
/// `crates/launch::launch`의 `args: &[&str]`에 그대로 넘기는 모양이다. 항상 명시적
/// 플래그(`--path` 등)만 만든다 — 맨 앞 위치 인자 형태는 구버전 발신자와의 하위
/// 호환을 위해 `parse_argv`만 받아들이고, 새로 만들지는 않는다.
pub fn build_argv(req: &OpenRequest) -> Result<Vec<String>, ParseError> {
    let mut out = Vec::new();

    match &req.target {
        OpenTarget::Path { path, line, column } => {
            out.push("--path".to_string());
            out.push(path.clone());
            if let Some(line) = line {
                out.push("--line".to_string());
                out.push(line.to_string());
            }
            if let Some(column) = column {
                out.push("--column".to_string());
                out.push(column.to_string());
            }
        }
        OpenTarget::Profile { id } => {
            out.push("--profile".to_string());
            out.push(id.clone());
        }
        OpenTarget::Workspace { path } => {
            out.push("--workspace".to_string());
            out.push(path.clone());
        }
        OpenTarget::Query { text, filter } => {
            out.push("--query".to_string());
            out.push(text.clone());
            if let Some(filter) = filter.as_ref() {
                let filter = filter.normalized()?;
                if !filter.is_empty() {
                    let json = serde_json::to_string(&filter)
                        .map_err(|_| ParseError("query filter encoding failed".into()))?;
                    out.push("--query-filter-v1".to_string());
                    out.push(json);
                }
            }
        }
        OpenTarget::Task { id } => {
            out.push("--task".to_string());
            out.push(id.clone());
        }
        OpenTarget::Install { app_id } => {
            out.push("--install-app".to_string());
            out.push(app_id.clone());
        }
        OpenTarget::Handoff { kind, id } => {
            out.push("--handoff-kind".to_string());
            out.push(kind.clone());
            out.push("--handoff-id".to_string());
            out.push(id.clone());
        }
    }

    if let Some(from) = &req.from {
        out.push("--from".to_string());
        out.push(from.clone());
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> String {
        v.to_string()
    }

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    // ---- 플래그별 타깃 파싱 ----

    #[test]
    fn parses_path_flag() {
        let req = parse_argv(&argv(&["exe", "--path", "/tmp/x"]))
            .unwrap()
            .unwrap();
        assert_eq!(
            req,
            OpenRequest {
                target: OpenTarget::Path {
                    path: s("/tmp/x"),
                    line: None,
                    column: None,
                },
                from: None,
            }
        );
    }

    #[test]
    fn parses_path_with_line_and_column() {
        let req = parse_argv(&argv(&[
            "exe", "--path", "/tmp/x", "--line", "10", "--column", "5",
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(
            req.target,
            OpenTarget::Path {
                path: s("/tmp/x"),
                line: Some(10),
                column: Some(5),
            }
        );
    }

    #[test]
    fn parses_path_with_line_only() {
        let req = parse_argv(&argv(&["exe", "--path", "/tmp/x", "--line", "3"]))
            .unwrap()
            .unwrap();
        assert_eq!(
            req.target,
            OpenTarget::Path {
                path: s("/tmp/x"),
                line: Some(3),
                column: None,
            }
        );
    }

    #[test]
    fn parses_path_with_column_only() {
        let req = parse_argv(&argv(&["exe", "--path", "/tmp/x", "--column", "7"]))
            .unwrap()
            .unwrap();
        assert_eq!(
            req.target,
            OpenTarget::Path {
                path: s("/tmp/x"),
                line: None,
                column: Some(7),
            }
        );
    }

    #[test]
    fn non_numeric_line_is_err() {
        let err = parse_argv(&argv(&["exe", "--path", "/tmp/x", "--line", "abc"])).unwrap_err();
        assert!(err.0.contains("--line"), "{err}");
    }

    #[test]
    fn non_numeric_column_is_err() {
        let err = parse_argv(&argv(&["exe", "--path", "/tmp/x", "--column", "abc"])).unwrap_err();
        assert!(err.0.contains("--column"), "{err}");
    }

    #[test]
    fn argv_shape_is_bounded_before_known_values_are_parsed() {
        let too_many = std::iter::once(s("exe"))
            .chain((0..MAX_ARGV_ARGS).map(|_| s("--unknown")))
            .collect::<Vec<_>>();
        assert!(parse_argv(&too_many).is_err());

        let too_large = vec![s("exe"), s("--query"), "x".repeat(MAX_ARGV_BYTES)];
        assert!(parse_argv(&too_large).is_err());
    }

    #[test]
    fn malformed_numeric_values_are_not_reflected_in_errors() {
        let secret = "Bearer top-secret-token";
        let error = parse_argv(&argv(&["exe", "--line", secret])).unwrap_err();
        assert!(!error.0.contains(secret));
    }

    #[test]
    fn parses_profile_flag() {
        let req = parse_argv(&argv(&["exe", "--profile", "prof-1"]))
            .unwrap()
            .unwrap();
        assert_eq!(req.target, OpenTarget::Profile { id: s("prof-1") });
    }

    #[test]
    fn parses_workspace_flag() {
        let req = parse_argv(&argv(&["exe", "--workspace", "/ws/path"]))
            .unwrap()
            .unwrap();
        assert_eq!(
            req.target,
            OpenTarget::Workspace {
                path: s("/ws/path")
            }
        );
    }

    #[test]
    fn parses_query_flag() {
        let req = parse_argv(&argv(&["exe", "--query", "hello world"]))
            .unwrap()
            .unwrap();
        assert_eq!(
            req.target,
            OpenTarget::Query {
                text: s("hello world"),
                filter: None,
            }
        );
    }

    #[test]
    fn query_filter_v1_round_trips_and_is_optional_for_legacy_queries() {
        let filter = QueryFilter {
            extensions: vec![".RS".into()],
            min_size: Some(10),
            source_root_id: Some(7),
            content_status: Some("TRUNCATED".into()),
            ..QueryFilter::default()
        };
        let request = OpenRequest {
            target: OpenTarget::Query {
                text: s("cargo"),
                filter: Some(filter.clone()),
            },
            from: Some(s("devbox-launcher")),
        };
        let mut full_argv = vec![s("everything-plus.exe")];
        full_argv.extend(build_argv(&request).unwrap());
        let parsed = parse_argv(&full_argv).unwrap().unwrap();
        assert_eq!(
            parsed.target,
            OpenTarget::Query {
                text: s("cargo"),
                filter: Some(filter.normalized().unwrap()),
            }
        );
        // A legacy text-only request remains unchanged.
        assert_eq!(
            parse_argv(&argv(&["exe", "--query", "cargo"]))
                .unwrap()
                .unwrap()
                .target,
            OpenTarget::Query {
                text: s("cargo"),
                filter: None,
            }
        );
    }

    #[test]
    fn query_filter_rejects_negative_timestamps_and_sizes() {
        for filter in [
            QueryFilter {
                modified_after: Some(-1),
                ..QueryFilter::default()
            },
            QueryFilter {
                modified_before: Some(-1),
                ..QueryFilter::default()
            },
            QueryFilter {
                min_size: Some(-1),
                ..QueryFilter::default()
            },
        ] {
            assert!(filter.normalized().is_err());
        }
        assert!(parse_argv(&argv(&[
            "exe",
            "--query-filter-v1",
            "{\"extensions\":[\"rs\"]}"
        ]))
        .is_err());
    }

    // ---- --from: 모든 타깃 종류에서 캡처 ----

    #[test]
    fn from_captured_with_path() {
        let req = parse_argv(&argv(&[
            "exe",
            "--path",
            "/tmp/x",
            "--from",
            "repo-manager",
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(req.from, Some(s("repo-manager")));
    }

    #[test]
    fn from_captured_with_profile() {
        let req = parse_argv(&argv(&["exe", "--profile", "p1", "--from", "workbench"]))
            .unwrap()
            .unwrap();
        assert_eq!(req.from, Some(s("workbench")));
    }

    #[test]
    fn from_captured_with_workspace() {
        let req = parse_argv(&argv(&["exe", "--workspace", "/ws", "--from", "workbench"]))
            .unwrap()
            .unwrap();
        assert_eq!(req.from, Some(s("workbench")));
    }

    #[test]
    fn from_captured_with_query() {
        let req = parse_argv(&argv(&["exe", "--query", "q", "--from", "everything-plus"]))
            .unwrap()
            .unwrap();
        assert_eq!(req.from, Some(s("everything-plus")));
    }

    #[test]
    fn from_captured_with_bare_positional_path() {
        let req = parse_argv(&argv(&["exe", "/tmp/x", "--from", "repo-manager"]))
            .unwrap()
            .unwrap();
        assert_eq!(req.from, Some(s("repo-manager")));
        assert_eq!(
            req.target,
            OpenTarget::Path {
                path: s("/tmp/x"),
                line: None,
                column: None,
            }
        );
    }

    // ---- 전방 호환: 모르는 플래그 ----

    /// 회귀 방지: 모르는 플래그는 절대 오류가 아니다. 새 발신자가 구버전 수신자에게
    /// 새 플래그를 보내도 앱이 평소대로(빈 상태로) 뜨는 것이 §1.3의 핵심 보장이다.
    #[test]
    fn unknown_flags_are_ignored_not_errors() {
        assert_eq!(
            parse_argv(&argv(&["exe", "--totally-unknown"])).unwrap(),
            None
        );
        // "somevalue"는 맨 앞 위치 인자가 아니다(앞에 이미 모르는 플래그가 있었다) —
        // Path로 잘못 해석되면 안 되고, 알려진 타깃도 없으므로 Ok(None)이어야 한다.
        assert_eq!(
            parse_argv(&argv(&["exe", "--totally-unknown", "somevalue"])).unwrap(),
            None
        );
    }

    #[test]
    fn unknown_flag_alone_is_ok_none() {
        assert_eq!(parse_argv(&argv(&["exe", "--foo"])).unwrap(), None);
    }

    #[test]
    fn unknown_flags_only_argv_is_ok_none() {
        assert_eq!(
            parse_argv(&argv(&["exe", "--foo", "--bar", "--baz"])).unwrap(),
            None
        );
    }

    #[test]
    fn unknown_flags_interleaved_with_known_flag() {
        let req = parse_argv(&argv(&["exe", "--foo", "--path", "/tmp/x", "--bar"]))
            .unwrap()
            .unwrap();
        assert_eq!(
            req.target,
            OpenTarget::Path {
                path: s("/tmp/x"),
                line: None,
                column: None,
            }
        );
    }

    #[test]
    fn unknown_flags_before_and_after_from() {
        let req = parse_argv(&argv(&[
            "exe",
            "--nope",
            "--profile",
            "p1",
            "--also-nope",
            "--from",
            "workbench",
            "--still-nope",
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(req.target, OpenTarget::Profile { id: s("p1") });
        assert_eq!(req.from, Some(s("workbench")));
    }

    // ---- 타깃 없음 / 빈 argv ----

    #[test]
    fn no_known_target_flag_is_ok_none() {
        assert_eq!(
            parse_argv(&argv(&["exe", "--from", "repo-manager"])).unwrap(),
            None
        );
    }

    #[test]
    fn empty_argv_is_ok_none() {
        assert_eq!(parse_argv(&[]).unwrap(), None);
    }

    #[test]
    fn exe_path_only_argv_is_ok_none() {
        // 0번 원소는 항상 실행 파일 경로로 건너뛴다 — Path 타깃으로 오인하면 안 된다.
        assert_eq!(
            parse_argv(&argv(&["C:\\devbox\\code-pad.exe"])).unwrap(),
            None
        );
    }

    // ---- 값 누락 ----

    #[test]
    fn missing_path_value_is_err() {
        assert!(parse_argv(&argv(&["exe", "--path"])).is_err());
    }

    #[test]
    fn missing_profile_value_is_err() {
        assert!(parse_argv(&argv(&["exe", "--profile"])).is_err());
    }

    #[test]
    fn missing_workspace_value_is_err() {
        assert!(parse_argv(&argv(&["exe", "--workspace"])).is_err());
    }

    #[test]
    fn missing_query_value_is_err() {
        assert!(parse_argv(&argv(&["exe", "--query"])).is_err());
    }

    #[test]
    fn missing_from_value_is_err() {
        assert!(parse_argv(&argv(&["exe", "--path", "/tmp/x", "--from"])).is_err());
    }

    #[test]
    fn missing_line_value_is_err() {
        assert!(parse_argv(&argv(&["exe", "--path", "/tmp/x", "--line"])).is_err());
    }

    #[test]
    fn missing_column_value_is_err() {
        assert!(parse_argv(&argv(&["exe", "--path", "/tmp/x", "--column"])).is_err());
    }

    // ---- 맨 앞 위치 인자 ----

    #[test]
    fn bare_positional_path_is_parsed_as_path() {
        let req = parse_argv(&argv(&["exe", "/some/repo"])).unwrap().unwrap();
        assert_eq!(
            req.target,
            OpenTarget::Path {
                path: s("/some/repo"),
                line: None,
                column: None,
            }
        );
        assert_eq!(req.from, None);
    }

    // ---- build_argv ----

    #[test]
    fn build_argv_excludes_exe_path() {
        let req = OpenRequest {
            target: OpenTarget::Query {
                text: s("q"),
                filter: None,
            },
            from: None,
        };
        assert_eq!(build_argv(&req).unwrap(), vec![s("--query"), s("q")]);
    }

    #[test]
    fn build_argv_appends_from_last() {
        let req = OpenRequest {
            target: OpenTarget::Profile { id: s("p1") },
            from: Some(s("workbench")),
        };
        assert_eq!(
            build_argv(&req).unwrap(),
            vec![s("--profile"), s("p1"), s("--from"), s("workbench")]
        );
    }

    // ---- 왕복 테스트: build_argv → parse_argv, 모든 OpenTarget variant ----

    fn round_trip(req: OpenRequest) {
        let mut full = vec![s("devbox-app.exe")];
        full.extend(build_argv(&req).unwrap());
        let parsed = parse_argv(&full).unwrap().unwrap();
        assert_eq!(parsed, req, "round-trip 실패: {full:?}");
    }

    #[test]
    fn build_argv_rejects_invalid_query_filter_without_text_only_downgrade() {
        let request = OpenRequest {
            target: OpenTarget::Query {
                text: s("cargo"),
                filter: Some(QueryFilter {
                    min_size: Some(-1),
                    ..QueryFilter::default()
                }),
            },
            from: Some(s("devbox-launcher")),
        };
        assert!(build_argv(&request).is_err());
    }

    #[test]
    fn round_trip_path_no_line_column() {
        round_trip(OpenRequest {
            target: OpenTarget::Path {
                path: s("/tmp/x"),
                line: None,
                column: None,
            },
            from: None,
        });
    }

    #[test]
    fn round_trip_path_with_line_and_column() {
        round_trip(OpenRequest {
            target: OpenTarget::Path {
                path: s("C:\\repo\\src\\lib.rs"),
                line: Some(42),
                column: Some(7),
            },
            from: Some(s("repo-manager")),
        });
    }

    #[test]
    fn round_trip_path_with_line_only() {
        round_trip(OpenRequest {
            target: OpenTarget::Path {
                path: s("/tmp/x"),
                line: Some(1),
                column: None,
            },
            from: None,
        });
    }

    #[test]
    fn round_trip_profile() {
        round_trip(OpenRequest {
            target: OpenTarget::Profile { id: s("prof-42") },
            from: Some(s("workbench")),
        });
    }

    #[test]
    fn round_trip_workspace() {
        round_trip(OpenRequest {
            target: OpenTarget::Workspace {
                path: s("/ws/proj"),
            },
            from: Some(s("workbench")),
        });
    }

    #[test]
    fn round_trip_query() {
        round_trip(OpenRequest {
            target: OpenTarget::Query {
                text: s("search term"),
                filter: None,
            },
            from: Some(s("everything-plus")),
        });
    }

    #[test]
    fn round_trip_query_no_from() {
        round_trip(OpenRequest {
            target: OpenTarget::Query {
                text: s("no sender"),
                filter: None,
            },
            from: None,
        });
    }

    #[test]
    fn round_trip_task_and_install_targets() {
        round_trip(OpenRequest {
            target: OpenTarget::Task { id: s("job-1") },
            from: Some(s("devbox-launcher")),
        });
        round_trip(OpenRequest {
            target: OpenTarget::Install {
                app_id: s("repo-manager"),
            },
            from: Some(s("devbox-launcher")),
        });
    }

    #[test]
    fn task_and_install_targets_are_bounded_before_becoming_ipc_payloads() {
        let secret = "TOP_SECRET_VALUE";
        let task_error =
            parse_argv(&argv(&["exe", "--task", &format!("../{secret}")])).unwrap_err();
        assert!(!task_error.0.contains(secret));
        assert!(parse_argv(&[
            "exe".into(),
            "--task".into(),
            "x".repeat(MAX_TASK_ID_BYTES + 1),
        ])
        .is_err());
        assert!(parse_argv(&argv(&["exe", "--install-app", "Unknown_App"])).is_err());
        assert!(parse_argv(&[
            "exe".into(),
            "--install-app".into(),
            "x".repeat(MAX_INSTALL_APP_ID_BYTES + 1),
        ])
        .is_err());
    }

    #[test]
    fn parses_and_round_trips_handoff_descriptor_without_payload() {
        let request = parse_argv(&argv(&[
            "exe",
            "--handoff-kind",
            "api-request/v1",
            "--handoff-id",
            "0123456789abcdef0123456789abcdef",
            "--from",
            "webhook-lab",
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(
            request,
            OpenRequest {
                target: OpenTarget::Handoff {
                    kind: s("api-request/v1"),
                    id: s("0123456789abcdef0123456789abcdef"),
                },
                from: Some(s("webhook-lab")),
            }
        );
        round_trip(request);
    }

    #[test]
    fn handoff_kind_and_id_are_an_atomic_argv_pair() {
        assert!(parse_argv(&argv(&["exe", "--handoff-kind", "api-request/v1"])).is_err());
        assert!(parse_argv(&argv(&[
            "exe",
            "--handoff-id",
            "0123456789abcdef0123456789abcdef"
        ]))
        .is_err());
    }

    // ---- serde JSON 왕복: Tauri 이벤트 payload로 쓰이므로 별도 확인 ----

    #[test]
    fn json_round_trip_open_request() {
        let req = OpenRequest {
            target: OpenTarget::Path {
                path: s("/tmp/x"),
                line: Some(10),
                column: None,
            },
            from: Some(s("repo-manager")),
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: OpenRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back, req);
    }

    #[test]
    fn json_uses_camel_case_kind_tag() {
        let req = OpenRequest {
            target: OpenTarget::Workspace { path: s("/ws") },
            from: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["target"]["kind"], "workspace");
        assert_eq!(json["target"]["path"], "/ws");
        assert!(json["from"].is_null());
    }

    #[test]
    fn json_round_trip_all_target_variants() {
        let reqs = vec![
            OpenRequest {
                target: OpenTarget::Path {
                    path: s("/a"),
                    line: None,
                    column: None,
                },
                from: None,
            },
            OpenRequest {
                target: OpenTarget::Profile { id: s("p") },
                from: Some(s("wsl-desktop")),
            },
            OpenRequest {
                target: OpenTarget::Workspace { path: s("/w") },
                from: None,
            },
            OpenRequest {
                target: OpenTarget::Query {
                    text: s("t"),
                    filter: None,
                },
                from: None,
            },
            OpenRequest {
                target: OpenTarget::Task { id: s("job-1") },
                from: Some(s("devbox-launcher")),
            },
            OpenRequest {
                target: OpenTarget::Install {
                    app_id: s("run-manager"),
                },
                from: Some(s("devbox-launcher")),
            },
            OpenRequest {
                target: OpenTarget::Handoff {
                    kind: s("knowledge-draft/v1"),
                    id: s("0123456789abcdef0123456789abcdef"),
                },
                from: Some(s("life-log")),
            },
        ];
        for req in reqs {
            let json = serde_json::to_string(&req).unwrap();
            let back: OpenRequest = serde_json::from_str(&json).unwrap();
            assert_eq!(back, req);
        }
    }

    #[test]
    fn handoff_json_keeps_variant_tag_and_payload_kind_distinct() {
        let request = OpenRequest {
            target: OpenTarget::Handoff {
                kind: s("knowledge-draft/v1"),
                id: s("0123456789abcdef0123456789abcdef"),
            },
            from: Some(s("life-log")),
        };
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["target"]["kind"], "handoff");
        assert_eq!(json["target"]["handoffKind"], "knowledge-draft/v1");
        assert_eq!(json["target"]["id"], "0123456789abcdef0123456789abcdef");
    }
}
