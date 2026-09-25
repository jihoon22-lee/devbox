//! Read-only v0.8.1 owner journal schema. No discovery, acquisition or import entrypoints.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Source {
    pub identifier: String,
    pub backups: Vec<String>,
}
pub fn identifiers(owner: &str) -> &'static [&'static str] {
    match owner {
        "api-studio" => &[
            "com.devbox.apiplayground",
            "com.devbox.webhooklab",
            "com.devbox.developertoolbox",
        ],
        "knowledge" => &[
            "com.devbox.knowledgebase",
            "com.devbox.lifelog",
            "com.devbox.everythingplus",
        ],
        "workspace" => &[
            "com.devbox.workbench",
            "com.devbox.codepad",
            "com.workbench.codepad",
            "com.devbox.repomanager",
            "com.devbox.portmanager",
            "com.devbox.loglens",
            "com.devbox.wsldesktop",
            "com.devbox.runmanager",
        ],
        "control-center" => &["com.devbox.devboxlauncher", "com.devbox.devboxmanager"],
        _ => &[],
    }
}
pub fn validate(owner: &str, rows: &[Source]) -> Result<(), &'static str> {
    let expected = identifiers(owner);
    let mut seen = std::collections::BTreeSet::new();
    if expected.is_empty() || rows.len() != expected.len() {
        return Err("migration_source_invalid");
    }
    for row in rows {
        if !expected.contains(&row.identifier.as_str())
            || !seen.insert(&row.identifier)
            || row.backups.len() > 128
            || row
                .backups
                .iter()
                .any(|id| !product_contract::commands::opaque_id(id))
            || row
                .backups
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != row.backups.len()
        {
            return Err("migration_source_invalid");
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Namespace {
    pub identifier: String,
    pub present: bool,
    pub revision: String,
    pub files: usize,
    pub bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn v081_owner_source_shapes_are_preserved_without_discovery() {
        let old = serde_json::json!({"sources":[
            {"identifier":"com.devbox.devboxlauncher","backups":[]},
            {"identifier":"com.devbox.devboxmanager","backups":[]}
        ],"namespaces":[
            {"identifier":"com.devbox.devboxlauncher","present":false,"revision":"a".repeat(64),"files":0,"bytes":0},
            {"identifier":"com.devbox.devboxmanager","present":false,"revision":"b".repeat(64),"files":0,"bytes":0}
        ]});
        let evidence: super::super::delivery::SourceEvidence =
            serde_json::from_value(old.clone()).unwrap();
        validate("control-center", &evidence.sources).unwrap();
        assert_eq!(serde_json::to_value(evidence).unwrap(), old);
    }
}
