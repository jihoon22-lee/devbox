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
    pub total_files: i64,
    pub indexed_files: i64,
    pub roots: usize,
    pub last_indexed_at: Option<i64>,
}
