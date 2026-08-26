//! 수신 요청 기록 (순수, 상한 적용).
//!
//! Authorization·Cookie·API key 헤더는 일반 history DTO에서 마스킹한다 (§15.3 안전 경계).
//! 원본 헤더는 bounded in-memory entry에만 남기고 명시적인 일회성 복사에서만 사용한다.

use serde::Serialize;
use std::collections::VecDeque;

pub const MAX_HISTORY: usize = 200;
pub const MAX_BODY_CHARS: usize = 256_000;
pub const MAX_BODY_BYTES: usize = 1_024_000;
pub const MAX_HEADERS: usize = 100;
pub const MAX_HEADER_CHARS: usize = 64_000;
/// A busy local endpoint must not let an unbounded stream of requests keep
/// allocating history entries. This is deliberately a small, fixed window;
/// it is a server admission limit, not a persistence policy.
pub const MAX_REQUESTS_PER_WINDOW: usize = 120;
pub const RATE_WINDOW_MS: i64 = 1_000;

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

struct HistoryEntry {
    masked: RequestRecord,
    // Serialize/Debug를 구현하지 않는다. 일반 history 조회 경계 밖으로 나가면 안 된다.
    raw_headers: Vec<(String, String)>,
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
    entries: Vec<HistoryEntry>,
    next_id: u64,
    rate_events: VecDeque<i64>,
}

impl Default for History {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            next_id: 1,
            rate_events: VecDeque::new(),
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
        let headers = bound_headers(headers);
        let masked: Vec<(String, String)> = headers
            .iter()
            .map(|(k, v)| (k.clone(), mask_header(k, v)))
            .collect();
        let body = if body.chars().count() > MAX_BODY_CHARS {
            body.chars().take(MAX_BODY_CHARS).collect()
        } else {
            body
        };
        let record = RequestRecord {
            id: self.next_id,
            method,
            url,
            headers: masked,
            body,
            received_at_ms,
        };
        self.entries.push(HistoryEntry {
            masked: record,
            raw_headers: headers,
        });
        self.next_id += 1;
        if self.entries.len() > MAX_HISTORY {
            let overflow = self.entries.len() - MAX_HISTORY;
            self.entries.drain(0..overflow);
        }
    }

    /// 일반 조회는 언제나 마스킹된 최신순 snapshot만 반환한다.
    pub fn list_masked(&self) -> Vec<RequestRecord> {
        self.entries
            .iter()
            .rev()
            .map(|entry| entry.masked.clone())
            .collect()
    }

    /// Return one masked request for an explicit opaque history ID. The raw
    /// header vault is intentionally not reachable through this accessor.
    pub fn masked_record(&self, id: u64) -> Option<RequestRecord> {
        self.entry(id).map(|entry| entry.masked.clone())
    }

    /// Admit a request before reading its body. This keeps the tiny HTTP
    /// server bounded even when a client sends a burst of large requests.
    pub fn allow_request(&mut self, received_at_ms: i64) -> bool {
        // `received_at_ms` comes from the wall clock. If that clock moves
        // backwards, retaining future-dated events would keep the listener
        // rate-limited until wall time catches up. Start a fresh bounded
        // window instead; this neither admits an unbounded burst nor leaves
        // the local server wedged after a clock correction.
        if self
            .rate_events
            .back()
            .is_some_and(|latest| received_at_ms < *latest)
        {
            self.rate_events.clear();
        }
        while self
            .rate_events
            .front()
            .is_some_and(|oldest| received_at_ms.saturating_sub(*oldest) >= RATE_WINDOW_MS)
        {
            self.rate_events.pop_front();
        }
        if self.rate_events.len() >= MAX_REQUESTS_PER_WINDOW {
            return false;
        }
        self.rate_events.push_back(received_at_ms);
        true
    }

    /// 마스킹된 전체 요청을 JSON으로 만든다.
    pub fn masked_copy(&self, id: u64) -> Option<String> {
        let entry = self.entry(id)?;
        serialize_record(&entry.masked).ok()
    }

    /// 명시적으로 확인된 일회성 복사를 위해서만 원본 헤더를 결합한다.
    pub fn raw_copy(&self, id: u64) -> Option<String> {
        let entry = self.entry(id)?;
        let mut raw = entry.masked.clone();
        raw.headers.clone_from(&entry.raw_headers);
        serialize_record(&raw).ok()
    }

    /// 별도 raw 확인이 없는 헤더 복사는 항상 마스킹된 값만 사용한다.
    pub fn masked_headers_copy(&self, id: u64) -> Option<String> {
        let entry = self.entry(id)?;
        Some(
            entry
                .masked
                .headers
                .iter()
                .map(|(name, value)| format!("{name}: {value}"))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }

    pub fn remove(&mut self, id: u64) -> bool {
        let original_len = self.entries.len();
        self.entries.retain(|entry| entry.masked.id != id);
        self.entries.len() != original_len
    }

    /// 열린 메뉴가 가리키던 ID가 clear 직후 새 요청에 재사용되지 않게 next_id는 유지한다.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    fn entry(&self, id: u64) -> Option<&HistoryEntry> {
        self.entries.iter().find(|entry| entry.masked.id == id)
    }
}

fn bound_headers(headers: Vec<(String, String)>) -> Vec<(String, String)> {
    let mut remaining = MAX_HEADER_CHARS;
    let mut bounded = Vec::new();
    for (name, value) in headers.into_iter().take(MAX_HEADERS) {
        if remaining == 0 {
            break;
        }
        let name: String = name.chars().take(remaining).collect();
        remaining -= name.chars().count();
        let value: String = value.chars().take(remaining).collect();
        remaining -= value.chars().count();
        bounded.push((name, value));
    }
    bounded
}

fn serialize_record(record: &RequestRecord) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(record)
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
        let records = h.list_masked();
        assert_eq!(records.len(), MAX_HISTORY);
        assert_eq!(records.last().unwrap().url, format!("/hook/{}", 50));
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
        let records = h.list_masked();
        assert_eq!(records[0].headers[0].1, "•••••");
        assert_eq!(records[0].body.chars().count(), MAX_BODY_CHARS);
    }

    #[test]
    fn history_bounds_raw_and_masked_header_snapshots() {
        let mut h = History::default();
        let mut headers = (0..MAX_HEADERS + 10)
            .map(|index| (format!("x-{index}"), "v".to_string()))
            .collect::<Vec<_>>();
        headers[0] = ("Authorization".into(), "s".repeat(MAX_HEADER_CHARS + 10));
        h.push("GET".into(), "/".into(), headers, "".into(), 1);

        let listed = h.list_masked();
        assert_eq!(listed[0].headers.len(), 1);
        assert_eq!(listed[0].headers[0].1, "•••••");
        let raw = h.raw_copy(1).unwrap();
        let raw: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let raw_value = raw["headers"][0][1].as_str().unwrap();
        assert_eq!(
            raw_value.chars().count(),
            MAX_HEADER_CHARS - "Authorization".len()
        );
        assert_eq!(raw["headers"].as_array().unwrap().len(), 1);

        let mut count_bounded = History::default();
        let headers = (0..MAX_HEADERS + 10)
            .map(|index| (format!("x-{index}"), "v".to_string()))
            .collect();
        count_bounded.push("GET".into(), "/".into(), headers, "".into(), 1);
        assert_eq!(count_bounded.list_masked()[0].headers.len(), MAX_HEADERS);
        let raw: serde_json::Value =
            serde_json::from_str(&count_bounded.raw_copy(1).unwrap()).unwrap();
        assert_eq!(raw["headers"].as_array().unwrap().len(), MAX_HEADERS);
    }

    #[test]
    fn raw_headers_only_leave_through_explicit_copy() {
        let mut h = History::default();
        h.push(
            "POST".into(),
            "/hook".into(),
            vec![
                ("Authorization".into(), "Bearer raw-secret".into()),
                ("Content-Type".into(), "application/json".into()),
            ],
            "{}".into(),
            1,
        );

        let listed = serde_json::to_string(&h.list_masked()).unwrap();
        let masked = h.masked_copy(1).unwrap();
        let headers = h.masked_headers_copy(1).unwrap();
        assert!(!listed.contains("raw-secret"));
        assert!(!masked.contains("raw-secret"));
        assert!(!headers.contains("raw-secret"));
        assert!(masked.contains("•••••"));
        assert_eq!(
            headers,
            "Authorization: •••••\nContent-Type: application/json"
        );

        let raw = h.raw_copy(1).unwrap();
        assert!(raw.contains("Bearer raw-secret"));
        assert!(!raw.contains("•••••"));
    }

    #[test]
    fn removal_and_clear_do_not_retarget_stale_ids() {
        let mut h = History::default();
        h.push("GET".into(), "/first".into(), vec![], "".into(), 1);
        h.push("GET".into(), "/second".into(), vec![], "".into(), 2);

        assert!(h.remove(1));
        assert!(!h.remove(1));
        assert!(h.masked_copy(1).is_none());
        assert_eq!(h.list_masked()[0].id, 2);

        h.clear();
        h.push("GET".into(), "/third".into(), vec![], "".into(), 3);
        assert_eq!(h.list_masked()[0].id, 3);
        assert!(h.raw_copy(2).is_none());
    }

    #[test]
    fn request_rate_is_bounded_per_fixed_window() {
        let mut h = History::default();
        for _ in 0..MAX_REQUESTS_PER_WINDOW {
            assert!(h.allow_request(10));
        }
        assert!(!h.allow_request(10));
        assert!(!h.allow_request(RATE_WINDOW_MS - 1));
        assert!(h.allow_request(RATE_WINDOW_MS + 10));
    }

    #[test]
    fn request_rate_recovers_when_the_wall_clock_moves_backwards() {
        let mut h = History::default();
        for _ in 0..MAX_REQUESTS_PER_WINDOW {
            assert!(h.allow_request(10_000));
        }
        assert!(!h.allow_request(10_000));

        assert!(h.allow_request(10));
        for _ in 1..MAX_REQUESTS_PER_WINDOW {
            assert!(h.allow_request(10));
        }
        assert!(!h.allow_request(10));
    }
}
