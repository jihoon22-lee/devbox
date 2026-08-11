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

/// 인덱스 상태
#[derive(Debug, Clone, Serialize)]
pub struct IndexStatus {
    pub indexing: bool,
    pub total_files: i64,
    pub roots: usize,
    pub last_indexed_at: Option<i64>,
}
