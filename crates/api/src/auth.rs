//! Basic·Bearer·API key와 OAuth 2(client credentials, auth code + PKCE). 토큰은 메모리에만 둔다.
use crate::events::Events;
use crate::{ApiError, Auth, KeyPlacement, validate};
use reqwest::header::{HeaderName, HeaderValue};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

pub(crate) struct OauthConfig {
    pub(crate) authorization_url: Option<url::Url>,
    pub(crate) token_url: url::Url,
    pub(crate) client_id: String,
    pub(crate) client_secret: Option<Zeroizing<String>>,
    pub(crate) scope: Option<String>,
}

pub(crate) enum PreparedAuth {
    None,
    Header(HeaderName, HeaderValue),
    Query(String, String),
    ClientCredentials(OauthConfig),
    AuthCode(OauthConfig),
}

fn auth_value(text: &str) -> Result<HeaderValue, ApiError> {
    let value = validate::field_value(text).map_err(|r| ApiError::input("auth", r))?;
    HeaderValue::from_str(value).map_err(|_| ApiError::input("auth", "control_character"))
}

fn oauth_url(text: &str) -> Result<url::Url, ApiError> {
    validate::url(text).map_err(|_| ApiError::input("auth", "oauth_url"))
}

/// 활성 인증을 검사하고 가림기에 넣을 직접 입력 값(비밀번호·토큰·키 값·client secret)을 돌려준다.
pub(crate) fn prepare(auth: &Auth) -> Result<(PreparedAuth, Vec<String>), ApiError> {
    Ok(match auth {
        Auth::None => (PreparedAuth::None, vec![]),
        Auth::Basic { username, password } => {
            let pair = format!("{username}:{password}");
            let encoded = data_encoding::BASE64.encode(pair.as_bytes());
            let value = auth_value(&format!("Basic {encoded}"))?;
            (
                PreparedAuth::Header(reqwest::header::AUTHORIZATION, value),
                vec![password.clone(), pair, encoded],
            )
        }
        Auth::Bearer { token } => (
            PreparedAuth::Header(
                reqwest::header::AUTHORIZATION,
                auth_value(&format!("Bearer {token}"))?,
            ),
            vec![token.clone()],
        ),
        Auth::ApiKey {
            name,
            value,
            placement,
        } => {
            if !validate::is_token(name) {
                return Err(ApiError::input("auth", "invalid_token"));
            }
            let prepared = match placement {
                KeyPlacement::Header => PreparedAuth::Header(
                    HeaderName::from_bytes(name.as_bytes())
                        .map_err(|_| ApiError::input("auth", "invalid_token"))?,
                    auth_value(value)?,
                ),
                KeyPlacement::Query => PreparedAuth::Query(name.clone(), value.clone()),
            };
            (prepared, vec![value.clone()])
        }
        Auth::Oauth2ClientCredentials {
            token_url,
            client_id,
            client_secret,
            scope,
        } => (
            PreparedAuth::ClientCredentials(OauthConfig {
                authorization_url: None,
                token_url: oauth_url(token_url)?,
                client_id: client_id.clone(),
                client_secret: Some(Zeroizing::new(client_secret.clone())),
                scope: scope.clone().filter(|s| !s.is_empty()),
            }),
            vec![client_secret.clone()],
        ),
        Auth::Oauth2AuthCode {
            authorization_url,
            token_url,
            client_id,
            client_secret,
            scope,
        } => (
            PreparedAuth::AuthCode(OauthConfig {
                authorization_url: Some(oauth_url(authorization_url)?),
                token_url: oauth_url(token_url)?,
                client_id: client_id.clone(),
                client_secret: client_secret.clone().map(Zeroizing::new),
                scope: scope.clone().filter(|s| !s.is_empty()),
            }),
            client_secret.iter().cloned().collect(),
        ),
    })
}

struct Cached {
    token: Zeroizing<String>,
    expires: Option<Instant>,
}

/// `(tokenUrl, clientId, scope, secret digest)`로 찾는 메모리 캐시. 만료 30초 전에 갱신한다.
#[derive(Default)]
pub(crate) struct TokenCache {
    tokens: Mutex<HashMap<String, Cached>>,
}

impl TokenCache {
    pub(crate) fn key(grant: &str, c: &OauthConfig) -> String {
        let mut h = Sha256::new();
        for part in [
            grant,
            c.token_url.as_str(),
            &c.client_id,
            c.scope.as_deref().unwrap_or(""),
            c.client_secret.as_deref().map(|s| s.as_str()).unwrap_or(""),
        ] {
            h.update(part.as_bytes());
            h.update([0]);
        }
        data_encoding::HEXLOWER.encode(&h.finalize())
    }
    fn get(&self, key: &str) -> Option<Zeroizing<String>> {
        let tokens = self.tokens.lock().ok()?;
        let cached = tokens.get(key)?;
        let fresh = cached
            .expires
            .is_none_or(|at| at > Instant::now() + Duration::from_secs(30));
        fresh.then(|| cached.token.clone())
    }
    fn put(&self, key: String, token: Zeroizing<String>, expires_in: Option<u64>) {
        if let Ok(mut tokens) = self.tokens.lock() {
            let expires = expires_in.map(|s| Instant::now() + Duration::from_secs(s));
            tokens.insert(key, Cached { token, expires });
        }
    }
    /// 401을 받으면 다음 송신에서 새 토큰을 받는다.
    pub(crate) fn expire(&self, key: &str) {
        if let Ok(mut tokens) = self.tokens.lock() {
            tokens.remove(key);
        }
    }
}

#[derive(serde::Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: Option<u64>,
}

pub(crate) async fn exchange(
    client: &reqwest::Client,
    config: &OauthConfig,
    form: Vec<(&str, String)>,
    deadline: tokio::time::Instant,
) -> Result<(Zeroizing<String>, Option<u64>), ApiError> {
    let mut fields = form;
    fields.push(("client_id", config.client_id.clone()));
    if let Some(secret) = &config.client_secret {
        fields.push(("client_secret", secret.to_string()));
    }
    let body = Zeroizing::new(
        url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(fields.iter().map(|(k, v)| (*k, v.as_str())))
            .finish(),
    );
    let request = client
        .post(config.token_url.clone())
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .header(reqwest::header::ACCEPT, "application/json")
        .body(body.to_string())
        .send();
    let response = tokio::time::timeout_at(deadline, request)
        .await
        .map_err(|_| ApiError::oauth("token_timeout"))?
        .map_err(|_| ApiError::oauth("token_request"))?;
    if !response.status().is_success() {
        return Err(ApiError::oauth("token_status"));
    }
    let bytes = bounded_body(response, deadline).await?;
    let parsed: TokenResponse =
        serde_json::from_slice(&bytes).map_err(|_| ApiError::oauth("token_response"))?;
    let token = Zeroizing::new(parsed.access_token);
    if token.is_empty() || validate::field_value(&token).is_err() {
        return Err(ApiError::oauth("token_response"));
    }
    Ok((token, parsed.expires_in))
}

/// 토큰 응답 본문 상한. 넘으면 읽기를 멈추고 실패한다(메모리·시간 보호).
const TOKEN_RESPONSE_MAX: usize = 64 * 1024;

async fn bounded_body(
    mut response: reqwest::Response,
    deadline: tokio::time::Instant,
) -> Result<Zeroizing<Vec<u8>>, ApiError> {
    if response
        .content_length()
        .is_some_and(|n| n > TOKEN_RESPONSE_MAX as u64)
    {
        return Err(ApiError::oauth("token_too_large"));
    }
    let mut bytes = Zeroizing::new(Vec::new());
    loop {
        let chunk = tokio::time::timeout_at(deadline, response.chunk())
            .await
            .map_err(|_| ApiError::oauth("token_timeout"))?
            .map_err(|_| ApiError::oauth("token_request"))?;
        let Some(chunk) = chunk else {
            return Ok(bytes);
        };
        if bytes.len() + chunk.len() > TOKEN_RESPONSE_MAX {
            return Err(ApiError::oauth("token_too_large"));
        }
        bytes.extend_from_slice(&chunk);
    }
}

/// OAuth 토큰을 얻는다(캐시 → client credentials 또는 브라우저 인증). `(토큰, 캐시 키)`.
pub(crate) async fn token(
    auth: &PreparedAuth,
    cache: &TokenCache,
    client: &reqwest::Client,
    events: &Events<'_>,
    deadline: tokio::time::Instant,
) -> Result<Option<(Zeroizing<String>, String)>, ApiError> {
    let (grant, config) = match auth {
        PreparedAuth::ClientCredentials(c) => ("client_credentials", c),
        PreparedAuth::AuthCode(c) => ("authorization_code", c),
        _ => return Ok(None),
    };
    let key = TokenCache::key(grant, config);
    if let Some(token) = cache.get(&key) {
        return Ok(Some((token, key)));
    }
    let (token, expires_in) = match auth {
        PreparedAuth::ClientCredentials(_) => {
            let mut form = vec![("grant_type", "client_credentials".to_string())];
            if let Some(scope) = &config.scope {
                form.push(("scope", scope.clone()));
            }
            exchange(client, config, form, deadline).await?
        }
        _ => crate::oauth_loopback::authorize(client, config, events).await?,
    };
    cache.put(key.clone(), token.clone(), expires_in);
    Ok(Some((token, key)))
}
