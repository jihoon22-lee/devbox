use catalog::{
    capable_producers, capable_targets, parse_catalog, select_catalog, CatalogError, CatalogSource,
    RuntimeFallbackReason, SCHEMA_V1, SCHEMA_V2,
};
use serde_json::{json, Value};

const V1_LEGACY: &str = include_str!("fixtures/v1-legacy.json");
const V2_BUILD: &str = include_str!("fixtures/v2-build.json");
const V2_RUNTIME_NEWER: &str = include_str!("fixtures/v2-runtime-newer.json");
const V2_RUNTIME_STALE: &str = include_str!("fixtures/v2-runtime-stale.json");
const RUNTIME_CORRUPT: &str = include_str!("fixtures/runtime-corrupt.json");
const FAKE_SIXTEENTH: &str = include_str!("fixtures/fake-sixteenth.json");
const REPOSITORY_CATALOG: &str = include_str!("../../../apps/catalog.json");

#[test]
fn v1_normalizes_revision_and_routing_fields_to_legacy_defaults() {
    let catalog = parse_catalog(V1_LEGACY).expect("v1 fixture should parse");

    assert_eq!(catalog.schema_version, SCHEMA_V1);
    assert_eq!(catalog.catalog_revision, None);
    assert_eq!(catalog.apps.len(), 16);
    assert!(catalog.apps.iter().all(|app| {
        app.accepts.is_empty() && app.produces.is_empty() && app.actions.is_empty()
    }));
}

#[test]
fn v2_parses_revision_capabilities_actions_and_fake_sixteenth_app() {
    let catalog = parse_catalog(FAKE_SIXTEENTH).expect("v2 fixture should parse");

    assert_eq!(catalog.schema_version, SCHEMA_V2);
    assert_eq!(catalog.catalog_revision, Some(5));
    assert_eq!(catalog.apps.len(), 16);
    assert_eq!(
        catalog.apps.last().map(|app| app.id.as_str()),
        Some("fake-sixteenth")
    );
    assert_eq!(
        capable_targets(&catalog, "path")
            .into_iter()
            .map(|app| app.id)
            .collect::<Vec<_>>(),
        vec![
            "wsl-desktop",
            "knowledge-base",
            "code-pad",
            "workbench",
            "repo-manager",
            "fake-sixteenth",
        ]
    );
    assert!(capable_targets(&catalog, "handoff:not-declared/v1").is_empty());
    assert_eq!(
        capable_producers(&catalog, "snapshot:knowledge-base/notes/v1")
            .into_iter()
            .map(|app| app.id)
            .collect::<Vec<_>>(),
        vec!["knowledge-base"]
    );
}

#[test]
fn repository_catalog_tracks_current_shipped_capabilities() {
    let catalog = parse_catalog(REPOSITORY_CATALOG).expect("repository catalog should parse");

    assert_eq!(catalog.schema_version, SCHEMA_V2);
    assert_eq!(catalog.catalog_revision, Some(3));
    assert_eq!(catalog.apps.len(), 13);
    assert!(catalog.apps.iter().all(|app| app.actions.is_empty()));
    assert_eq!(
        capable_targets(&catalog, "path")
            .into_iter()
            .map(|app| app.id)
            .collect::<Vec<_>>(),
        vec!["wsl-desktop", "knowledge-base", "code-pad", "workbench"]
    );
    assert_eq!(
        capable_targets(&catalog, "query")
            .into_iter()
            .map(|app| app.id)
            .collect::<Vec<_>>(),
        vec!["everything-plus", "knowledge-base"]
    );
}

#[test]
fn runtime_selection_uses_safe_build_time_fallbacks() {
    let missing = select_catalog(V2_BUILD, None).expect("missing runtime should fall back");
    assert_eq!(missing.source, CatalogSource::BuildTime);
    assert_eq!(
        missing.fallback_reason,
        Some(RuntimeFallbackReason::Missing)
    );

    let corrupt =
        select_catalog(V2_BUILD, Some(RUNTIME_CORRUPT)).expect("corrupt runtime should fall back");
    assert_eq!(corrupt.source, CatalogSource::BuildTime);
    assert_eq!(
        corrupt.fallback_reason,
        Some(RuntimeFallbackReason::Invalid)
    );

    let legacy = select_catalog(V2_BUILD, Some(V1_LEGACY)).expect("v1 runtime should fall back");
    assert_eq!(legacy.source, CatalogSource::BuildTime);
    assert_eq!(
        legacy.fallback_reason,
        Some(RuntimeFallbackReason::MissingRevision)
    );

    let stale =
        select_catalog(V2_BUILD, Some(V2_RUNTIME_STALE)).expect("stale runtime should fall back");
    assert_eq!(stale.source, CatalogSource::BuildTime);
    assert_eq!(
        stale.fallback_reason,
        Some(RuntimeFallbackReason::Stale {
            runtime_revision: 4,
            build_revision: 5,
        })
    );
    assert_eq!(stale.catalog.catalog_revision, Some(5));
}

#[test]
fn equal_or_newer_valid_runtime_catalog_wins() {
    let equal = select_catalog(V2_BUILD, Some(V2_BUILD)).expect("equal runtime should win");
    assert_eq!(equal.source, CatalogSource::Runtime);
    assert_eq!(equal.fallback_reason, None);
    assert_eq!(equal.catalog.catalog_revision, Some(5));

    let newer = select_catalog(V2_BUILD, Some(V2_RUNTIME_NEWER)).expect("newer runtime should win");
    assert_eq!(newer.source, CatalogSource::Runtime);
    assert_eq!(newer.fallback_reason, None);
    assert_eq!(newer.catalog.catalog_revision, Some(6));
}

#[test]
fn invalid_build_time_catalog_is_an_error_instead_of_a_runtime_fallback() {
    assert_eq!(
        select_catalog(RUNTIME_CORRUPT, Some(V2_RUNTIME_NEWER)),
        Err(CatalogError::InvalidJson)
    );
}

#[test]
fn revision_and_schema_contracts_are_strict() {
    let mut value: Value = serde_json::from_str(V2_BUILD).expect("fixture JSON");
    value
        .as_object_mut()
        .expect("catalog object")
        .remove("catalogRevision");
    assert_eq!(
        parse_catalog(&value.to_string()),
        Err(CatalogError::MissingCatalogRevision)
    );

    value["catalogRevision"] = json!(0);
    assert_eq!(
        parse_catalog(&value.to_string()),
        Err(CatalogError::InvalidCatalogRevision)
    );

    value["catalogRevision"] = json!(1);
    value["schemaVersion"] = json!(3);
    assert_eq!(
        parse_catalog(&value.to_string()),
        Err(CatalogError::UnsupportedSchema { schema_version: 3 })
    );
}

#[test]
fn invalid_capabilities_actions_and_duplicate_ids_are_rejected() {
    let mut invalid_direction: Value = serde_json::from_str(V2_BUILD).expect("fixture JSON");
    invalid_direction["apps"][0]["accepts"] = json!(["snapshot:port-manager/status/v1"]);
    assert!(matches!(
        parse_catalog(&invalid_direction.to_string()),
        Err(CatalogError::InvalidCapability {
            app_index: 0,
            field: "accepts"
        })
    ));

    let mut duplicate_capability: Value = serde_json::from_str(V2_BUILD).expect("fixture JSON");
    duplicate_capability["apps"][0]["accepts"] = json!(["path", "path"]);
    assert!(matches!(
        parse_catalog(&duplicate_capability.to_string()),
        Err(CatalogError::DuplicateCapability {
            app_index: 0,
            field: "accepts"
        })
    ));

    let mut broken_action: Value = serde_json::from_str(V2_BUILD).expect("fixture JSON");
    broken_action["apps"][1]["actions"][0]["target"] = json!("port-manager");
    assert!(matches!(
        parse_catalog(&broken_action.to_string()),
        Err(CatalogError::InvalidAction {
            app_index: 1,
            action_index: 0,
            field: "target"
        })
    ));

    let mut duplicate_id: Value = serde_json::from_str(V2_BUILD).expect("fixture JSON");
    duplicate_id["apps"][1]["id"] = json!("port-manager");
    duplicate_id["apps"][1]["appDir"] = json!("apps/port-manager");
    assert_eq!(
        parse_catalog(&duplicate_id.to_string()),
        Err(CatalogError::DuplicateAppId { index: 1 })
    );
}

#[test]
fn identity_and_snapshot_producer_contracts_are_rejected_fail_closed() {
    let mut duplicate_identifier: Value = serde_json::from_str(V2_BUILD).expect("fixture JSON");
    duplicate_identifier["apps"][1]["identifier"] =
        duplicate_identifier["apps"][0]["identifier"].clone();
    assert_eq!(
        parse_catalog(&duplicate_identifier.to_string()),
        Err(CatalogError::DuplicateAppIdentity {
            index: 1,
            field: "identifier",
        })
    );

    let mut invalid_id: Value = serde_json::from_str(V2_BUILD).expect("fixture JSON");
    invalid_id["apps"][0]["id"] = json!("port-manager.");
    invalid_id["apps"][0]["appDir"] = json!("apps/port-manager.");
    assert!(matches!(
        parse_catalog(&invalid_id.to_string()),
        Err(CatalogError::InvalidApp {
            index: 0,
            field: "id"
        })
    ));

    let mut invalid_identifier: Value = serde_json::from_str(V2_BUILD).expect("fixture JSON");
    invalid_identifier["apps"][0]["identifier"] = json!("com.devbox.");
    assert!(matches!(
        parse_catalog(&invalid_identifier.to_string()),
        Err(CatalogError::InvalidApp {
            index: 0,
            field: "identifier"
        })
    ));

    let mut spoofed_snapshot: Value = serde_json::from_str(V2_BUILD).expect("fixture JSON");
    spoofed_snapshot["apps"][5]["produces"] = json!(["snapshot:run-manager/notes/v1"]);
    assert!(matches!(
        parse_catalog(&spoofed_snapshot.to_string()),
        Err(CatalogError::InvalidCapability {
            app_index: 5,
            field: "produces"
        })
    ));
}

#[test]
fn duplicate_actions_unknown_fields_and_empty_catalog_are_rejected() {
    let mut duplicate_action: Value = serde_json::from_str(V2_BUILD).expect("fixture JSON");
    let action = duplicate_action["apps"][1]["actions"][0].clone();
    duplicate_action["apps"][1]["actions"] = json!([action.clone(), action]);
    assert_eq!(
        parse_catalog(&duplicate_action.to_string()),
        Err(CatalogError::DuplicateActionId {
            app_index: 1,
            action_index: 1,
        })
    );

    let mut unknown_field: Value = serde_json::from_str(V2_BUILD).expect("fixture JSON");
    unknown_field["runtimeSecret"] = json!("must-not-be-accepted");
    assert_eq!(
        parse_catalog(&unknown_field.to_string()),
        Err(CatalogError::InvalidJson)
    );

    let empty = json!({"schemaVersion": 2, "catalogRevision": 1, "apps": []});
    assert_eq!(
        parse_catalog(&empty.to_string()),
        Err(CatalogError::EmptyCatalog)
    );
}

#[test]
fn parse_errors_do_not_echo_untrusted_catalog_values() {
    let secret = "catalog-secret-must-not-appear";
    let input = V2_BUILD.replacen("port-manager", secret, 1);
    let message = parse_catalog(&input)
        .expect_err("invalid app id should fail")
        .to_string();

    assert!(!message.contains(secret));
    assert_eq!(message, "catalog app 0 has invalid appDir");
    assert_eq!(
        parse_catalog(RUNTIME_CORRUPT),
        Err(CatalogError::InvalidJson)
    );
}
