use crate::core::api_workspace::Store;
use product_contract::context::ProjectContext;
use serde_json::Value;
use tauri::Manager;

const STORAGE: &str = "api_workspace_unavailable";
fn mock_profiles(
    root: &std::path::Path,
) -> Result<Vec<crate::ipc::workspace::MockProfileChoice>, String> {
    use webhook_core::core::service_profile as profiles;
    let root = root.join("webhooks");
    let directory = root.join(profiles::SERVICE_PROFILE_DIRECTORY);
    match std::fs::symlink_metadata(&directory) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(_) => return Err(STORAGE.into()),
        Ok(_) => devbox_filesystem::ensure_no_links(&directory).map_err(|_| STORAGE)?,
    }
    let mut result = vec![];
    for (index, entry) in std::fs::read_dir(&directory)
        .map_err(|_| STORAGE)?
        .enumerate()
    {
        if index >= profiles::MAX_PROFILE_DIRECTORY_ENTRIES {
            return Err(STORAGE.into());
        }
        let entry = entry.map_err(|_| STORAGE)?;
        let name = entry.file_name().into_string().map_err(|_| STORAGE)?;
        let Some(id) = name.strip_suffix(".json") else {
            continue;
        };
        if result.len() >= profiles::MAX_SERVICE_PROFILES {
            return Err(STORAGE.into());
        }
        let profile = profiles::load_profile(&root, id).map_err(|_| STORAGE)?;
        result.push(crate::ipc::workspace::MockProfileChoice {
            id: profile.id,
            label: format!(
                "{}:{} · {} rules",
                profile.bind,
                profile.port,
                profile.rules.len()
            ),
        });
    }
    result.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(result)
}
pub async fn dispatch_typed(
    app: &tauri::AppHandle,
    call: crate::ipc::workspace::WorkspaceCall,
    verified_context: Option<ProjectContext>,
) -> Result<Value, String> {
    let root = app.path().app_local_data_dir().map_err(|_| STORAGE)?;
    tauri::async_runtime::spawn_blocking(move || {
        use crate::core::openapi_definitions::{Definition, Store as Definitions};
        use crate::ipc::workspace::WorkspaceCall;
        let call = match call {
            WorkspaceCall::ListOpenapiDefinitions {} => {
                return serde_json::to_value(Definitions::open(&root)?.list()?)
                    .map_err(|_| "openapi_definition_unavailable".into())
            }
            WorkspaceCall::SaveOpenapiDefinition(input) => {
                let definition = Definition::prepare(input)?;
                Definitions::open(&root)?.save(&definition)?;
                return serde_json::to_value(definition)
                    .map_err(|_| "openapi_definition_unavailable".into());
            }
            WorkspaceCall::GetOpenapiDefinition { id } => {
                return serde_json::to_value(Definitions::open(&root)?.get(&id)?)
                    .map_err(|_| "openapi_definition_unavailable".into())
            }
            WorkspaceCall::DeleteOpenapiDefinition { id } => {
                Definitions::open(&root)?.delete(&id)?;
                return Ok(Value::Null);
            }
            call => call,
        };
        let store = Store::open(&root)?;
        let document = store.load()?;
        let next = match call {
            WorkspaceCall::ApiWorkspaceState {} => {
                return serde_json::to_value(crate::ipc::workspace::ApiWorkspaceState {
                    document,
                    current_project_id: verified_context.as_ref().map(|v| v.project_id.clone()),
                    mock_profiles: mock_profiles(&root)?,
                })
                .map_err(|_| STORAGE.into())
            }
            WorkspaceCall::SaveApiWorkspace(input) => {
                document.save(input, verified_context.as_ref())?
            }
            WorkspaceCall::SelectApiWorkspace {
                expected_revision,
                id,
            } => document.select(expected_revision, id)?,
            WorkspaceCall::DeleteApiWorkspace {
                expected_revision,
                id,
            } => document.delete(expected_revision, &id)?,
            _ => unreachable!("OpenAPI handled before workspace store"),
        };
        store.write(&next)?;
        serde_json::to_value(next).map_err(|_| STORAGE.into())
    })
    .await
    .map_err(|_| STORAGE.to_string())?
}
