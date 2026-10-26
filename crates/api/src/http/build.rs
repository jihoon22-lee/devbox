//! 치환·비밀 해석을 마친 snapshot을 송신 가능한 모양으로 검사한다. 여기서 실패하면 연결하지 않는다.
use super::body::{self, Graphql, PreparedBody};
use crate::auth::{self, PreparedAuth};
use crate::tls::{self, PreparedTls};
use crate::validate::{self, MAX_HEADER_BYTES, MAX_ROWS, too_large};
use crate::{ApiError, RequestSnapshot};
use reqwest::header::{HeaderName, HeaderValue};

pub(crate) struct Prepared {
    pub(crate) method: reqwest::Method,
    pub(crate) url: url::Url,
    pub(crate) headers: Vec<(HeaderName, HeaderValue)>,
    pub(crate) user_cookie_header: bool,
    pub(crate) user_content_type: bool,
    pub(crate) cookies: Vec<(String, String)>,
    pub(crate) body: PreparedBody,
    pub(crate) graphql: Option<Graphql>,
    pub(crate) auth: PreparedAuth,
    pub(crate) tls: PreparedTls,
    pub(crate) timeout: std::time::Duration,
    pub(crate) follow: bool,
    /// 가림기에 넣을 직접 입력 인증 값.
    pub(crate) seeds: zeroize::Zeroizing<Vec<String>>,
}

fn headers(s: &RequestSnapshot) -> Result<Vec<(HeaderName, HeaderValue)>, ApiError> {
    if s.headers.len() > MAX_ROWS {
        return Err(too_large(MAX_ROWS));
    }
    let mut out = Vec::with_capacity(s.headers.len());
    let mut total = 0;
    for row in &s.headers {
        let value = validate::header(row.row, &row.name, &row.value)?;
        total += row.name.len() + value.len() + 4;
        let name = HeaderName::from_bytes(row.name.as_bytes())
            .map_err(|_| ApiError::header(row.row, "name", "invalid_token"))?;
        let value = HeaderValue::from_str(&value)
            .map_err(|_| ApiError::header(row.row, "value", "control_character"))?;
        out.push((name, value));
    }
    if total > MAX_HEADER_BYTES {
        return Err(too_large(MAX_HEADER_BYTES));
    }
    Ok(out)
}

fn cookies(s: &RequestSnapshot, user_header: bool) -> Result<Vec<(String, String)>, ApiError> {
    if s.cookies.len() > MAX_ROWS {
        return Err(too_large(MAX_ROWS));
    }
    for row in &s.cookies {
        validate::cookie(row.row, &row.name, &row.value)?;
    }
    if user_header && let Some(first) = s.cookies.first() {
        return Err(ApiError::InvalidCookie {
            index: first.row,
            reason: "duplicate_header".into(),
        });
    }
    Ok(s.cookies
        .iter()
        .map(|p| (p.name.clone(), p.value.clone()))
        .collect())
}

pub(crate) fn prepare(s: &RequestSnapshot) -> Result<Prepared, ApiError> {
    let method = validate::method(&s.method)?;
    let query: Vec<(String, String)> = s
        .query
        .iter()
        .map(|p| (p.name.clone(), p.value.clone()))
        .collect();
    if query.len() > MAX_ROWS {
        return Err(too_large(MAX_ROWS));
    }
    let graphql_body = matches!(s.body, crate::Body::Graphql { .. });
    if graphql_body {
        validate::graphql_endpoint(&s.url, &query)?;
    }
    let mut url = validate::url(&s.url)?;
    let timeout = validate::timeout(s.timeout_ms)?;
    let headers = headers(s)?;
    let has = |name: &str| {
        headers
            .iter()
            .any(|(n, _)| n.as_str().eq_ignore_ascii_case(name))
    };
    let user_cookie_header = has("cookie");
    let cookies = cookies(s, user_cookie_header)?;
    let (body, graphql) = body::prepare(&s.body)?;
    let (auth, seeds) = auth::prepare(&s.auth)?;
    let tls = tls::prepare(&s.tls)?;
    let mut appended = query;
    if let PreparedAuth::Query(name, value) = &auth {
        appended.push((name.clone(), value.clone()));
    }
    if let Some(g) = graphql.as_ref().filter(|g| !g.post) {
        appended.extend(g.params.iter().cloned());
    }
    if !appended.is_empty() {
        url.query_pairs_mut().extend_pairs(appended);
    }
    if url.as_str().len() > validate::MAX_URL_BYTES {
        return Err(ApiError::url("too_long"));
    }
    // 본문이 정하는 헤더를 뺀 뒤에 사용자 Content-Type이 남았는지 본다(GraphQL·multipart는 우리 값).
    let headers: Vec<_> = headers
        .into_iter()
        .filter(|(n, _)| !body::derived(&body, graphql.is_some(), n.as_str()))
        .collect();
    let user_content_type = headers
        .iter()
        .any(|(n, _)| n == reqwest::header::CONTENT_TYPE);
    Ok(Prepared {
        method,
        url,
        headers,
        user_cookie_header,
        user_content_type,
        cookies,
        body,
        graphql,
        auth,
        tls,
        timeout,
        follow: s.follow_redirects,
        seeds: zeroize::Zeroizing::new(seeds),
    })
}
