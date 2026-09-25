//! Privacy rule commands. Rules are stored as JSON under the `privacy_rules`
//! setting and compiled once into [`PrivacyState`].

use crate::commands::tracking::AppState;
use crate::core::privacy::{parse_stored_rules, CompiledRules, InvalidRule, PrivacyRules};
use rusqlite::Connection;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

pub(crate) const RULES_KEY: &str = "privacy_rules";
// Cover all three maximum-length lists, including JSON escaping of each character.
const MAX_RULES_JSON_BYTES: usize =
    3 * crate::core::privacy::MAX_RULES_PER_LIST * (crate::core::privacy::MAX_RULE_CHARS * 6 + 3)
        + 128;

/// Compiled rules plus whether the stored value was readable.
pub struct PrivacyState {
    compiled: RwLock<Arc<CompiledRules>>,
    healthy: AtomicBool,
}

impl PrivacyState {
    pub fn load(conn: &Connection) -> Self {
        let (compiled, healthy) = load_compiled(conn);
        Self {
            compiled: RwLock::new(Arc::new(compiled)),
            healthy: AtomicBool::new(healthy),
        }
    }
    pub fn current(&self) -> Arc<CompiledRules> {
        self.compiled
            .read()
            .map(|rules| rules.clone())
            .unwrap_or_else(|_| Arc::new(CompiledRules::fail_closed()))
    }
    pub fn healthy(&self) -> bool {
        self.healthy.load(Ordering::SeqCst)
    }
    fn replace(&self, compiled: CompiledRules) {
        if let Ok(mut rules) = self.compiled.write() {
            *rules = Arc::new(compiled);
            self.healthy.store(true, Ordering::SeqCst);
        }
    }
}

fn stored_rules(conn: &Connection) -> Result<PrivacyRules, ()> {
    let text = crate::core::db::get_setting_bounded(conn, RULES_KEY, "{}", MAX_RULES_JSON_BYTES)
        .map_err(|_| ())?;
    parse_stored_rules(Some(&text))
}

fn load_compiled(conn: &Connection) -> (CompiledRules, bool) {
    match stored_rules(conn)
        .ok()
        .and_then(|rules| CompiledRules::compile(&rules).ok())
    {
        Some(compiled) => (compiled, true),
        None => (CompiledRules::fail_closed(), false),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(ts_rs::TS)]
pub struct PrivacyRulesView {
    pub rules: PrivacyRules,
    pub healthy: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(ts_rs::TS)]
pub struct PrivacySaveResult {
    pub saved: bool,
    pub invalid: Vec<InvalidRule>,
}

pub(crate) fn view(state: &AppState) -> Result<PrivacyRulesView, String> {
    let conn = state
        .db
        .lock()
        .map_err(|_| "privacy_rules_invalid".to_string())?;
    let rules = stored_rules(&conn).unwrap_or_default();
    Ok(PrivacyRulesView {
        rules,
        healthy: state.privacy.healthy(),
    })
}

pub(crate) fn save_rules(
    state: &AppState,
    rules: PrivacyRules,
) -> Result<PrivacySaveResult, String> {
    let compiled = match CompiledRules::compile(&rules) {
        Ok(compiled) => compiled,
        Err(invalid) => {
            return Ok(PrivacySaveResult {
                saved: false,
                invalid,
            })
        }
    };
    let _control = state
        .tracking_control
        .lock()
        .map_err(|_| "privacy_rules_save_failed".to_string())?;
    let json =
        serde_json::to_string(&rules).map_err(|_| "privacy_rules_save_failed".to_string())?;
    let conn = state
        .db
        .lock()
        .map_err(|_| "privacy_rules_save_failed".to_string())?;
    crate::core::db::try_set_setting(&conn, RULES_KEY, &json)
        .map_err(|_| "privacy_rules_save_failed".to_string())?;
    state.privacy.replace(compiled);
    drop(conn);
    Ok(PrivacySaveResult {
        saved: true,
        invalid: Vec::new(),
    })
}

pub(crate) fn redact_existing_inner(state: &AppState) -> Result<i64, String> {
    if !state.privacy.healthy() {
        return Err("privacy_rules_invalid".into());
    }
    let rules = state.privacy.current();
    let mut conn = state
        .db
        .lock()
        .map_err(|_| "privacy_redaction_failed".to_string())?;
    apply_to_existing(&mut conn, &rules).map_err(|_| "privacy_redaction_failed".into())
}

pub fn get_privacy_rules(
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<PrivacyRulesView, String> {
    view(&state)
}

pub fn set_privacy_rules(
    state: tauri::State<'_, Arc<AppState>>,
    rules: PrivacyRules,
) -> Result<PrivacySaveResult, String> {
    save_rules(&state, rules)
}

/// Apply the current rules to stored sessions (user action). All-or-nothing.
pub fn redact_existing(state: tauri::State<'_, Arc<AppState>>) -> Result<i64, String> {
    redact_existing_inner(&state)
}

fn apply_to_existing(conn: &mut Connection, rules: &CompiledRules) -> rusqlite::Result<i64> {
    let transaction = conn.transaction()?;
    let rows: Vec<(i64, String, String)> = {
        let mut statement = transaction.prepare("SELECT id, app, title FROM sessions")?;
        let rows = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let mut affected = 0i64;
    for (id, app, title) in rows {
        match rules.apply(&app, &title) {
            None => {
                transaction.execute("DELETE FROM sessions WHERE id = ?1", rusqlite::params![id])?;
                affected += 1;
            }
            Some((new_app, new_title)) if new_app != app || new_title != title => {
                transaction.execute(
                    "UPDATE sessions SET app = ?1, title = ?2 WHERE id = ?3",
                    rusqlite::params![new_app, new_title, id],
                )?;
                affected += 1;
            }
            Some(_) => {}
        }
    }
    transaction.commit()?;
    Ok(affected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::privacy::{RuleField, RuleProblem};

    fn state() -> AppState {
        let conn = Connection::open_in_memory().unwrap();
        crate::core::db::migrate(&conn).unwrap();
        crate::commands::tracking::test_state(conn)
    }

    #[test]
    fn valid_maximum_unicode_rules_survive_reload() {
        let state = state();
        let pattern = "😀".repeat(512);
        let rules = PrivacyRules {
            excluded_processes: vec![pattern.clone(); 64],
            excluded_title_patterns: vec![pattern.clone()],
            redact_title_patterns: vec![pattern; 64],
            mask_all_titles: false,
        };
        assert!(serde_json::to_vec(&rules).unwrap().len() > 256 * 1024);
        assert!(save_rules(&state, rules.clone()).unwrap().saved);
        let conn = state.db.lock().unwrap();
        assert!(PrivacyState::load(&conn).healthy());
        assert_eq!(stored_rules(&conn).unwrap(), rules);
    }

    #[test]
    fn invalid_rules_are_returned_not_stored() {
        let state = state();
        let result = save_rules(
            &state,
            PrivacyRules {
                excluded_title_patterns: vec!["(".into()],
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!result.saved);
        assert_eq!(result.invalid[0].field, RuleField::ExcludedTitlePatterns);
        assert_eq!(result.invalid[0].problem, RuleProblem::Syntax);
        let conn = state.db.lock().unwrap();
        assert_eq!(
            crate::core::db::get_setting(&conn, RULES_KEY, "missing"),
            "missing"
        );
    }

    #[test]
    fn write_failure_keeps_previous_compiled_rules() {
        let state = state();
        save_rules(
            &state,
            PrivacyRules {
                mask_all_titles: true,
                ..Default::default()
            },
        )
        .unwrap();
        state
            .db
            .lock()
            .unwrap()
            .execute_batch("PRAGMA query_only = ON")
            .unwrap();
        let error = save_rules(&state, PrivacyRules::default()).unwrap_err();
        assert_eq!(error, "privacy_rules_save_failed");
        assert_eq!(state.privacy.current().apply("a.exe", "t").unwrap().1, "");
    }

    #[test]
    fn redaction_is_all_or_nothing() {
        let state = state();
        {
            let conn = state.db.lock().unwrap();
            conn.execute(
                "INSERT INTO sessions (app, title, start_ts, end_ts, duration_ms) VALUES ('a.exe','secret one',0,10,10),('b.exe','plain',10,20,10)",
                [],
            )
            .unwrap();
        }
        save_rules(
            &state,
            PrivacyRules {
                redact_title_patterns: vec!["secret".into()],
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(redact_existing_inner(&state).unwrap(), 1);
        let conn = state.db.lock().unwrap();
        let title: String = conn
            .query_row("SELECT title FROM sessions WHERE app = 'a.exe'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(title, "[redacted] one");
    }

    #[test]
    fn redaction_refuses_to_run_with_unreadable_rules() {
        let conn = Connection::open_in_memory().unwrap();
        crate::core::db::migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO settings VALUES ('privacy_rules', '{broken')",
            [],
        )
        .unwrap();
        let state = crate::commands::tracking::test_state(conn);
        assert_eq!(
            redact_existing_inner(&state).unwrap_err(),
            "privacy_rules_invalid"
        );
    }
    #[test]
    fn empty_stored_value_and_database_read_failure_fail_closed() {
        let conn = Connection::open_in_memory().unwrap();
        crate::core::db::migrate(&conn).unwrap();
        conn.execute("INSERT INTO settings VALUES ('privacy_rules', '')", [])
            .unwrap();
        let privacy = PrivacyState::load(&conn);
        assert!(!privacy.healthy());
        assert_eq!(privacy.current().apply("a.exe", "private").unwrap().1, "");
        conn.execute("DROP TABLE settings", []).unwrap();
        assert!(!PrivacyState::load(&conn).healthy());
    }

    #[test]
    fn redaction_rolls_back_when_a_later_row_fails() {
        let state = state();
        {
            let conn = state.db.lock().unwrap();
            conn.execute_batch("INSERT INTO sessions (app, title, start_ts, end_ts, duration_ms) VALUES ('a.exe','secret one',0,10,10),('b.exe','secret two',10,20,10);
                CREATE TRIGGER fail_second BEFORE UPDATE ON sessions WHEN OLD.app = 'b.exe' BEGIN SELECT RAISE(ABORT, 'fixture'); END;").unwrap();
        }
        save_rules(
            &state,
            PrivacyRules {
                redact_title_patterns: vec!["secret".into()],
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            redact_existing_inner(&state).unwrap_err(),
            "privacy_redaction_failed"
        );
        let conn = state.db.lock().unwrap();
        let title: String = conn
            .query_row(
                "SELECT title FROM sessions WHERE app = 'a.exe'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(title, "secret one");
    }
}
