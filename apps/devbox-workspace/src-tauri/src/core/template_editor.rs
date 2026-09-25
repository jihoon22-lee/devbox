//! Editable destination templates. Legacy snapshots retain the original bytes;
//! template edits never change an already instantiated project profile.
use super::{templates::ImportedTemplate, registry::Registry};
use workbench_lib::component::ProfileTemplate;

type Result<T> = std::result::Result<T, &'static str>;

pub fn save(
    registry: &mut Registry,
    revision: u64,
    id: Option<&str>,
    mut template: ProfileTemplate,
) -> Result<String> {
    registry.validate()?;
    if registry.revision != revision {
        return Err("stale_registry");
    }
    let mut next = registry.clone();
    let index = match id {
        Some(id) => {
            let index = next
                .imported_templates
                .iter()
                .position(|entry| entry.id == id)
                .ok_or("unknown_imported_template")?;
            if template.id != next.imported_templates[index].template.id {
                return Err("template_identity_changed");
            }
            Some(index)
        }
        None => {
            if !template.id.is_empty() {
                return Err("template_identity_changed");
            }
            template.id = uuid::Uuid::new_v4().to_string();
            None
        }
    };
    template
        .validate()
        .map_err(|_| "invalid_imported_template")?;
    if let Some(index) = index {
        let entry = &next.imported_templates[index];
        if !entry.archived && entry.template == template {
            return Ok(entry.id.clone());
        }
    }
    if next
        .imported_templates
        .iter()
        .enumerate()
        .any(|(other, entry)| {
            Some(other) != index
                && !entry.archived
                && entry.template.name.to_lowercase() == template.name.to_lowercase()
        })
    {
        return Err("template_name_conflict");
    }
    let id = if let Some(index) = index {
        let entry = &mut next.imported_templates[index];
        entry.template = template;
        entry.archived = false;
        entry.id.clone()
    } else {
        let id = uuid::Uuid::new_v4().to_string();
        next.imported_templates.push(ImportedTemplate {
            id: id.clone(),
            source_snapshot_id: None,
            local: true,
            archived: false,
            template,
        });
        id
    };
    if next != *registry {
        next.revision = next.revision.checked_add(1).ok_or("invalid_revision")?;
        next.encode()?;
        *registry = next;
    }
    Ok(id)
}

/// Retain provenance referenced by existing profiles. Explicit edit/save can
/// restore the template to the active list; imports never restore it implicitly.
pub fn archive(registry: &mut Registry, revision: u64, id: &str) -> Result<()> {
    registry.validate()?;
    if registry.revision != revision {
        return Err("stale_registry");
    }
    let mut next = registry.clone();
    let entry = next
        .imported_templates
        .iter_mut()
        .find(|entry| entry.id == id)
        .ok_or("unknown_imported_template")?;
    if !entry.archived {
        entry.archived = true;
        next.revision = next.revision.checked_add(1).ok_or("invalid_revision")?;
        next.encode()?;
        *registry = next;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn draft(name: &str) -> ProfileTemplate {
        let mut template = ProfileTemplate::new(name);
        template.id.clear();
        template.expected_ports = vec![4321];
        template
    }
    #[test]
    fn local_templates_have_native_identity_and_explicit_origin_with_bounded_atomic_edits() {
        let mut registry = Registry::default();
        let id = save(&mut registry, 1, None, draft("로컬 기본값")).unwrap();
        assert_eq!(registry.revision, 2);
        let entry = registry.imported_templates[0].clone();
        assert!(entry.local && entry.source_snapshot_id.is_none() && !entry.archived);
        assert_ne!(entry.id, entry.template.id);
        assert!(uuid::Uuid::parse_str(&entry.template.id).is_ok());
        let before = registry.clone();
        assert_eq!(
            save(&mut registry, 1, Some(&id), entry.template.clone()),
            Err("stale_registry")
        );
        assert_eq!(archive(&mut registry, 1, &id), Err("stale_registry"));
        assert_eq!(
            save(&mut registry, 2, Some(&id), draft("identity")),
            Err("template_identity_changed")
        );
        assert_eq!(
            save(&mut registry, 2, None, entry.template.clone()),
            Err("template_identity_changed")
        );
        assert_eq!(
            save(&mut registry, 2, None, draft("로컬 기본값")),
            Err("template_name_conflict")
        );
        assert_eq!(registry, before);
        save(&mut registry, 2, Some(&id), entry.template.clone()).unwrap();
        assert_eq!(registry, before);
        archive(&mut registry, 2, &id).unwrap();
        let archived = registry.clone();
        archive(&mut registry, 3, &id).unwrap();
        assert_eq!(registry, archived);
        save(&mut registry, 3, Some(&id), entry.template).unwrap();
        assert!(!registry.imported_templates[0].archived);
        assert!(
            registry.projects.is_empty()
                && registry.worktrees.is_empty()
                && registry.imported_profiles.is_empty()
        );
        assert_eq!(
            Registry::parse(&registry.encode().unwrap()).unwrap(),
            registry
        );
        registry.revision = 9_007_199_254_740_991;
        let before = registry.clone();
        assert_eq!(
            save(&mut registry, before.revision, None, draft("overflow")),
            Err("invalid_revision")
        );
        assert_eq!(registry, before);
    }
    #[test]
    fn malformed_origin_secret_fields_and_capacity_fail_without_replacement() {
        let mut registry = Registry::default();
        save(&mut registry, 1, None, draft("local")).unwrap();
        let good = serde_json::to_value(&registry).unwrap();
        for origin in [
            serde_json::json!({}),
            serde_json::json!({"sourceSnapshotId":"a".repeat(64),"local":true}),
        ] {
            let mut malformed = good.clone();
            malformed["importedTemplates"][0]
                .as_object_mut()
                .unwrap()
                .remove("local");
            for (key, value) in origin.as_object().unwrap() {
                malformed["importedTemplates"][0][key] = value.clone();
            }
            assert!(Registry::parse(&serde_json::to_vec(&malformed).unwrap()).is_err());
        }
        let mut secret = serde_json::to_value(draft("invalid")).unwrap();
        secret["environment"] = serde_json::json!({"enabled":true});
        assert!(serde_json::from_value::<ProfileTemplate>(secret).is_err());
        while registry.imported_templates.len() < 512 {
            let mut entry = registry.imported_templates[0].clone();
            entry.id = uuid::Uuid::new_v4().to_string();
            entry.template.id = uuid::Uuid::new_v4().to_string();
            registry.imported_templates.push(entry);
        }
        let before = registry.clone();
        assert_eq!(
            save(&mut registry, 2, None, draft("over capacity")),
            Err("registry_limit")
        );
        assert_eq!(registry, before);
    }
}
