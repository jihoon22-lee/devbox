//! Destination copies of validated Workbench templates. Creating a concrete
//! project requires a separate native registration preview.
use super::legacy_profiles::{
    is_false, snapshot_id, valid_origin, Applied, Choice, Decision, Disposition, Mapping,
};
use super::{legacy_inventory::digest, registry::Registry};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use workbench_lib::component::{ProfileTemplate, ProfileTemplateStore};

type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportedTemplate {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_snapshot_id: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub local: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub archived: bool,
    pub template: ProfileTemplate,
}
impl ImportedTemplate {
    pub fn validate(&self) -> Result<()> {
        if uuid::Uuid::parse_str(&self.id).is_err()
            || !valid_origin(self.source_snapshot_id.as_deref(), self.local)
            || self.template.validate().is_err()
        {
            return Err("invalid_imported_template");
        }
        Ok(())
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub template: ProfileTemplate,
    pub disposition: Disposition,
    pub existing_ids: Vec<String>,
    pub already_imported: bool,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub registry_revision: u64,
    pub source_snapshot_id: String,
    pub rows: Vec<Row>,
    #[serde(skip)]
    before_digest: String,
}
fn fingerprint(templates: &[ImportedTemplate]) -> Result<String> {
    Ok(digest(
        &serde_json::to_vec(templates).map_err(|_| "invalid_imported_template")?,
    ))
}
fn collides(a: &ProfileTemplate, b: &ProfileTemplate) -> bool {
    a.id == b.id || a.name.to_lowercase() == b.name.to_lowercase()
}
impl Plan {
    pub fn build(
        source_snapshot_id: String,
        source: ProfileTemplateStore,
        registry: &Registry,
    ) -> Result<Self> {
        if !snapshot_id(&source_snapshot_id) {
            return Err("invalid_legacy_snapshot");
        }
        source.validate().map_err(|_| "invalid_imported_template")?;
        registry.validate()?;
        let rows = source
            .templates
            .into_iter()
            .map(|template| {
                let previous = registry.imported_templates.iter().find(|saved| {
                    saved.source_snapshot_id.as_deref() == Some(source_snapshot_id.as_str())
                        && saved.template.id == template.id
                });
                let identical = previous.or_else(|| {
                    registry
                        .imported_templates
                        .iter()
                        .find(|saved| saved.template == template)
                });
                let (disposition, existing_ids) = if let Some(saved) = identical {
                    (Disposition::Identical, vec![saved.id.clone()])
                } else {
                    let ids = registry
                        .imported_templates
                        .iter()
                        .filter(|saved| collides(&saved.template, &template))
                        .map(|saved| saved.id.clone())
                        .collect::<Vec<_>>();
                    (
                        if ids.is_empty() {
                            Disposition::New
                        } else {
                            Disposition::Conflict
                        },
                        ids,
                    )
                };
                Row {
                    template,
                    disposition,
                    existing_ids,
                    already_imported: previous.is_some(),
                }
            })
            .collect();
        Ok(Self {
            registry_revision: registry.revision,
            source_snapshot_id,
            rows,
            before_digest: fingerprint(&registry.imported_templates)?,
        })
    }
    pub fn apply(&self, registry: &mut Registry, choices: Vec<Choice>) -> Result<Applied> {
        registry.validate()?;
        if registry.revision != self.registry_revision
            || fingerprint(&registry.imported_templates)? != self.before_digest
        {
            return Err("stale_registry");
        }
        if choices.len() > self.rows.len() {
            return Err("invalid_legacy_choices");
        }
        let mut selected = BTreeSet::new();
        for choice in &choices {
            if !selected.insert(&choice.source_id)
                || !self
                    .rows
                    .iter()
                    .any(|row| row.template.id == choice.source_id)
            {
                return Err("invalid_legacy_choices");
            }
        }
        let mut next = registry.clone();
        let mut result = Applied {
            added: 0,
            reused: 0,
            skipped: 0,
            mappings: vec![],
        };
        for row in &self.rows {
            let decision = choices
                .iter()
                .find(|choice| choice.source_id == row.template.id)
                .map(|choice| choice.decision)
                .unwrap_or(Decision::Skip);
            let imported_id = match (row.disposition, decision) {
                (_, Decision::Skip) => {
                    result.skipped += 1;
                    None
                }
                (Disposition::Identical, Decision::Reuse) => {
                    result.reused += 1;
                    Some(row.existing_ids[0].clone())
                }
                (Disposition::New, Decision::Import)
                | (Disposition::Conflict, Decision::KeepBoth) => {
                    let id = uuid::Uuid::new_v4().to_string();
                    next.imported_templates.push(ImportedTemplate {
                        id: id.clone(),
                        source_snapshot_id: Some(self.source_snapshot_id.clone()),
                        local: false,
                        archived: false,
                        template: row.template.clone(),
                    });
                    result.added += 1;
                    Some(id)
                }
                _ => return Err("invalid_legacy_choices"),
            };
            result.mappings.push(Mapping {
                source_id: row.template.id.clone(),
                imported_id,
            });
        }
        if result.added > 0 {
            next.revision = next.revision.checked_add(1).ok_or("invalid_revision")?;
            next.encode()?;
            *registry = next;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn template() -> ProfileTemplate {
        let mut template = ProfileTemplate::new("웹 기본값");
        template.expected_ports = vec![3000, 5173];
        template.run_manager_service_ids = vec!["old-service".into()];
        template
    }
    fn source(template: &ProfileTemplate) -> ProfileTemplateStore {
        ProfileTemplateStore {
            version: 1,
            templates: vec![template.clone()],
        }
    }
    fn choices(template: &ProfileTemplate, decision: Decision) -> Vec<Choice> {
        vec![Choice {
            source_id: template.id.clone(),
            decision,
        }]
    }
    #[test]
    fn empty_path_templates_roundtrip_with_original_ids_and_repeat_without_authority() {
        let template = template();
        let mut registry = Registry::default();
        let plan = Plan::build("a".repeat(64), source(&template), &registry).unwrap();
        let result = plan
            .apply(&mut registry, choices(&template, Decision::Import))
            .unwrap();
        assert_eq!(result.added, 1);
        assert_eq!(registry.imported_templates[0].template, template);
        assert_ne!(registry.imported_templates[0].id, template.id);
        assert!(
            registry.projects.is_empty()
                && registry.worktrees.is_empty()
                && registry.imported_profiles.is_empty()
        );
        let saved = Registry::parse(&registry.encode().unwrap()).unwrap();
        let repeat = Plan::build("b".repeat(64), source(&template), &registry).unwrap();
        assert_eq!(repeat.rows[0].disposition, Disposition::Identical);
        let result = repeat
            .apply(&mut registry, choices(&template, Decision::Reuse))
            .unwrap();
        assert_eq!(result.reused, 1);
        assert_eq!(registry, saved);
        let value = serde_json::to_value(&template).unwrap();
        assert!(value.get("environment").is_none());
        let mut invalid = value;
        invalid["environment"] = serde_json::json!({"enabled": true});
        assert!(serde_json::from_value::<ProfileTemplate>(invalid).is_err());
    }
    #[test]
    fn conflicts_require_explicit_keep_both_and_stale_or_invalid_choices_preserve_all_bytes() {
        let mut template = template();
        let mut registry = Registry::default();
        Plan::build("a".repeat(64), source(&template), &registry)
            .unwrap()
            .apply(&mut registry, choices(&template, Decision::Import))
            .unwrap();
        template.id = uuid::Uuid::new_v4().to_string();
        template.name = template.name.to_uppercase();
        template.expected_ports = vec![8080];
        let plan = Plan::build("b".repeat(64), source(&template), &registry).unwrap();
        assert_eq!(plan.rows[0].disposition, Disposition::Conflict);
        let before = registry.clone();
        for choice in [Decision::Import, Decision::Reuse] {
            assert!(matches!(
                plan.apply(&mut registry, choices(&template, choice)),
                Err("invalid_legacy_choices")
            ));
            assert_eq!(registry, before);
        }
        let invalid = vec![Choice {
            source_id: "foreign".into(),
            decision: Decision::Skip,
        }];
        assert!(matches!(
            plan.apply(&mut registry, invalid),
            Err("invalid_legacy_choices")
        ));
        let mut duplicate = choices(&template, Decision::KeepBoth);
        duplicate.extend(choices(&template, Decision::Skip));
        assert!(matches!(
            plan.apply(&mut registry, duplicate),
            Err("invalid_legacy_choices")
        ));
        assert_eq!(registry, before);
        registry.imported_templates[0].template.expected_ports = vec![9090];
        let changed = registry.clone();
        assert!(matches!(
            plan.apply(&mut registry, choices(&template, Decision::KeepBoth)),
            Err("stale_registry")
        ));
        assert_eq!(registry, changed);
        registry = before.clone();
        plan.apply(&mut registry, choices(&template, Decision::KeepBoth))
            .unwrap();
        assert_eq!(registry.imported_templates.len(), 2);
        assert_eq!(registry.imported_templates[0], before.imported_templates[0]);
        assert_eq!(registry.imported_templates[1].template, template);
        registry
            .imported_templates
            .push(registry.imported_templates[0].clone());
        assert_eq!(registry.validate(), Err("duplicate_imported_template"));
    }
}
