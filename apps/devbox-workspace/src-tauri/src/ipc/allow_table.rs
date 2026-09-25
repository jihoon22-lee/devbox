//! Permission baseline captured before the typed Workspace migration.
//! Candidate methods deliberately include unsupported names to detect widening.
use std::collections::BTreeSet;
type Row = (String, String, String);
pub(crate) fn committed() -> Vec<Row> {
    serde_json::from_str(include_str!("../../tests/fixtures/allow-table.json")).unwrap()
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
                if crate::component::allowed(component, route, method) {
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
