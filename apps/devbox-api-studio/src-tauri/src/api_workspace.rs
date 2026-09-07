use crate::core::api_workspace::{Save, Store};
use product_contract::context::ProjectContext;
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::Manager;
pub const COMMANDS: &[&str] = &[
    "api_workspace_state",
    "save_openapi_definition",
    "list_openapi_definitions",
    "get_openapi_definition",
    "delete_openapi_definition",
    "save_api_workspace",
    "select_api_workspace",
    "delete_api_workspace",
];
const INVALID: &str = "api_workspace_invalid";
const STORAGE: &str = "api_workspace_unavailable";
fn mock_profiles(root: &std::path::Path) -> Result<Vec<Value>, String> {
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
        result.push(json!({"id": profile.id, "label": format!("{}:{} · {} rules", profile.bind, profile.port, profile.rules.len())}));
    }
    result.sort_by_key(|value| value["id"].as_str().unwrap_or_default().to_string());
    Ok(result)
}
pub async fn dispatch(
    app: &tauri::AppHandle,
    method: &str,
    args: Value,
    verified_context: Option<ProjectContext>,
) -> Result<Value, String> {
    let root = app.path().app_local_data_dir().map_err(|_| STORAGE)?;
    let method = method.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        if method.contains("openapi_definition") {
            use crate::core::openapi_definitions::{Create, Definition, Store as Definitions};
            let store = Definitions::open(&root)?;
            return match method.as_str() {
                "list_openapi_definitions" if args.as_object().is_some_and(|v| v.is_empty()) => serde_json::to_value(store.list()?).map_err(|_| "openapi_definition_unavailable".into()),
                "save_openapi_definition" => {
                    let input: Create = serde_json::from_value(args).map_err(|_| "openapi_definition_invalid")?;
                    let definition = Definition::prepare(input)?; store.save(&definition)?;
                    serde_json::to_value(definition).map_err(|_| "openapi_definition_unavailable".into())
                },
                "get_openapi_definition" | "delete_openapi_definition" => {
                    #[derive(Deserialize)]
                    #[serde(deny_unknown_fields)]
                    struct Input { id: String }
                    let Input { id } = serde_json::from_value(args).map_err(|_| "openapi_definition_invalid")?;
                    if method == "delete_openapi_definition" { store.delete(&id)?; Ok(Value::Null) }
                    else { serde_json::to_value(store.get(&id)?).map_err(|_| "openapi_definition_unavailable".into()) }
                },
                _ => Err("openapi_definition_invalid".into()),
            };
        }
        let store = Store::open(&root)?;
        let document = store.load()?;
        let next = match method.as_str() {
            "api_workspace_state" if args.as_object().is_some_and(|v| v.is_empty()) => {
                return Ok(json!({"document": document, "currentProjectId": verified_context.as_ref().map(|v| &v.project_id), "mockProfiles": mock_profiles(&root)?}));
            }
            "save_api_workspace" => {
                let input: Save = serde_json::from_value(args).map_err(|_| INVALID)?;
                document.save(input, verified_context.as_ref())?
            }
            "select_api_workspace" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Input { expected_revision: u64, id: Option<String> }
                let Input { expected_revision, id } = serde_json::from_value(args).map_err(|_| INVALID)?;
                document.select(expected_revision, id)?
            }
            "delete_api_workspace" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Input { expected_revision: u64, id: String }
                let Input { expected_revision, id } = serde_json::from_value(args).map_err(|_| INVALID)?;
                document.delete(expected_revision, &id)?
            }
            _ => return Err(INVALID.into()),
        };
        store.write(&next)?;
        serde_json::to_value(next).map_err(|_| STORAGE.into())
    }).await.map_err(|_| STORAGE.to_string())?
}
