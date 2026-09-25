//! Privacy rules (pure logic). Rules apply **before** a session reaches the
//! DB; they are not a UI filter. Excluded or replaced text must never be
//! stored, logged, or written to an integration snapshot.

use regex::{Regex, RegexBuilder, RegexSet, RegexSetBuilder};
use serde::{Deserialize, Serialize};

pub const MAX_RULES_PER_LIST: usize = 64;
pub const MAX_RULE_CHARS: usize = 512;
const REGEX_SIZE_LIMIT: usize = 1 << 20;
pub const REDACTED: &str = "[redacted]";

/// Stored and edited rule set. Field names are the persisted wire format.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyRules {
    /// Exact process names (case-insensitive). A match drops the session.
    #[serde(default)]
    pub excluded_processes: Vec<String>,
    /// A match keeps the session but stores an empty title.
    #[serde(default)]
    pub excluded_title_patterns: Vec<String>,
    /// Every match is replaced with `[redacted]`.
    #[serde(default)]
    pub redact_title_patterns: Vec<String>,
    /// Never store any title.
    #[serde(default)]
    pub mask_all_titles: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RuleField {
    ExcludedProcesses,
    ExcludedTitlePatterns,
    RedactTitlePatterns,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuleProblem {
    Empty,
    TooLong,
    TooMany,
    Syntax,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InvalidRule {
    pub field: RuleField,
    /// Zero-based position in the submitted list. `TooMany` reports the first
    /// entry beyond the limit.
    pub index: usize,
    pub problem: RuleProblem,
}

/// Validated rules, compiled once and shared by the collector.
#[derive(Debug)]
pub struct CompiledRules {
    excluded_processes: Vec<String>,
    excluded_titles: RegexSet,
    redactions: Vec<Regex>,
    mask_all_titles: bool,
}

fn check_list(field: RuleField, values: &[String], problems: &mut Vec<InvalidRule>) {
    if values.len() > MAX_RULES_PER_LIST {
        problems.push(InvalidRule {
            field,
            index: MAX_RULES_PER_LIST,
            problem: RuleProblem::TooMany,
        });
    }
    for (index, value) in values.iter().enumerate().take(MAX_RULES_PER_LIST) {
        if value.trim().is_empty() {
            problems.push(InvalidRule {
                field,
                index,
                problem: RuleProblem::Empty,
            });
        } else if value.chars().count() > MAX_RULE_CHARS {
            problems.push(InvalidRule {
                field,
                index,
                problem: RuleProblem::TooLong,
            });
        }
    }
}

fn build_regex(pattern: &str) -> Result<Regex, regex::Error> {
    RegexBuilder::new(pattern)
        .size_limit(REGEX_SIZE_LIMIT)
        .build()
}

fn has_problem(problems: &[InvalidRule], field: RuleField, index: usize) -> bool {
    problems
        .iter()
        .any(|p| p.field == field && p.index == index)
}

impl CompiledRules {
    /// Validate and compile every rule. Nothing is partially accepted.
    pub fn compile(rules: &PrivacyRules) -> Result<Self, Vec<InvalidRule>> {
        let mut problems = Vec::new();
        check_list(
            RuleField::ExcludedProcesses,
            &rules.excluded_processes,
            &mut problems,
        );
        check_list(
            RuleField::ExcludedTitlePatterns,
            &rules.excluded_title_patterns,
            &mut problems,
        );
        check_list(
            RuleField::RedactTitlePatterns,
            &rules.redact_title_patterns,
            &mut problems,
        );
        for (index, pattern) in rules
            .excluded_title_patterns
            .iter()
            .enumerate()
            .take(MAX_RULES_PER_LIST)
        {
            if !has_problem(&problems, RuleField::ExcludedTitlePatterns, index)
                && build_regex(pattern).is_err()
            {
                problems.push(InvalidRule {
                    field: RuleField::ExcludedTitlePatterns,
                    index,
                    problem: RuleProblem::Syntax,
                });
            }
        }
        let mut redactions = Vec::new();
        for (index, pattern) in rules
            .redact_title_patterns
            .iter()
            .enumerate()
            .take(MAX_RULES_PER_LIST)
        {
            if has_problem(&problems, RuleField::RedactTitlePatterns, index) {
                continue;
            }
            match build_regex(pattern) {
                Ok(regex) => redactions.push(regex),
                Err(_) => problems.push(InvalidRule {
                    field: RuleField::RedactTitlePatterns,
                    index,
                    problem: RuleProblem::Syntax,
                }),
            }
        }
        if !problems.is_empty() {
            return Err(problems);
        }
        let excluded_titles = RegexSetBuilder::new(&rules.excluded_title_patterns)
            .size_limit(REGEX_SIZE_LIMIT)
            .build()
            .map_err(|_| {
                vec![InvalidRule {
                    field: RuleField::ExcludedTitlePatterns,
                    index: 0,
                    problem: RuleProblem::Syntax,
                }]
            })?;
        Ok(Self {
            excluded_processes: rules
                .excluded_processes
                .iter()
                .map(|p| p.trim().to_lowercase())
                .collect(),
            excluded_titles,
            redactions,
            mask_all_titles: rules.mask_all_titles,
        })
    }

    /// Used when stored rules cannot be read or compiled: the session keeps
    /// its process and duration but no title is ever stored.
    pub fn fail_closed() -> Self {
        Self {
            excluded_processes: Vec::new(),
            excluded_titles: RegexSet::empty(),
            redactions: Vec::new(),
            mask_all_titles: true,
        }
    }

    /// `None` drops the whole session. `Some((app, title))` is what may be stored.
    pub fn apply(&self, app: &str, title: &str) -> Option<(String, String)> {
        let app_lower = app.to_lowercase();
        if self.excluded_processes.contains(&app_lower) {
            return None;
        }
        if self.mask_all_titles || self.excluded_titles.is_match(title) {
            return Some((app.to_string(), String::new()));
        }
        let mut out = title.to_string();
        for regex in &self.redactions {
            out = regex.replace_all(&out, REDACTED).into_owned();
        }
        Some((app.to_string(), out))
    }
}

/// Stored JSON → rules. A missing value means "no rules". Anything unreadable
/// is an error so callers fail closed instead of silently collecting titles.
pub fn parse_stored_rules(json: Option<&str>) -> Result<PrivacyRules, ()> {
    match json {
        None => Ok(PrivacyRules::default()),
        Some(text) => serde_json::from_str(text).map_err(|_| ()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(processes: &[&str], excluded: &[&str], redact: &[&str]) -> PrivacyRules {
        PrivacyRules {
            excluded_processes: processes.iter().map(|s| s.to_string()).collect(),
            excluded_title_patterns: excluded.iter().map(|s| s.to_string()).collect(),
            redact_title_patterns: redact.iter().map(|s| s.to_string()).collect(),
            mask_all_titles: false,
        }
    }

    #[test]
    fn empty_rules_pass_titles_through() {
        let compiled = CompiledRules::compile(&PrivacyRules::default()).unwrap();
        assert_eq!(
            compiled.apply("chrome.exe", "GitHub"),
            Some(("chrome.exe".to_string(), "GitHub".to_string()))
        );
    }

    #[test]
    fn quantifier_with_comma_is_a_single_working_pattern() {
        let compiled = CompiledRules::compile(&rules(&[], &[], &[r"patient \d{1,3}"])).unwrap();
        assert_eq!(
            compiled.apply("hosp.exe", "patient 123 chart").unwrap().1,
            "[redacted] chart"
        );
    }

    #[test]
    fn spaces_and_hangul_are_matched_verbatim() {
        let compiled = CompiledRules::compile(&rules(
            &[],
            &["InPrivate - Microsoft Edge", "은행 거래"],
            &[],
        ))
        .unwrap();
        assert_eq!(
            compiled
                .apply("msedge.exe", "뉴스 - InPrivate - Microsoft Edge")
                .unwrap()
                .1,
            ""
        );
        assert_eq!(compiled.apply("app.exe", "은행 거래 내역").unwrap().1, "");
        assert_eq!(compiled.apply("app.exe", "은행").unwrap().1, "은행");
    }

    #[test]
    fn excluded_process_drops_session_case_insensitively() {
        let compiled = CompiledRules::compile(&rules(&["LockApp.exe"], &[], &[])).unwrap();
        assert!(compiled.apply("lockapp.exe", "Lock screen").is_none());
        assert!(compiled.apply("chrome.exe", "x").is_some());
    }

    #[test]
    fn mask_all_titles_keeps_session_without_title() {
        let mut value = rules(&[], &[], &["secret"]);
        value.mask_all_titles = true;
        let compiled = CompiledRules::compile(&value).unwrap();
        assert_eq!(
            compiled.apply("a.exe", "secret doc"),
            Some(("a.exe".into(), String::new()))
        );
    }

    #[test]
    fn syntax_error_is_reported_with_field_and_index() {
        let error = CompiledRules::compile(&rules(&[], &["ok", "(unclosed"], &[])).unwrap_err();
        assert_eq!(
            error,
            vec![InvalidRule {
                field: RuleField::ExcludedTitlePatterns,
                index: 1,
                problem: RuleProblem::Syntax
            }]
        );
    }

    #[test]
    fn empty_too_long_and_too_many_are_rejected() {
        let long = "a".repeat(MAX_RULE_CHARS + 1);
        let error = CompiledRules::compile(&rules(&["  "], &[long.as_str()], &[])).unwrap_err();
        assert!(error.contains(&InvalidRule {
            field: RuleField::ExcludedProcesses,
            index: 0,
            problem: RuleProblem::Empty
        }));
        assert!(error.contains(&InvalidRule {
            field: RuleField::ExcludedTitlePatterns,
            index: 0,
            problem: RuleProblem::TooLong
        }));
        let many: Vec<String> = (0..=MAX_RULES_PER_LIST).map(|n| format!("p{n}")).collect();
        let value = PrivacyRules {
            redact_title_patterns: many,
            ..PrivacyRules::default()
        };
        assert!(CompiledRules::compile(&value)
            .unwrap_err()
            .contains(&InvalidRule {
                field: RuleField::RedactTitlePatterns,
                index: MAX_RULES_PER_LIST,
                problem: RuleProblem::TooMany
            }));
    }

    #[test]
    fn fail_closed_never_keeps_a_title() {
        let compiled = CompiledRules::fail_closed();
        assert_eq!(
            compiled.apply("a.exe", "private"),
            Some(("a.exe".into(), String::new()))
        );
    }

    #[test]
    fn stored_rules_parse_missing_as_default_and_garbage_as_error() {
        assert_eq!(parse_stored_rules(None), Ok(PrivacyRules::default()));
        assert!(parse_stored_rules(Some("{not json")).is_err());
        assert_eq!(
            parse_stored_rules(Some(r#"{"maskAllTitles":true}"#)).unwrap(),
            PrivacyRules {
                mask_all_titles: true,
                ..PrivacyRules::default()
            }
        );
    }
}
