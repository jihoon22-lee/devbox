pub mod activity;

/// 다른 앱의 기본 데이터 경로.
/// 규약: `%LOCALAPPDATA%\Workbench\<App>\data.db`
pub fn default_activity_db() -> String {
    let base = if cfg!(target_os = "windows") {
        std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into())
    } else {
        std::env::temp_dir().to_string_lossy().into_owned()
    };
    format!("{base}/Workbench/activity-timeline/data.db")
}
