//! Host-only project associations layered over the Activity engine response.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
pub enum AssociationState {
    Mapped,
    Unmapped,
    Unavailable,
    Ambiguous,
    Offline,
    Missing,
    Unverified,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
pub struct ProjectAssociation {
    pub state: AssociationState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<product_contract::ProjectContext>,
}
#[derive(Serialize, Deserialize, ts_rs::TS)]
pub struct ActivityProjectCommit {
    pub path: String,
    pub commits: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(
        rename = "projectAssociation",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub project_association: Option<ProjectAssociation>,
}
#[derive(Serialize, Deserialize, ts_rs::TS)]
pub struct ActivityGitDay {
    pub projects: Vec<ActivityProjectCommit>,
    pub total_commits: u32,
}
#[derive(Serialize, Deserialize, ts_rs::TS)]
pub struct ActivityDaySummary {
    pub date: String,
    pub pc_usage_ms: i64,
    pub app_totals: Vec<activity_engine::api::AppTotal>,
    pub git: ActivityGitDay,
}
#[derive(Serialize, Deserialize, ts_rs::TS)]
pub struct ActivityRangeSummary {
    pub label: String,
    pub pc_usage_ms: i64,
    pub app_totals: Vec<activity_engine::api::AppTotal>,
    pub git: ActivityGitDay,
    pub daily: Vec<activity_engine::api::DayPoint>,
}
#[derive(Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ActivityDigestResponse {
    pub origin: activity_engine::api::DigestOrigin,
    pub document: activity_engine::api::DigestDocument,
    pub markdown: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_associations: Option<BTreeMap<String, ProjectAssociation>>,
}
#[derive(Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ActivityAttribution {
    pub project_id: String,
    pub sessions: usize,
    pub duration_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_association: Option<ProjectAssociation>,
}
#[derive(Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ActivityAttributionResult {
    pub attributed: Vec<ActivityAttribution>,
    pub unattributed: ActivityAttribution,
    pub profile_count: usize,
}
fn typed<T: serde::de::DeserializeOwned + Serialize>(value: Value) -> Result<Value, String> {
    let value: T = serde_json::from_value(value).map_err(|_| "component_response_invalid")?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
}
pub fn validate(method: &str, value: Value) -> Result<Value, String> {
    match method {
        "get_day" => typed::<ActivityDaySummary>(value),
        "get_range" => typed::<ActivityRangeSummary>(value),
        "get_digest" => typed::<ActivityDigestResponse>(value),
        "project_attribution" => typed::<ActivityAttributionResult>(value),
        _ => Ok(value),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_projection_preserves_associations_and_absent_optional_fields() {
        for association in [
            None,
            Some(serde_json::json!({"state":"unmapped"})),
            Some(
                serde_json::json!({"state":"mapped","context":{"projectId":"p","worktreeId":"w","target":{"kind":"windows"},"revision":1}}),
            ),
        ] {
            let mut project = serde_json::json!({"path":"C:/fixture","commits":1});
            if let Some(association) = association {
                project["projectAssociation"] = association;
            }
            let value = serde_json::json!({"date":"2026-09-24","pc_usage_ms":0,"app_totals":[],"git":{"projects":[project],"total_commits":1}});
            assert_eq!(validate("get_day", value.clone()).unwrap(), value);
        }
    }
}
