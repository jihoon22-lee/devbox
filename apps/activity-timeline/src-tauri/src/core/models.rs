use serde::Serialize;

/// 저장된 세션 (DB 행과 1:1)
#[derive(Debug, Clone, Serialize)]
pub struct Session {
    pub id: i64,
    pub app: String,
    pub title: String,
    pub start_ts: i64,
    pub end_ts: i64,
    pub duration_ms: i64,
}

/// 앱별 사용 시간 합계
#[derive(Debug, Clone, Serialize)]
pub struct AppTotal {
    pub app: String,
    pub duration_ms: i64,
    pub sessions: i64,
}

/// 닫힌 세션 (sessionizer가 배출)
#[derive(Debug, Clone, Serialize)]
pub struct ClosedSession {
    pub app: String,
    pub title: String,
    pub start_ts: i64,
    pub end_ts: i64,
}
