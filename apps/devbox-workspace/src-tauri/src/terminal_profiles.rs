//! One product-owned profile revision shared by the manager and companions.
use crate::private_metadata::MetadataRoot;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use wsl_desktop_lib::component::{ProfileStore, WorkspaceProfile};

type Result<T> = std::result::Result<T, &'static str>;
const FILE: &str = "product-profiles-v1.json";
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Envelope {
    schema_version: u32,
    content: ProfileStore,
    /// Import receipts survive ordinary CRUD and prevent repeat imports from
    /// resurrecting a profile deleted after its first import.
    receipts: BTreeMap<String, String>,
}
fn load(root: &MetadataRoot) -> Result<(Envelope, Option<Vec<u8>>, String)> {
    let bytes = root.read(FILE)?;
    let envelope = match bytes.as_ref() {
        Some(bytes) => {
            serde_json::from_slice::<Envelope>(bytes).map_err(|_| "terminal_profiles_invalid")?
        }
        None => Envelope {
            schema_version: 1,
            content: ProfileStore::default(),
            receipts: BTreeMap::new(),
        },
    };
    if envelope.schema_version != 1
        || envelope.receipts.len() > 64
        || envelope
            .receipts
            .iter()
            .any(|(key, value)| key.len() > 128 || value.len() > 128)
    {
        return Err("terminal_profiles_invalid");
    }
    envelope
        .content
        .validate()
        .map_err(|_| "terminal_profiles_invalid")?;
    let revision =
        crate::definitions::digest(bytes.as_deref().unwrap_or(b"missing-terminal-profiles"));
    Ok((envelope, bytes, revision))
}

pub(crate) fn snapshot(root: &MetadataRoot, id: &str) -> Result<(WorkspaceProfile, String)> {
    let (envelope, _, revision) = load(root)?;
    let profile = envelope
        .content
        .profiles
        .into_iter()
        .find(|profile| profile.id == id)
        .ok_or("terminal_profile_missing")?;
    Ok((profile, revision))
}

/// The enclosing Terminals mutex owns this read/modify/write transaction.
pub(crate) fn dispatch(root: &MetadataRoot, method: &str, args: Value) -> Result<Value> {
    if !args.is_object() {
        return Err("terminal_args_invalid");
    }
    let (mut envelope, before, revision) = load(root)?;
    if method == "list_workspace_profiles" {
        if args.as_object().is_none_or(|args| !args.is_empty()) {
            return Err("terminal_args_invalid");
        }
        return Ok(json!({"revision":revision,"profiles":envelope.content.profiles}));
    }
    let profile = match method {
        "save_workspace_profile" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Save {
                expected_revision: String,
                profile: WorkspaceProfile,
            }
            let mut input: Save =
                serde_json::from_value(args).map_err(|_| "terminal_args_invalid")?;
            if input.expected_revision != revision {
                return Err("terminal_profiles_changed");
            }
            if input.profile.id.is_empty() {
                input.profile.id = uuid::Uuid::new_v4().to_string();
            }
            envelope
                .content
                .upsert(input.profile.clone())
                .map_err(|_| "terminal_profile_invalid")?;
            Some(input.profile)
        }
        "delete_workspace_profile" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Delete {
                expected_revision: String,
                id: String,
            }
            let input: Delete =
                serde_json::from_value(args).map_err(|_| "terminal_args_invalid")?;
            if input.expected_revision != revision {
                return Err("terminal_profiles_changed");
            }
            if !envelope.content.remove(&input.id) {
                return Err("terminal_profile_missing");
            }
            None
        }
        _ => return Err("terminal_method_invalid"),
    };
    let bytes = serde_json::to_vec(&envelope).map_err(|_| "terminal_profiles_invalid")?;
    if root.read(FILE)? != before {
        return Err("terminal_profiles_changed");
    }
    root.write(FILE, &bytes)?;
    Ok(json!({"revision":crate::definitions::digest(&bytes),"profile":profile}))
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LayoutEnvelope {
    schema_version: u32,
    layout: Option<WorkspaceProfile>,
}

fn layout_file(session: &str) -> Result<String> {
    if !uuid::Uuid::parse_str(session).is_ok_and(|id| id.to_string() == session) {
        return Err("terminal_session_missing");
    }
    Ok(format!("terminal-layout-{session}.json"))
}
pub(crate) fn read_layout(
    root: &MetadataRoot,
    session: &str,
) -> Result<(Option<WorkspaceProfile>, String)> {
    let bytes = root.read(&layout_file(session)?)?;
    let layout = match bytes.as_ref() {
        Some(bytes) => {
            let envelope: LayoutEnvelope =
                serde_json::from_slice(bytes).map_err(|_| "terminal_layout_invalid")?;
            if envelope.schema_version != 1 {
                return Err("terminal_layout_invalid");
            }
            if let Some(layout) = &envelope.layout {
                layout.validate().map_err(|_| "terminal_layout_invalid")?;
                if layout.id != session {
                    return Err("terminal_layout_invalid");
                }
            }
            envelope.layout
        }
        None => None,
    };
    Ok((
        layout,
        crate::definitions::digest(bytes.as_deref().unwrap_or(b"missing-terminal-layout")),
    ))
}
pub(crate) fn layout(
    root: &MetadataRoot,
    session: &str,
    method: &str,
    args: Value,
) -> Result<Value> {
    if !args.is_object() {
        return Err("terminal_args_invalid");
    }
    let (current, revision) = read_layout(root, session)?;
    if method == "terminal_layout" {
        if args.as_object().is_none_or(|args| !args.is_empty()) {
            return Err("terminal_args_invalid");
        }
        return Ok(json!({"layout":current,"revision":revision}));
    }
    if method != "save_terminal_layout" {
        return Err("terminal_method_invalid");
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Save {
        expected_revision: String,
        layout: Option<WorkspaceProfile>,
    }
    let input: Save = serde_json::from_value(args).map_err(|_| "terminal_args_invalid")?;
    if input.expected_revision != revision {
        return Err("terminal_layout_changed");
    }
    if let Some(layout) = &input.layout {
        layout.validate().map_err(|_| "terminal_layout_invalid")?;
        if layout.id != session {
            return Err("terminal_layout_invalid");
        }
    }
    let bytes = serde_json::to_vec(&LayoutEnvelope {
        schema_version: 1,
        layout: input.layout,
    })
    .map_err(|_| "terminal_layout_invalid")?;
    if read_layout(root, session)?.1 != revision {
        return Err("terminal_layout_changed");
    }
    root.write(&layout_file(session)?, &bytes)?;
    Ok(json!({"revision":crate::definitions::digest(&bytes)}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn corrupt_and_future_profile_envelopes_remain_unchanged() {
        let directory = tempfile::tempdir().unwrap();
        let root = MetadataRoot::open(directory.path()).unwrap();
        for bytes in [
            b"not-json".as_slice(),
            br#"{"schemaVersion":999,"content":{"version":2,"profiles":[]},"receipts":{}}"#,
        ] {
            std::fs::write(directory.path().join(FILE), bytes).unwrap();
            assert!(dispatch(&root, "list_workspace_profiles", json!({})).is_err());
            assert_eq!(std::fs::read(directory.path().join(FILE)).unwrap(), bytes);
        }
    }
}
