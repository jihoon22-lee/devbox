use crate::{Definition, DefinitionSource, RunDef, RuntimeError};
use devbox_core::input::validate_command_text;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug)]
pub struct MergedDefinitions {
    pub defs: Vec<Definition>,
    pub shadowed: Vec<Definition>,
}

impl MergedDefinitions {
    /// Validated effective graph in dependency order, preserving configured sibling order.
    pub fn order_for(&self, id: &str) -> Vec<String> {
        let map: BTreeMap<_, _> = self.defs.iter().map(|d| (d.def.id(), &d.def)).collect();
        let mut seen = BTreeSet::new();
        let mut order = Vec::new();
        let mut pending = vec![(id, false)];
        while let Some((id, expanded)) = pending.pop() {
            if expanded {
                order.push(id.into());
            } else if seen.insert(id)
                && let Some(def) = map.get(id)
            {
                pending.push((id, true));
                for dependency in def.dependencies().iter().rev() {
                    pending.push((dependency, false));
                }
            }
        }
        order
    }
}

fn invalid(id: &str, reason: impl Into<String>) -> RuntimeError {
    RuntimeError::DefInvalid {
        id: id.into(),
        reason: reason.into(),
    }
}

pub fn validate_definition(def: &RunDef) -> Result<(), RuntimeError> {
    let id = def.id();
    if id.trim().is_empty() || id.len() > 128 || id.contains('\0') {
        return Err(invalid(
            id,
            "definition ID must be 1–128 UTF-8 bytes without NUL",
        ));
    }
    if let Some(command) = def.command() {
        if command.trim().is_empty() {
            return Err(invalid(id, "command is empty"));
        }
        validate_command_text(command).map_err(|error| invalid(id, error.to_string()))?;
    }
    let fields = match def {
        RunDef::Task(d) => Some((&d.cwd, &d.env, &d.worktree, &d.secrets)),
        RunDef::Service(d) => Some((&d.cwd, &d.env, &d.worktree, &d.secrets)),
        RunDef::Schedule(d) => {
            crate::cron::validate_cron(&d.cron)
                .map_err(|error| invalid(id, format!("invalid cron: {error}")))?;
            if let Some(tz) = &d.tz {
                tz.parse::<chrono_tz::Tz>()
                    .map_err(|_| invalid(id, "unknown timezone"))?;
            }
            None
        }
    };
    if let Some((cwd, env, worktree, secrets)) = fields {
        if cwd
            .iter()
            .chain(worktree.iter())
            .any(|text| text.contains('\0'))
        {
            return Err(invalid(id, "working directory contains NUL"));
        }
        if env
            .iter()
            .any(|(key, value)| key.is_empty() || key.contains(['=', '\0']) || value.contains('\0'))
        {
            return Err(invalid(id, "invalid environment variable"));
        }
        if secrets
            .iter()
            .any(|name| name.is_empty() || name.contains(['=', '\0']))
        {
            return Err(invalid(id, "invalid secret name"));
        }
    }
    // Each accepted item must fit by itself into the bounded list response.
    if serde_json::to_vec(def)
        .map_err(|e| invalid(id, e.to_string()))?
        .len()
        > crate::service::PAGE_BYTES - 4096
    {
        return Err(invalid(id, "definition exceeds the list response budget"));
    }
    Ok(())
}

pub fn merge(
    file_defs: Vec<RunDef>,
    local_defs: Vec<RunDef>,
) -> Result<MergedDefinitions, RuntimeError> {
    let mut definitions = BTreeMap::new();
    for (source, defs) in [
        (DefinitionSource::ThisPc, local_defs),
        (DefinitionSource::ProjectFile, file_defs),
    ] {
        let mut seen = BTreeSet::new();
        for def in &defs {
            validate_definition(def)?;
            if !seen.insert(def.id()) {
                return Err(invalid(
                    def.id(),
                    "duplicate definition ID in the same source",
                ));
            }
        }
        for def in defs {
            let id = def.id().to_owned();
            definitions
                .entry(id)
                .or_insert_with(Vec::new)
                .push(Definition { source, def });
        }
    }
    let mut merged = MergedDefinitions {
        defs: Vec::new(),
        shadowed: Vec::new(),
    };
    for mut entries in definitions.into_values() {
        merged
            .defs
            .push(entries.pop().expect("nonempty definitions"));
        merged.shadowed.extend(entries);
    }
    validate_graph(&merged.defs)?;
    Ok(merged)
}

fn validate_graph(defs: &[Definition]) -> Result<(), RuntimeError> {
    let map: BTreeMap<_, _> = defs.iter().map(|d| (d.def.id(), &d.def)).collect();
    for def in map.values() {
        for dependency in def.dependencies() {
            let target = map
                .get(dependency.as_str())
                .ok_or_else(|| invalid(def.id(), format!("missing dependency: {dependency}")))?;
            if matches!(target, RunDef::Schedule(_))
                || (matches!(def, RunDef::Schedule(_)) && !matches!(target, RunDef::Task(_)))
            {
                return Err(invalid(
                    def.id(),
                    format!("invalid dependency kind: {dependency}"),
                ));
            }
        }
    }
    let mut complete = BTreeSet::new();
    for root in map.keys() {
        if complete.contains(root) {
            continue;
        }
        let mut path = Vec::new();
        let mut active = BTreeSet::new();
        let mut pending = vec![(*root, false)];
        while let Some((id, expanded)) = pending.pop() {
            if expanded {
                active.remove(id);
                path.pop();
                complete.insert(id);
            } else if !complete.contains(id) {
                if !active.insert(id) {
                    let start = path.iter().position(|p| *p == id).unwrap_or(0);
                    let cycle = path[start..]
                        .iter()
                        .copied()
                        .chain(std::iter::once(id))
                        .collect::<Vec<_>>()
                        .join(" → ");
                    return Err(invalid(id, format!("dependency cycle: {cycle}")));
                }
                path.push(id);
                pending.push((id, true));
                for dependency in map[id].dependencies().iter().rev() {
                    pending.push((dependency.as_str(), false));
                }
            }
        }
    }
    Ok(())
}
