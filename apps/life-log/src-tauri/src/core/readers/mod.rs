pub mod activity;

/// activity-timeline 앱의 data.db 경로.
///
/// Tauri의 `app_local_data_dir()`은 번들 identifier 기준 폴더를 사용하므로
/// 실제 경로는 `%LOCALAPPDATA%\com.workbench.activitytimeline\data.db` 이다.
/// (앱이 `%LOCALAPPDATA%\Workbench\activity-timeline\data.db`를 쓰지 않음에 주의)
pub fn default_activity_db() -> String {
    let base = if cfg!(target_os = "windows") {
        std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into())
    } else {
        std::env::temp_dir().to_string_lossy().into_owned()
    };
    format!("{base}/com.workbench.activitytimeline/data.db")
}
