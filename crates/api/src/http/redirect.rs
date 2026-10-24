//! 리디렉션 규칙(v0.9.0 시험 이식): 다른 origin으로 가면 민감 헤더·본문·클라이언트 인증서를 보내지 않는다.

pub(crate) fn is_cross_origin(from: &url::Url, to: &url::Url) -> bool {
    from.scheme() != to.scheme()
        || from.host_str() != to.host_str()
        || from.port_or_known_default() != to.port_or_known_default()
}

/// 다른 origin에 보낼 수 있는 헤더인가(자격 증명처럼 보이는 이름은 같은 origin에서만).
pub(crate) fn should_send_header(name: &str, allow_sensitive: bool) -> bool {
    allow_sensitive || !crate::validate::is_sensitive_name(name)
}

/// 본문과 함께만 의미가 있는 헤더(본문을 떼면 같이 뗀다).
pub(crate) fn is_body_header(name: &str) -> bool {
    let normalized = name.to_ascii_lowercase().replace('_', "-");
    normalized.starts_with("content-")
        || matches!(
            normalized.as_str(),
            "transfer-encoding" | "trailer" | "expect" | "digest" | "repr-digest"
        )
}

/// 303(HEAD 제외)과 POST의 301/302는 GET으로 바꾼다. 307/308은 메서드·본문을 유지한다.
pub(crate) fn switches_to_get(status: u16, method: &reqwest::Method) -> bool {
    status == 303 && *method != reqwest::Method::HEAD
        || matches!(status, 301 | 302) && *method == reqwest::Method::POST
}

pub(crate) fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_origin_redirect_strips_sensitive_headers() {
        let parse = |u: &str| url::Url::parse(u).expect("fixture url");
        let origin = parse("https://a.test/start");
        assert!(!is_cross_origin(&origin, &parse("https://a.test/next")));
        assert!(!is_cross_origin(&origin, &parse("https://a.test:443/next")));
        assert!(is_cross_origin(&origin, &parse("https://b.test/next")));
        assert!(is_cross_origin(&origin, &parse("http://a.test/next")));
        assert!(should_send_header("Authorization", true));
        assert!(!should_send_header("Authorization", false));
        assert!(!should_send_header("Cookie", false));
        assert!(!should_send_header("X-Api-Key", false));
        assert!(should_send_header("Accept", false));
    }

    #[test]
    fn body_headers_are_identified_for_body_suppression() {
        assert!(is_body_header("Content-Length"));
        assert!(is_body_header("content_type"));
        assert!(is_body_header("Transfer-Encoding"));
        assert!(is_body_header("Expect"));
        assert!(!is_body_header("Accept"));
        assert!(!is_body_header("X-Request-Id"));
    }

    #[test]
    fn redirect_method_rules_preserve_307_and_switch_post_302() {
        assert!(switches_to_get(302, &reqwest::Method::POST));
        assert!(switches_to_get(303, &reqwest::Method::PUT));
        assert!(!switches_to_get(307, &reqwest::Method::POST));
        assert!(!switches_to_get(302, &reqwest::Method::PUT));
        assert!(!switches_to_get(303, &reqwest::Method::HEAD));
    }
}
