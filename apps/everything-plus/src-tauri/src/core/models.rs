use serde::Serialize;

/// 인덱스된 파일 하나
#[derive(Debug, Clone, Serialize)]
pub struct FileEntry {
    pub id: i64,
    pub path: String,
    pub name: String,
    pub ext: String,
    pub size: i64,
    pub modified_ts: i64,
}

/// 내용 검색 결과
#[derive(Debug, Clone, Serialize)]
pub struct ContentResult {
    pub path: String,
    pub name: String,
    pub snippet: String,
}

/// 검색 루트 (내용 인덱싱 여부 포함)
#[derive(Debug, Clone, Serialize)]
pub struct RootInfo {
    pub path: String,
    pub content: bool,
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
