//! Internal handoffs use the existing one-time wire format in this installation's
//! private namespace. Metadata and wake-ups confer no receiver authority.
use applink::{CreateHandoff, HandoffStore, OpenRequest, ToolboxTextPayload};
use product_contract::{references::ArtifactReference, Provenance};
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{Emitter, Manager};

struct Store(HandoffStore);
pub fn initialize(app: &tauri::AppHandle) -> Result<HandoffStore, String> {
    let root = app
        .path()
        .app_local_data_dir()
        .map_err(|_| "handoff_storage_unavailable")?;
    let store = HandoffStore::new(root.join("handoff/v1"));
    if !app.manage(Store(store.clone())) {
        return Err("handoff_state_conflict".into());
    }
    Ok(store)
}

pub fn is_send(component: &str, method: &str) -> bool {
    matches!(
        (component, method),
        ("api-studio.api", "send_selection_to_toolbox")
            | (
                "api-studio.webhooks",
                "send_history_to_api" | "send_fixture_to_api"
            )
    )
}

fn publish(
    store: &HandoffStore,
    create: CreateHandoff,
    provenance: Provenance,
    now: u64,
    deliver: impl FnOnce(OpenRequest) -> Result<(), String>,
) -> Result<Value, String> {
    let expires = now
        .checked_add(applink::DEFAULT_HANDOFF_TTL_MS)
        .ok_or("handoff_clock_invalid")?;
    let source = create.source_app.clone();
    let recipient = create.target_app.clone().ok_or("handoff_target_invalid")?;
    let publication = store
        .create_with_publication(create, now)
        .map_err(|_| "handoff_publication_failed")?;
    let link = OpenRequest {
        target: publication.descriptor.clone().into(),
        from: Some(source.clone()),
    };
    if let Err(error) = deliver(link.clone()) {
        store
            .remove_pending(&publication)
            .map_err(|_| "handoff_cleanup_failed")?;
        return Err(error);
    }
    Ok(json!({
        "handoffId": publication.descriptor.id,
        "producerId": source,
        "consumerId": recipient,
        "createdAtMs": now,
        "expiresAtMs": expires,
        "artifact": ArtifactReference { provenance, recipient, link },
    }))
}

pub fn send(
    app: &tauri::AppHandle,
    component: &str,
    method: &str,
    args: Value,
    provenance: Provenance,
) -> Result<Value, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|value| u64::try_from(value.as_millis()).ok())
        .filter(|value| *value > 0)
        .ok_or("handoff_clock_invalid")?;
    let store = app.state::<Store>();
    let (create, route, redacted) = match (component, method) {
        ("api-studio.api", "send_selection_to_toolbox") => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Input {
                text: String,
            }
            let Input { text } =
                serde_json::from_value(args).map_err(|_| "handoff_input_invalid")?;
            let text = zeroize::Zeroizing::new(text);
            let (payload, redacted) =
                ToolboxTextPayload::from_selected_text("api-playground", &text)
                    .map_err(|_| "handoff_input_invalid")?;
            (
                CreateHandoff {
                    kind: applink::TOOLBOX_TEXT_HANDOFF_KIND.into(),
                    source_app: "api-playground".into(),
                    target_app: Some(applink::TOOLBOX_TEXT_TARGET_APP.into()),
                    payload: serde_json::to_value(payload).map_err(|_| "handoff_input_invalid")?,
                },
                "transforms",
                redacted,
            )
        }
        ("api-studio.webhooks", "send_history_to_api" | "send_fixture_to_api") => {
            let payload = webhook_lab_lib::component::prepare_api_handoff(
                app,
                args,
                method == "send_fixture_to_api",
            )?;
            (
                CreateHandoff {
                    kind: "api-request/v1".into(),
                    source_app: "webhook-lab".into(),
                    target_app: Some("api-playground".into()),
                    payload,
                },
                "requests",
                false,
            )
        }
        _ => return Err("handoff_route_invalid".into()),
    };
    let mut result = publish(&store.0, create, provenance, now, |link| {
        if route == "transforms" {
            developer_toolbox_lib::component::deliver(app, link)
        } else {
            api_playground_lib::component::deliver(app, link)
        }
    })?;
    result["redacted"] = Value::Bool(redacted);
    let _ = app.emit_to("main", "api-studio://navigate", route);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn provenance() -> Provenance {
        Provenance {
            product: "api-studio".into(),
            component: "api-studio.api".into(),
            request_id: "fixture".into(),
            revision: 1,
        }
    }
    fn input() -> CreateHandoff {
        let (payload, _) =
            ToolboxTextPayload::from_selected_text("api-playground", "fixture output").unwrap();
        CreateHandoff {
            kind: applink::TOOLBOX_TEXT_HANDOFF_KIND.into(),
            source_app: "api-playground".into(),
            target_app: Some(applink::TOOLBOX_TEXT_TARGET_APP.into()),
            payload: serde_json::to_value(payload).unwrap(),
        }
    }
    #[test]
    fn scoped_publication_is_one_time_and_cannot_be_claimed_from_legacy_store() {
        let dir = tempfile::tempdir().unwrap();
        let product = HandoffStore::new(dir.path().join("product/handoff/v1"));
        let legacy = HandoffStore::new(dir.path().join("legacy/handoff/v1"));
        let result = publish(&product, input(), provenance(), 1_000, |_| Ok(())).unwrap();
        let id = result["handoffId"].as_str().unwrap();
        assert_eq!(
            result["artifact"]["provenance"]["component"],
            "api-studio.api"
        );
        assert!(legacy
            .claim(
                id,
                applink::TOOLBOX_TEXT_HANDOFF_KIND,
                applink::TOOLBOX_TEXT_TARGET_APP,
                1_001
            )
            .is_err());
        let claim = product
            .claim(
                id,
                applink::TOOLBOX_TEXT_HANDOFF_KIND,
                applink::TOOLBOX_TEXT_TARGET_APP,
                1_001,
            )
            .unwrap();
        product
            .ack(&claim, applink::TOOLBOX_TEXT_TARGET_APP, 1_002)
            .unwrap();
        assert!(product
            .claim(
                id,
                applink::TOOLBOX_TEXT_HANDOFF_KIND,
                applink::TOOLBOX_TEXT_TARGET_APP,
                1_003
            )
            .is_err());
    }
    #[test]
    fn busy_receiver_revokes_publication_instead_of_leaving_a_false_delivery() {
        let dir = tempfile::tempdir().unwrap();
        let store = HandoffStore::new(dir.path().join("handoff/v1"));
        let mut id = String::new();
        assert!(publish(&store, input(), provenance(), 1_000, |link| {
            let applink::OpenTarget::Handoff { id: value, .. } = link.target else {
                panic!("handoff")
            };
            id = value;
            Err("receiver_busy".into())
        })
        .is_err());
        assert!(store
            .claim(
                &id,
                applink::TOOLBOX_TEXT_HANDOFF_KIND,
                applink::TOOLBOX_TEXT_TARGET_APP,
                1_001
            )
            .is_err());
        assert!(!is_send(
            "api-studio.transforms",
            "send_selection_to_toolbox"
        ));
    }
}
