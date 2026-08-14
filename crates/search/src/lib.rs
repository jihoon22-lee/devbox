//! FTS5 쿼리 빌드 프리미티브.
//!
//! 추출 근거: `build_fts_query`가 everything-plus와 knowledge-base에 동일하게
//! 두 벌 존재했다. 두 구현을 diff로 대조한 결과 **IDENTICAL**이라 한 벌만 남긴다.
//! (채택 근거: 완전히 같은 로직)
//!
//! 스키마 DDL은 이 크레이트가 소유하지 않는다 — 두 앱의 테이블 구조가 다르므로
//! (files_fts(name/content) vs docs_fts(title/body)) 공통화하면 각 앱의 스키마
//! 진화를 막는다.

/// 사용자 입력을 FTS5 MATCH 식으로 변환한다.
///
/// 토큰 단위 prefix 매치이며, FTS5 특수문자는 인용부호로 감싸 무력화한다.
/// `"` 문자는 FTS5 문자열 리터럴 안에서 `""`로 이스케이프한다.
pub fn build_fts_query(input: &str) -> String {
    input
        .split_whitespace()
        .map(|tok| {
            let escaped = tok.replace('"', "\"\"");
            format!("\"{escaped}\"*")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_on_whitespace() {
        assert_eq!(build_fts_query("foo bar"), "\"foo\"* \"bar\"*");
    }

    #[test]
    fn collapses_multiple_spaces() {
        assert_eq!(build_fts_query("foo   bar"), "\"foo\"* \"bar\"*");
    }

    #[test]
    fn escapes_double_quotes() {
        assert_eq!(build_fts_query("my \"file\""), "\"my\"* \"\"\"file\"\"\"*");
    }

    #[test]
    fn handles_empty_input() {
        assert_eq!(build_fts_query(""), "");
        assert_eq!(build_fts_query("   "), "");
    }

    #[test]
    fn prefixes_every_token() {
        assert_eq!(build_fts_query("abc"), "\"abc\"*");
    }

    #[test]
    fn fts5_special_chars_are_quoted_away() {
        // FTS5 특수문자는 인용 안에서 무력화된다
        let q = build_fts_query("a-b");
        assert!(q.starts_with("\""), "{q}");
        let q2 = build_fts_query("*x");
        assert!(q2.starts_with("\""), "{q2}");
        let q3 = build_fts_query("foo:bar");
        assert!(q3.contains("\"foo:bar\"*"), "{q3}");
    }
}
