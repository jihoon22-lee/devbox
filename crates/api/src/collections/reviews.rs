//! 프로젝트 컬렉션 파일의 "비밀 재검토" 판정(01 §6.3·§12, 07b 에이전트 결정 ⑤).
//! 저장소 내용이 비밀·로컬 파일·명령을 끌어내지 못하게, 판정은 보수적으로(fail-closed) 한다.
//! 화면 파일 형식(3c `requestFile.ts`)이 맞춰야 하는 키:
//! - 모든 문자열 값·키(JSON 이스케이프를 푼 뒤)에 `{{secret:`가 있으면 검토 필요
//! - 객체의 `kind`가 `"file"`이거나 `file_path`·`filePath` 값이 빈 문자열이 아니면(multipart 파일 부분) 검토 필요
//! - 객체의 `transport`·`kind`가 `"stdio"`이거나 `command` 값이 빈 문자열이 아니면(MCP stdio 명령) 검토 필요
//! - 객체의 `kind`가 `"binary"`이면서 `path`가 빈 문자열이 아니면(binary 본문 파일) 검토 필요
//! - `caPath`·`clientCertPath`·`clientKeyPath`(또는 `ca_path`·`client_cert_path`·`client_key_path`) 값이
//!   빈 문자열이 아니면(TLS 인증서·키 파일) 검토 필요
//! - UTF-8이 아니거나 JSON으로 읽히지 않거나 너무 깊으면 검토 필요
use serde_json::Value;

const SECRET: &str = "{{secret:";
const MAX_DEPTH: usize = 64;
const TLS_PATHS: [&str; 6] = [
    "caPath",
    "clientCertPath",
    "clientKeyPath",
    "ca_path",
    "client_cert_path",
    "client_key_path",
];

pub(crate) fn needs_review(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return true;
    };
    if text.contains(SECRET) {
        return true;
    }
    match serde_json::from_str::<Value>(text) {
        Ok(value) => risky(&value, 0),
        Err(_) => true,
    }
}

fn non_empty(value: Option<&Value>) -> bool {
    value.and_then(Value::as_str).is_some_and(|s| !s.is_empty())
}

fn risky(value: &Value, depth: usize) -> bool {
    if depth > MAX_DEPTH {
        return true;
    }
    match value {
        Value::String(s) => s.contains(SECRET),
        Value::Array(items) => items.iter().any(|v| risky(v, depth + 1)),
        Value::Object(map) => {
            let tag = |key: &str| map.get(key).and_then(Value::as_str);
            if tag("kind") == Some("file")
                || tag("kind") == Some("stdio")
                || tag("transport") == Some("stdio")
                || non_empty(map.get("file_path"))
                || non_empty(map.get("filePath"))
                || non_empty(map.get("command"))
                || (tag("kind") == Some("binary") && non_empty(map.get("path")))
                || TLS_PATHS.iter().any(|key| non_empty(map.get(*key)))
            {
                return true;
            }
            map.iter()
                .any(|(k, v)| k.contains(SECRET) || risky(v, depth + 1))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::needs_review;

    #[test]
    fn plain_requests_need_no_review_and_risky_shapes_do() {
        assert!(!needs_review(
            br#"{"version":1,"request":{"url":"https://x.test","headers":[]}}"#
        ));
        assert!(!needs_review(
            br#"{"parts":[{"kind":"text","name":"a","value":"b"}],"file_path":""}"#
        ));
        assert!(!needs_review(
            br#"{"body":{"kind":"binary","path":""},"tls":{"caPath":null,"verify":true}}"#
        ));
        for risky in [
            r#"{"h":"Bearer {{secret:T}}"}"#,
            r#"{"h":"\u007b\u007bsecret:T}}"}"#,
            r#"{"parts":[{"kind":"file","name":"a"}]}"#,
            r#"{"parts":[{"filePath":"/home/u/x"}]}"#,
            r#"{"server":{"transport":"stdio"}}"#,
            r#"{"server":{"command":"node"}}"#,
            // 리뷰 3b-B2: binary 본문 경로와 TLS 경로도 로컬 파일을 읽는다.
            r#"{"request":{"body":{"kind":"binary","path":"/home/u/.ssh/id_ed25519"}}}"#,
            r#"{"request":{"tls":{"caPath":"/etc/x.pem"}}}"#,
            r#"{"request":{"tls":{"clientCertPath":"/home/u/c.pem"}}}"#,
            r#"{"request":{"tls":{"clientKeyPath":"/home/u/k.pem"}}}"#,
            r#"{"request":{"tls":{"client_key_path":"/home/u/k.pem"}}}"#,
            r#"{"request":{"tls":{"ca_path":"/etc/x.pem"}}}"#,
            r#"{"request":{"tls":{"client_cert_path":"/home/u/c.pem"}}}"#,
            "{broken",
        ] {
            assert!(needs_review(risky.as_bytes()), "{risky}");
        }
        assert!(needs_review(&[0xff, 0xfe]));
        let deep = format!("{}1{}", "[".repeat(80), "]".repeat(80));
        assert!(needs_review(deep.as_bytes()));
    }
}
