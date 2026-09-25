//! Activity's typed native API; no unregistered Tauri command shims.
use crate::commands::{autostart, digest, export, handoff, life, privacy, queries, tracking};
pub use crate::core::digest::{DigestDocument, DigestOrigin};
pub use crate::core::models::{AppTotal, DayPoint};
use serde::Deserialize;
use serde_json::Value;
use tauri::Manager as _;

#[derive(Debug, Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ActivityCall {
    GetDigest {
        input: crate::core::digest::DigestInput,
    },
    CancelDigest {},
    SaveDigest {
        request: crate::commands::digest::SaveDigestRequest,
    },
    SendDigestToKnowledge {
        input: crate::core::digest::DigestInput,
        regenerated_from: Option<String>,
    },
    KnowledgeDraftHistory {},
    ExportLifeLog {
        input: crate::core::export::ExportInput,
    },
    SaveLifeLog {
        input: crate::core::export::ExportInput,
    },
    SetProjects {
        paths: Vec<String>,
    },
    GetProjects {},
    ProbeProject {
        path: String,
    },
    GetDay {
        date: String,
        day_start: i64,
        day_end: i64,
    },
    GetRange {
        label: String,
        day_start: i64,
        day_end: i64,
    },
    StartTracking {},
    StopTracking {},
    IsTracking {},
    SetIdleThreshold {
        threshold_ms: i64,
    },
    GetIdleThreshold {},
    GetPrivacyRules {},
    SetPrivacyRules {
        rules: crate::core::privacy::PrivacyRules,
    },
    RedactExisting {},
    AutostartStatus {},
    SetAutostart {
        enabled: bool,
    },
    IntegrationSources {},
    ProjectAttribution {
        day_start: i64,
        day_end: i64,
    },
    Timeline {
        day_start: i64,
        day_end: i64,
    },
    AppStats {
        start: i64,
        end: i64,
    },
}

pub const METHODS: &[&str] = &[
    "get_digest",
    "cancel_digest",
    "save_digest",
    "send_digest_to_knowledge",
    "knowledge_draft_history",
    "export_life_log",
    "save_life_log",
    "set_projects",
    "get_projects",
    "probe_project",
    "get_day",
    "get_range",
    "start_tracking",
    "stop_tracking",
    "is_tracking",
    "set_idle_threshold",
    "get_idle_threshold",
    "get_privacy_rules",
    "set_privacy_rules",
    "redact_existing",
    "autostart_status",
    "set_autostart",
    "integration_sources",
    "project_attribution",
    "timeline",
    "app_stats",
];
impl ActivityCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::GetDigest { .. } => "get_digest",
            Self::CancelDigest { .. } => "cancel_digest",
            Self::SaveDigest { .. } => "save_digest",
            Self::SendDigestToKnowledge { .. } => "send_digest_to_knowledge",
            Self::KnowledgeDraftHistory { .. } => "knowledge_draft_history",
            Self::ExportLifeLog { .. } => "export_life_log",
            Self::SaveLifeLog { .. } => "save_life_log",
            Self::SetProjects { .. } => "set_projects",
            Self::GetProjects { .. } => "get_projects",
            Self::ProbeProject { .. } => "probe_project",
            Self::GetDay { .. } => "get_day",
            Self::GetRange { .. } => "get_range",
            Self::StartTracking { .. } => "start_tracking",
            Self::StopTracking { .. } => "stop_tracking",
            Self::IsTracking { .. } => "is_tracking",
            Self::SetIdleThreshold { .. } => "set_idle_threshold",
            Self::GetIdleThreshold { .. } => "get_idle_threshold",
            Self::GetPrivacyRules { .. } => "get_privacy_rules",
            Self::SetPrivacyRules { .. } => "set_privacy_rules",
            Self::RedactExisting { .. } => "redact_existing",
            Self::AutostartStatus { .. } => "autostart_status",
            Self::SetAutostart { .. } => "set_autostart",
            Self::IntegrationSources { .. } => "integration_sources",
            Self::ProjectAttribution { .. } => "project_attribution",
            Self::Timeline { .. } => "timeline",
            Self::AppStats { .. } => "app_stats",
        }
    }
}
product_ipc::issue_codes! { pub enum ActivityIssue {
    ActivityConsentSaveFailed = "activity_consent_save_failed",
    AutostartOwnerConflict = "autostart_owner_conflict",
    AutostartOwnerInvalid = "autostart_owner_invalid",
    AutostartSaveFailed = "autostart_save_failed",
    AutostartUnavailable = "autostart_unavailable",
    ClosePolicySaveFailed = "close_policy_save_failed",
    ClosePolicyUnavailable = "close_policy_unavailable",
    ComponentArgsInvalid = "component_args_invalid",
    ComponentResponseInvalid = "component_response_invalid",
    DigestBusy = "digest_busy",
    DigestCancelTimeout = "digest_cancel_timeout",
    DigestCancelled = "digest_cancelled",
    DigestHandleExpired = "digest_handle_expired",
    DigestOutputInvalid = "digest_output_invalid",
    ExportCancelled = "export_cancelled",
    ExportOutputInvalid = "export_output_invalid",
    GitInvalidArguments = "git_invalid_arguments",
    GitInvalidTarget = "git_invalid_target",
    GitOutputInvalid = "git_output_invalid",
    GitTimeout = "git_timeout",
    PrivacyRedactionFailed = "privacy_redaction_failed",
    PrivacyRulesInvalid = "privacy_rules_invalid",
    PrivacyRulesSaveFailed = "privacy_rules_save_failed",
    ProjectProbeFailed = "project_probe_failed",
    ProjectSettingsUnavailable = "project_settings_unavailable",
    SnapshotPayloadInvalid = "snapshot_payload_invalid",
    StoreBusy = "store_busy",
    TrayUnavailable = "tray_unavailable",
    TrackingStateUnavailable = "tracking_state_unavailable",
    Unavailable = "unavailable",
}}
pub fn classify(error: &str) -> &'static str {
    ActivityIssue::from_code(error)
        .unwrap_or(ActivityIssue::Unavailable)
        .code()
}
pub async fn dispatch(app: &tauri::AppHandle, call: ActivityCall) -> Result<Value, String> {
    match call {
        ActivityCall::GetDigest { input } => {
            to_value(digest::get_digest(app.state(), input).await?)
        }
        ActivityCall::CancelDigest {} => to_value(digest::cancel_digest(app.state()).await?),
        ActivityCall::SaveDigest { request } => {
            to_value(digest::save_digest(app.state(), request).await?)
        }
        ActivityCall::SendDigestToKnowledge {
            input,
            regenerated_from,
        } => {
            to_value(handoff::send_digest_to_knowledge(app.state(), input, regenerated_from).await?)
        }
        ActivityCall::KnowledgeDraftHistory {} => {
            to_value(handoff::knowledge_draft_history(app.state())?)
        }
        ActivityCall::ExportLifeLog { input } => {
            to_value(export::export_life_log(app.state(), input).await?)
        }
        ActivityCall::SaveLifeLog { input } => {
            to_value(export::save_life_log(app.state(), input).await?)
        }
        ActivityCall::SetProjects { paths } => to_value(life::set_projects(app.state(), paths)?),
        ActivityCall::GetProjects {} => to_value(life::get_projects(app.state())),
        ActivityCall::ProbeProject { path } => to_value(life::probe_project(path).await?),
        ActivityCall::GetDay {
            date,
            day_start,
            day_end,
        } => to_value(life::get_day(app.state(), date, day_start, day_end).await?),
        ActivityCall::GetRange {
            label,
            day_start,
            day_end,
        } => to_value(life::get_range(app.state(), label, day_start, day_end).await?),
        ActivityCall::StartTracking {} => to_value(tracking::start_tracking(app.state())?),
        ActivityCall::StopTracking {} => to_value(tracking::stop_tracking(app.state())?),
        ActivityCall::IsTracking {} => to_value(tracking::is_tracking(app.state())),
        ActivityCall::SetIdleThreshold { threshold_ms } => {
            to_value(tracking::set_idle_threshold(app.state(), threshold_ms)?)
        }
        ActivityCall::GetIdleThreshold {} => to_value(tracking::get_idle_threshold(app.state())),
        ActivityCall::GetPrivacyRules {} => to_value(privacy::get_privacy_rules(app.state())?),
        ActivityCall::SetPrivacyRules { rules } => {
            to_value(privacy::set_privacy_rules(app.state(), rules)?)
        }
        ActivityCall::RedactExisting {} => to_value(privacy::redact_existing(app.state())?),
        ActivityCall::AutostartStatus {} => to_value(autostart::product_status(app)?),
        ActivityCall::SetAutostart { enabled } => {
            to_value(autostart::set_product_autostart(app, enabled)?)
        }
        ActivityCall::IntegrationSources {} => to_value(life::integration_sources(app.state())),
        ActivityCall::ProjectAttribution { day_start, day_end } => {
            to_value(life::project_attribution(app.state(), day_start, day_end)?)
        }
        ActivityCall::Timeline { day_start, day_end } => {
            to_value(queries::timeline(app.state(), day_start, day_end)?)
        }
        ActivityCall::AppStats { start, end } => {
            to_value(queries::app_stats(app.state(), start, end)?)
        }
    }
}
fn to_value<T: serde::Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
}
pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    export.register::<crate::core::export::ExportDocument>()?;
    Ok(vec![
        (
            "get_digest",
            export.register::<crate::core::digest::DigestResponse>()?,
        ),
        ("cancel_digest", export.register::<bool>()?),
        (
            "save_digest",
            export.register::<crate::commands::digest::SaveDigestResult>()?,
        ),
        (
            "send_digest_to_knowledge",
            export.register::<crate::commands::handoff::SendKnowledgeDraftResult>()?,
        ),
        (
            "knowledge_draft_history",
            export.register::<Vec<crate::core::draft_history::DraftHistoryEntry>>()?,
        ),
        (
            "export_life_log",
            export.register::<crate::core::export::RenderedExport>()?,
        ),
        (
            "save_life_log",
            export.register::<crate::commands::export::SaveExportResult>()?,
        ),
        ("set_projects", export.register::<Vec<String>>()?),
        ("get_projects", export.register::<Vec<String>>()?),
        (
            "probe_project",
            export.register::<crate::commands::life::ProjectProbe>()?,
        ),
        (
            "get_day",
            export.register::<crate::core::models::DaySummary>()?,
        ),
        (
            "get_range",
            export.register::<crate::core::models::RangeSummary>()?,
        ),
        ("start_tracking", export.register::<bool>()?),
        ("stop_tracking", export.register::<()>()?),
        ("is_tracking", export.register::<bool>()?),
        ("set_idle_threshold", export.register::<()>()?),
        ("get_idle_threshold", export.register::<i64>()?),
        (
            "get_privacy_rules",
            export.register::<crate::commands::privacy::PrivacyRulesView>()?,
        ),
        (
            "set_privacy_rules",
            export.register::<crate::commands::privacy::PrivacySaveResult>()?,
        ),
        ("redact_existing", export.register::<i64>()?),
        (
            "autostart_status",
            export.register::<crate::commands::autostart::AutostartStatus>()?,
        ),
        (
            "set_autostart",
            export.register::<crate::commands::autostart::AutostartStatus>()?,
        ),
        (
            "integration_sources",
            export.register::<Vec<crate::commands::life::SourceStatus>>()?,
        ),
        (
            "project_attribution",
            export.register::<crate::commands::life::AttributionResult>()?,
        ),
        (
            "timeline",
            export.register::<Vec<crate::core::models::Session>>()?,
        ),
        (
            "app_stats",
            export.register::<Vec<crate::core::models::AppTotal>>()?,
        ),
    ])
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn listed_calls_parse_and_unknown_fields_are_refused() {
        for (value, method) in [
            (
                serde_json::json!({"method":"get_day","args":{"date":"2026-09-24","dayStart":0,"dayEnd":86400000}}),
                "get_day",
            ),
            (
                serde_json::json!({"method":"stop_tracking","args":{}}),
                "stop_tracking",
            ),
            (
                serde_json::json!({"method":"set_idle_threshold","args":{"thresholdMs":300000}}),
                "set_idle_threshold",
            ),
            (
                serde_json::json!({"method":"set_autostart","args":{"enabled":true}}),
                "set_autostart",
            ),
            (
                serde_json::json!({"method":"app_stats","args":{"start":0,"end":1}}),
                "app_stats",
            ),
        ] {
            let call: ActivityCall = serde_json::from_value(value).unwrap();
            assert_eq!(call.method(), method);
        }
        assert_eq!(METHODS.len(), 26);
        assert!(serde_json::from_value::<ActivityCall>(
            serde_json::json!({"method":"stop_tracking","args":{"unexpected":1}})
        )
        .is_err());
        assert_eq!(
            ActivityCall::CancelDigest {}.class(),
            ExecutionClass::Control
        );
    }
    #[test]
    fn unknown_errors_never_expose_free_form_values() {
        assert_eq!(classify("digest_cancelled"), "digest_cancelled");
        assert_eq!(classify("private_token_from_remote"), "unavailable");
    }
}
