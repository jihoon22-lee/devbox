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
    #[serde(default)]
    preferences: BTreeMap<String, String>,
}
fn decode(bytes: Option<&[u8]>) -> Result<Envelope> {
    let envelope = match bytes {
        Some(bytes) => {
            serde_json::from_slice::<Envelope>(bytes).map_err(|_| "terminal_profiles_invalid")?
        }
        None => Envelope {
            schema_version: 1,
            content: ProfileStore::default(),
            receipts: BTreeMap::new(),
            preferences: BTreeMap::new(),
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
    if envelope.preferences.iter().any(|(key, value)| {
        !crate::terminal_export::KEYS.contains(&key.as_str())
            || key.ends_with(":last-layout")
            || value.len() > 1024 * 1024
    }) {
        return Err("terminal_preferences_invalid");
    }
    envelope
        .content
        .validate()
        .map_err(|_| "terminal_profiles_invalid")?;
    Ok(envelope)
}
fn load(root: &MetadataRoot) -> Result<(Envelope, Option<Vec<u8>>, String)> {
    let bytes = root.read(FILE)?;
    let envelope = decode(bytes.as_deref())?;
    let revision =
        crate::definitions::digest(bytes.as_deref().unwrap_or(b"missing-terminal-profiles"));
    Ok((envelope, bytes, revision))
}

pub(crate) fn preferences(root: &MetadataRoot, method: &str, args: Value) -> Result<Value> {
    let (mut envelope, before, _) = load(root)?;
    if method == "terminal_preferences" {
        if args.as_object().is_none_or(|args| !args.is_empty()) {
            return Err("terminal_args_invalid");
        }
        return Ok(json!(envelope.preferences));
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Set {
        key: String,
        expected: Option<String>,
        value: String,
    }
    let input: Set = serde_json::from_value(args).map_err(|_| "terminal_args_invalid")?;
    if !crate::terminal_export::KEYS.contains(&input.key.as_str())
        || input.key.ends_with(":last-layout")
        || input.value.len() > 1024 * 1024
    {
        return Err("terminal_preferences_invalid");
    }
    if envelope.preferences.get(&input.key) != input.expected.as_ref() {
        return Err("terminal_preferences_changed");
    }
    envelope.preferences.insert(input.key, input.value);
    if root.read(FILE)? != before {
        return Err("terminal_profiles_changed");
    }
    root.write(
        FILE,
        &serde_json::to_vec(&envelope).map_err(|_| "terminal_profiles_invalid")?,
    )?;
    Ok(Value::Null)
}

pub(crate) fn import(
    root: &MetadataRoot,
    source_root: &std::path::Path,
    method: &str,
    args: Value,
) -> Result<Value> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        id: String,
        expected_revision: Option<String>,
        source_revision: Option<String>,
        #[serde(default)]
        replace_preferences: bool,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "terminal_args_invalid")?;
    let (prepared, fingerprint) = crate::terminal_import::prepared(source_root, &input.id)?;
    let (mut envelope, before, revision) = load(root)?;
    let applied = envelope.receipts.contains_key(&fingerprint);
    let mapping = prepared.profiles.iter().enumerate().map(|(index,profile)|json!({"sourceId":profile.id,"name":profile.name,"destinationId":format!("import-{}-{index}",&fingerprint[..24])})).collect::<Vec<_>>();
    let conflicts = prepared
        .preferences
        .iter()
        .filter(|(key, value)| {
            envelope
                .preferences
                .get(*key)
                .is_some_and(|previous| previous != *value)
        })
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    if method == "preview_terminal_import" {
        return Ok(
            json!({"id":input.id,"revision":revision,"sourceRevision":fingerprint,"applied":applied,"profiles":mapping,"preferenceKeys":prepared.preferences.keys().collect::<Vec<_>>(),"conflicts":conflicts,"notices":prepared.notices}),
        );
    }
    if method != "apply_terminal_import" {
        return Err("terminal_method_invalid");
    }
    if applied {
        return Ok(json!({"applied":true,"repeated":true}));
    }
    if input.expected_revision.as_ref() != Some(&revision)
        || input.source_revision.as_ref() != Some(&fingerprint)
    {
        return Err("terminal_import_changed");
    }
    if envelope.receipts.len() >= 64 {
        return Err("terminal_import_limit");
    }
    for (index, mut profile) in prepared.profiles.into_iter().enumerate() {
        profile.id = format!("import-{}-{index}", &fingerprint[..24]);
        if envelope
            .content
            .profiles
            .iter()
            .any(|current| current.id == profile.id)
        {
            return Err("terminal_import_conflict");
        }
        envelope.content.profiles.push(profile);
    }
    for (key, value) in prepared.preferences {
        if input.replace_preferences || !envelope.preferences.contains_key(&key) {
            envelope.preferences.insert(key, value);
        }
    }
    envelope
        .content
        .validate()
        .map_err(|_| "terminal_import_limit")?;
    envelope
        .receipts
        .insert(fingerprint.clone(), input.id.clone());
    let bytes = serde_json::to_vec(&envelope).map_err(|_| "terminal_profiles_invalid")?;
    // Definitions, preferences and repeat receipt commit in one owner document.
    // Keep the exact preimage and original-to-destination mapping for recovery.
    let history = root.child("import-history")?;
    let record = json!({"schemaVersion":1,"sourceRevision":fingerprint,"mapping":mapping,"beforePresent":before.is_some(),"beforeRevision":revision,"preservedPreferenceConflicts":if input.replace_preferences {Vec::<String>::new()}else{conflicts}});
    if let Some(before_bytes) = &before {
        let before_name = format!("{fingerprint}.before.json");
        match history.read(&before_name)? {
            Some(existing) if &existing != before_bytes => return Err("terminal_import_changed"),
            Some(_) => {}
            None => history.create_new(&before_name, before_bytes)?,
        }
    }
    let name = format!("{fingerprint}.json");
    let record = serde_json::to_vec(&record).map_err(|_| "terminal_import_invalid")?;
    match history.read(&name)? {
        Some(existing) if existing != record => return Err("terminal_import_changed"),
        Some(_) => {}
        None => history.create_new(&name, &record)?,
    }
    if root.read(FILE)? != before {
        return Err("terminal_profiles_changed");
    }
    root.write(FILE, &bytes)?;
    Ok(json!({"applied":true,"repeated":false}))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HistoryRecord {
    schema_version: u32,
    source_revision: String,
    mapping: Vec<Value>,
    before_present: bool,
    before_revision: String,
    preserved_preference_conflicts: Vec<String>,
}
fn fingerprint(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}
fn previous(history: &MetadataRoot, key: &str) -> Result<(Envelope, String)> {
    if !fingerprint(key) {
        return Err("terminal_import_invalid");
    }
    let record_bytes = history
        .read(&format!("{key}.json"))?
        .ok_or("terminal_import_history_missing")?;
    let record: HistoryRecord =
        serde_json::from_slice(&record_bytes).map_err(|_| "terminal_import_history_invalid")?;
    if record.schema_version != 1
        || record.source_revision != key
        || record.mapping.len() > 100
        || record.preserved_preference_conflicts.len() > 6
    {
        return Err("terminal_import_history_invalid");
    }
    let bytes = history.read(&format!("{key}.before.json"))?;
    if bytes.is_some() != record.before_present
        || crate::definitions::digest(bytes.as_deref().unwrap_or(b"missing-terminal-profiles"))
            != record.before_revision
    {
        return Err("terminal_import_history_changed");
    }
    let envelope = decode(bytes.as_deref())?;
    // The checked record binds exact preimage bytes as well as the source.
    Ok((envelope, crate::definitions::digest(&record_bytes)))
}
pub(crate) fn history(root: &MetadataRoot, method: &str, args: Value) -> Result<Value> {
    let (current, before, current_revision) = load(root)?;
    let history = root.child("import-history")?;
    if method == "terminal_import_history" {
        if args.as_object().is_none_or(|args| !args.is_empty()) {
            return Err("terminal_args_invalid");
        }
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(history.path())
            .map_err(|_| "terminal_import_history_invalid")?
            .take(257)
        {
            let entry = entry.map_err(|_| "terminal_import_history_invalid")?;
            let name = entry.file_name().to_string_lossy().to_string();
            let Some(key) = name.strip_suffix(".json").filter(|key| fingerprint(key)) else {
                continue;
            };
            entries.push(match previous(&history,key){Ok((value,_))=>json!({"sourceRevision":key,"profiles":value.content.profiles.len(),"preferences":value.preferences.len(),"issue":null}),Err(issue)=>json!({"sourceRevision":key,"issue":issue})});
        }
        return Ok(json!({"entries":entries}));
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        source_revision: String,
        expected_revision: Option<String>,
        restore_revision: Option<String>,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "terminal_args_invalid")?;
    let (mut restored, restore_revision) = previous(&history, &input.source_revision)?;
    if method == "preview_terminal_import_restore" {
        return Ok(
            json!({"sourceRevision":input.source_revision,"revision":current_revision,"restoreRevision":restore_revision,"profiles":restored.content.profiles.len(),"preferences":restored.preferences.len()}),
        );
    }
    if method != "restore_terminal_import" {
        return Err("terminal_method_invalid");
    }
    if input.expected_revision.as_ref() != Some(&current_revision)
        || input.restore_revision.as_ref() != Some(&restore_revision)
    {
        return Err("terminal_import_changed");
    }
    // Restoring a preimage does not erase the record that this source was imported.
    restored.receipts.extend(current.receipts);
    if restored.receipts.len() > 64 {
        return Err("terminal_import_limit");
    }
    if std::fs::read_dir(history.path())
        .map_err(|_| "terminal_import_history_invalid")?
        .take(257)
        .count()
        >= 256
    {
        return Err("terminal_import_limit");
    }
    if let Some(bytes) = &before {
        history.create_new(
            &format!("restore-before-{}.json", uuid::Uuid::new_v4()),
            bytes,
        )?;
    }
    if root.read(FILE)? != before {
        return Err("terminal_profiles_changed");
    }
    root.write(
        FILE,
        &serde_json::to_vec(&restored).map_err(|_| "terminal_profiles_invalid")?,
    )?;
    Ok(json!({"restored":true}))
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
    fn import_receipt_keeps_later_preferences_and_deleted_profiles_on_repeat() {
        let directory = tempfile::tempdir().unwrap();
        let root = MetadataRoot::open(directory.path()).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let stage = root.child("terminal-imports").unwrap().child(&id).unwrap();
        stage
            .write(
                "job.json",
                &serde_json::to_vec(
                    &json!({"schemaVersion":1,"id":id,"state":"ready","issue":null}),
                )
                .unwrap(),
            )
            .unwrap();
        let profile:WorkspaceProfile=serde_json::from_value(json!({"id":"old","name":"Synthetic profile","tabs":[{"id":"tab","title":"Tab","layout":"grid","paneKeys":["pane"],"sizing":{"columns":[1.0],"rows":[1.0]}}],"panes":[{"key":"pane","distro":"Synthetic","multiplexer":"native"}],"activeTabId":"tab","activePaneKey":"pane"})).unwrap();
        let prepared = crate::terminal_import::Prepared {
            schema_version: 1,
            profiles: vec![profile],
            preferences: BTreeMap::from([("wsl-desktop:copy-on-select".into(), "0".into())]),
            notices: vec![],
        };
        stage
            .write("prepared.json", &serde_json::to_vec(&prepared).unwrap())
            .unwrap();
        let review = import(
            &root,
            directory.path(),
            "preview_terminal_import",
            json!({"id":id}),
        )
        .unwrap();
        let apply = json!({"id":id,"expectedRevision":review["revision"],"sourceRevision":review["sourceRevision"],"replacePreferences":true});
        import(
            &root,
            directory.path(),
            "apply_terminal_import",
            apply.clone(),
        )
        .unwrap();
        preferences(
            &root,
            "set_terminal_preference",
            json!({"key":"wsl-desktop:copy-on-select","expected":"0","value":"1"}),
        )
        .unwrap();
        let (envelope, _, revision) = load(&root).unwrap();
        let imported_id = &envelope.content.profiles[0].id;
        dispatch(
            &root,
            "delete_workspace_profile",
            json!({"id":imported_id,"expectedRevision":revision}),
        )
        .unwrap();
        let before = root.read(FILE).unwrap();
        let repeated = import(&root, directory.path(), "apply_terminal_import", apply).unwrap();
        assert_eq!(repeated["repeated"], true);
        assert_eq!(root.read(FILE).unwrap(), before);
        assert!(load(&root).unwrap().0.content.profiles.is_empty());
        assert_eq!(
            load(&root).unwrap().0.preferences["wsl-desktop:copy-on-select"],
            "1"
        );
        let restore = history(
            &root,
            "preview_terminal_import_restore",
            json!({"sourceRevision":review["sourceRevision"]}),
        )
        .unwrap();
        history(&root,"restore_terminal_import",json!({"sourceRevision":review["sourceRevision"],"expectedRevision":restore["revision"],"restoreRevision":restore["restoreRevision"]})).unwrap();
        assert!(load(&root).unwrap().0.preferences.is_empty());
        assert!(load(&root)
            .unwrap()
            .0
            .receipts
            .contains_key(review["sourceRevision"].as_str().unwrap()));
        let after_restore = root.read(FILE).unwrap();
        import(
            &root,
            directory.path(),
            "apply_terminal_import",
            json!({"id":id}),
        )
        .unwrap();
        assert_eq!(root.read(FILE).unwrap(), after_restore);
    }
    #[test]
    fn a_stale_companion_cannot_replace_imported_preferences() {
        let directory = tempfile::tempdir().unwrap();
        let root = MetadataRoot::open(directory.path()).unwrap();
        preferences(
            &root,
            "set_terminal_preference",
            json!({"key":"wsl-desktop:font-size","expected":null,"value":"16"}),
        )
        .unwrap();
        let before = root.read(FILE).unwrap();
        assert!(preferences(
            &root,
            "set_terminal_preference",
            json!({"key":"wsl-desktop:font-size","expected":null,"value":"12"})
        )
        .is_err());
        assert_eq!(root.read(FILE).unwrap(), before);
    }
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
