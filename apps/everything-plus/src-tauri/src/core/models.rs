use serde::{Deserialize, Serialize};

use devbox_applink::QueryFilter;

/// Native search filters shared by filename/content search and saved queries.
///
/// A source is deliberately represented by the database root id rather than a
/// caller-provided path.  This keeps filtering a read-only indexed operation
/// and prevents a saved query or a stale UI from becoming filesystem authority.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchFilter {
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_after: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_before: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_size: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_size: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_root_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_status: Option<String>,
}

impl SearchFilter {
    pub const MAX_EXTENSIONS: usize = 64;
    pub const MAX_EXTENSION_BYTES: usize = 16;

    /// Normalize UI/Launcher input before it reaches SQL or persistence.
    /// Errors intentionally do not echo the rejected value.
    pub fn normalized(&self) -> Result<Self, &'static str> {
        if self.extensions.len() > Self::MAX_EXTENSIONS {
            return Err("검색 필터를 사용할 수 없습니다.");
        }

        let mut extensions = Vec::with_capacity(self.extensions.len());
        for extension in &self.extensions {
            let normalized = extension
                .trim()
                .trim_start_matches('.')
                .to_ascii_lowercase();
            if normalized.is_empty()
                || normalized.len() > Self::MAX_EXTENSION_BYTES
                || !normalized.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'_' | b'-' | b'+')
                })
            {
                return Err("검색 필터를 사용할 수 없습니다.");
            }
            extensions.push(normalized);
        }
        extensions.sort_unstable();
        extensions.dedup();

        if self
            .modified_after
            .zip(self.modified_before)
            .is_some_and(|(after, before)| after > before)
            || self.modified_after.is_some_and(|timestamp| timestamp < 0)
            || self.modified_before.is_some_and(|timestamp| timestamp < 0)
            || self.min_size.is_some_and(|size| size < 0)
            || self.max_size.is_some_and(|size| size < 0)
            || self
                .min_size
                .zip(self.max_size)
                .is_some_and(|(minimum, maximum)| minimum > maximum)
            || self.source_root_id.is_some_and(|root_id| root_id <= 0)
        {
            return Err("검색 필터를 사용할 수 없습니다.");
        }

        let content_status = self
            .content_status
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_ascii_lowercase);
        if content_status.as_deref().is_some_and(|status| {
            !matches!(
                status,
                "indexed"
                    | "truncated"
                    | "partial"
                    | "failed"
                    | "not_indexed"
                    | "too_large"
                    | "unsupported_encoding"
                    | "read_error"
                    | "timeout"
                    | "changed_during_read"
                    | "skipped_sensitive"
                    | "no_text"
                    | "unsupported_encrypted"
                    | "extract_error"
            )
        }) {
            return Err("검색 필터를 사용할 수 없습니다.");
        }

        Ok(Self {
            extensions,
            modified_after: self.modified_after,
            modified_before: self.modified_before,
            min_size: self.min_size,
            max_size: self.max_size,
            source_root_id: self.source_root_id,
            content_status,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.extensions.is_empty()
            && self.modified_after.is_none()
            && self.modified_before.is_none()
            && self.min_size.is_none()
            && self.max_size.is_none()
            && self.source_root_id.is_none()
            && self.content_status.is_none()
    }

    pub fn to_applink(&self) -> QueryFilter {
        QueryFilter {
            extensions: self.extensions.clone(),
            modified_after: self.modified_after,
            modified_before: self.modified_before,
            min_size: self.min_size,
            max_size: self.max_size,
            source_root_id: self.source_root_id,
            content_status: self.content_status.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SearchFilter;

    #[test]
    fn normalizes_extensions_and_status_aliases() {
        let normalized = SearchFilter {
            extensions: vec![" .RS ".into(), "md".into(), ".rs".into()],
            content_status: Some("PARTIAL".into()),
            ..SearchFilter::default()
        }
        .normalized()
        .unwrap();

        assert_eq!(
            normalized.extensions,
            vec!["md".to_string(), "rs".to_string()]
        );
        assert_eq!(normalized.content_status.as_deref(), Some("partial"));
    }

    #[test]
    fn rejects_unbounded_or_reversed_filter_values() {
        assert!(SearchFilter {
            extensions: vec!["rs".into(); SearchFilter::MAX_EXTENSIONS + 1],
            ..SearchFilter::default()
        }
        .normalized()
        .is_err());
        assert!(SearchFilter {
            min_size: Some(10),
            max_size: Some(9),
            ..SearchFilter::default()
        }
        .normalized()
        .is_err());
        assert!(SearchFilter {
            content_status: Some("unknown".into()),
            ..SearchFilter::default()
        }
        .normalized()
        .is_err());
        assert!(SearchFilter {
            modified_before: Some(-1),
            ..SearchFilter::default()
        }
        .normalized()
        .is_err());
    }
}

/// 인덱스된 파일 하나
#[derive(Debug, Clone, Serialize)]
pub struct FileEntry {
    pub id: i64,
    pub path: String,
    pub name: String,
    pub ext: String,
    pub size: i64,
    pub modified_ts: i64,
    pub root_id: Option<i64>,
    pub content_status: Option<String>,
    pub content_truncated: bool,
}

/// 내용 검색 결과
#[derive(Debug, Clone, Serialize)]
pub struct ContentResult {
    pub path: String,
    pub name: String,
    pub snippet: String,
    pub ext: String,
    pub size: i64,
    pub modified_ts: i64,
    pub root_id: Option<i64>,
    pub content_status: String,
    pub truncated: bool,
    pub error_code: Option<String>,
    pub extractor_version: String,
    pub indexed_at: Option<i64>,
    pub encoding: Option<String>,
    pub text_chars: i64,
}

/// 검색 루트 (내용 인덱싱 여부 포함)
#[derive(Debug, Clone, Serialize)]
pub struct RootInfo {
    pub id: i64,
    pub path: String,
    pub content: bool,
}

/// A user-named query.  Results are intentionally not part of this DTO: a
/// saved query is re-evaluated against the current local index when opened.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SavedQuery {
    pub id: i64,
    pub name: String,
    pub query: String,
    pub filter: SearchFilter,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 인덱스 상태
#[derive(Debug, Clone, Serialize)]
pub struct IndexStatus {
    pub indexing: bool,
    pub cancel_requested: bool,
    pub total_files: i64,
    pub indexed_files: i64,
    pub content_indexed_files: i64,
    pub content_truncated_files: i64,
    pub content_failed_files: i64,
    pub roots: usize,
    pub last_indexed_at: Option<i64>,
    pub last_error: Option<String>,
}

/// watcher 루트별 상태
#[derive(Debug, Clone, Serialize)]
pub struct RootStatus {
    pub root: String,
    /// 마지막으로 증분 반영한 시각 (epoch ms)
    pub last_synced_at: Option<i64>,
    /// 아직 반영 대기 중인 이벤트 수
    pub pending: u32,
    /// 최근 오류 (있으면)
    pub error: Option<String>,
}
