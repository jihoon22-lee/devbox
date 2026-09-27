#[test]
fn workspace_has_an_agents_route_backed_by_the_agents_component() {
    let catalog = catalog::products::ProductCatalog::parse(catalog::products::SOURCE).unwrap();
    let feature = catalog
        .features
        .iter()
        .find(|f| f.id == "workspace.agents")
        .expect("agents feature");
    assert_eq!(
        (feature.route.as_str(), feature.label.as_str()),
        ("agents", "에이전트")
    );
    let component = catalog
        .components
        .iter()
        .find(|c| c.id == "workspace.agents")
        .expect("agents component");
    assert_eq!(component.authority, "agent-task");
}
