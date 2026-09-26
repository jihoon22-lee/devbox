//! Permission baseline captured before the typed Workspace migration.
//! Candidate methods deliberately include unsupported names to detect widening.
use std::collections::BTreeSet;
type Row = (String, String, String);
pub(crate) fn committed() -> Vec<Row> {
    {
        let rows: Vec<Row> =
            serde_json::from_str(include_str!("../../tests/fixtures/allow-table.json")).unwrap();
        let mut rows: Vec<_> = rows
            .into_iter()
            .map(|(component, route, method)| {
                (
                    component.replace("workspace.migration", "workspace.setup"),
                    route,
                    method,
                )
            })
            .collect();
        rows.sort();
        rows
    }
}
pub(crate) fn current_allow_table() -> Vec<Row> {
    let catalog: serde_json::Value =
        serde_json::from_str(include_str!("../../../../products.json")).unwrap();
    let methods: Vec<String> =
        serde_json::from_str(include_str!("../../tests/fixtures/allow-candidates.json")).unwrap();
    let components: BTreeSet<_> = catalog["components"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["owner"] == "workspace")
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    let routes: BTreeSet<_> = catalog["features"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["owner"] == "workspace")
        .map(|r| r["route"].as_str().unwrap())
        .collect();
    let mut rows = Vec::new();
    for component in components {
        for route in &routes {
            for method in &methods {
                if permits(component, route, method) {
                    rows.push((component.into(), (*route).into(), method.clone()));
                }
            }
        }
    }
    rows.sort();
    rows
}
#[test]
fn allow_table_matches_the_committed_fixture() {
    assert_eq!(current_allow_table(), committed());
}

pub(crate) fn permits(component: &str, route: &str, method: &str) -> bool {
    let routes = match component {
        "workspace.runtime" => super::runtime::routes_for(method),
        "workspace.processes" => super::processes::routes_for(method),
        "workspace.process-actions" => super::process_actions::routes_for(method),
        "workspace.logs" => super::logs::routes_for(method),
        "workspace.terminal" => super::terminal::routes_for(method),
        "workspace.problems" => super::problems::routes_for(method),
        "workspace.commands" => super::commands::routes_for(method),
        "workspace.files" => super::files::routes_for(method),
        "workspace.lsp" => super::lsp::routes_for(method),
        "workspace.source" => super::source::routes_for(method),
        "workspace.registry" => super::registry::routes_for(method),
        "workspace.setup" => super::setup::routes_for(method),
        "workspace.definitions" => super::definitions::routes_for(method),
        "workspace.dependencies" => super::dependencies::routes_for(method),
        _ => &[],
    };
    routes.contains(&route)
}
