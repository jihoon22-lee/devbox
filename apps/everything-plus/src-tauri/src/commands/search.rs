use crate::commands::indexing::AppState;
use crate::core::db::{search, search_content_with_filter, search_with_filter};
use crate::core::models::{ContentResult, FileEntry, SearchFilter};
use std::sync::Arc;

const MAX_QUERY_BYTES: usize = 4 * 1024;
// Filename regex mode intentionally asks the native FTS prefilter for up to
// 2,000 candidates and applies the regex in the frontend.  Keep that existing
// contract separate from the smaller content-result boundary.
const DEFAULT_RESULTS: i64 = 200;
const MAX_FILE_RESULTS: i64 = 2_000;
const MAX_CONTENT_RESULTS: i64 = 200;
const SEARCH_ERROR: &str = "검색을 처리할 수 없습니다.";
const FILTER_ERROR: &str = "검색 필터를 사용할 수 없습니다.";

fn validate_query(query: &str) -> Result<&str, String> {
    if query.len() > MAX_QUERY_BYTES || query.chars().any(|character| character.is_control()) {
        return Err(SEARCH_ERROR.to_string());
    }
    Ok(query.trim())
}

fn file_result_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(DEFAULT_RESULTS).clamp(1, MAX_FILE_RESULTS)
}

fn content_result_limit(limit: Option<i64>) -> i64 {
    limit
        .unwrap_or(DEFAULT_RESULTS)
        .clamp(1, MAX_CONTENT_RESULTS)
}

/// 파일명 FTS5 검색.
#[tauri::command]
pub fn search_files(
    state: tauri::State<'_, Arc<AppState>>,
    query: String,
    limit: Option<i64>,
    filter: Option<SearchFilter>,
) -> Result<Vec<FileEntry>, String> {
    let limit = file_result_limit(limit);
    let q = validate_query(&query)?;
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let filter = filter
        .unwrap_or_default()
        .normalized()
        .map_err(|_| FILTER_ERROR.to_string())?;
    let conn = state.db.lock().map_err(|_| SEARCH_ERROR.to_string())?;
    if filter.is_empty() {
        search(&conn, q, limit).map_err(|_| SEARCH_ERROR.to_string())
    } else {
        search_with_filter(&conn, q, limit, &filter).map_err(|_| SEARCH_ERROR.to_string())
    }
}

/// 파일 내용 FTS5 검색.
#[tauri::command]
pub fn search_content(
    state: tauri::State<'_, Arc<AppState>>,
    query: String,
    limit: Option<i64>,
    filter: Option<SearchFilter>,
) -> Result<Vec<ContentResult>, String> {
    let limit = content_result_limit(limit);
    let q = validate_query(&query)?;
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let filter = filter
        .unwrap_or_default()
        .normalized()
        .map_err(|_| FILTER_ERROR.to_string())?;
    let conn = state.db.lock().map_err(|_| SEARCH_ERROR.to_string())?;
    if filter.is_empty() {
        crate::core::db::search_content(&conn, q, limit).map_err(|_| SEARCH_ERROR.to_string())
    } else {
        search_content_with_filter(&conn, q, limit, &filter).map_err(|_| SEARCH_ERROR.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_untrimmed_query_bounds_and_control_characters() {
        assert_eq!(validate_query("  cargo  ").unwrap(), "cargo");
        assert!(validate_query(&format!("{}  ", "x".repeat(MAX_QUERY_BYTES))).is_err());
        assert!(validate_query("line\nquery").is_err());
    }

    #[test]
    fn keeps_filename_and_content_result_caps_distinct() {
        assert_eq!(file_result_limit(None), 200);
        assert_eq!(file_result_limit(Some(500)), 500);
        assert_eq!(file_result_limit(Some(5_000)), 2_000);
        assert_eq!(content_result_limit(None), 200);
        assert_eq!(content_result_limit(Some(500)), 200);
    }
}
