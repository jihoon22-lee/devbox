//! Bounded GitHub CLI JSON projections; diagnostics map to fixed issue codes.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct CheckSummary {
    pub passed: u32,
    pub failed: u32,
    pub pending: u32,
}
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub url: String,
    pub is_draft: bool,
    pub head: String,
    pub base: String,
    pub review_decision: Option<String>,
    pub checks: CheckSummary,
}
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct PrListItem {
    pub number: u64,
    pub title: String,
    pub head: String,
    pub author: String,
    pub updated_at: String,
    pub is_draft: bool,
    pub url: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct View {
    number: u64,
    title: String,
    state: String,
    url: String,
    is_draft: bool,
    head_ref_name: String,
    base_ref_name: String,
    review_decision: Option<String>,
    status_check_rollup: Option<Vec<Check>>,
}
#[derive(Deserialize)]
struct Check {
    conclusion: Option<String>,
    state: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListItem {
    number: u64,
    title: String,
    head_ref_name: String,
    author: Option<Author>,
    updated_at: String,
    is_draft: bool,
    url: String,
}
#[derive(Deserialize)]
struct Author {
    login: String,
}
pub fn valid_url(value: &str) -> bool {
    value.starts_with("https://")
        && value.len() <= 4096
        && value.len() > 8
        && !value.chars().any(char::is_control)
}
pub fn parse_view(json: &str) -> Result<PullRequest, String> {
    let view: View = serde_json::from_str(json).map_err(|_| "pr_failed")?;
    if !valid_url(&view.url) {
        return Err("pr_failed".into());
    }
    let mut checks = CheckSummary::default();
    for check in view.status_check_rollup.unwrap_or_default() {
        // StatusContext uses state; CheckRun uses conclusion.
        match check
            .conclusion
            .as_deref()
            .filter(|value| !value.is_empty())
            .or(check.state.as_deref())
            .unwrap_or("")
        {
            "SUCCESS" | "NEUTRAL" | "SKIPPED" => checks.passed += 1,
            "FAILURE" | "ERROR" | "CANCELLED" | "TIMED_OUT" | "ACTION_REQUIRED"
            | "STARTUP_FAILURE" => checks.failed += 1,
            _ => checks.pending += 1,
        }
    }
    Ok(PullRequest {
        number: view.number,
        title: view.title,
        state: view.state,
        url: view.url,
        is_draft: view.is_draft,
        head: view.head_ref_name,
        base: view.base_ref_name,
        review_decision: view.review_decision,
        checks,
    })
}
pub fn parse_list(json: &str) -> Result<Vec<PrListItem>, String> {
    let items: Vec<ListItem> = serde_json::from_str(json).map_err(|_| "pr_failed")?;
    if items.len() > 30 || items.iter().any(|item| !valid_url(&item.url)) {
        return Err("pr_failed".into());
    }
    Ok(items
        .into_iter()
        .map(|item| PrListItem {
            number: item.number,
            title: item.title,
            head: item.head_ref_name,
            author: item.author.map(|a| a.login).unwrap_or_default(),
            updated_at: item.updated_at,
            is_draft: item.is_draft,
            url: item.url,
        })
        .collect())
}
pub fn classify_gh_failure(stderr: &str) -> &'static str {
    if stderr.contains("must first push") || stderr.contains("not yet pushed") {
        "pr_branch_not_pushed"
    } else if stderr.contains("pull request") && stderr.contains("already exists") {
        "pr_exists"
    } else {
        "pr_failed"
    }
}

// core/pull_requests.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_json_summarizes_checks() {
        let json = r#"{"number":42,"title":"Fix login","state":"OPEN","url":"https://github.com/me/devbox/pull/42","isDraft":false,
            "headRefName":"agent/fix-login","baseRefName":"main","reviewDecision":"REVIEW_REQUIRED",
            "statusCheckRollup":[{"conclusion":"SUCCESS"},{"conclusion":"FAILURE"},{"status":"IN_PROGRESS","conclusion":""},{"conclusion":"SKIPPED"}]}"#;
        let pr = parse_view(json).unwrap();
        assert_eq!(
            (pr.number, pr.head.as_str(), pr.base.as_str()),
            (42, "agent/fix-login", "main")
        );
        assert_eq!(
            pr.checks,
            CheckSummary {
                passed: 2,
                failed: 1,
                pending: 1
            }
        );
    }

    #[test]
    fn gh_failures_map_to_codes() {
        assert_eq!(
            classify_gh_failure("aborted: you must first push the current branch to a remote"),
            "pr_branch_not_pushed"
        );
        assert_eq!(classify_gh_failure("a pull request for branch \"x\" into branch \"main\" already exists:\nhttps://github.com/me/r/pull/1"), "pr_exists");
        assert_eq!(classify_gh_failure("something else"), "pr_failed");
    }
    #[test]
    fn lists_preserve_deleted_authors_and_reject_unsafe_urls() {
        let items = parse_list(r#"[{"number":1,"title":"one","headRefName":"feature","author":null,"updatedAt":"today","isDraft":true,"url":"https://example.test/pr/1"}]"#).unwrap();
        assert!(items[0].author.is_empty() && items[0].is_draft);
        assert!(parse_list(r#"[{"number":1,"title":"one","headRefName":"feature","author":null,"updatedAt":"today","isDraft":true,"url":"file:///private"}]"#).is_err());
    }
}
