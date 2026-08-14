//! 비정상 종료 대비 미저장 버퍼 recovery (순수 로직).
//!
//! 세션 파일(열려 있던 파일 목록)과 recovery 파일(저장하지 않은 내용)을 분리한다.
//! 정상 저장·닫기 시 해당 recovery를 제거하고, 비정상 종료 후엔 원본 파일과
//! recovery의 시간·hash를 비교해 diff/복구/폐기를 사용자가 선택한다 (§12.1).

use serde::{Deserialize, Serialize};

pub const RECOVERY_VERSION: u32 = 1;

/// 항목 하나: 파일 경로 + 저장하지 않은 버퍼 스냅샷.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryEntry {
    pub path: String,
    pub content: String,
    /// 이 버퍼가 마지막으로 정상 저장된 뒤의 원본 sha256 (비교용).
    #[serde(default)]
    pub base_hash: Option<String>,
    /// 스냅샷 시각 (epoch ms).
    pub snapshot_at_ms: i64,
}

/// recovery 파일 스키마. 세션 파일과 분리된 파일로 저장된다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryFile {
    pub version: u32,
    pub entries: Vec<RecoveryEntry>,
}

/// 전체 저장량 상한 (합계 문자 수). 초과 시 가장 오래된 항목부터 정리.
pub const MAX_TOTAL_CHARS: usize = 512_000;
/// 문서 하나 최대 스냅샷 크기 (초과 시 스냅샷 안 함).
pub const MAX_DOC_CHARS: usize = 128_000;

impl RecoveryFile {
    pub fn empty() -> Self {
        Self {
            version: RECOVERY_VERSION,
            entries: Vec::new(),
        }
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// 손상·미지원 버전이면 빈 파일 (복구 유도가 아니라 안전 폐기).
    pub fn load(input: &str) -> Self {
        serde_json::from_str::<RecoveryFile>(input)
            .ok()
            .filter(|f| f.version == RECOVERY_VERSION)
            .unwrap_or_else(Self::empty)
    }

    pub fn upsert(&mut self, entry: RecoveryEntry) {
        self.entries.retain(|e| e.path != entry.path);
        self.entries.push(entry);
        self.enforce_bounds();
    }

    pub fn remove(&mut self, path: &str) {
        self.entries.retain(|e| e.path != path);
    }

    pub fn get(&self, path: &str) -> Option<&RecoveryEntry> {
        self.entries.iter().find(|e| e.path == path)
    }

    /// 전체 저장량 상한 적용 — 초과 시 가장 오래된 항목부터 버린다.
    fn enforce_bounds(&mut self) {
        let mut total: usize = self.entries.iter().map(|e| e.content.chars().count()).sum();
        while total > MAX_TOTAL_CHARS && self.entries.len() > 1 {
            // 가장 오래된(snapshot_at_ms 최소) 항목 제거
            let oldest_idx = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.snapshot_at_ms)
                .map(|(i, _)| i)
                .unwrap_or(0);
            total -= self.entries[oldest_idx].content.chars().count();
            self.entries.remove(oldest_idx);
        }
    }
}

/// 스냅샷을 남길지 판정. 버퍼가 비어 있으면(원본과 동일) 남기지 않는다.
pub fn should_snapshot(content: &str) -> bool {
    !content.is_empty() && content.chars().count() <= MAX_DOC_CHARS
}

/// 스냅샷 사이 간격(디바운스)이 지났는지. 연속 스냅샷은 5초 이상 떨어뜨린다.
pub fn snapshot_interval_elapsed(now_ms: i64, last_ms: Option<i64>) -> bool {
    last_ms
        .map(|last| now_ms.saturating_sub(last) >= 5_000)
        .unwrap_or(true)
}

impl Default for RecoveryFile {
    fn default() -> Self {
        Self::empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, content: &str, at: i64) -> RecoveryEntry {
        RecoveryEntry {
            path: path.into(),
            content: content.into(),
            base_hash: None,
            snapshot_at_ms: at,
        }
    }

    #[test]
    fn upsert_replaces_same_path() {
        let mut f = RecoveryFile::empty();
        f.upsert(entry("/a.rs", "one", 1));
        f.upsert(entry("/a.rs", "two", 2));
        assert_eq!(f.entries.len(), 1);
        assert_eq!(f.get("/a.rs").unwrap().content, "two");
    }

    #[test]
    fn remove_drops_path() {
        let mut f = RecoveryFile::empty();
        f.upsert(entry("/a.rs", "one", 1));
        f.remove("/a.rs");
        assert!(f.get("/a.rs").is_none());
    }

    #[test]
    fn bounds_evict_oldest_when_over_limit() {
        let mut f = RecoveryFile::empty();
        // MAX_TOTAL_CHARS를 초과하는 두 항목 (각각 절반+α)
        let half = MAX_TOTAL_CHARS / 2 + 1;
        f.upsert(entry("/old.rs", &"x".repeat(half), 1));
        f.upsert(entry("/new.rs", &"x".repeat(half), 2));
        // 둘 다 넣으면 상한 초과 → 가장 오래된 것 제거, 새 것 유지
        assert_eq!(f.entries.len(), 1);
        assert_eq!(f.entries[0].path, "/new.rs");
    }

    #[test]
    fn corrupt_recovery_loads_empty() {
        assert_eq!(RecoveryFile::load("not json"), RecoveryFile::empty());
    }

    #[test]
    fn should_snapshot_policy() {
        assert!(!should_snapshot(""));
        assert!(should_snapshot("abc"));
        assert!(!should_snapshot(&"x".repeat(MAX_DOC_CHARS + 1)));
    }

    #[test]
    fn interval_gating() {
        assert!(snapshot_interval_elapsed(10_000, None));
        assert!(!snapshot_interval_elapsed(10_000, Some(9_000)));
        assert!(snapshot_interval_elapsed(10_000, Some(4_000)));
    }
}
