//! Fixed, reviewed legacy failures only. Never publish interpolated backend text.
use serde::Deserialize;
use std::{collections::BTreeMap, sync::LazyLock};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Contract {
    schema_version: u32,
    components: BTreeMap<String, Vec<String>>,
}
static CONTRACT: LazyLock<Contract> = LazyLock::new(|| {
    let contract: Contract = serde_json::from_str(include_str!("../../component-errors.json"))
        .expect("invalid compiled component error contract");
    assert_eq!(contract.schema_version, 1);
    contract
});
pub fn project(component: &str, error: &str) -> &'static str {
    CONTRACT
        .components
        .get(component)
        .and_then(|values| values.iter().find(|value| value.as_str() == error))
        .map(String::as_str)
        .unwrap_or("component_unavailable")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_errors_survive_and_details_or_foreign_errors_do_not() {
        for error in [
            "grpc_connection_stale",
            "grpc_request_cancelled",
            "mcp_oauth_reauthorization_required",
            "mcp_stdio_connection_stale",
            "mcp_stdio_cleanup_failed",
        ] {
            assert_eq!(project("api-studio.api", error), error);
        }
        assert_eq!(
            project("api-studio.transforms", "mcp_request_cancelled"),
            "component_unavailable"
        );
        assert_eq!(
            project("api-studio.api", "mcp_request_cancelled fixture-secret"),
            "component_unavailable"
        );
        assert_eq!(
            project("unknown", "grpc_connection_stale"),
            "component_unavailable"
        );
        assert_eq!(CONTRACT.components.len(), 3);
        for values in CONTRACT.components.values() {
            assert!(values.len() <= 512);
            assert!(values.windows(2).all(|pair| pair[0] < pair[1]));
            assert!(values
                .iter()
                .all(|value| value.len() <= 1024 && !value.contains('\n')));
        }
    }
}
