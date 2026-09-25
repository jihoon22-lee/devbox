//! Reconnect saved current-product Runtime logs without acquiring retired sources.
use super::*;
use logs_engine::core::{runtime_views, FilterSpec};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    sources: Vec<SourceSpec>,
    filter: FilterSpec,
}
fn reconnect(value: Value, mut revision: impl FnMut(&str) -> Option<String>) -> Result<Value> {
    let mut input: Input = args(value)?;
    logs_engine::core::validate_source_list(&input.sources)
        .map_err(|_| "runtime_settings_invalid")?;
    input
        .filter
        .validate()
        .map_err(|_| "runtime_settings_invalid")?;
    let unavailable =
        runtime_views::rebase(&mut input.sources, &mut input.filter, |run, imported| {
            if imported {
                None
            } else {
                revision(run)
            }
        })?;
    Ok(json!({"sources":input.sources,"filter":input.filter,"unavailableSources":unavailable}))
}
pub(super) fn execute(app: &tauri::AppHandle, value: Value, deadline: u64) -> Result<Value> {
    let result = reconnect(value, |run| {
        let lease = runtime_engine::component::log_descriptor(app, run).ok()?;
        lease.revalidate().ok()?;
        Some(lease.revision().into())
    })?;
    crate::files_host::current_deadline(deadline)?;
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn current_run_reconnects_and_preserves_selected_filter() {
        let source = SourceSpec::RuntimeRun {
            run_id: "current-run".into(),
            stream: "stdout".into(),
            revision: "1".repeat(64),
        };
        let value = json!({"sources":[source],"filter":FilterSpec{source_id:Some(source.opaque_id()),..Default::default()}});
        let result = reconnect(value, |run| {
            assert_eq!(run, "current-run");
            Some("2".repeat(64))
        })
        .unwrap();
        let sources: Vec<SourceSpec> = serde_json::from_value(result["sources"].clone()).unwrap();
        assert_eq!(result["unavailableSources"], 0);
        assert_eq!(result["filter"]["sourceId"], sources[0].opaque_id());
        assert_eq!(result["sources"][0]["revision"], "2".repeat(64));
    }
    #[test]
    fn retired_source_never_binds_to_same_named_current_run() {
        let source = SourceSpec::Run {
            source_id: "run-manager:old-run:stdout".into(),
        };
        let result = reconnect(
            json!({"sources":[source],"filter":FilterSpec::default()}),
            |_| panic!("retired run must not resolve"),
        )
        .unwrap();
        assert_eq!(result["unavailableSources"], 1);
        assert_eq!(result["sources"][0]["revision"], "0".repeat(64));
    }
}
