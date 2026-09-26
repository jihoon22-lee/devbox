//! Input contracts for the product-owned definition and Registry operations.
//! Native Workspace owners execute these calls under their existing capabilities.
use product_ipc::workspace::{Lane, DEFAULT_BUDGET_MS, LONG_BUDGET_MS};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Deserialize, Serialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
pub enum EditTarget {
    Project,
    Local,
}
#[derive(Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditRequest {
    pub target: EditTarget,
    pub content: String,
    pub edit_revision: String,
}
#[derive(Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum RegistrationAction {
    Register,
    Rebind,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, ts_rs::TS)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileTarget {
    Windows,
    Wsl,
}
impl ProfileTarget {
    pub fn of(target: &product_contract::ExecutionTarget) -> Self {
        match target {
            product_contract::ExecutionTarget::Windows => Self::Windows,
            product_contract::ExecutionTarget::Wsl { .. } => Self::Wsl,
        }
    }
}
#[derive(Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WslTemplateRequest {
    pub template_id: String,
    pub distro_id: String,
    pub root: String,
    pub name: String,
    pub start_stopped: bool,
}
#[derive(Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(optional_fields = nullable)]
pub enum DefinitionsCall {
    Load {},
    PreviewTrust {},
    ApproveTrust { preview_id: String },
    RevokeTrust { revision: u64 },
    Cancel { preview_id: String },
    PreviewEdit(EditRequest),
    ApplyEdit { preview_id: String },
}
impl DefinitionsCall {
    pub const METHODS: &'static [&'static str] = &[
        "load",
        "preview_trust",
        "approve_trust",
        "revoke_trust",
        "cancel",
        "preview_edit",
        "apply_edit",
    ];
    pub fn method(&self) -> &'static str {
        match self {
            Self::Load { .. } => "load",
            Self::PreviewTrust { .. } => "preview_trust",
            Self::ApproveTrust { .. } => "approve_trust",
            Self::RevokeTrust { .. } => "revoke_trust",
            Self::Cancel { .. } => "cancel",
            Self::PreviewEdit(..) => "preview_edit",
            Self::ApplyEdit { .. } => "apply_edit",
        }
    }
    pub fn lane(&self) -> Lane {
        Lane::Probes
    }
    pub fn deadline_budget_ms(&self) -> u64 {
        DEFAULT_BUDGET_MS
    }
}
#[derive(Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(optional_fields = nullable)]
pub enum RegistryEngineCall {
    SaveTemplate {
        revision: u64,
        id: Option<String>,
        template: crate::component::ProfileTemplate,
    },
    ArchiveTemplate {
        revision: u64,
        id: String,
    },
    PreviewTemplateProfileWsl(WslTemplateRequest),
    PreviewTemplateProfileWindows {
        template_id: String,
        root: String,
        name: String,
    },
    UnbindImportedProfile {
        revision: u64,
        imported_id: String,
        target: ProfileTarget,
    },
    Snapshot {},
    SelectProject {
        context: product_contract::ProjectContext,
    },
    ClearProject {},
    ListWslDistros {},
    PreviewWsl {
        distro_id: String,
        root: String,
        start_stopped: bool,
    },
    PreviewWindows {
        root: String,
    },
    CancelRegistration {
        preview_id: String,
    },
    ApplyRegistration {
        preview_id: String,
        name: String,
        action: RegistrationAction,
    },
    Rename {
        revision: u64,
        project_id: String,
        name: String,
    },
    Remove {
        revision: u64,
        context: product_contract::ProjectContext,
    },
}
impl RegistryEngineCall {
    pub const METHODS: &'static [&'static str] = &[
        "save_template",
        "archive_template",
        "preview_template_profile_wsl",
        "preview_template_profile_windows",
        "unbind_imported_profile",
        "snapshot",
        "select_project",
        "clear_project",
        "list_wsl_distros",
        "preview_wsl",
        "preview_windows",
        "cancel_registration",
        "apply_registration",
        "rename",
        "remove",
    ];
    pub fn method(&self) -> &'static str {
        match self {
            Self::SaveTemplate { .. } => "save_template",
            Self::ArchiveTemplate { .. } => "archive_template",
            Self::PreviewTemplateProfileWsl(..) => "preview_template_profile_wsl",
            Self::PreviewTemplateProfileWindows { .. } => "preview_template_profile_windows",
            Self::UnbindImportedProfile { .. } => "unbind_imported_profile",
            Self::Snapshot { .. } => "snapshot",
            Self::SelectProject { .. } => "select_project",
            Self::ClearProject { .. } => "clear_project",
            Self::ListWslDistros { .. } => "list_wsl_distros",
            Self::PreviewWsl { .. } => "preview_wsl",
            Self::PreviewWindows { .. } => "preview_windows",
            Self::CancelRegistration { .. } => "cancel_registration",
            Self::ApplyRegistration { .. } => "apply_registration",
            Self::Rename { .. } => "rename",
            Self::Remove { .. } => "remove",
        }
    }
    pub fn lane(&self) -> Lane {
        match self {
            Self::PreviewTemplateProfileWsl(..)
            | Self::PreviewWindows { .. }
            | Self::ListWslDistros {}
            | Self::PreviewWsl { .. }
            | Self::PreviewTemplateProfileWindows { .. }
            | Self::SelectProject { .. } => Lane::Probes,
            _ => Lane::Metadata,
        }
    }
    pub fn deadline_budget_ms(&self) -> u64 {
        match self {
            Self::ListWslDistros {}
            | Self::PreviewWsl { .. }
            | Self::ApplyRegistration { .. }
            | Self::CancelRegistration { .. }
            | Self::SelectProject { .. }
            | Self::ClearProject {} => LONG_BUDGET_MS,
            _ => DEFAULT_BUDGET_MS,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use product_ipc::workspace::Lane;
    #[test]
    fn definition_and_registry_calls_preserve_the_existing_wire_shape() {
        let edit:DefinitionsCall=serde_json::from_str(r#"{"method":"preview_edit","args":{"target":"project","content":"{}","editRevision":"revision"}}"#).unwrap();
        assert_eq!(edit.method(), "preview_edit");
        assert_eq!(edit.lane(), Lane::Probes);
        let preview:RegistryEngineCall=serde_json::from_str(r#"{"method":"preview_wsl","args":{"distroId":"distro","root":"/fixture","startStopped":false}}"#).unwrap();
        assert_eq!(preview.deadline_budget_ms(), 29_000);
        assert!(serde_json::from_str::<RegistryEngineCall>(
            r#"{"method":"start_workspace","args":{}}"#
        )
        .is_err());
    }
}
