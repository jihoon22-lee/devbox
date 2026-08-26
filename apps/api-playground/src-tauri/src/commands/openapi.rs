use reqwest::{header, StatusCode, Url};
use serde::Serialize;
use std::time::{Duration, Instant};

const MAX_OPENAPI_BYTES: usize = 4 * 1024 * 1024;
const MAX_OPENAPI_URL_LENGTH: usize = 2_048;
const MAX_REDIRECTS: usize = 3;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(15);

const INVALID_URL: &str = "OpenAPI URL이 올바르지 않습니다";
const UNSAFE_URL: &str =
    "인증정보나 민감 query, fragment가 없는 HTTP(S) OpenAPI URL만 사용할 수 있습니다";
const NETWORK_ERROR: &str = "OpenAPI URL에 연결하지 못했습니다";
const STATUS_ERROR: &str = "OpenAPI URL이 성공 응답을 반환하지 않았습니다";
const REDIRECT_ERROR: &str = "OpenAPI URL의 안전한 redirect 범위를 벗어났습니다";
const SIZE_ERROR: &str = "OpenAPI 문서는 4 MiB 이하만 가져올 수 있습니다";
const BODY_ERROR: &str = "OpenAPI 문서를 안전하게 읽지 못했습니다";
const TIMEOUT_ERROR: &str = "OpenAPI URL 가져오기 시간이 초과되었습니다";

fn is_sensitive_name(value: &str) -> bool {
    let normalized = value
        .chars()
        .filter(|character| !matches!(character, '-' | '_' | '.'))
        .flat_map(char::to_lowercase)
        .collect::<String>();
    let generic_key = normalized == "key";
    generic_key
        || [
            "authorization",
            "cookie",
            "setcookie",
            "apikey",
            "accesskey",
            "accesstoken",
            "clientkey",
            "clientsecret",
            "refreshtoken",
            "token",
            "secret",
            "password",
            "passwd",
            "credential",
            "privatekey",
        ]
        .iter()
        .any(|needle| normalized.contains(needle))
}

fn looks_like_credential(value: &str) -> bool {
    let known_prefix = [
        "sk-",
        "ghp_",
        "github_pat_",
        "glpat-",
        "xoxb-",
        "xoxa-",
        "xoxp-",
        "xoxr-",
        "xoxs-",
    ]
    .iter()
    .any(|prefix| value.starts_with(prefix) && value.len() >= prefix.len() + 12);
    let aws_access_key = value.starts_with("AKIA")
        && value.len() == 20
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit());
    let jwt = {
        let parts = value.split('.').collect::<Vec<_>>();
        parts.len() == 3
            && parts.iter().all(|part| {
                part.len() >= 10
                    && part
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            })
    };
    let auth_header = value
        .split_once(char::is_whitespace)
        .is_some_and(|(scheme, remainder)| {
            matches!(scheme.to_ascii_lowercase().as_str(), "bearer" | "basic")
                && !remainder.trim().is_empty()
        });
    known_prefix || aws_access_key || jwt || auth_header
}

// The raw document may itself contain credential examples. Keep it serializable
// for the one requested IPC response, but deliberately do not make it Debug.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteOpenApiSource {
    text: String,
    format: &'static str,
}

fn validate_url(value: &str) -> Result<Url, String> {
    if value.is_empty()
        || value.len() > MAX_OPENAPI_URL_LENGTH
        || value.chars().any(char::is_whitespace)
    {
        return Err(INVALID_URL.to_string());
    }
    let parsed = Url::parse(value).map_err(|_| INVALID_URL.to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(INVALID_URL.to_string());
    }
    let unsafe_query = parsed
        .query_pairs()
        .any(|(key, value)| is_sensitive_name(&key) || looks_like_credential(&value));
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || unsafe_query
        || parsed.fragment().is_some()
    {
        return Err(UNSAFE_URL.to_string());
    }
    Ok(parsed)
}

fn redirect_allowed(from: &Url, to: &Url) -> bool {
    let same_host = from
        .host_str()
        .zip(to.host_str())
        .is_some_and(|(left, right)| left.eq_ignore_ascii_case(right));
    let from_port = from.port_or_known_default();
    let to_port = to.port_or_known_default();
    let same_port = from_port == to_port
        || from.scheme() == "http"
            && to.scheme() == "https"
            && from_port == Some(80)
            && to_port == Some(443);
    let safe_scheme =
        from.scheme() == to.scheme() || from.scheme() == "http" && to.scheme() == "https";
    same_host && same_port && safe_scheme
}

fn response_format(content_type: Option<&header::HeaderValue>, url: &Url) -> &'static str {
    let json_content = content_type
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| {
            let value = value.trim();
            value.eq_ignore_ascii_case("application/json")
                || value.to_ascii_lowercase().ends_with("+json")
        });
    if json_content || url.path().to_ascii_lowercase().ends_with(".json") {
        "json"
    } else {
        "yaml"
    }
}

fn remaining_timeout(started: Instant) -> Result<Duration, String> {
    TOTAL_TIMEOUT
        .checked_sub(started.elapsed())
        .ok_or_else(|| TIMEOUT_ERROR.to_string())
}

async fn fetch_openapi_source_impl(value: &str) -> Result<RemoteOpenApiSource, String> {
    let mut current = validate_url(value)?;
    let client = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(TOTAL_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| NETWORK_ERROR.to_string())?;
    let started = Instant::now();

    for redirect_count in 0..=MAX_REDIRECTS {
        let remaining = remaining_timeout(started)?;
        let mut response = client
            .get(current.clone())
            .timeout(remaining)
            .header(
                header::ACCEPT,
                "application/json, application/yaml, text/yaml, */*;q=0.1",
            )
            .header(header::USER_AGENT, "devbox-api-playground/openapi-import")
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    TIMEOUT_ERROR.to_string()
                } else {
                    NETWORK_ERROR.to_string()
                }
            })?;
        let status = response.status();
        if status.is_redirection() {
            if redirect_count == MAX_REDIRECTS {
                return Err(REDIRECT_ERROR.to_string());
            }
            let location = response
                .headers()
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| REDIRECT_ERROR.to_string())?;
            let next = current
                .join(location)
                .map_err(|_| REDIRECT_ERROR.to_string())?;
            let next = validate_url(next.as_str()).map_err(|_| REDIRECT_ERROR.to_string())?;
            if !redirect_allowed(&current, &next) {
                return Err(REDIRECT_ERROR.to_string());
            }
            current = next;
            continue;
        }
        if status < StatusCode::OK || status >= StatusCode::MULTIPLE_CHOICES {
            return Err(STATUS_ERROR.to_string());
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_OPENAPI_BYTES as u64)
        {
            return Err(SIZE_ERROR.to_string());
        }
        let format = response_format(response.headers().get(header::CONTENT_TYPE), &current);
        let mut body = Vec::new();
        loop {
            let chunk = tokio::time::timeout(remaining_timeout(started)?, response.chunk())
                .await
                .map_err(|_| TIMEOUT_ERROR.to_string())?
                .map_err(|error| {
                    if error.is_timeout() {
                        TIMEOUT_ERROR.to_string()
                    } else {
                        BODY_ERROR.to_string()
                    }
                })?;
            let Some(chunk) = chunk else { break };
            if body.len().saturating_add(chunk.len()) > MAX_OPENAPI_BYTES {
                return Err(SIZE_ERROR.to_string());
            }
            body.extend_from_slice(&chunk);
        }
        if started.elapsed() > TOTAL_TIMEOUT {
            return Err(TIMEOUT_ERROR.to_string());
        }
        let text = String::from_utf8(body).map_err(|_| BODY_ERROR.to_string())?;
        return Ok(RemoteOpenApiSource { text, format });
    }
    Err(REDIRECT_ERROR.to_string())
}

#[tauri::command]
pub async fn fetch_openapi_source(url: String) -> Result<RemoteOpenApiSource, String> {
    fetch_openapi_source_impl(url.trim()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn serve_once(response: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 2_048];
            let _ = stream.read(&mut request);
            stream.write_all(&response).unwrap();
        });
        format!("http://{address}/openapi.json")
    }

    #[test]
    fn url_validation_rejects_credentials_query_fragment_and_unsafe_schemes() {
        for value in [
            "file:///tmp/openapi.json",
            "https://user:pass@example.test/openapi.json",
            "https://example.test/openapi.json?token=secret",
            "https://example.test/openapi.json?key=value",
            "https://example.test/openapi.json?value=sk-abcdefghijklmnop",
            "https://example.test/openapi.json?value=Bearer%20opaque-token",
            "https://example.test/openapi.json#fragment",
            "https://example.test/open api.json",
        ] {
            assert!(validate_url(value).is_err(), "accepted {value}");
        }
        assert!(validate_url("https://example.test/openapi.json").is_ok());
        assert!(validate_url("https://example.test/openapi.json?format=json").is_ok());
    }

    #[test]
    fn fetches_a_bounded_utf8_document_without_reflecting_the_url() {
        let body = br#"{"openapi":"3.1.0","info":{"title":"x","version":"1"},"paths":{}}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            String::from_utf8_lossy(body)
        );
        let url = serve_once(response.into_bytes());
        let result = tauri::async_runtime::block_on(fetch_openapi_source_impl(&url)).unwrap();
        assert_eq!(result.format, "json");
        assert_eq!(result.text.as_bytes(), body);
    }

    #[test]
    fn rejects_oversized_content_length_before_reading_the_body() {
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            MAX_OPENAPI_BYTES + 1
        );
        let url = serve_once(response.into_bytes());
        let error = match tauri::async_runtime::block_on(fetch_openapi_source_impl(&url)) {
            Ok(_) => panic!("oversized source unexpectedly succeeded"),
            Err(error) => error,
        };
        assert_eq!(error, SIZE_ERROR);
        assert!(!error.contains(&url));
    }

    #[test]
    fn only_allows_same_host_redirects_without_https_downgrade() {
        let from = Url::parse("http://example.test/openapi.yaml").unwrap();
        assert!(redirect_allowed(
            &from,
            &Url::parse("https://example.test/spec.yaml").unwrap()
        ));
        assert!(!redirect_allowed(
            &Url::parse("https://example.test/openapi.yaml").unwrap(),
            &Url::parse("http://example.test/spec.yaml").unwrap()
        ));
        assert!(!redirect_allowed(
            &from,
            &Url::parse("http://other.test/spec.yaml").unwrap()
        ));
        assert!(!redirect_allowed(
            &from,
            &Url::parse("http://example.test:8080/spec.yaml").unwrap()
        ));
        assert!(redirect_allowed(
            &from,
            &Url::parse("https://example.test:443/spec.yaml").unwrap()
        ));
    }
}
