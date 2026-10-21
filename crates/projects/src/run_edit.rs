//! Project file writes and the matching approval/revision have one domain owner.
use crate::{ProjectsError, ProjectsService, R, config, run_config::*, store, trust};
use devbox_protocol::Internal;
use sha2::{Digest, Sha256};
use std::path::Path;
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, Value};

fn invalid(error: impl std::fmt::Display) -> ProjectsError {
    ProjectsError::ConfigurationInvalid {
        message: error.to_string(),
    }
}
fn normalized(key: &str) -> String {
    key.replace('_', "").to_ascii_lowercase()
}
fn fresh_table(parsed: &Table) -> Table {
    let mut table = Table::new();
    for (key, item) in parsed.iter() {
        let item = match item {
            Item::Table(t) => Item::Value(Value::InlineTable(t.clone().into_inline_table())),
            other => other.clone(),
        };
        table.insert(key, item);
    }
    table
}
fn patch_table(old: &mut Table, new: &Table, aliases: bool) {
    let equivalent = |a: &str, b: &str| {
        if aliases {
            normalized(a) == normalized(b)
        } else {
            a == b
        }
    };
    let removed: Vec<_> = old
        .iter()
        .filter(|(key, _)| !new.iter().any(|(name, _)| equivalent(key, name)))
        .map(|(key, _)| key.to_owned())
        .collect();
    for key in removed {
        old.remove(&key);
    }
    for (name, value) in new.iter() {
        let existing = old
            .iter()
            .find(|(key, _)| equivalent(key, name))
            .map(|(key, _)| key.to_owned());
        if let Some(key) = existing {
            // Only this typed nested structure has snake/camel aliases. Environment
            // map keys are user-defined identifiers and must compare exactly.
            let child_aliases = aliases && normalized(name) == "startlimit";
            patch_item(&mut old[&key], value, child_aliases);
        } else {
            old.insert(name, value.clone());
        }
    }
}
fn patch_item(old: &mut Item, new: &Item, aliases: bool) {
    if let (Some(table), Some(inline)) = (old.as_table_mut(), new.as_inline_table()) {
        patch_table(table, &inline.clone().into_table(), aliases);
        return;
    }
    if let (Some(old), Some(new)) = (old.as_inline_table_mut(), new.as_inline_table()) {
        let equivalent = |a: &str, b: &str| {
            if aliases {
                normalized(a) == normalized(b)
            } else {
                a == b
            }
        };
        let removed: Vec<_> = old
            .iter()
            .filter(|(key, _)| !new.iter().any(|(name, _)| equivalent(key, name)))
            .map(|(key, _)| key.to_owned())
            .collect();
        for key in removed {
            old.remove(&key);
        }
        for (key, value) in new.iter() {
            let existing = old
                .iter()
                .find(|(name, _)| equivalent(name, key))
                .map(|(name, _)| name.to_owned());
            if let Some(current) = existing.as_deref().and_then(|name| old.get_mut(name)) {
                let decor = current.decor().clone();
                *current = value.clone();
                *current.decor_mut() = decor;
            } else {
                old.insert(key, value.clone());
            }
        }
        return;
    }
    if let (Some(current), Some(value)) = (old.as_value_mut(), new.as_value()) {
        let decor = current.decor().clone();
        *current = value.clone();
        *current.decor_mut() = decor;
    } else {
        *old = new.clone();
    }
}
fn edited(bytes: &[u8], change: &RunConfigChange) -> R<Vec<u8>> {
    let text = std::str::from_utf8(bytes).map_err(invalid)?;
    let mut doc: DocumentMut = text.parse().map_err(invalid)?;
    let (id, replacement) = match change {
        RunConfigChange::Define(def) => {
            let (section, text) = match def {
                RunDef::Task(d) => ("task", toml::to_string(d)),
                RunDef::Service(d) => ("service", toml::to_string(d)),
                RunDef::Schedule(d) => ("schedule", toml::to_string(d)),
            };
            let parsed: DocumentMut = text.map_err(invalid)?.parse().map_err(invalid)?;
            let table = fresh_table(parsed.as_table());
            (def.id(), Some((section, table)))
        }
        RunConfigChange::Undefine { id } => (id.as_str(), None),
    };
    let mut inserted = false;
    for section in ["task", "service", "schedule"] {
        if let Some(array) = doc.get_mut(section).and_then(Item::as_array_mut) {
            let mut index = 0;
            while index < array.len() {
                let same = array
                    .get(index)
                    .and_then(Value::as_inline_table)
                    .and_then(|t| t.get("id"))
                    .and_then(Value::as_str)
                    == Some(id);
                if same {
                    if let Some((target, replacement)) = &replacement
                        && *target == section
                        && !inserted
                    {
                        let mut table = array
                            .get(index)
                            .expect("matched entry")
                            .as_inline_table()
                            .expect("matched inline table")
                            .clone()
                            .into_table();
                        patch_table(&mut table, replacement, true);
                        array.replace(index, Value::InlineTable(table.into_inline_table()));
                        inserted = true;
                        index += 1;
                        continue;
                    }
                    array.remove(index);
                } else {
                    index += 1;
                }
            }
        }
        if let Some(tables) = doc.get_mut(section).and_then(Item::as_array_of_tables_mut) {
            let mut next = ArrayOfTables::new();
            for table in tables.iter() {
                if table.get("id").and_then(Item::as_str) == Some(id) {
                    if let Some((target, replacement)) = &replacement
                        && *target == section
                        && !inserted
                    {
                        let mut updated = table.clone();
                        patch_table(&mut updated, replacement, true);
                        next.push(updated);
                        inserted = true;
                    }
                } else {
                    next.push(table.clone());
                }
            }
            *tables = next;
        }
    }
    if !inserted && let Some((section, table)) = replacement {
        if doc.get(section).is_none() {
            doc[section] = Item::ArrayOfTables(ArrayOfTables::new());
        }
        if let Some(array) = doc[section].as_array_mut() {
            array.push(Value::InlineTable(table.into_inline_table()));
        } else {
            doc[section]
                .as_array_of_tables_mut()
                .ok_or_else(|| invalid("execution definitions must use arrays"))?
                .push(table);
        }
    }
    let bytes = doc.to_string().into_bytes();
    if bytes.len() > config::CONFIG_MAX_BYTES {
        return Err(invalid("project configuration exceeds size limit").into());
    }
    Ok(bytes)
}

fn checked_write(root: &Path, expected: &[u8], replacement: &[u8], id: &str) -> R<()> {
    let path = root.join(".devbox/devbox.toml");
    config::check_ancestors(&path).map_err(invalid)?;
    std::fs::create_dir_all(root.join(".devbox")).map_err(invalid)?;
    let temporary = root
        .join(".devbox")
        .join(format!(".devbox-{}.tmp", uuid::Uuid::new_v4().simple()));
    let result = (|| {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(invalid)?;
        if let Ok(metadata) = std::fs::metadata(&path) {
            file.set_permissions(metadata.permissions())
                .map_err(invalid)?;
        }
        file.write_all(replacement).map_err(invalid)?;
        file.sync_all().map_err(invalid)?;
        drop(file);
        // The compare is next to the rename, after all potentially slow preparation.
        let (current, _) = config::read_config(root).map_err(invalid)?;
        if current != expected {
            return Err(ProjectsError::TrustPreviewStale { id: id.into() }.into());
        }
        std::fs::rename(&temporary, &path).map_err(invalid)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

impl ProjectsService {
    pub async fn update_run_config(&self, id: &str, edit: RunConfigEdit) -> R<UpdatedRunConfig> {
        let lock = self
            .run_edits
            .lock()
            .await
            .entry(id.to_owned())
            .or_default()
            .clone();
        let _guard = lock.lock().await;
        let s = self.trust_snapshot(id).await?;
        if s.digest != edit.expected_digest {
            return Err(ProjectsError::TrustPreviewStale { id: id.into() }.into());
        }
        let bytes = edited(&s.bytes, &edit.change)?;
        let parsed =
            config::parse(std::str::from_utf8(&bytes).map_err(invalid)?).map_err(invalid)?;
        let required_scope = trust::required_scope_version(&parsed);
        let result_config = parsed.run_config();
        let digest = format!("{:x}", Sha256::digest(&bytes));
        #[cfg(test)]
        let fail_run_write = self
            .fail_run_write
            .swap(false, std::sync::atomic::Ordering::SeqCst);
        let saved_digest = digest.clone();
        let hub = self.hub.clone();
        let (trust, rev) = self
            .db
            .write_checked_with_commit(
                move |tx| {
                    if !store::get(tx, &s.project.id)?.is_some_and(|p| p.path == s.registered_path)
                    {
                        return Ok(Err(
                            ProjectsError::TrustPreviewStale { id: s.project.id }.into()
                        ));
                    }
                    // Approval is evaluated against the exact previous bytes inside this writer.
                    let refresh = match trust::required_scope_version(&s.config) {
                        None => true,
                        Some(scope) => {
                            store::approved(tx, &s.project.id, &s.project.path, &s.digest, scope)?
                        }
                    };
                    #[cfg(test)]
                    if fail_run_write {
                        return Ok(Err(invalid(std::io::Error::other(
                            "injected config write failure",
                        ))
                        .into()));
                    }
                    if let Err(error) =
                        checked_write(Path::new(&s.project.path), &s.bytes, &bytes, &s.project.id)
                    {
                        return Ok(Err(error));
                    }
                    if refresh {
                        store::approve(
                            tx,
                            &s.project.id,
                            &s.project.path,
                            &digest,
                            trust::SCOPE_VERSION,
                            crate::now_ms(),
                        )?;
                    }
                    // Restoring previously approved bytes can reuse the existing row,
                    // even when the immediately preceding bytes were unapproved.
                    let status = match required_scope {
                        None => crate::TrustPreviewStatus::NotRequired,
                        Some(scope) => {
                            if store::approved(tx, &s.project.id, &s.project.path, &digest, scope)?
                            {
                                crate::TrustPreviewStatus::Approved
                            } else {
                                crate::TrustPreviewStatus::ApprovalRequired
                            }
                        }
                    };
                    Ok(Ok(status))
                },
                move |_, rev| crate::publish_changed(&hub, rev),
            )
            .await
            .internal("save project execution definitions")??;
        Ok(UpdatedRunConfig {
            trust,
            digest: saved_digest,
            config: result_config,
            rev: rev.to_string(),
        })
    }
}

#[cfg(test)]
mod tests;
