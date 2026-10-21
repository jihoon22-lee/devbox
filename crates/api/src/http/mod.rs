//! 송신: 리디렉션을 직접 따라가며(최대 10회) 모든 hop과 본문이 시한 하나를 공유한다.
pub(crate) mod body;
pub(crate) mod build;
pub(crate) mod redirect;
pub(crate) mod stream;

use crate::auth::{self, PreparedAuth, TokenCache};
use crate::cookies::{self, Jar};
use crate::events::Events;
use crate::redact::Redactor;
use crate::validate::{MAX_REDIRECTS, is_sensitive_name};
use crate::{ApiError, Redirect};
use build::Prepared;
use reqwest::header::{AUTHORIZATION, COOKIE, HeaderMap, HeaderValue, LOCATION};
use stream::{Captured, Stop};

/// 가림 대상 값과 그 가림기. OAuth 토큰을 받으면 늘어난다.
pub(crate) struct Redaction {
    pub(crate) values: Vec<String>,
    pub(crate) redactor: Redactor,
}
impl Redaction {
    pub(crate) fn new(values: Vec<String>) -> Self {
        let redactor = Redactor::new(&values);
        Self { values, redactor }
    }
    fn add(&mut self, value: &str) {
        self.values.push(value.to_owned());
        self.redactor = Redactor::new(&self.values);
    }
}

#[derive(Debug)]
pub(crate) struct DnsFailed;
impl std::fmt::Display for DnsFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("name resolution failed")
    }
}
impl std::error::Error for DnsFailed {}

/// `io::Error::source()`는 감싼 오류를 건너뛰므로 `get_ref()`로 내려가며 rustls 오류를 찾는다.
fn is_tls(mut e: &(dyn std::error::Error + 'static)) -> bool {
    loop {
        if e.is::<rustls::Error>() {
            return true;
        }
        match e
            .downcast_ref::<std::io::Error>()
            .and_then(|io| io.get_ref())
        {
            Some(inner) => e = inner,
            None => return false,
        }
    }
}

/// reqwest 오류를 고정 종류로만 옮긴다(오류 문장에는 URL이 있으므로 쓰지 않는다).
pub(crate) fn kind(error: &reqwest::Error) -> ApiError {
    if error.is_timeout() {
        return ApiError::network("timeout");
    }
    let mut source: Option<&(dyn std::error::Error + 'static)> = std::error::Error::source(error);
    while let Some(e) = source {
        if e.is::<DnsFailed>() || e.to_string().starts_with("dns error") {
            return ApiError::network("dns");
        }
        if is_tls(e) {
            return ApiError::network("tls");
        }
        source = e.source();
    }
    if error.is_connect() {
        ApiError::network("connect")
    } else {
        ApiError::network("protocol")
    }
}

fn blocked() -> Stop {
    Stop::Fail(ApiError::network("redirect"))
}

pub(crate) struct Exec<'a> {
    pub(crate) prepared: &'a Prepared,
    pub(crate) client: &'a reqwest::Client,
    pub(crate) jar: Option<&'a Jar>,
    pub(crate) tokens: &'a TokenCache,
    pub(crate) events: &'a Events<'a>,
}

pub(crate) async fn execute(
    x: Exec<'_>,
    redaction: &mut Redaction,
    out: &mut Captured,
) -> Result<(), Stop> {
    let p = x.prepared;
    let started = std::time::Instant::now();
    let token_deadline = tokio::time::Instant::now() + p.timeout;
    let bearer = auth::token(&p.auth, x.tokens, x.client, x.events, token_deadline)
        .await
        .map_err(Stop::Fail)?;
    // 브라우저 인증을 기다린 시간은 빼고, 모든 hop과 본문이 시한 하나를 공유한다.
    let deadline = tokio::time::Instant::now() + p.timeout;
    if let Some((token, _)) = &bearer {
        redaction.add(token);
    }
    let bearer_value = bearer
        .as_ref()
        .map(|(t, _)| HeaderValue::from_str(&format!("Bearer {}", t.as_str())))
        .transpose()
        .map_err(|_| Stop::Fail(ApiError::oauth("token_response")))?;
    let (mut method, mut url) = (p.method.clone(), p.url.clone());
    let (mut allow_sensitive, mut include_body) = (true, true);
    for hop in 0..=MAX_REDIRECTS {
        let redactor = &redaction.redactor;
        let mut map = HeaderMap::new();
        for (name, value) in &p.headers {
            let secret = !allow_sensitive
                && (!redirect::should_send_header(name.as_str(), false)
                    || redactor.contains(&String::from_utf8_lossy(value.as_bytes())));
            if secret || !include_body && redirect::is_body_header(name.as_str()) {
                continue;
            }
            map.append(name.clone(), value.clone());
        }
        if allow_sensitive {
            if let PreparedAuth::Header(name, value) = &p.auth {
                map.insert(name.clone(), value.clone());
            }
            if let Some(value) = &bearer_value {
                map.insert(AUTHORIZATION, value.clone());
            }
        }
        let jar_values = x.jar.map(|j| j.values(&url)).unwrap_or_default();
        let rows: &[(String, String)] = if allow_sensitive { &p.cookies } else { &[] };
        if (!p.user_cookie_header || !allow_sensitive)
            && let Some(value) = cookies::header(rows, &jar_values)
            && let Ok(value) = HeaderValue::from_str(&value)
        {
            map.insert(COOKIE, value);
        }
        let mut builder = x.client.request(method.clone(), url.clone()).headers(map);
        if include_body {
            builder = body::apply(builder, &p.body, p.user_content_type)
                .await
                .map_err(Stop::Fail)?;
        }
        let response = tokio::time::timeout_at(deadline, builder.send())
            .await
            .map_err(|_| Stop::Fail(ApiError::network("timeout")))?
            .map_err(|e| Stop::Fail(kind(&e)))?;
        if let Some(jar) = x.jar {
            jar.store(&url, response.headers());
        }
        let status = response.status().as_u16();
        let location = response.headers().get(LOCATION).cloned();
        if p.follow
            && redirect::is_redirect(status)
            && let Some(location) = location
        {
            if hop == MAX_REDIRECTS {
                return Err(blocked());
            }
            let location = location.to_str().map_err(|_| blocked())?;
            let mut next = url.join(location).map_err(|_| blocked())?;
            next.set_fragment(None);
            if !matches!(next.scheme(), "http" | "https") {
                return Err(blocked());
            }
            let cross = redirect::is_cross_origin(&url, &next);
            if cross && p.tls.has_identity() {
                return Err(blocked());
            }
            if cross {
                allow_sensitive = false;
                include_body = false;
            }
            // 화면에는 서버가 준 위치만 보인다(같은 origin GET으로 다시 붙인 GraphQL query 제외).
            let shown = redaction.redactor.text(next.as_str());
            if !allow_sensitive && shown != next.as_str() {
                return Err(blocked());
            }
            if redirect::switches_to_get(status, &method) {
                method = reqwest::Method::GET;
                include_body = false;
                if allow_sensitive && let Some(g) = p.graphql.as_ref().filter(|g| g.post) {
                    next.query_pairs_mut().extend_pairs(g.params.iter());
                }
            }
            if p.graphql.is_some()
                && next
                    .query_pairs()
                    .any(|(n, _)| is_sensitive_name(&n) && cross)
            {
                return Err(blocked());
            }
            out.redirects.push(Redirect {
                status,
                location: shown,
            });
            url = next;
            continue;
        }
        if status == 401
            && allow_sensitive
            && let Some((_, key)) = &bearer
        {
            x.tokens.expire(key);
        }
        return stream::relay(
            response,
            deadline,
            started,
            &redaction.redactor,
            x.events,
            out,
        )
        .await;
    }
    Err(blocked())
}
