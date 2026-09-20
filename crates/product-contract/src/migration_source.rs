//! Fresh owner observations. Matching a retained import is not a statement that
//! every legacy preference was transferred; the cutover review records omissions.
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
            || row.backups.iter().any(|id| !crate::commands::opaque_id(id))
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
pub fn empty(owner: &str) -> Vec<Source> {
    identifiers(owner)
        .iter()
        .map(|id| Source {
            identifier: (*id).into(),
            backups: vec![],
        })
        .collect()
}
