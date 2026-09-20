//! Explicit per-source review bound to native owner observations. Unknown or
//! changed sources can only be preserved as skipped, never marked transferred.
use super::{delivery::Journal, legacy_sources::Namespace};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
type Result<T> = std::result::Result<T, &'static str>;
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Disposition {
    AcceptedImport,
    KeepLegacy,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Choice {
    pub identifier: String,
    pub disposition: Disposition,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub revision: String,
    pub choices: Vec<Choice>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Plan {
    pub schema_version: u32,
    pub installation_key: String,
    pub operation_id: String,
    pub revision: String,
    pub choices: Vec<Choice>,
    pub namespaces: Vec<Namespace>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub identifier: String,
    pub owner: String,
    pub current_import: bool,
    pub backup_count: usize,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub revision: String,
    pub sources: Vec<Row>,
    pub prepared: bool,
    pub choices: Vec<Choice>,
}
pub fn revision(journal: &Journal) -> Result<String> {
    let bytes = serde_json::to_vec(&(
        &journal.installation_key,
        &journal.operation_id,
        &journal.candidate,
        &journal.owner_evidence,
    ))
    .map_err(|_| "cutover_evidence_invalid")?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
pub fn review(journal: &Journal) -> Result<Review> {
    journal.validate()?;
    if journal.previous.is_some() || journal.failure.is_some() || journal.owner_evidence.len() != 4
    {
        return Err("cutover_owner_review_required");
    }
    let mut rows = vec![];
    for evidence in &journal.owner_evidence {
        if evidence.summary.busy
            || evidence.summary.review_required
            || !evidence.summary.setup_selected
        {
            return Err("cutover_owner_review_required");
        }
        let sources = evidence
            .sources
            .as_ref()
            .ok_or("cutover_source_observation_required")?;
        for namespace in sources.namespaces.iter().filter(|row| row.present) {
            let source = sources
                .sources
                .iter()
                .find(|source| source.identifier == namespace.identifier)
                .ok_or("cutover_evidence_invalid")?;
            rows.push(Row {
                identifier: namespace.identifier.clone(),
                owner: evidence.summary.owner.clone(),
                current_import: !source.backups.is_empty(),
                backup_count: source.backups.len(),
            });
        }
    }
    rows.sort_by(|a, b| a.identifier.cmp(&b.identifier));
    Ok(Review {
        revision: revision(journal)?,
        sources: rows,
        prepared: false,
        choices: vec![],
    })
}
pub fn prepare(journal: &Journal, request: Request) -> Result<Plan> {
    let reviewed = review(journal)?;
    if journal.committed
        || request.revision != reviewed.revision
        || request.choices.len() != reviewed.sources.len()
    {
        return Err("cutover_review_stale");
    }
    let mut choices = request.choices;
    choices.sort_by(|a, b| a.identifier.cmp(&b.identifier));
    for (choice, row) in choices.iter().zip(&reviewed.sources) {
        if choice.identifier != row.identifier
            || (choice.disposition == Disposition::AcceptedImport && !row.current_import)
        {
            return Err("cutover_choice_invalid");
        }
    }
    let mut namespaces = journal
        .owner_evidence
        .iter()
        .flat_map(|evidence| {
            evidence
                .sources
                .as_ref()
                .into_iter()
                .flat_map(|source| &source.namespaces)
        })
        .cloned()
        .collect::<Vec<_>>();
    namespaces.sort_by(|a, b| a.identifier.cmp(&b.identifier));
    Ok(Plan {
        schema_version: 1,
        installation_key: journal.installation_key.clone(),
        operation_id: journal.operation_id.clone(),
        revision: reviewed.revision,
        choices,
        namespaces,
    })
}
impl Plan {
    pub fn validate(&self, journal: &Journal) -> Result<()> {
        if self.schema_version != 1
            || self.installation_key != journal.installation_key
            || self.operation_id != journal.operation_id
            || self.revision != revision(journal)?
        {
            return Err("cutover_review_stale");
        }
        // A committed journal may replay cleanup; review itself remains valid.
        let mut before_commit = journal.clone();
        before_commit.committed = false;
        before_commit.phase = super::delivery::Phase::Health;
        let expected = prepare(
            &before_commit,
            Request {
                revision: self.revision.clone(),
                choices: self.choices.clone(),
            },
        )?;
        if expected.namespaces != self.namespaces {
            return Err("cutover_evidence_changed");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::delivery::{OwnerEvidence, Phase, SourceEvidence};
    use product_contract::{
        installation::{Manifest, Member, PRODUCTS},
        migration_status::Summary,
    };
    fn fixture() -> Journal {
        let candidate = Manifest {
            schema_version: 1,
            installation_id: "fixture-install".into(),
            generation: "next".into(),
            suite_version: "0.8.0".into(),
            protocol_version: 1,
            members: PRODUCTS
                .iter()
                .map(|owner| Member {
                    product: (*owner).into(),
                    executable: format!("generations/next/products/{owner}/devbox-{owner}.exe"),
                    sha256: "a".repeat(64),
                })
                .collect(),
        };
        let mut journal =
            Journal::begin("fixture-operation".into(), "b".repeat(64), None, candidate).unwrap();
        journal.phase = Phase::Snapshot;
        for owner in PRODUCTS {
            let sources = product_contract::migration_source::empty(owner);
            let namespaces = sources
                .iter()
                .map(|source| Namespace {
                    identifier: source.identifier.clone(),
                    present: source.identifier == "com.devbox.devboxlauncher",
                    revision: "c".repeat(64),
                    files: 1,
                    bytes: 20,
                })
                .collect();
            journal
                .record_owner(
                    journal.revision,
                    OwnerEvidence {
                        summary: Summary::new(owner, "0.8.0", false, true, false, b"native")
                            .unwrap(),
                        backups: vec![],
                        sources: Some(SourceEvidence {
                            sources,
                            namespaces,
                        }),
                    },
                )
                .unwrap();
        }
        journal
    }
    #[test]
    fn unimported_source_needs_explicit_preservation_and_old_review_cannot_authorize_new_evidence()
    {
        let mut journal = fixture();
        let reviewed = review(&journal).unwrap();
        assert_eq!(reviewed.sources.len(), 1);
        assert!(prepare(
            &journal,
            Request {
                revision: reviewed.revision.clone(),
                choices: vec![]
            }
        )
        .is_err());
        let choice = Choice {
            identifier: "com.devbox.devboxlauncher".into(),
            disposition: Disposition::AcceptedImport,
        };
        assert!(prepare(
            &journal,
            Request {
                revision: reviewed.revision.clone(),
                choices: vec![choice.clone()]
            }
        )
        .is_err());
        let plan = prepare(
            &journal,
            Request {
                revision: reviewed.revision,
                choices: vec![Choice {
                    disposition: Disposition::KeepLegacy,
                    ..choice
                }],
            },
        )
        .unwrap();
        plan.validate(&journal).unwrap();
        journal.owner_evidence[0].summary.revision = "d".repeat(64);
        assert!(plan.validate(&journal).is_err());
    }
    #[test]
    fn observations_without_fresh_sources_are_not_cutover_permission() {
        let mut journal = fixture();
        journal.owner_evidence[0].sources = None;
        assert!(review(&journal).is_err());
    }
}
