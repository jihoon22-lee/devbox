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
    assert_eq!(catalog.catalog_revision, Some(17));
    assert_eq!(catalog.apps.len(), 15);
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
            "repo-manager"
        ]
    );
    assert_eq!(
        capable_targets(&catalog, "query")
            .into_iter()
            .map(|app| app.id)
            .collect::<Vec<_>>(),
        vec!["everything-plus", "knowledge-base"]
    );
    assert_eq!(
        capable_targets(&catalog, "profile")
            .into_iter()
            .map(|app| app.id)
            .collect::<Vec<_>>(),
        vec!["wsl-desktop", "workbench"]
    );
    assert_eq!(
        capable_targets(&catalog, "task")
            .into_iter()
            .map(|app| app.id)
            .collect::<Vec<_>>(),
        vec!["run-manager"]
    );
    assert_eq!(
        capable_producers(&catalog, "snapshot:run-manager/status/v1")
            .into_iter()
            .map(|app| app.id)
            .collect::<Vec<_>>(),
        vec!["run-manager"]
    );
    assert_eq!(
        capable_producers(&catalog, "snapshot:run-manager/jobs-services/v1")
            .into_iter()
            .map(|app| app.id)
            .collect::<Vec<_>>(),
        vec!["run-manager"]
    );
    assert_eq!(
        capable_targets(&catalog, "handoff:knowledge-draft/v1")
            .into_iter()
            .map(|app| app.id)
            .collect::<Vec<_>>(),
        vec!["knowledge-base"]
    );
    assert_eq!(
        capable_targets(&catalog, "handoff:knowledge-draft/v2")
            .into_iter()
            .map(|app| app.id)
            .collect::<Vec<_>>(),
        vec!["knowledge-base"]
    );
    assert_eq!(
        capable_targets(&catalog, "handoff:log-source/v1")
            .into_iter()
            .map(|app| app.id)
            .collect::<Vec<_>>(),
        vec!["log-lens"]
    );
    assert_eq!(
        capable_producers(&catalog, "handoff:log-source/v1")
            .into_iter()
            .map(|app| app.id)
            .collect::<Vec<_>>(),
        vec!["port-manager", "wsl-desktop", "run-manager"]
    );
    assert_eq!(
        capable_producers(&catalog, "snapshot:port-bindings/v1")
            .into_iter()
            .map(|app| app.id)
            .collect::<Vec<_>>(),
        vec!["run-manager", "workbench"]
    );
    assert_eq!(
        capable_producers(&catalog, "snapshot:life-log/projects/v1")
            .into_iter()
            .map(|app| app.id)
            .collect::<Vec<_>>(),
        vec!["life-log"]
    );
    assert_eq!(
        capable_producers(&catalog, "snapshot:wsl-desktop/runtime/v1")
            .into_iter()
            .map(|app| app.id)
            .collect::<Vec<_>>(),
        vec!["wsl-desktop"]
    );
    assert_eq!(
        capable_producers(&catalog, "snapshot:wsl-desktop/profiles/v1")
            .into_iter()
            .map(|app| app.id)
            .collect::<Vec<_>>(),
        vec!["wsl-desktop"]
    );
    assert_eq!(
        capable_producers(&catalog, "snapshot:workbench/profiles/v1")
            .into_iter()
            .map(|app| app.id)
            .collect::<Vec<_>>(),
        vec!["workbench"]
    );
    assert_eq!(
        capable_producers(&catalog, "snapshot:repo-manager/dependency-summary/v1")
            .into_iter()
            .map(|app| app.id)
            .collect::<Vec<_>>(),
        vec!["repo-manager"]
    );
    assert_eq!(
        capable_producers(&catalog, "snapshot:repo-manager/repositories/v1")
            .into_iter()
            .map(|app| app.id)
            .collect::<Vec<_>>(),
        vec!["repo-manager"]
    );
    assert_eq!(
        capable_producers(&catalog, "snapshot:everything-plus/saved-queries/v1")
            .into_iter()
            .map(|app| app.id)
            .collect::<Vec<_>>(),
        vec!["everything-plus"]
    );
    assert_eq!(
        capable_targets(&catalog, "handoff:api-request/v1")
            .into_iter()
            .map(|app| app.id)
            .collect::<Vec<_>>(),
        vec!["api-playground"]
    );
    assert_eq!(
        capable_producers(&catalog, "handoff:api-request/v1")
            .into_iter()
            .map(|app| app.id)
            .collect::<Vec<_>>(),
        vec!["developer-toolbox", "webhook-lab"]
    );
    assert_eq!(
        capable_targets(&catalog, "handoff:webhook-log/v1")
            .into_iter()
            .map(|app| app.id)
            .collect::<Vec<_>>(),
        vec!["log-lens"]
    );
    assert_eq!(
        capable_producers(&catalog, "handoff:webhook-log/v1")
            .into_iter()
            .map(|app| app.id)
            .collect::<Vec<_>>(),
        vec!["webhook-lab"]
    );
    assert_eq!(
        capable_targets(&catalog, "handoff:toolbox-text/v1")
            .into_iter()
            .map(|app| app.id)
            .collect::<Vec<_>>(),
        vec!["developer-toolbox"]
    );
    assert_eq!(
        capable_producers(&catalog, "handoff:toolbox-text/v1")
            .into_iter()
            .map(|app| app.id)
            .collect::<Vec<_>>(),
        vec!["api-playground", "devbox-launcher", "log-lens"]
    );
    assert_eq!(
        capable_producers(&catalog, "handoff:knowledge-draft/v2")
            .into_iter()
            .map(|app| app.id)
            .collect::<Vec<_>>(),
        vec!["developer-toolbox"]
    );
    assert_eq!(
        capable_producers(&catalog, "snapshot:daily-activity/v1")
            .into_iter()
            .map(|app| app.id)
            .collect::<Vec<_>>(),
        vec!["knowledge-base", "run-manager"]
    );
    assert_eq!(
        capable_producers(&catalog, "handoff:knowledge-draft/v1")
            .into_iter()
            .map(|app| app.id)
            .collect::<Vec<_>>(),
        vec!["life-log"]
    );
    assert_eq!(
        capable_targets(&catalog, "handoff:knowledge-draft/v1")
            .into_iter()
            .map(|app| app.id)
            .collect::<Vec<_>>(),
        vec!["knowledge-base"]
    );
    let log_lens = catalog
        .apps
        .iter()
        .find(|app| app.id == "log-lens")
        .expect("Log Lens must remain in the repository catalog");
    assert_eq!(
        log_lens.accepts,
        vec!["handoff:log-source/v1", "handoff:webhook-log/v1"]
    );
    assert_eq!(log_lens.produces, vec!["handoff:toolbox-text/v1"]);
    assert_eq!(log_lens.actions.len(), 1);
    assert_eq!(log_lens.actions[0].action_id, "transform-selected-logs");
    assert_eq!(log_lens.actions[0].target, "developer-toolbox");
    let port_manager = catalog
        .apps
        .iter()
        .find(|app| app.id == "port-manager")
        .expect("Port Manager must remain in the repository catalog");
    assert_eq!(port_manager.produces, vec!["handoff:log-source/v1"]);
    assert_eq!(port_manager.actions.len(), 1);
    assert_eq!(port_manager.actions[0].action_id, "open-listener-log");
    assert_eq!(port_manager.actions[0].target, "log-lens");
    assert_eq!(
        port_manager.actions[0].payload_kind,
        "handoff:log-source/v1"
    );
    let webhook_lab = catalog
        .apps
        .iter()
        .find(|app| app.id == "webhook-lab")
        .expect("Webhook Lab must remain in the repository catalog");
    assert_eq!(
        webhook_lab.produces,
        vec!["handoff:api-request/v1", "handoff:webhook-log/v1"]
    );
    assert_eq!(webhook_lab.actions.len(), 1);
    assert_eq!(webhook_lab.actions[0].action_id, "inspect-capture-logs");
    assert_eq!(webhook_lab.actions[0].target, "log-lens");
    assert_eq!(
        webhook_lab.actions[0].payload_kind,
        "handoff:webhook-log/v1"
    );
    let life_log = catalog
        .apps
        .iter()
        .find(|app| app.id == "life-log")
        .expect("Life Log must remain in the repository catalog");
    assert_eq!(
        life_log.produces,
        vec![
            "snapshot:life-log/projects/v1",
            "handoff:knowledge-draft/v1"
        ]
    );
    assert_eq!(life_log.actions.len(), 1);
    assert_eq!(life_log.actions[0].target, "knowledge-base");
    assert_eq!(
        life_log.actions[0].payload_kind,
        "handoff:knowledge-draft/v1"
    );
    let toolbox = catalog
        .apps
        .iter()
        .find(|app| app.id == "developer-toolbox")
        .expect("Developer Toolbox must remain in the repository catalog");
    assert_eq!(toolbox.accepts, vec!["handoff:toolbox-text/v1"]);
    assert_eq!(
        toolbox.produces,
        vec!["handoff:api-request/v1", "handoff:knowledge-draft/v2"]
    );
    assert_eq!(toolbox.actions.len(), 2);
    assert_eq!(toolbox.actions[0].target, "api-playground");
    assert_eq!(toolbox.actions[0].payload_kind, "handoff:api-request/v1");
    assert_eq!(toolbox.actions[1].action_id, "save-output-to-knowledge");
    assert_eq!(toolbox.actions[1].target, "knowledge-base");
    assert_eq!(
        toolbox.actions[1].payload_kind,
        "handoff:knowledge-draft/v2"
    );
    let api_playground = catalog
        .apps
        .iter()
        .find(|app| app.id == "api-playground")
        .expect("API Playground must remain in the repository catalog");
    assert_eq!(api_playground.produces, vec!["handoff:toolbox-text/v1"]);
    assert_eq!(api_playground.actions.len(), 1);
    assert_eq!(
        api_playground.actions[0].action_id,
        "transform-response-text"
    );
    assert_eq!(api_playground.actions[0].target, "developer-toolbox");
    let knowledge_base = catalog
        .apps
        .iter()
        .find(|app| app.id == "knowledge-base")
        .expect("Knowledge Base must remain in the repository catalog");
    assert_eq!(
        knowledge_base.accepts,
        vec![
            "path",
            "query",
            "handoff:knowledge-draft/v1",
            "handoff:knowledge-draft/v2"
        ]
    );
    assert_eq!(
        knowledge_base.produces,
        vec![
            "snapshot:knowledge-base/activity/v1",
            "snapshot:daily-activity/v1"
        ]
    );
    let run_manager = catalog
        .apps
        .iter()
        .find(|app| app.id == "run-manager")
        .expect("Run Manager must remain in the repository catalog");
    assert!(run_manager
        .produces
        .contains(&"snapshot:daily-activity/v1".to_string()));
    let launcher = catalog
        .apps
        .iter()
        .find(|app| app.id == "devbox-launcher")
        .expect("Devbox Launcher must remain in the repository catalog");
    assert_eq!(launcher.produces, vec!["handoff:toolbox-text/v1"]);
    assert_eq!(launcher.actions.len(), 1);
    assert_eq!(launcher.actions[0].action_id, "transform-text");
    assert_eq!(launcher.actions[0].target, "developer-toolbox");
    assert_eq!(launcher.actions[0].payload_kind, "handoff:toolbox-text/v1");
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

    let mut spoofed_shared_snapshot: Value =
        serde_json::from_str(V2_BUILD).expect("v2 fixture JSON");
    spoofed_shared_snapshot["apps"][0]["produces"] = json!(["snapshot:daily-activity/v1"]);
    assert!(matches!(
        parse_catalog(&spoofed_shared_snapshot.to_string()),
        Err(CatalogError::InvalidCapability {
            app_index: 0,
            field: "produces"
        })
    ));

    let mut spoofed_port_bindings: Value = serde_json::from_str(V2_BUILD).expect("v2 fixture JSON");
    spoofed_port_bindings["apps"][0]["produces"] = json!(["snapshot:port-bindings/v1"]);
    assert!(matches!(
        parse_catalog(&spoofed_port_bindings.to_string()),
        Err(CatalogError::InvalidCapability {
            app_index: 0,
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
