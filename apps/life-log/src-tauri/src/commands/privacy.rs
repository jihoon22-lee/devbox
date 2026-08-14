//! privacy rule 관리 명령. 규칙은 settings 테이블에 JSON으로 저장된다.

use crate::commands::tracking::AppState;
use crate::core::privacy::{apply as apply_privacy, parse_rules, PrivacyRules};
use rusqlite::Connection;
use std::sync::Arc;

const RULES_KEY: &str = "privacy_rules";

fn read_rules(conn: &Connection) -> PrivacyRules {
    let json = crate::core::db::get_setting(conn, RULES_KEY, "{}");
    parse_rules(&json)
}

#[tauri::command]
pub fn get_privacy_rules(state: tauri::State<'_, Arc<AppState>>) -> PrivacyRules {
    read_rules(&state.db.lock().unwrap())
}

#[tauri::command]
pub fn set_privacy_rules(
    state: tauri::State<'_, Arc<AppState>>,
    rules: PrivacyRules,
) -> Result<(), String> {
    let json = serde_json::to_string(&rules).map_err(|e| e.to_string())?;
    crate::core::db::set_setting(&state.db.lock().unwrap(), RULES_KEY, &json);
    Ok(())
}

/// 기존에 저장된 세션에 규칙을 소급 적용한다 (사용자 선택).
/// - 제외 대상(process) 세션은 삭제
/// - 제목 규칙은 제목을 치환/공란으로 갱신
/// 반환값은 영향을 받은 세션 수.
#[tauri::command]
pub fn redact_existing(state: tauri::State<'_, Arc<AppState>>) -> Result<i64, String> {
    let conn = state.db.lock().unwrap();
    let rules = read_rules(&conn);
    apply_to_existing(&conn, &rules).map_err(|e| e.to_string())
}

fn apply_to_existing(conn: &Connection, rules: &PrivacyRules) -> rusqlite::Result<i64> {
    let mut stmt = conn.prepare("SELECT id, app, title FROM sessions")?;
    let rows: Vec<(i64, String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    let mut affected = 0i64;
    for (id, app, title) in rows {
        match apply_privacy(rules, &app, &title) {
            None => {
                // 제외 대상 — 삭제
                conn.execute("DELETE FROM sessions WHERE id = ?1", rusqlite::params![id])?;
                affected += 1;
            }
            Some((new_app, new_title)) => {
                if new_app != app || new_title != title {
                    conn.execute(
                        "UPDATE sessions SET app = ?1, title = ?2 WHERE id = ?3",
                        rusqlite::params![new_app, new_title, id],
                    )?;
                    affected += 1;
                }
            }
        }
    }
    Ok(affected)
}
