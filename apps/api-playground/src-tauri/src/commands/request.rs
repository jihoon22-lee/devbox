use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KeyValue {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthConfig {
    pub kind: String,
    pub username: String,
    pub password: String,
    pub token: String,
    pub api_key: String,
    pub api_value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<KeyValue>,
    pub params: Vec<KeyValue>,
    /// none | json | form | raw
    pub body_kind: String,
    pub body: String,
    pub auth: Option<AuthConfig>,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<KeyValue>,
    pub duration_ms: u64,
    pub size_bytes: usize,
    pub body: String,
    pub is_json: bool,
}

/// HTTP 요청을 Rust(reqwest)가 직접 수행한다. 브라우저가 아니므로 CORS 제약이 없다.
#[tauri::command]
pub async fn send_request(req: ApiRequest) -> Result<ApiResponse, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(req.timeout_ms.max(1000)))
        .build()
        .map_err(|e| format!("클라이언트 생성 실패: {e}"))?;

    let method = reqwest::Method::from_bytes(req.method.as_bytes())
        .map_err(|_| format!("잘못된 메서드: {}", req.method))?;

    let url = append_query(&req.url, &req.params);
    let mut builder = client.request(method, &url);

    for kv in &req.headers {
        if !kv.key.is_empty() {
            if let (Ok(name), Ok(value)) = (
                reqwest::header::HeaderName::from_bytes(kv.key.as_bytes()),
                reqwest::header::HeaderValue::from_str(&kv.value),
            ) {
                builder = builder.header(name, value);
            }
        }
    }

    if let Some(auth) = &req.auth {
        match auth.kind.as_str() {
            "basic" => builder = builder.basic_auth(&auth.username, Some(&auth.password)),
            "bearer" => builder = builder.bearer_auth(&auth.token),
            "apikey" => {
                let value = reqwest::header::HeaderValue::from_str(&auth.api_value)
                    .map_err(|e| format!("API key 헤더 값 오류: {e}"))?;
                builder = builder.header(auth.api_key.as_str(), value);
            }
            _ => {}
        }
    }

    match req.body_kind.as_str() {
        "json" => {
            // JSON 검증 후 그대로 전송
            let _: serde_json::Value =
                serde_json::from_str(&req.body).map_err(|e| format!("JSON 본문 오류: {e}"))?;
            builder = builder
                .header("Content-Type", "application/json")
                .body(req.body.clone());
        }
        "form" => {
            let encoded = encode_form(&req.body);
            builder = builder
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body(encoded);
        }
        _ => {
            if !req.body.is_empty() {
                builder = builder.body(req.body.clone());
            }
        }
    }

    let start = Instant::now();
    let resp = builder
        .send()
        .await
        .map_err(|e| format!("요청 실패: {e}"))?;
    let duration_ms = start.elapsed().as_millis() as u64;
    let status = resp.status();
    let headers: Vec<KeyValue> = resp
        .headers()
        .iter()
        .map(|(k, v)| KeyValue {
            key: k.to_string(),
            value: v.to_str().unwrap_or("").to_string(),
        })
        .collect();
    let body = resp
        .text()
        .await
        .map_err(|e| format!("응답 읽기 실패: {e}"))?;
    let is_json = headers
        .iter()
        .any(|h| h.key.eq_ignore_ascii_case("content-type") && h.value.contains("json"));
    let size_bytes = body.len();

    Ok(ApiResponse {
        status: status.as_u16(),
        status_text: status.canonical_reason().unwrap_or("").to_string(),
        headers,
        duration_ms,
        size_bytes,
        body,
        is_json,
    })
}

/// 파라미터를 query string으로 만들어 URL에 붙인다. 기존 ?query는 유지.
fn append_query(url: &str, params: &[KeyValue]) -> String {
    let pairs: Vec<&KeyValue> = params.iter().filter(|kv| !kv.key.is_empty()).collect();
    if pairs.is_empty() {
        return url.to_string();
    }
    let qs = pairs
        .iter()
        .map(|kv| format!("{}={}", kv.key, kv.value))
        .collect::<Vec<_>>()
        .join("&");
    if url.contains('?') {
        format!("{url}&{qs}")
    } else {
        format!("{url}?{qs}")
    }
}

/// `key=value&...` 폼 본문을 URL 인코딩된 폼 문자열로 변환.
fn encode_form(body: &str) -> String {
    body.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            match line.split_once('=') {
                Some((k, v)) => Some(format!("{}={}", k.trim(), v.trim())),
                None => Some(line.to_string()),
            }
        })
        .collect::<Vec<_>>()
        .join("&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_query_handles_existing_query() {
        let url = append_query(
            "https://x.com/api?a=1",
            &[KeyValue {
                key: "b".into(),
                value: "2".into(),
            }],
        );
        assert_eq!(url, "https://x.com/api?a=1&b=2");
    }

    #[test]
    fn append_query_starts_question_mark() {
        let url = append_query(
            "https://x.com/api",
            &[KeyValue {
                key: "b".into(),
                value: "2".into(),
            }],
        );
        assert_eq!(url, "https://x.com/api?b=2");
    }

    #[test]
    fn append_query_skips_empty_keys() {
        let url = append_query(
            "https://x.com",
            &[KeyValue {
                key: "".into(),
                value: "2".into(),
            }],
        );
        assert_eq!(url, "https://x.com");
    }

    #[test]
    fn encode_form_parses_lines() {
        let s = encode_form("# comment\nname=John Doe\nage=30\n");
        assert_eq!(s, "name=John Doe&age=30");
    }

    #[test]
    fn serialize_request_roundtrip() {
        let req = ApiRequest {
            method: "POST".into(),
            url: "https://x.com".into(),
            headers: vec![],
            params: vec![],
            body_kind: "none".into(),
            body: String::new(),
            auth: None,
            timeout_ms: 5000,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: ApiRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.method, "POST");
    }
}
