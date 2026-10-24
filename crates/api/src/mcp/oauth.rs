//! MCP OAuth 2.1(MCP 2025-06 인가 절): 401의 `WWW-Authenticate` → 보호 자원 메타데이터(RFC 9728) →
//! 인가 서버 메타데이터(RFC 8414) → 동적 클라이언트 등록(RFC 7591) → PKCE S256 + state + `resource`(RFC 8707).
//! 콜백은 127.0.0.1(인증 중에만), 끝점은 https 또는 loopback http만, 메타데이터·토큰 응답은 크기 상한,
//! 보호 자원 `resource`·인가 서버 `issuer` 일치 확인, 요청은 MCP 연결의 TLS 설정이 아닌 기본 검증으로 한다.
//! 등록한 client_id와 토큰은 이 연결의 메모리에만 둔다(파일·기록 없음).
use crate::ApiError;
use crate::auth::{OauthConfig, exchange};
use crate::oauth_loopback::{Loopback, pkce, random};
use std::time::Duration;
use zeroize::Zeroizing;

const MAX_METADATA_BYTES: usize = 64 * 1024;
const MAX_TOKEN_BYTES: usize = 16 * 1024;
const STEP_TIMEOUT: Duration = Duration::from_secs(30);

/// https이거나 loopback 호스트의 http인 끝점만 쓴다(인가 코드·토큰이 평문으로 밖에 나가지 않게).
pub(crate) fn endpoint(text: &str) -> Result<url::Url, ApiError> {
    let url = crate::validate::url(text).map_err(|_| ApiError::oauth("endpoint"))?;
    let loopback = matches!(url.host(), Some(url::Host::Ipv4(ip)) if ip.is_loopback())
        || matches!(url.host(), Some(url::Host::Ipv6(ip)) if ip.is_loopback())
        || url.host_str() == Some("localhost");
    if url.scheme() == "https" || loopback {
        Ok(url)
    } else {
        Err(ApiError::oauth("endpoint"))
    }
}

/// `WWW-Authenticate: Bearer resource_metadata="…", scope="…"`의 값 하나.
pub(crate) fn challenge_param(header: &str, name: &str) -> Option<String> {
    let mut rest = header;
    while let Some(at) = rest.find(name) {
        let before = rest[..at].chars().last();
        let tail = &rest[at + name.len()..];
        rest = tail;
        if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let Some(value) = tail.trim_start().strip_prefix('=') else {
            continue;
        };
        let value = value.trim_start();
        return Some(match value.strip_prefix('"') {
            Some(quoted) => quoted.split('"').next().unwrap_or("").to_owned(),
            None => value.split([',', ' ']).next().unwrap_or("").to_owned(),
        });
    }
    None
}

async fn bounded(
    response: reqwest::Response,
    limit: usize,
    stage: &str,
) -> Result<Vec<u8>, ApiError> {
    if response.content_length().is_some_and(|n| n > limit as u64) {
        return Err(ApiError::oauth(stage));
    }
    let mut response = response;
    let mut out = vec![];
    while let Some(chunk) = response.chunk().await.map_err(|_| ApiError::oauth(stage))? {
        if out.len() + chunk.len() > limit {
            return Err(ApiError::oauth(stage));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

async fn get_json(
    client: &reqwest::Client,
    url: url::Url,
) -> Result<Option<serde_json::Value>, ApiError> {
    let send = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send();
    let response = tokio::time::timeout(STEP_TIMEOUT, send)
        .await
        .map_err(|_| ApiError::oauth("metadata_timeout"))?
        .map_err(|_| ApiError::oauth("metadata"))?;
    if !response.status().is_success() {
        return Ok(None);
    }
    let bytes = bounded(response, MAX_METADATA_BYTES, "metadata_too_large").await?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| ApiError::oauth("metadata"))
}

/// 두 URL이 같은지(파싱한 모양으로, fragment 없이). 값이 없거나 URL이 아니면 다르다.
fn same_url(text: Option<&str>, expected: &url::Url) -> bool {
    let strip = |mut url: url::Url| {
        url.set_fragment(None);
        url
    };
    text.and_then(|t| url::Url::parse(t).ok())
        .is_some_and(|url| strip(url) == strip(expected.clone()))
}

fn well_known(base: &url::Url, name: &str) -> Option<url::Url> {
    let path = base.path().trim_end_matches('/');
    let mut url = base.clone();
    url.set_query(None);
    url.set_path(&format!("/.well-known/{name}{path}"));
    Some(url)
}

pub(crate) struct Server {
    authorization: url::Url,
    token: url::Url,
    registration: url::Url,
}

/// 보호 자원 → 인가 서버 메타데이터. S256을 알리지 않는 서버는 쓰지 않는다(PKCE 필수).
pub(crate) async fn discover(
    client: &reqwest::Client,
    resource: &url::Url,
    challenge: Option<&str>,
) -> Result<Server, ApiError> {
    let prm_url = match challenge.and_then(|h| challenge_param(h, "resource_metadata")) {
        Some(url) => endpoint(&url)?,
        None => well_known(resource, "oauth-protected-resource")
            .ok_or_else(|| ApiError::oauth("metadata"))?,
    };
    let prm = get_json(client, prm_url)
        .await?
        .ok_or_else(|| ApiError::oauth("metadata"))?;
    // RFC 9728 §3.3: 메타데이터의 `resource`는 연결한 MCP 주소와 같아야 한다.
    if !same_url(prm["resource"].as_str(), resource) {
        return Err(ApiError::oauth("resource_mismatch"));
    }
    let issuer = prm["authorization_servers"]
        .get(0)
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::oauth("metadata"))?;
    let issuer = endpoint(issuer)?;
    let mut metadata = None;
    for name in ["oauth-authorization-server", "openid-configuration"] {
        let url = well_known(&issuer, name).ok_or_else(|| ApiError::oauth("metadata"))?;
        if let Some(found) = get_json(client, url).await? {
            metadata = Some(found);
            break;
        }
    }
    let metadata = metadata.ok_or_else(|| ApiError::oauth("metadata"))?;
    // RFC 8414 §3.3: 응답의 `issuer`는 메타데이터를 찾은 issuer와 같아야 한다(다른 서버의 끝점을 쓰지 않음).
    if !same_url(metadata["issuer"].as_str(), &issuer) {
        return Err(ApiError::oauth("issuer_mismatch"));
    }
    let s256 = metadata["code_challenge_methods_supported"]
        .as_array()
        .is_some_and(|m| m.iter().any(|v| v == "S256"));
    if !s256 {
        return Err(ApiError::oauth("pkce_unsupported"));
    }
    let field = |name: &str| -> Result<url::Url, ApiError> {
        endpoint(
            metadata[name]
                .as_str()
                .ok_or_else(|| ApiError::oauth("metadata"))?,
        )
    };
    Ok(Server {
        authorization: field("authorization_endpoint")?,
        token: field("token_endpoint")?,
        registration: metadata["registration_endpoint"]
            .as_str()
            .ok_or_else(|| ApiError::oauth("registration_unsupported"))
            .and_then(endpoint)?,
    })
}

async fn register(
    client: &reqwest::Client,
    server: &Server,
    redirect_uri: &str,
) -> Result<String, ApiError> {
    let body = serde_json::json!({
        "client_name": "Devbox",
        "redirect_uris": [redirect_uri],
        "grant_types": ["authorization_code"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
    });
    let send = client
        .post(server.registration.clone())
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(reqwest::header::ACCEPT, "application/json")
        .body(body.to_string())
        .send();
    let response = tokio::time::timeout(STEP_TIMEOUT, send)
        .await
        .map_err(|_| ApiError::oauth("registration"))?
        .map_err(|_| ApiError::oauth("registration"))?;
    if !response.status().is_success() {
        return Err(ApiError::oauth("registration"));
    }
    let bytes = bounded(response, MAX_METADATA_BYTES, "registration").await?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| ApiError::oauth("registration"))?;
    let id = value["client_id"]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 1024 && crate::validate::field_value(id).is_ok())
        .ok_or_else(|| ApiError::oauth("registration"))?;
    Ok(id.to_owned())
}

/// 브라우저 인증을 끝내고 access token을 돌려준다. `emit`은 인증 URL을 화면에 알린다.
pub(crate) async fn authorize<F, Fut>(
    client: &reqwest::Client,
    resource: &url::Url,
    challenge: Option<&str>,
    emit: F,
) -> Result<Zeroizing<String>, ApiError>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<(), ApiError>>,
{
    let server = discover(client, resource, challenge).await?;
    let loopback = Loopback::bind().await?;
    let redirect_uri = loopback.redirect_uri.clone();
    let client_id = register(client, &server, &redirect_uri).await?;
    let scope = challenge
        .and_then(|h| challenge_param(h, "scope"))
        .filter(|s| !s.is_empty());
    let state = random();
    let (verifier, code_challenge) = pkce();
    let mut url = server.authorization.clone();
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("response_type", "code")
            .append_pair("client_id", &client_id)
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("state", &state)
            .append_pair("code_challenge", &code_challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("resource", resource.as_str());
        if let Some(scope) = &scope {
            q.append_pair("scope", scope);
        }
    }
    emit(url.to_string()).await?;
    let code = loopback.code(&state).await?;
    let config = OauthConfig {
        authorization_url: Some(server.authorization),
        token_url: server.token,
        client_id,
        client_secret: None,
        scope,
    };
    let form = vec![
        ("grant_type", "authorization_code".to_string()),
        ("code", code.to_string()),
        ("redirect_uri", redirect_uri),
        ("code_verifier", verifier.to_string()),
        ("resource", resource.to_string()),
    ];
    let (token, _) = exchange(
        client,
        &config,
        form,
        tokio::time::Instant::now() + Duration::from_secs(60),
    )
    .await?;
    if token.len() > MAX_TOKEN_BYTES {
        return Err(ApiError::oauth("token_too_large"));
    }
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_are_https_or_loopback_and_challenges_parse() {
        assert!(endpoint("https://auth.example/authorize").is_ok());
        assert!(endpoint("http://127.0.0.1:8080/token").is_ok());
        assert!(endpoint("http://localhost:1/token").is_ok());
        assert!(endpoint("http://auth.example/token").is_err());
        assert!(endpoint("ftp://127.0.0.1/x").is_err());
        let h = r#"Bearer error="invalid_token", resource_metadata="http://127.0.0.1:9/.well-known/oauth-protected-resource", scope="mcp:read""#;
        assert_eq!(
            challenge_param(h, "resource_metadata").as_deref(),
            Some("http://127.0.0.1:9/.well-known/oauth-protected-resource")
        );
        assert_eq!(challenge_param(h, "scope").as_deref(), Some("mcp:read"));
        assert_eq!(challenge_param("Bearer", "scope"), None);
        assert_eq!(challenge_param("Bearer xscope=\"a\"", "scope"), None);
        let base = url::Url::parse("http://127.0.0.1:9").expect("url");
        assert!(same_url(Some("http://127.0.0.1:9/"), &base));
        assert!(!same_url(Some("http://127.0.0.1:9/evil"), &base));
        assert!(!same_url(None, &base));
    }
}
