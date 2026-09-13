//! Federated queries reuse Knowledge's bounded native index/FS readers. Their
//! generations cannot evict or cancel the product search UI's active references.
use crate::{
    core::source_search::{Consumer, Row, Snapshot},
    search,
};
use product_contract::{
    commands::{self, ContextRequirement, Descriptor, DisabledReason, EntityKind, Target},
    query::Cancellation,
    transport::{Call, QueryMode, Source},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}
struct SearchLease {
    app: tauri::AppHandle,
    generation: String,
    keep: bool,
}
impl Drop for SearchLease {
    fn drop(&mut self) {
        if !self.keep {
            let _ = search::dispatch_for(
                &self.app,
                "source_cancel",
                json!({"generation":self.generation}),
                Consumer::Federated,
            );
        }
    }
}
fn descriptor(
    source: &str,
    id: &str,
    name: &str,
    available: bool,
) -> Result<Descriptor, &'static str> {
    let kind = if source == "notes" { "note" } else { "file" };
    let name = if name.is_empty()
        || name.len() > 220
        || name.chars().any(char::is_control)
        || devbox_applink::contains_sensitive_value(name)
    {
        "이름 숨김"
    } else {
        name
    };
    let result = Descriptor {
        id: format!("knowledge.{kind}-{id}"),
        owner: "knowledge".into(),
        component: "knowledge.search".into(),
        label: name.into(),
        revision: Sha256::digest(format!("knowledge-source-ref-v1:{source}:{id}").as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        target: Target::Entity {
            entity: if kind == "note" {
                EntityKind::Note
            } else {
                EntityKind::File
            },
            id: id.into(),
        },
        review_route: Some("search".into()),
        required_context: ContextRequirement::None,
        context: None,
        destructive: false,
        requires_review: true,
        disabled_reason: (!available).then_some(DisabledReason::Stale),
    };
    result.validate()?;
    Ok(result)
}
fn row_descriptor(
    source: &str,
    row: &Row,
    generation: &str,
    index: usize,
) -> Result<Descriptor, &'static str> {
    let id = row
        .reference
        .clone()
        .unwrap_or_else(|| format!("unverified-{generation}-{index}"));
    descriptor(
        source,
        &id,
        row.value.get("name").and_then(Value::as_str).unwrap_or(""),
        row.reference.is_some() && row.availability == "available",
    )
}
fn capture_command() -> Descriptor {
    Descriptor {
        id: "knowledge.quick-capture".into(),
        owner: "knowledge".into(),
        component: "knowledge.notes".into(),
        label: "빠른 캡처".into(),
        revision: Sha256::digest(b"knowledge-quick-capture-v1")
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
        target: Target::Entity {
            entity: EntityKind::Capture,
            id: "inbox".into(),
        },
        review_route: Some("notes".into()),
        required_context: ContextRequirement::None,
        context: None,
        destructive: false,
        requires_review: true,
        disabled_reason: None,
    }
}
async fn saved_index(
    app: tauri::AppHandle,
) -> Result<(product_contract::command_index::Index, Vec<Value>), &'static str> {
    use std::sync::{Arc, OnceLock};
    static READERS: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
    let permit = READERS
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(2)))
        .clone()
        .try_acquire_owned()
        .map_err(|_| "knowledge_source_busy")?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let generation = search::current_generation(&app).map_err(|_| "knowledge_source_stale")?;
        let rows = everything_plus_lib::component::saved_query_definitions(&app)
            .map_err(|_| "knowledge_source_unavailable")?;
        let rows = rows
            .as_array()
            .filter(|rows| rows.len() <= 2048)
            .ok_or("knowledge_source_invalid")?
            .clone();
        let mut index = product_contract::command_index::Index::default();
        for row in &rows {
            let id = row["id"]
                .as_i64()
                .filter(|id| *id > 0)
                .ok_or("knowledge_source_invalid")?
                .to_string();
            let name = row["name"].as_str().ok_or("knowledge_source_invalid")?;
            let revision = Sha256::digest(
                serde_json::to_vec(&(&generation, row)).map_err(|_| "knowledge_source_invalid")?,
            )
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
            index.insert(Descriptor {
                id: format!("knowledge.saved-query-{id}"),
                owner: "knowledge".into(),
                component: "knowledge.search".into(),
                label: name.into(),
                revision,
                target: Target::Entity {
                    entity: EntityKind::SavedQuery,
                    id,
                },
                review_route: Some("search".into()),
                required_context: ContextRequirement::None,
                context: None,
                destructive: false,
                requires_review: true,
                disabled_reason: None,
            })?;
        }
        if search::current_generation(&app).map_err(|_| "knowledge_source_stale")? != generation {
            return Err("knowledge_source_stale");
        }
        Ok((index, rows))
    })
    .await
    .map_err(|_| "knowledge_source_unavailable")?
}
pub(crate) async fn saved_reference(app: &tauri::AppHandle, args: Value) -> Result<Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        id: String,
        revision: String,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    if !commands::opaque_id(&input.id) || !commands::revision(&input.revision) {
        return Err("component_args_invalid".into());
    }
    let (index, rows) = saved_index(app.clone()).await?;
    index.resolve(&commands::Request {
        operation_id: uuid::Uuid::new_v4().to_string(),
        command_id: format!("knowledge.saved-query-{}", input.id),
        revision: input.revision,
        context: None,
        selection_id: None,
    })?;
    rows.into_iter()
        .find(|row| {
            row["id"]
                .as_i64()
                .is_some_and(|id| id.to_string() == input.id)
        })
        .ok_or_else(|| "search_stale".into())
}
pub(crate) fn handle(
    app: tauri::AppHandle,
    call: Call,
    deadline: u64,
    cancellation: Option<Cancellation>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, &'static str>> + Send>> {
    Box::pin(async move {
        match call {
            Call::ReadFileReference { reference } => crate::search::open(
                &app,
                "native_file_reference",
                json!({"reference":reference}),
            )
            .await
            .map_err(|_| "knowledge_file_stale"),
            Call::DeliverSessionSummary {
                source_id,
                operation_id,
                revision,
            } => crate::session_receive::offer(&app, &source_id, &operation_id, &revision),
            Call::InvalidateProjectSnapshot {} => {
                crate::project_provider::invalidate(&app);
                Ok(Value::Null)
            }
            Call::ResolveShortcut { command } if command == "knowledge.quick-capture" => {
                serde_json::to_value(capture_command()).map_err(|_| "knowledge_command_invalid")
            }
            Call::Query {
                source: Source::Commands,
                query,
                generation,
                ..
            } => {
                let mut index = product_contract::command_index::Index::default();
                index.insert(capture_command())?;
                Ok(
                    json!({"generation":generation,"source":"commands","owner":"knowledge","result":index.search(&query)?}),
                )
            }

            Call::Query {
                source: Source::SavedQueries,
                query,
                generation,
                mode,
                ..
            } => {
                if mode != QueryMode::Name {
                    return Err("knowledge_query_mode_unavailable");
                }
                let (index, _) = saved_index(app).await?;
                if now() >= deadline || cancellation.as_ref().is_some_and(|token| token.requested())
                {
                    return Err("query_cancelled");
                }
                Ok(
                    json!({"owner":"knowledge","source":"savedQueries","generation":generation,"result":index.search(&query)?}),
                )
            }

            Call::Query {
                source: source @ (Source::Notes | Source::Files),
                query,
                generation,
                mode,
                ..
            } => {
                let source_name = if matches!(source, Source::Notes) {
                    "notes"
                } else {
                    "files"
                };
                if cancellation.as_ref().is_some_and(|token| token.requested()) {
                    return Err("query_cancelled");
                }
                let value=search::dispatch_for(&app,"source_query",json!({"source":source_name,"query":query,"mode":if mode==QueryMode::Content{"content"}else{"name"},"limit":128,"filter":{}}),Consumer::Federated).map_err(|_|"knowledge_source_unavailable")?;
                let id = value["generation"]
                    .as_str()
                    .filter(|id| commands::opaque_id(id))
                    .ok_or("knowledge_source_invalid")?
                    .to_owned();
                let mut lease = SearchLease {
                    app: app.clone(),
                    generation: id.clone(),
                    keep: false,
                };
                let mut snapshot: Snapshot =
                    serde_json::from_value(value).map_err(|_| "knowledge_source_invalid")?;
                while snapshot.state == "running" {
                    if now() >= deadline
                        || cancellation.as_ref().is_some_and(|token| token.requested())
                    {
                        return Err("query_cancelled");
                    }
                    tokio::time::sleep(Duration::from_millis(30)).await;
                    snapshot = serde_json::from_value(
                        search::dispatch_for(
                            &app,
                            "source_poll",
                            json!({"generation":id}),
                            Consumer::Federated,
                        )
                        .map_err(|_| "knowledge_source_unavailable")?,
                    )
                    .map_err(|_| "knowledge_source_invalid")?;
                }
                if now() >= deadline || cancellation.as_ref().is_some_and(|token| token.requested())
                {
                    return Err("query_cancelled");
                }
                let mut results = Vec::new();
                let mut skipped = false;
                let mut freshness = serde_json::Map::new();
                for (index, row) in snapshot.rows.iter().take(128).enumerate() {
                    match row_descriptor(source_name, row, &id, index) {
                        Ok(item) => {
                            freshness.insert(item.id.clone(),json!({"availability":row.availability,"indexStale":row.index_stale}));
                            results.push(item)
                        }
                        Err(_) => skipped = true,
                    }
                }
                lease.keep = true;
                Ok(
                    json!({"owner":"knowledge","source":source,"generation":generation,"state":snapshot.state,"partial":snapshot.partial,"result":{"results":results,"freshness":freshness,"state":snapshot.state,"partial":snapshot.partial,"truncated":snapshot.partial||skipped||snapshot.rows.len()>128}}),
                )
            }
            Call::PreviewCommand { request } => {
                if request.command_id == "knowledge.quick-capture" {
                    let current = capture_command();
                    current.validate_request(&request)?;
                    return serde_json::to_value(current).map_err(|_| "knowledge_command_invalid");
                }

                if request.command_id.starts_with("knowledge.saved-query-") {
                    return serde_json::to_value(saved_index(app).await?.0.resolve(&request)?)
                        .map_err(|_| "knowledge_source_invalid");
                }

                let (source, id) =
                    if let Some(id) = request.command_id.strip_prefix("knowledge.note-") {
                        ("notes", id)
                    } else if let Some(id) = request.command_id.strip_prefix("knowledge.file-") {
                        ("files", id)
                    } else {
                        return Err("knowledge_command_unavailable");
                    };
                if uuid::Uuid::parse_str(id).is_err() {
                    return Err("knowledge_reference_invalid");
                }
                let reference = search::federated_reference(&app, id)
                    .map_err(|_| "knowledge_reference_stale")?;
                if reference.source != source {
                    return Err("knowledge_reference_invalid");
                }
                let current = descriptor(
                    source,
                    id,
                    reference.candidate.value["name"].as_str().unwrap_or(""),
                    true,
                )?;
                current.validate_request(&request)?;
                serde_json::to_value(current).map_err(|_| "knowledge_source_invalid")
            }
            _ => Err("knowledge_source_unavailable"),
        }
    })
}
