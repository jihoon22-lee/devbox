//! 수신 요청 기록 (순수, 상한 적용).
//!
//! Authorization·Cookie·API key 헤더는 저장 전에 마스킹한다 (§15.3 안전 경계).

use serde::Serialize;

pub const MAX_HISTORY: usize = 200;
pub const MAX_BODY_CHARS: usize = 256_000;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RequestRecord {
    pub id: u64,
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub received_at_ms: i64,
}

/// 민감 헤더 이름 (대소문자 무시).
const SENSITIVE_HEADERS: &[&str] = &["authorization", "cookie", "x-api-key", "api-key"];

pub fn mask_header(name: &str, value: &str) -> String {
    let lower = name.to_lowercase();
    if SENSITIVE_HEADERS.iter().any(|s| *s == lower) {
        "•••••".to_string()
    } else {
        value.to_string()
    }
}

pub struct History {
    pub records: Vec<RequestRecord>,
    next_id: u64,
}

impl Default for History {
    fn default() -> Self {
        Self {
            records: Vec::new(),
            next_id: 1,
        }
    }
}

impl History {
    /// 요청을 기록하고 마스킹을 적용한다. 상한 초과 시 가장 오래된 것을 버린다.
    pub fn push(
        &mut self,
        method: String,
        url: String,
        headers: Vec<(String, String)>,
        body: String,
        received_at_ms: i64,
    ) {
        let masked: Vec<(String, String)> = headers
            .iter()
            .map(|(k, v)| (k.clone(), mask_header(k, v)))
            .collect();
        let body = if body.chars().count() > MAX_BODY_CHARS {
            body.chars().take(MAX_BODY_CHARS).collect()
        } else {
            body
        };
        self.records.push(RequestRecord {
            id: self.next_id,
            method,
            url,
            headers: masked,
            body,
            received_at_ms,
        });
        self.next_id += 1;
        if self.records.len() > MAX_HISTORY {
            let overflow = self.records.len() - MAX_HISTORY;
            self.records.drain(0..overflow);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_sensitive_headers() {
        assert_eq!(mask_header("Authorization", "Bearer xyz"), "•••••");
        assert_eq!(mask_header("cookie", "a=b"), "•••••");
        assert_eq!(mask_header("X-API-Key", "k"), "•••••");
        assert_eq!(
            mask_header("Content-Type", "application/json"),
            "application/json"
        );
    }

    #[test]
    fn history_bounded() {
        let mut h = History::default();
        for i in 0..MAX_HISTORY + 50 {
            h.push(
                "POST".into(),
                format!("/hook/{i}"),
                vec![],
                "{}".into(),
                i as i64,
            );
        }
        assert_eq!(h.records.len(), MAX_HISTORY);
        assert_eq!(h.records[0].url, format!("/hook/{}", 50));
    }

    #[test]
    fn history_masks_and_truncates_body() {
        let mut h = History::default();
        let big = "x".repeat(MAX_BODY_CHARS + 10);
        h.push(
            "POST".into(),
            "/hook".into(),
            vec![("Authorization".into(), "secret".into())],
            big,
            1,
        );
        assert_eq!(h.records[0].headers[0].1, "•••••");
        assert_eq!(h.records[0].body.chars().count(), MAX_BODY_CHARS);
    }
}
