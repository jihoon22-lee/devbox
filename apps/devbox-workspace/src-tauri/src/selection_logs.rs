//! Selection resolves exact native read rows, never renderer-supplied log text.
use log_lens_lib::core::{export_records, LogRecord};
use product_contract::{transform_selection::Selection, ProjectContext};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::VecDeque,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
type Result<T> = std::result::Result<T, &'static str>;
#[derive(Clone)]
struct Snapshot {
    request: Value,
    records: Vec<Value>,
    context: Option<ProjectContext>,
    created: Instant,
}
#[derive(Default)]
struct Owner {
    generation: u64,
    snapshots: VecDeque<Snapshot>,
}
fn owner() -> &'static Mutex<Owner> {
    static OWNER: OnceLock<Mutex<Owner>> = OnceLock::new();
    OWNER.get_or_init(Mutex::default)
}
pub(crate) fn begin(generation: u64) {
    if let Ok(mut owner) = owner().lock() {
        owner.generation = generation;
    }
}
pub(crate) fn capture(request: Value, response: &Value, context: Option<&ProjectContext>) {
    let Ok(mut owner) = owner().lock() else {
        return;
    };
    if request["generation"].as_u64() != Some(owner.generation) {
        return;
    }
    let Some(records) = response["records"].as_array() else {
        return;
    };
    if records.is_empty() {
        return;
    }
    let bytes = serde_json::to_vec(records).map_or(usize::MAX, |value| value.len());
    if bytes > 4 * 1024 * 1024 {
        owner.snapshots.clear();
        return;
    }
    owner.snapshots.retain(|entry| {
        entry.created.elapsed() < Duration::from_secs(120)
            && entry.context.as_ref() == context
            && entry.request["sources"] == request["sources"]
    });
    owner.snapshots.push_back(Snapshot {
        request,
        records: records.clone(),
        context: context.cloned(),
        created: Instant::now(),
    });
    while owner.snapshots.len() > 16
        || owner
            .snapshots
            .iter()
            .map(|entry| {
                serde_json::to_vec(&entry.records).map_or(usize::MAX / 32, |value| value.len())
            })
            .sum::<usize>()
            > 4 * 1024 * 1024
    {
        owner.snapshots.pop_front();
    }
}
#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Key {
    source_id: String,
    sequence: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    generation: u64,
    keys: Vec<Key>,
}
#[derive(Clone)]
pub(crate) struct Proof {
    generation: u64,
    groups: Vec<Snapshot>,
}
fn key(value: &Value) -> Option<Key> {
    Some(Key {
        source_id: value["sourceId"].as_str()?.into(),
        sequence: value["sequence"].as_u64()?,
    })
}
pub(crate) fn prepare(args: Value, context: Option<&ProjectContext>) -> Result<(Proof, Selection)> {
    let input: Input = serde_json::from_value(args).map_err(|_| "selection_invalid")?;
    if input.keys.is_empty() || input.keys.len() > 256 {
        return Err("selection_limit");
    }
    let owner = owner().lock().map_err(|_| "selection_busy")?;
    if input.generation != owner.generation {
        return Err("selection_stale");
    }
    let mut groups: Vec<Snapshot> = Vec::new();
    let mut selected = Vec::new();
    for (index, target) in input.keys.iter().enumerate() {
        if input.keys[..index].contains(target) {
            return Err("selection_invalid");
        }
        let (source, row) = owner
            .snapshots
            .iter()
            .rev()
            .filter(|snapshot| {
                snapshot.context.as_ref() == context
                    && snapshot.created.elapsed() < Duration::from_secs(120)
            })
            .find_map(|snapshot| {
                snapshot
                    .records
                    .iter()
                    .find(|row| key(row).as_ref() == Some(target))
                    .map(|row| (snapshot, row))
            })
            .ok_or("selection_stale")?;
        selected.push(
            serde_json::from_value::<LogRecord>(row.clone()).map_err(|_| "selection_invalid")?,
        );
        if let Some(group) = groups
            .iter_mut()
            .find(|group| group.request == source.request)
        {
            group.records.push(row.clone());
        } else {
            let mut group = source.clone();
            group.records = vec![row.clone()];
            groups.push(group);
        }
    }
    let exported = export_records(&selected).map_err(|_| "selection_invalid")?;
    if exported.truncated {
        return Err("selection_limit");
    }
    let value = Selection::prepare("log-lens", &exported.text)?;
    Ok((
        Proof {
            generation: input.generation,
            groups,
        },
        value,
    ))
}
pub(crate) async fn revalidate(app: &tauri::AppHandle, proof: &Proof, deadline: u64) -> Result<()> {
    if owner().lock().map_err(|_| "selection_busy")?.generation != proof.generation {
        return Err("selection_stale");
    }
    for group in &proof.groups {
        crate::files_host::current_deadline(deadline)?;
        let mut request = group.request.clone();
        request["operationId"] = Value::String(uuid::Uuid::new_v4().to_string());
        let current = log_lens_lib::component::dispatch(app, "read_sources", request)
            .await
            .map_err(|_| "selection_stale")?;
        let rows = current["records"].as_array().ok_or("selection_stale")?;
        if group
            .records
            .iter()
            .any(|expected| !rows.contains(expected))
        {
            return Err("selection_stale");
        }
    }
    if owner().lock().map_err(|_| "selection_busy")?.generation != proof.generation {
        return Err("selection_stale");
    }
    Ok(())
}
