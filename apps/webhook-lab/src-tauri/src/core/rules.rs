//! response rule (순수). method+path 매치 → 고정 status/header/body 응답.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResponseRule {
    pub id: String,
    /// None이면 모든 method
    pub method: Option<String>,
    pub path: String,
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
    /// 응답 지연 (ms)
    pub delay_ms: u64,
}

/// rule이 요청과 매치하는지. method는 대소문자 무시.
pub fn matches(rule: &ResponseRule, method: &str, path: &str) -> bool {
    if let Some(m) = &rule.method {
        if !m.eq_ignore_ascii_case(method) {
            return false;
        }
    }
    rule.path == path
        || (rule.path.ends_with('*') && path.starts_with(&rule.path[..rule.path.len() - 1]))
}

/// 새 규칙에는 실제 ID를 부여하고 기존 규칙은 같은 ID로 교체한다.
/// 반환한 ID와 저장된 rule.id가 항상 같아야 context-menu 대상이 안정적이다.
pub fn upsert(rules: &mut HashMap<String, ResponseRule>, mut rule: ResponseRule) -> String {
    if rule.id.is_empty() {
        rule.id = uuid::Uuid::new_v4().to_string();
    }
    let id = rule.id.clone();
    rules.insert(id.clone(), rule);
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(id: &str, method: Option<&str>, path: &str) -> ResponseRule {
        ResponseRule {
            id: id.into(),
            method: method.map(|s| s.into()),
            path: path.into(),
            status: 200,
            headers: vec![],
            body: String::new(),
            delay_ms: 0,
        }
    }

    #[test]
    fn matches_exact_and_wildcard() {
        assert!(matches(&rule("r1", Some("POST"), "/hook"), "POST", "/hook"));
        assert!(!matches(&rule("r1", Some("GET"), "/hook"), "POST", "/hook"));
        assert!(matches(&rule("r2", None, "/any"), "DELETE", "/any"));
        assert!(matches(
            &rule("r3", None, "/events/*"),
            "POST",
            "/events/123"
        ));
        assert!(!matches(&rule("r3", None, "/events/*"), "POST", "/other"));
    }

    #[test]
    fn upsert_assigns_and_preserves_rule_identity() {
        let mut rules = HashMap::new();
        let generated = upsert(&mut rules, rule("", Some("POST"), "/hook"));
        assert!(!generated.is_empty());
        assert_eq!(rules[&generated].id, generated);

        let same = upsert(&mut rules, rule(&generated, Some("GET"), "/updated"));
        assert_eq!(same, generated);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[&generated].path, "/updated");
    }
}
