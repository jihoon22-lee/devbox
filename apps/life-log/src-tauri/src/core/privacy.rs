//! privacy rule (순수 로직). **DB insert 전에** 적용한다 — UI 필터가 아니다.
//!
//! 제외하거나 치환하기로 한 원문은 DB·진단 로그·integration snapshot 어디에도
//! 남지 않아야 한다 (§9.3).

use serde::{Deserialize, Serialize};

/// 수집 제외·치환 규칙.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyRules {
    /// 프로세스 이름이 정확히 일치하면 세션 전체를 저장하지 않는다.
    #[serde(default)]
    pub excluded_processes: Vec<String>,
    /// 이 정규식과 일치하는 제목은 저장하지 않는다 (세션은 기록하되 제목 공란).
    #[serde(default)]
    pub excluded_title_patterns: Vec<String>,
    /// 이 정규식과 일치하는 부분을 `[redacted]`로 치환한다.
    #[serde(default)]
    pub redact_title_patterns: Vec<String>,
    /// 모든 제목을 저장하지 않는다 (제목 공란).
    #[serde(default)]
    pub mask_all_titles: bool,
}

/// 규칙 적용 결과: `None`이면 세션 전체를 저장하지 않는다.
/// `Some((app, title))`이면 저장할 (process, title)이다.
pub fn apply(rules: &PrivacyRules, app: &str, title: &str) -> Option<(String, String)> {
    if rules
        .excluded_processes
        .iter()
        .any(|p| p.eq_ignore_ascii_case(app))
    {
        return None;
    }
    if rules
        .excluded_title_patterns
        .iter()
        .any(|p| regex_matches(p, title))
    {
        return Some((app.to_string(), String::new()));
    }
    let mut out_title = title.to_string();
    for pattern in &rules.redact_title_patterns {
        out_title = regex_replace_all(pattern, &out_title);
    }
    if rules.mask_all_titles {
        out_title = String::new();
    }
    Some((app.to_string(), out_title))
}

fn regex_matches(pattern: &str, text: &str) -> bool {
    regex::Regex::new(pattern).map(|re| re.is_match(text)).unwrap_or(false)
}

fn regex_replace_all(pattern: &str, text: &str) -> String {
    match regex::Regex::new(pattern) {
        Ok(re) => re.replace_all(text, "[redacted]").into_owned(),
        Err(_) => text.to_string(),
    }
}

/// 설정 문자열(JSON) 파싱. 잘못된 JSON이면 기본값(빈 규칙).
pub fn parse_rules(json: &str) -> PrivacyRules {
    serde_json::from_str(json).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(excluded: &[&str], excluded_titles: &[&str], redact: &[&str]) -> PrivacyRules {
        PrivacyRules {
            excluded_processes: excluded.iter().map(|s| s.to_string()).collect(),
            excluded_title_patterns: excluded_titles.iter().map(|s| s.to_string()).collect(),
            redact_title_patterns: redact.iter().map(|s| s.to_string()).collect(),
            mask_all_titles: false,
        }
    }

    #[test]
    fn no_rules_passes_through() {
        let r = PrivacyRules::default();
        assert_eq!(apply(&r, "chrome.exe", "GitHub").unwrap(), ("chrome.exe".to_string(), "GitHub".to_string()));
    }

    #[test]
    fn excluded_process_drops_session() {
        let r = rules(&["lockapp.exe"], &[], &[]);
        assert!(apply(&r, "LockApp.exe", "Lock screen").is_none());
        assert!(apply(&r, "chrome.exe", "x").is_some());
    }

    #[test]
    fn excluded_title_keeps_session_blank_title() {
        let r = rules(&[], &["banking.*"], &[]);
        let out = apply(&r, "chrome.exe", "banking - statement").unwrap();
        assert_eq!(out.1, "");
    }

    #[test]
    fn redact_title_replaces_matches() {
        let r = rules(&[], &[], &["patient[ -]?\\d+"]);
        let out = apply(&r, "hosp.exe", "patient 1234 chart").unwrap();
        assert_eq!(out.1, "[redacted] chart");
    }

    #[test]
    fn mask_all_titles_blanks_title() {
        let r = PrivacyRules { mask_all_titles: true, ..Default::default() };
        let out = apply(&r, "chrome.exe", "any title").unwrap();
        assert_eq!(out.1, "");
    }

    #[test]
    fn invalid_regex_is_safe() {
        let r = rules(&[], &["[unclosed"], &["(bad"]);
        // 잘못된 정규식은 매치 실패(치환 안 함)로 안전하게 처리
        assert_eq!(apply(&r, "a", "b").unwrap().1, "b");
    }

    #[test]
    fn parse_rules_falls_back_on_bad_json() {
        assert_eq!(parse_rules("{not json"), PrivacyRules::default());
        let parsed = parse_rules(r#"{"excludedProcesses":["a"]}"#);
        assert_eq!(parsed.excluded_processes, vec!["a"]);
    }
}
