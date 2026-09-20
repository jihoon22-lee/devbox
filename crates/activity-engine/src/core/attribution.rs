//! 활동 → 프로젝트 귀속 (순수 로직).
//!
//! [설계] 귀속 규칙: 세션 창 제목에서 각 프로젝트 디렉터리 basename을
//! 대소문자 무시 substring 매치한다. **가장 긴 basename**이 이긴다
//! (중첩 프로젝트에서 모호성 최소화). 한 세션은 최대 한 프로젝트에 귀속 —
//! 중복 집계가 없다. 매치 없으면 미귀속.

/// 프로젝트 매치 후보. `basenames`는 이 프로젝트를 식별할 수 있는 이름들이다
/// (디렉터리 basename + 사용자 별칭). 긴 것이 우선한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectMatch {
    pub project_id: String,
    pub basenames: Vec<String>,
}

/// 귀속 결과 하나.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attribution {
    pub project_id: String,
    pub sessions: usize,
    pub duration_ms: i64,
}

/// 세션 제목을 프로젝트에 귀속한다. 가장 긴 basename 매치를 반환한다.
pub fn attribute_title<'a>(title: &str, profiles: &'a [ProjectMatch]) -> Option<&'a ProjectMatch> {
    let title_lower = title.to_lowercase();
    profiles
        .iter()
        .filter(|p| {
            p.basenames
                .iter()
                .any(|b| !b.is_empty() && title_lower.contains(&b.to_lowercase()))
        })
        .max_by_key(|p| p.basenames.iter().map(|b| b.len()).max().unwrap_or(0))
}

/// 세션 목록을 프로젝트별로 집계한다. 미귀속은 별도로 반환한다.
pub fn attribute_sessions(
    sessions: &[(String, String, i64)], // (app, title, duration_ms)
    profiles: &[ProjectMatch],
) -> (Vec<Attribution>, Attribution) {
    use std::collections::HashMap;
    let mut map: HashMap<String, (usize, i64)> = HashMap::new();
    let mut unattributed = (0usize, 0i64);
    for (_app, title, duration) in sessions {
        match attribute_title(title, profiles) {
            Some(profile) => {
                let entry = map.entry(profile.project_id.clone()).or_insert((0, 0));
                entry.0 += 1;
                entry.1 += duration;
            }
            None => {
                unattributed.0 += 1;
                unattributed.1 += duration;
            }
        }
    }
    let mut attributed: Vec<Attribution> = map
        .into_iter()
        .map(|(project_id, (sessions, duration_ms))| Attribution {
            project_id,
            sessions,
            duration_ms,
        })
        .collect();
    attributed.sort_by_key(|a| std::cmp::Reverse(a.duration_ms));
    (
        attributed,
        Attribution {
            project_id: "unattributed".into(),
            sessions: unattributed.0,
            duration_ms: unattributed.1,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(id: &str, names: &[&str]) -> ProjectMatch {
        ProjectMatch {
            project_id: id.into(),
            basenames: names.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn matches_basename_in_title() {
        let profiles = [
            profile("devbox", &["devbox"]),
            profile("familycard", &["familycard"]),
        ];
        assert_eq!(
            attribute_title("devbox — app.tsx — VS Code", &profiles)
                .unwrap()
                .project_id,
            "devbox"
        );
        assert_eq!(
            attribute_title("FamilyCard - Main.java - IntelliJ", &profiles)
                .unwrap()
                .project_id,
            "familycard"
        );
    }

    #[test]
    fn longest_basename_wins() {
        let profiles = [
            profile("outer", &["devbox"]),
            profile("inner", &["devbox-api"]),
        ];
        assert_eq!(
            attribute_title("devbox-api — readme", &profiles)
                .unwrap()
                .project_id,
            "inner"
        );
    }

    #[test]
    fn case_insensitive() {
        let profiles = [profile("devbox", &["DevBox"])];
        assert!(attribute_title("DEVBOX 작업", &profiles).is_some());
    }

    #[test]
    fn no_match_is_unattributed() {
        let profiles = [profile("devbox", &["devbox"])];
        assert!(attribute_title("chrome — GitHub", &profiles).is_none());
    }

    #[test]
    fn aggregates_without_double_count() {
        let profiles = [profile("devbox", &["devbox"])];
        let sessions = vec![
            ("Code".to_string(), "devbox work".to_string(), 100),
            ("Code".to_string(), "devbox more".to_string(), 200),
            ("chrome".to_string(), "GitHub".to_string(), 300),
            ("chrome".to_string(), "devbox docs".to_string(), 50),
        ];
        let (attributed, unattributed) = attribute_sessions(&sessions, &profiles);
        assert_eq!(attributed[0].project_id, "devbox");
        assert_eq!(attributed[0].sessions, 3); // 중복 없이 3개
        assert_eq!(attributed[0].duration_ms, 350);
        assert_eq!(unattributed.sessions, 1);
        assert_eq!(unattributed.duration_ms, 300);
    }
}
