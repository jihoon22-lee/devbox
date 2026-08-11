use serde::Serialize;

/// 앱별 사용 시간 합계
#[derive(Debug, Clone, Serialize)]
pub struct AppTotal {
    pub app: String,
    pub duration_ms: i64,
}

/// 프로젝트별 커밋 수
#[derive(Debug, Clone, Serialize)]
pub struct ProjectCommit {
    pub path: String,
    pub commits: u32,
}

/// git 집계
#[derive(Debug, Clone, Serialize, Default)]
pub struct GitDay {
    pub projects: Vec<ProjectCommit>,
    pub total_commits: u32,
}

/// 하루 요약
#[derive(Debug, Clone, Serialize, Default)]
pub struct DaySummary {
    pub date: String,
    pub pc_usage_ms: i64,
    pub app_totals: Vec<AppTotal>,
    pub git: GitDay,
}
