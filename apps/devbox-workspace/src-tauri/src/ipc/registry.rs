use product_ipc::{workspace::Lane, ComponentCall};
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(transparent)]
pub struct RegistryCall(pub projects_engine::api::RegistryEngineCall);
pub const METHODS: &[&str] = &[
    "apply_registration",
    "archive_template",
    "cancel_registration",
    "clear_project",
    "list_wsl_distros",
    "preview_template_profile_windows",
    "preview_template_profile_wsl",
    "preview_windows",
    "preview_wsl",
    "remove",
    "rename",
    "save_template",
    "select_project",
    "snapshot",
    "unbind_imported_profile",
];
pub fn routes_for(method: &str) -> &'static [&'static str] {
    match method {
        "apply_registration" => &[
            "dependencies",
            "files",
            "logs",
            "overview",
            "runtime",
            "source",
            "tasks",
        ],
        "archive_template" => &["overview"],
        "cancel_registration" => &[
            "dependencies",
            "files",
            "logs",
            "overview",
            "runtime",
            "source",
            "tasks",
        ],
        "clear_project" => &[
            "dependencies",
            "files",
            "logs",
            "overview",
            "runtime",
            "source",
            "tasks",
        ],
        "list_wsl_distros" => &["overview"],
        "preview_template_profile_windows" => &[
            "dependencies",
            "files",
            "logs",
            "overview",
            "runtime",
            "source",
            "tasks",
        ],
        "preview_template_profile_wsl" => &["overview"],
        "preview_windows" => &[
            "dependencies",
            "files",
            "logs",
            "overview",
            "runtime",
            "source",
            "tasks",
        ],
        "preview_wsl" => &["overview"],
        "remove" => &[
            "dependencies",
            "files",
            "logs",
            "overview",
            "runtime",
            "source",
            "tasks",
        ],
        "rename" => &[
            "dependencies",
            "files",
            "logs",
            "overview",
            "runtime",
            "source",
            "tasks",
        ],
        "save_template" => &["overview"],
        "select_project" => &[
            "dependencies",
            "files",
            "logs",
            "overview",
            "runtime",
            "source",
            "tasks",
        ],
        "snapshot" => &[
            "dependencies",
            "files",
            "logs",
            "overview",
            "runtime",
            "source",
            "tasks",
        ],
        "unbind_imported_profile" => &[
            "dependencies",
            "files",
            "logs",
            "overview",
            "runtime",
            "source",
            "tasks",
        ],
        _ => &[],
    }
}
impl ComponentCall for RegistryCall {
    const COMPONENT: &'static str = "workspace.registry";
    const IMPORT_PHASE: bool = false;
    const SHARED_REQUEST_LIMIT: bool = false;
    const MAX_ARGUMENT_BYTES: usize = 65536;
    fn valid_arguments(method: &str, args: &serde_json::Value) -> bool {
        let _ = method;
        let limit = Self::MAX_ARGUMENT_BYTES;
        super::bounded_arguments(args, limit)
    }
    fn method(&self) -> &'static str {
        self.0.method()
    }
    fn routes(&self) -> &'static [&'static str] {
        routes_for(self.method())
    }
}
impl RegistryCall {
    pub fn lane(&self) -> Lane {
        self.0.lane()
    }
    pub fn deadline_budget_ms(&self) -> u64 {
        deadline_budget_for(self.method())
    }
}
pub fn deadline_budget_for(method: &str) -> u64 {
    super::deadlines::budget("workspace.registry", method)
}
#[tauri::command]
pub(crate) async fn registry(
    window: tauri::WebviewWindow,
    runtime: tauri::State<'_, crate::component::Runtime>,
    request: product_ipc::IncomingRequest,
) -> Result<product_shell_tauri::Reply, product_contract::Problem> {
    super::execute::<RegistryCall>(window, runtime, request).await
}
impl super::WorkspaceCall for RegistryCall {
    fn into_call(self) -> super::Call {
        super::Call::Registry(self)
    }
}

use crate::host::Host;
use serde_json::{json, Value};

pub(crate) fn dispatch(host: &Host, call: RegistryCall) -> Result<Value, &'static str> {
    use projects_engine::api::RegistryEngineCall as C;
    match call.0 {
        C::SaveTemplate {
            revision,
            id,
            template,
        } => Ok(json!(host.projects()?.save_template(
            revision,
            id.as_deref(),
            template
        )?)),
        C::ArchiveTemplate { revision, id } => {
            Ok(json!(host.projects()?.archive_template(revision, &id)?))
        }
        C::PreviewTemplateProfileWsl(request) => Ok(json!(host
            .projects()?
            .preview_template_profile_wsl(host.helper_directory()?, request)?)),
        C::PreviewTemplateProfileWindows {
            template_id,
            root,
            name,
        } => Ok(json!(host.projects()?.preview_template_profile_windows(
            &template_id,
            &root,
            &name
        )?)),
        C::UnbindImportedProfile {
            revision,
            imported_id,
            target,
        } => Ok(json!(host.projects()?.unbind_imported_profile(
            revision,
            &imported_id,
            target
        )?)),
        C::Snapshot {} => Ok(json!(host.projects()?.snapshot()?)),
        C::SelectProject { context } => {
            let binding = if cfg!(windows)
                && matches!(
                    context.target,
                    product_contract::ExecutionTarget::Wsl { .. }
                ) {
                host.projects()?
                    .admit_selection(host.helper_directory()?, &context)?
            } else {
                host.projects()?.admit(&context)?.binding().clone()
            };
            Ok(json!(RegistrySelection { context, binding }))
        }
        C::ClearProject {} => Err("invalid_request"), // session replacement runs on the admitted window
        C::ListWslDistros {} => Ok(json!(crate::platform::wsl_distro::list()?)),
        C::PreviewWsl {
            distro_id,
            root,
            start_stopped,
        } => Ok(json!(host.projects()?.preview_wsl(
            host.helper_directory()?,
            &distro_id,
            &root,
            start_stopped
        )?)),
        C::PreviewWindows { root } => Ok(json!(host.projects()?.preview_windows(&root)?)),
        C::CancelRegistration { preview_id } => {
            host.projects()?.cancel(&preview_id)?;
            Ok(json!({}))
        }
        C::ApplyRegistration {
            preview_id,
            name,
            action,
        } => {
            let (registry, context) = host.projects()?.apply(&preview_id, &name, action)?;
            Ok(json!(RegistrationApplied { registry, context }))
        }
        C::Rename {
            revision,
            project_id,
            name,
        } => Ok(json!(host.projects()?.rename(
            revision,
            &project_id,
            &name
        )?)),
        C::Remove { revision, context } => {
            Ok(json!(host.projects()?.remove(revision, &context)?))
        }
    }
}
#[derive(serde::Serialize, ts_rs::TS)]
pub struct RegistrySelection {
    pub context: product_contract::ProjectContext,
    pub binding: crate::core::registry::Binding,
}
#[derive(serde::Serialize, ts_rs::TS)]
pub struct RegistrationApplied {
    pub registry: crate::core::registry::Registry,
    pub context: product_contract::ProjectContext,
}

pub(crate) fn project_probe(method: &str) -> bool {
    matches!(
        method,
        "preview_template_profile_wsl"
            | "preview_windows"
            | "list_wsl_distros"
            | "preview_wsl"
            | "preview_template_profile_windows"
            | "select_project"
    )
}

pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    export.register::<RegistryCall>()?;
    Ok(vec![
        (
            "snapshot",
            export.register::<crate::core::registry::Registry>()?,
        ),
        (
            "save_template",
            export.register::<crate::core::registry::Registry>()?,
        ),
        (
            "archive_template",
            export.register::<crate::core::registry::Registry>()?,
        ),
        (
            "unbind_imported_profile",
            export.register::<crate::core::registry::Registry>()?,
        ),
        (
            "rename",
            export.register::<crate::core::registry::Registry>()?,
        ),
        (
            "remove",
            export.register::<crate::core::registry::Registry>()?,
        ),
        (
            "preview_windows",
            export.register::<crate::project_owner::RegistrationPreview>()?,
        ),
        (
            "preview_wsl",
            export.register::<crate::project_owner::RegistrationPreview>()?,
        ),
        (
            "preview_template_profile_windows",
            export.register::<crate::project_owner::RegistrationPreview>()?,
        ),
        (
            "preview_template_profile_wsl",
            export.register::<crate::project_owner::RegistrationPreview>()?,
        ),
        ("select_project", export.register::<RegistrySelection>()?),
        (
            "clear_project",
            export.register::<super::results::ContextCleared>()?,
        ),
        (
            "list_wsl_distros",
            export.register::<Vec<crate::platform::wsl_distro::Distro>>()?,
        ),
        (
            "cancel_registration",
            export.register::<super::results::EmptyReply>()?,
        ),
        (
            "apply_registration",
            export.register::<RegistrationApplied>()?,
        ),
    ])
}
