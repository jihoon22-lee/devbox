//! B03 receiver contract for the B06 native session provider. No renderer method
//! accepts this input. The owner supplies the exact registered binding separately.
use crate::ProjectContext;
use serde::{Deserialize, Serialize};

pub const KIND: &str = "knowledge-session/v1";
pub const PRODUCER: &str = "devbox-workspace";
pub const MAX_INPUT_BYTES: usize = 16 * 1024;
const INVALID: &str = "session_summary_invalid";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Binding {
    pub context: ProjectContext,
    pub session_id: String,
    pub revision: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum ProblemCategory {
    BuildFailed,
    TestFailed,
    ReadinessFailed,
    PortConflict,
    ToolMissing,
    DependencyUnavailable,
    DiagnosticsError,
    DiagnosticsWarning,
}
impl ProblemCategory {
    fn label(self) -> &'static str {
        match self {
            Self::BuildFailed => "빌드 실패",
            Self::TestFailed => "테스트 실패",
            Self::ReadinessFailed => "준비 상태 확인 실패",
            Self::PortConflict => "포트 충돌",
            Self::ToolMissing => "필수 도구 없음",
            Self::DependencyUnavailable => "의존성 확인 불가",
            Self::DiagnosticsError => "진단 오류",
            Self::DiagnosticsWarning => "진단 경고",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectedProblem {
    pub category: ProblemCategory,
    pub count: u32,
}

/// None means unavailable, never a successful zero. Counts cover exactly the
/// provider's session and recorded interval, not a latest/today snapshot.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Metadata {
    pub schema_version: u32,
    pub binding: Binding,
    pub started_at: String,
    pub through_at: String,
    pub failed_runs: Option<u32>,
    pub git_commits: Option<u32>,
    pub selected_problems: Vec<SelectedProblem>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Draft {
    pub metadata: Metadata,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
}

pub fn valid_date_key(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| !matches!(index, 4 | 7) && !byte.is_ascii_digit())
    {
        return false;
    }
    let year = value[0..4].parse::<i32>().ok();
    let month = value[5..7].parse::<u32>().ok();
    let day = value[8..10].parse::<u32>().ok();
    let (Some(year), Some(month), Some(day)) = (year, month, day) else {
        return false;
    };
    year > 0 && (1..=12).contains(&month) && (1..=days_in_month(year, month)).contains(&day)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn utc(value: &str) -> bool {
    let b = value.as_bytes();
    b.len() == 20
        && b.is_ascii()
        && valid_date_key(&value[..10])
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && b[19] == b'Z'
        && [11, 12, 14, 15, 17, 18]
            .into_iter()
            .all(|i| b[i].is_ascii_digit())
        && &value[11..13] < "24"
        && &value[14..16] < "60"
        && &value[17..19] < "60"
}
fn validate(metadata: &Metadata) -> Result<(), &'static str> {
    let binding = &metadata.binding;
    if metadata.schema_version != 1
        || binding.context.validate().is_err()
        || binding.session_id.is_empty()
        || binding.session_id.len() > 128
        || !binding
            .session_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        || binding.revision == 0
        || binding.revision > 9_007_199_254_740_991
        || !utc(&metadata.started_at)
        || !utc(&metadata.through_at)
        || metadata.started_at > metadata.through_at
        || metadata.failed_runs.is_some_and(|v| v > 1_000_000)
        || metadata.git_commits.is_some_and(|v| v > 1_000_000)
        || metadata.selected_problems.len() > 8
        || metadata
            .selected_problems
            .iter()
            .any(|p| p.count == 0 || p.count > 1_000_000)
    {
        return Err(INVALID);
    }
    let mut seen = std::collections::BTreeSet::new();
    if metadata
        .selected_problems
        .iter()
        .any(|p| !seen.insert(p.category))
    {
        return Err(INVALID);
    }
    Ok(())
}

fn render(mut metadata: Metadata) -> Draft {
    metadata.selected_problems.sort_by_key(|p| p.category);
    let count = |value: Option<u32>| value.map_or_else(|| "확인 불가".into(), |v| format!("{v}개"));
    let title = format!("개발 세션 요약 · {}", &metadata.started_at[..10]);
    // Opaque project/session IDs are provenance only and never Markdown content.
    let mut body = format!("# 개발 세션 요약\n\n기간: {} ~ {} (UTC)\n\n- 실패한 실행: {}\n- Git 커밋: {}\n\n## 선택한 문제\n\n", metadata.started_at, metadata.through_at, count(metadata.failed_runs), count(metadata.git_commits));
    if metadata.selected_problems.is_empty() {
        body.push_str("선택한 문제가 없습니다.\n");
    }
    for problem in &metadata.selected_problems {
        body.push_str(&format!(
            "- {}: {}개\n",
            problem.category.label(),
            problem.count
        ));
    }
    Draft {
        metadata,
        title,
        body,
        tags: vec!["development-session".into(), "summary".into()],
    }
}

pub fn prepare(bytes: &[u8], expected: &Binding) -> Result<Draft, &'static str> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(INVALID);
    }
    let metadata: Metadata = serde_json::from_slice(bytes).map_err(|_| INVALID)?;
    validate(&metadata)?;
    if &metadata.binding != expected {
        return Err("session_summary_stale");
    }
    Ok(render(metadata))
}

impl Draft {
    pub fn validate(&self) -> Result<(), &'static str> {
        validate(&self.metadata)?;
        if self != &render(self.metadata.clone()) {
            return Err(INVALID);
        }
        Ok(())
    }
    pub fn note_stem(&self) -> String {
        format!(
            "Journal/{}-development-session",
            &self.metadata.started_at[..10]
        )
    }
}

#[cfg(test)]
pub(crate) fn fixture() -> Metadata {
    serde_json::from_str(include_str!("../tests/fixtures/session-summary-v1.json")).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixture_provider_renders_exact_interval_and_canonical_allowlisted_metadata() {
        let m = fixture();
        let draft = prepare(&serde_json::to_vec(&m).unwrap(), &m.binding).unwrap();
        draft.validate().unwrap();
        assert!(draft
            .body
            .contains("2026-09-07T23:00:00Z ~ 2026-09-08T01:00:00Z"));
        assert!(draft
            .body
            .contains("실패한 실행: 2개\n- Git 커밋: 확인 불가"));
        assert!(draft.body.find("빌드 실패").unwrap() < draft.body.find("테스트 실패").unwrap());
        assert!(!draft.body.contains("fixture-"));
        let mut reordered = m.clone();
        reordered.selected_problems.reverse();
        assert_eq!(
            draft,
            prepare(&serde_json::to_vec(&reordered).unwrap(), &m.binding).unwrap()
        );
    }
    #[test]
    fn rejects_unknown_sensitive_fields_invalid_bounds_and_other_session_revisions() {
        let m = fixture();
        let value = serde_json::to_value(&m).unwrap();
        for field in ["command", "log", "windowTitle", "secret", "body"] {
            let mut injected = value.clone();
            injected[field] = "synthetic-private-value".into();
            assert!(prepare(&serde_json::to_vec(&injected).unwrap(), &m.binding).is_err());
        }
        let mut expected = m.binding.clone();
        expected.revision += 1;
        assert_eq!(
            prepare(&serde_json::to_vec(&m).unwrap(), &expected).unwrap_err(),
            "session_summary_stale"
        );
        expected = m.binding.clone();
        expected.context.worktree_id = "another-worktree".into();
        assert!(prepare(&serde_json::to_vec(&m).unwrap(), &expected).is_err());
        for date in [
            "2026-02-30T01:00:00Z",
            "2026-09-08T24:00:00Z",
            "💥💥💥💥💥",
            "2026-09-06T01:00:00Z",
        ] {
            let mut invalid = m.clone();
            invalid.through_at = date.into();
            assert!(prepare(&serde_json::to_vec(&invalid).unwrap(), &m.binding).is_err());
        }
        let mut invalid = m.clone();
        invalid
            .selected_problems
            .push(invalid.selected_problems[0].clone());
        assert!(prepare(&serde_json::to_vec(&invalid).unwrap(), &m.binding).is_err());
        assert!(prepare(&vec![b' '; MAX_INPUT_BYTES + 1], &m.binding).is_err());
        let mut draft = prepare(&serde_json::to_vec(&m).unwrap(), &m.binding).unwrap();
        draft.body.push_str("\nsynthetic-private-value");
        assert!(draft.validate().is_err());
    }
}
