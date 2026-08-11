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

/// 기간 내 하루 한 칸 (일별 사용량 차트용)
#[derive(Debug, Clone, Serialize)]
pub struct DayPoint {
    /// 하루 시작 epoch ms
    pub day_ms: i64,
    pub pc_usage_ms: i64,
}

/// 기간(주/월) 요약
#[derive(Debug, Clone, Serialize, Default)]
pub struct RangeSummary {
    pub label: String,
    pub pc_usage_ms: i64,
    pub app_totals: Vec<AppTotal>,
    pub git: GitDay,
    pub daily: Vec<DayPoint>,
}
