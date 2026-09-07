//! Pure domain merge for destination-owned migration. Imported records never
//! replace edited product records. Receipts distinguish a repeat from a new
//! source revision, and every conflict receives an explicit mapping.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum StoreKind {
    Collections,
    History,
    Environments,
    GrpcHistory,
    Fixtures,
    Profiles,
    Workflows,
    OAuth,
    GrpcTls,
}
impl StoreKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Collections => "collections",
            Self::History => "history",
            Self::Environments => "environments",
            Self::GrpcHistory => "grpc-history",
            Self::Fixtures => "fixtures",
            Self::Profiles => "profiles",
            Self::Workflows => "workflows",
            Self::OAuth => "oauth",
            Self::GrpcTls => "grpc-tls",
        }
    }
    pub fn browser_key(self) -> Option<&'static str> {
        match self {
            Self::Collections => Some("apip-collections-v2"),
            Self::History => Some("apip-history-v2"),
            Self::Environments => Some("apip-environments"),
            Self::GrpcHistory => Some("devbox.api-playground.grpc-history/v1"),
            _ => None,
        }
    }
}
pub const KINDS: [StoreKind; 9] = [
    StoreKind::Collections,
    StoreKind::History,
    StoreKind::Environments,
    StoreKind::GrpcHistory,
    StoreKind::Fixtures,
    StoreKind::Profiles,
    StoreKind::Workflows,
    StoreKind::OAuth,
    StoreKind::GrpcTls,
];
pub type Documents = BTreeMap<StoreKind, Value>;
pub type BrowserState = BTreeMap<String, Option<String>>;
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
    pub source_store: String,
    pub source_id: String,
    pub fingerprint: String,
    pub destination_id: String,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MergeSummary {
    pub added: usize,
    pub matched: usize,
    pub already_imported: usize,
    pub conflicts: usize,
    pub capacity_excluded: usize,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Merged {
    pub documents: Documents,
    pub receipts: Vec<Receipt>,
    pub summary: MergeSummary,
}

pub fn fingerprint(value: &Value) -> String {
    Sha256::digest(serde_json::to_vec(value).expect("JSON Value encoding is infallible"))
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
pub fn receipt_key(receipt: &Receipt) -> String {
    fingerprint(&json!([
        receipt.source_store,
        receipt.source_id,
        receipt.fingerprint
    ]))
}
fn rows<'a>(value: &'a Value, field: &str) -> Result<&'a Vec<Value>, String> {
    value
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| "migration_schema_invalid".into())
}
fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str, String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 4096)
        .ok_or_else(|| "migration_record_invalid".into())
}
fn fields(
    kind: StoreKind,
) -> (
    &'static str,
    Option<&'static str>,
    Option<&'static str>,
    usize,
) {
    match kind {
        StoreKind::Collections => ("collections", Some("id"), Some("name"), 10_000),
        StoreKind::History => ("history", Some("id"), None, 50),
        StoreKind::Environments => ("environments", Some("id"), Some("name"), 10_000),
        StoreKind::GrpcHistory => ("entries", None, None, 50),
        StoreKind::Fixtures => ("fixtures", Some("id"), None, 200),
        StoreKind::Profiles => ("profiles", Some("id"), None, 64),
        StoreKind::OAuth => ("grants", Some("grantId"), None, 32),
        StoreKind::GrpcTls => ("credentials", Some("credentialId"), Some("label"), 16),
        StoreKind::Workflows => ("pipelines", Some("id"), None, 20),
    }
}
fn empty_like(kind: StoreKind, source: &Value) -> Result<Value, String> {
    if kind == StoreKind::Workflows {
        return Ok(serde_json::to_value(
            transforms_core::core::workflows::WorkflowMetadata::default(),
        )
        .map_err(|_| "migration_schema_invalid")?);
    }
    if kind == StoreKind::Fixtures {
        return Ok(
            serde_json::to_value(webhook_core::core::fixtures::FixtureDocument::default())
                .map_err(|_| "migration_schema_invalid")?,
        );
    }
    let mut value = source.clone();
    let field = fields(kind).0;
    *value.get_mut(field).ok_or("migration_schema_invalid")? = json!([]);
    Ok(value)
}
pub fn validate_document(
    kind: StoreKind,
    value: &Value,
    api_owner: &impl Fn(StoreKind, &[u8]) -> Result<(), String>,
) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|_| "migration_schema_invalid")?;
    if bytes.len() > 20 * 1024 * 1024 {
        return Err("migration_store_too_large".into());
    }
    match kind {
        StoreKind::Fixtures => {
            let doc = serde_json::from_value::<webhook_core::core::fixtures::FixtureDocument>(
                value.clone(),
            )
            .map_err(|_| "migration_schema_invalid")?;
            webhook_core::core::fixtures::validate_document(&doc)
                .map_err(|_| "migration_schema_invalid")?;
        }
        StoreKind::Profiles => {
            if value.get("schemaVersion").and_then(Value::as_u64) != Some(1) {
                return Err("migration_schema_invalid".into());
            }
            for profile in rows(value, "profiles")? {
                let profile = serde_json::from_value::<
                    webhook_core::core::service_profile::ServiceProfile,
                >(profile.clone())
                .map_err(|_| "migration_schema_invalid")?;
                webhook_core::core::service_profile::validate_profile(&profile)?;
            }
        }
        StoreKind::Workflows => {
            let metadata = serde_json::from_value::<
                transforms_core::core::workflows::WorkflowMetadata,
            >(value.clone())
            .map_err(|_| "migration_schema_invalid")?;
            transforms_core::core::workflows::validate(&metadata)
                .map_err(|_| "migration_schema_invalid")?;
        }
        StoreKind::OAuth | StoreKind::GrpcTls => api_owner(kind, &bytes)?,
        StoreKind::GrpcHistory => {
            if value.get("schema").and_then(Value::as_str)
                != Some("devbox.api-playground.grpc-history/v1")
            {
                return Err("migration_schema_invalid".into());
            }
        }
        StoreKind::Collections | StoreKind::History | StoreKind::Environments => {
            let version = if kind == StoreKind::Environments {
                1
            } else {
                2
            };
            if value.get("version").and_then(Value::as_u64) != Some(version) {
                return Err("migration_schema_invalid".into());
            }
        }
    }
    let (field, id, _, max) = fields(kind);
    let values = rows(value, field)?;
    if values.len() > max {
        return Err("migration_store_too_large".into());
    }
    let mut ids = HashSet::new();
    for record in values {
        if !record.is_object() {
            return Err("migration_record_invalid".into());
        }
        if let Some(id) = id {
            if !ids.insert(text(record, id)?) {
                return Err("migration_duplicate_source_id".into());
            }
        }
        if matches!(kind, StoreKind::Collections | StoreKind::History)
            && !record.get("request").is_some_and(Value::is_object)
        {
            return Err("migration_record_invalid".into());
        }
    }
    Ok(())
}
fn new_id(kind: StoreKind, hash: &str, attempt: usize) -> String {
    let hash = fingerprint(&json!([kind.key(), hash, attempt]));
    match kind {
        StoreKind::Profiles => format!(
            "{}-{}-4{}-a{}-{}",
            &hash[..8],
            &hash[8..12],
            &hash[13..16],
            &hash[17..20],
            &hash[20..32]
        ),
        StoreKind::OAuth | StoreKind::GrpcTls => hash[..32].to_string(),
        _ => format!("import-{}", &hash[..24]),
    }
}
fn oauth_binding(value: &Value) -> Value {
    json!([
        value.get("issuer"),
        value.get("resource"),
        value.get("clientId")
    ])
}
fn merge_rows(
    kind: StoreKind,
    source: &Value,
    target: &mut Value,
    prior: &HashSet<String>,
    output: &mut Vec<Receipt>,
    summary: &mut MergeSummary,
) -> Result<(), String> {
    let (field, id_field, name_field, capacity) = fields(kind);
    let mut destination = rows(target, field)?.clone();
    let mut next_fixture = if kind == StoreKind::Fixtures {
        target
            .get("nextId")
            .and_then(Value::as_u64)
            .ok_or("migration_schema_invalid")?
    } else {
        0
    };
    for original in rows(source, field)? {
        let hash = fingerprint(original);
        let source_id = id_field
            .map(|field| text(original, field).map(str::to_string))
            .transpose()?
            .unwrap_or_else(|| hash.clone());
        let source_store = if kind == StoreKind::Workflows {
            "workflows-pipelines"
        } else {
            kind.key()
        }
        .to_string();
        let mut receipt = Receipt {
            source_store,
            source_id: source_id.clone(),
            fingerprint: hash.clone(),
            destination_id: source_id.clone(),
        };
        if prior.contains(&receipt_key(&receipt)) {
            summary.already_imported += 1;
            continue;
        }
        if let Some(existing) = destination.iter().find(|value| {
            *value == original
                || (kind == StoreKind::OAuth && oauth_binding(value) == oauth_binding(original))
        }) {
            receipt.destination_id = id_field
                .map(|field| text(existing, field).map(str::to_string))
                .transpose()?
                .unwrap_or_else(|| hash.clone());
            if existing != original {
                summary.conflicts += 1;
            }
            summary.matched += 1;
            output.push(receipt);
            continue;
        }
        if destination.len() >= capacity {
            summary.capacity_excluded += 1;
            continue;
        }
        let mut imported = original.clone();
        if matches!(kind, StoreKind::Collections | StoreKind::History) {
            imported["request"]["requiresSecretReview"] = Value::Bool(true);
            if kind == StoreKind::Collections {
                imported["requiresSecretReview"] = Value::Bool(true);
            }
        }
        if kind == StoreKind::Fixtures {
            receipt.destination_id = format!("fixture-{next_fixture}");
            next_fixture = next_fixture
                .checked_add(1)
                .ok_or("migration_fixture_id_exhausted")?;
            imported["id"] = Value::String(receipt.destination_id.clone());
        } else if let Some(id_field) = id_field {
            if destination.iter().any(|value| {
                value.get(id_field).and_then(Value::as_str) == Some(source_id.as_str())
            }) {
                summary.conflicts += 1;
                let id = (0..100)
                    .map(|attempt| new_id(kind, &hash, attempt))
                    .find(|candidate| {
                        !destination.iter().any(|value| {
                            value.get(id_field).and_then(Value::as_str) == Some(candidate)
                        })
                    })
                    .ok_or("migration_destination_id_conflict")?;
                receipt.destination_id = id.clone();
                imported[id_field] = Value::String(id);
            }
        }
        if let Some(name_field) = name_field {
            let name = text(original, name_field)?;
            if destination
                .iter()
                .any(|value| value.get(name_field).and_then(Value::as_str) == Some(name))
            {
                summary.conflicts += 1;
                let mut base = name.to_string();
                if kind == StoreKind::GrpcTls {
                    while base.len() > 200 {
                        base.pop();
                    }
                }
                imported[name_field] = Value::String(format!("{base} (가져옴 {})", &hash[..8]));
            }
        }
        destination.push(imported);
        output.push(receipt);
        summary.added += 1;
    }
    target[field] = Value::Array(destination);
    if kind == StoreKind::Fixtures {
        target["nextId"] = json!(next_fixture);
    }
    Ok(())
}
fn merge_workflow_metadata(
    source: &Value,
    target: &mut Value,
    prior: &HashSet<String>,
    output: &mut Vec<Receipt>,
    summary: &mut MergeSummary,
) -> Result<(), String> {
    let mut favorites = rows(target, "favoriteTools")?.clone();
    for tool in rows(source, "favoriteTools")? {
        let id = tool.as_str().ok_or("migration_record_invalid")?;
        let receipt = Receipt {
            source_store: "workflows-favorites".into(),
            source_id: id.into(),
            fingerprint: fingerprint(tool),
            destination_id: id.into(),
        };
        if prior.contains(&receipt_key(&receipt)) {
            summary.already_imported += 1;
            continue;
        }
        if !favorites.contains(tool) {
            favorites.push(tool.clone());
            summary.added += 1;
        } else {
            summary.matched += 1;
        }
        output.push(receipt);
    }
    target["favoriteTools"] = Value::Array(favorites);
    let mut recent = rows(target, "recentTools")?.clone();
    for value in rows(source, "recentTools")? {
        let id = text(value, "toolId")?;
        let receipt = Receipt {
            source_store: "workflows-recent".into(),
            source_id: id.into(),
            fingerprint: fingerprint(value),
            destination_id: id.into(),
        };
        if prior.contains(&receipt_key(&receipt)) {
            summary.already_imported += 1;
            continue;
        }
        if let Some(existing) = recent
            .iter_mut()
            .find(|existing| existing.get("toolId").and_then(Value::as_str) == Some(id))
        {
            if value.get("usedAt").and_then(Value::as_u64)
                > existing.get("usedAt").and_then(Value::as_u64)
            {
                *existing = value.clone();
            }
            summary.matched += 1;
        } else {
            recent.push(value.clone());
            summary.added += 1;
        }
        output.push(receipt);
    }
    recent.sort_by_key(|value| {
        std::cmp::Reverse(value.get("usedAt").and_then(Value::as_u64).unwrap_or(0))
    });
    recent.truncate(transforms_core::core::workflows::MAX_RECENT_TOOLS);
    target["recentTools"] = Value::Array(recent);
    Ok(())
}
pub fn merge(
    source: &Documents,
    destination: &Documents,
    receipts: &[Receipt],
    api_owner: impl Fn(StoreKind, &[u8]) -> Result<(), String>,
) -> Result<Merged, String> {
    let mut documents = destination.clone();
    let mut output = Vec::new();
    let mut summary = MergeSummary::default();
    let prior: HashSet<_> = receipts.iter().map(receipt_key).collect();
    for (kind, value) in source {
        validate_document(*kind, value, &api_owner)?;
        if let Some(existing) = documents.get(kind) {
            validate_document(*kind, existing, &api_owner)?;
        }
        let target = documents.entry(*kind).or_insert(empty_like(*kind, value)?);
        merge_rows(*kind, value, target, &prior, &mut output, &mut summary)?;
        if *kind == StoreKind::Workflows {
            merge_workflow_metadata(value, target, &prior, &mut output, &mut summary)?;
        }
        validate_document(*kind, target, &api_owner)?;
    }
    if output.len() > 10_000 {
        return Err("migration_too_many_records".into());
    }
    Ok(Merged {
        documents,
        receipts: output,
        summary,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn collection(id: &str, name: &str, url: &str) -> Value {
        json!({ "id": id, "name": name, "folder": "", "saved_at": 1, "request": { "url": url, "requiresSecretReview": false }, "requiresSecretReview": false })
    }
    fn store(value: Value) -> Documents {
        [(
            StoreKind::Collections,
            json!({ "version": 2, "collections": [value] }),
        )]
        .into()
    }
    #[test]
    fn same_name_and_id_conflicts_keep_both_and_repeat_does_not_overwrite_later_edits() {
        let source = store(collection("same-id", "same name", "https://source.test"));
        let destination = store(collection("same-id", "same name", "https://product.test"));
        let first = merge(&source, &destination, &[], |_, _| Ok(())).unwrap();
        let records = first.documents[&StoreKind::Collections]["collections"]
            .as_array()
            .unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(
            records[0],
            destination[&StoreKind::Collections]["collections"][0]
        );
        assert_ne!(records[1]["id"], "same-id");
        assert!(records[1]["name"].as_str().unwrap().contains("가져옴"));
        let mut edited = first.documents.clone();
        edited.get_mut(&StoreKind::Collections).unwrap()["collections"][1]["request"]["url"] =
            json!("https://edited.test");
        let second = merge(&source, &edited, &first.receipts, |_, _| Ok(())).unwrap();
        assert_eq!(second.documents, edited);
        assert_eq!(second.summary.already_imported, 1);
        assert!(second.receipts.is_empty());
    }
    #[test]
    fn fixture_counter_and_mapping_are_destination_owned() {
        let fixture = json!({ "id": "fixture-73", "method": "POST", "url": "/hook", "headers": [], "body": "fixture", "receivedAtMs": 1 });
        let source = [(
            StoreKind::Fixtures,
            json!({ "schemaVersion": 1, "nextId": 74, "fixtures": [fixture] }),
        )]
        .into();
        let result = merge(&source, &Documents::new(), &[], |_, _| Ok(())).unwrap();
        assert_eq!(result.receipts[0].source_id, "fixture-73");
        assert_eq!(result.receipts[0].destination_id, "fixture-1");
        assert_eq!(result.documents[&StoreKind::Fixtures]["nextId"], 2);
    }
    #[test]
    fn future_stores_and_non_persistable_pipeline_steps_fail_without_mutation() {
        let mut source = store(collection("c1", "fixture", "https://fixture.test"));
        source.get_mut(&StoreKind::Collections).unwrap()["version"] = json!(99);
        assert!(merge(&source, &Documents::new(), &[], |_, _| Ok(())).is_err());
        let source = [(StoreKind::Workflows, json!({ "schemaVersion": 1, "recentTools": [], "favoriteTools": [], "pipelines": [{ "id": "bad", "inputType": "text", "steps": [{"transformerId":"hmac"}], "updatedAt": 1 }] }))].into();
        assert!(merge(&source, &Documents::new(), &[], |_, _| Ok(())).is_err());
    }
}
