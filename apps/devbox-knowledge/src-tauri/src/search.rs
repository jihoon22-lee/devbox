//! Product source API and separate opener. Renderer paths are never authority.
use crate::core::{
    project_provider::{Availability, Registry, Selection},
    source_search::{Candidate, ProjectReference, SearchJobs, VerifiedProject, Work},
    stores,
};
use everything_plus_lib::component::query::{self, SearchFilter};
use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tauri::Manager;

pub const METHODS: &[&str] = &[
    "source_query",
    "source_poll",
    "source_cancel",
    "source_reference",
    "source_saved_reference",
];
struct Host {
    root: PathBuf,
    manifest: stores::Manifest,
    jobs: SearchJobs,
    openers: Arc<AtomicUsize>,
    projects: Registry,
}
pub fn initialize(
    app: &tauri::AppHandle,
    root: &Path,
    manifest: &stores::Manifest,
) -> Result<(), String> {
    let jobs = SearchJobs::default();
    jobs.start_expiry()?;
    if !app.manage(Host {
        root: root.into(),
        manifest: manifest.clone(),
        jobs,
        openers: Arc::default(),
        projects: Registry::default(),
    }) {
        return Err("component_state_conflict".into());
    }
    Ok(())
}
/// B04/B07 calls this only after authenticating the registry owner and resolving
/// its Windows/WSL target bindings. There is no renderer invoke for this API.
pub fn install_project_snapshot(app: &tauri::AppHandle, bytes: &[u8]) -> Result<(), String> {
    use tauri::Emitter;
    let host = app.try_state::<Host>().ok_or("setup_required")?;
    if host.projects.replace(bytes)? {
        host.jobs.cancel_project();
        let _ = app.emit_to("main", "devbox://project-context", ());
    }
    Ok(())
}
/// Called on provider disconnect/expiry; existing file/note source jobs survive.
pub fn disconnect_project_provider(app: &tauri::AppHandle) {
    use tauri::Emitter;
    if let Some(host) = app.try_state::<Host>() {
        host.projects.disconnect();
        host.jobs.cancel_project();
        let _ = app.emit_to("main", "devbox://project-context", ());
    }
}
/// Decorate the existing Activity projections without rewriting source IDs,
/// dropping unmapped rows or changing duration/commit aggregation.
pub fn associate_activity(app: &tauri::AppHandle, method: &str, mut value: Value) -> Value {
    let Some(host) = app.try_state::<Host>() else {
        return value;
    };
    let registry = &host.projects;
    if method == "project_attribution" {
        if let Some(rows) = value["attributed"].as_array_mut() {
            for row in rows {
                if let Some(path) = row["projectId"].as_str() {
                    row["projectAssociation"] = registry.association(path);
                }
            }
        }
    }
    if method == "get_digest" {
        if let Some(rows) = value["document"]["git"]["projects"].as_array() {
            let associations: serde_json::Map<String, Value> = rows
                .iter()
                .filter_map(|row| {
                    row["path"]
                        .as_str()
                        .map(|path| (path.into(), registry.association(path)))
                })
                .collect();
            // The export document and its Markdown remain the existing immutable
            // native digest. Registry mapping is a separate UI projection.
            value["projectAssociations"] = Value::Object(associations);
        }
    }
    if matches!(method, "get_day" | "get_range") {
        if let Some(rows) = value["git"]["projects"].as_array_mut() {
            for row in rows {
                if let Some(path) = row["path"].as_str() {
                    row["projectAssociation"] = registry.association(path);
                }
            }
        }
    }
    value
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Query {
    source: String,
    query: String,
    mode: String,
    limit: Option<i64>,
    #[serde(default)]
    filter: SearchFilter,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Generation {
    generation: String,
}
fn read_connection(
    path: &Path,
    work: Option<&Work>,
    deadline: Instant,
) -> Result<Connection, String> {
    devbox_filesystem::ensure_no_links(path).map_err(|_| "search_unavailable")?;
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| "search_unavailable")?;
    conn.busy_timeout(Duration::from_millis(50))
        .map_err(|_| "search_unavailable")?;
    let cancel = work.map(|work| work.cancelled.clone());
    conn.progress_handler(
        1000,
        Some(move || {
            Instant::now() >= deadline
                || cancel
                    .as_ref()
                    .is_some_and(|cancel| cancel.load(Ordering::Acquire))
        }),
    );
    conn.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA query_only=ON; BEGIN")
        .map_err(|_| "search_unavailable")?;
    Ok(conn)
}
fn revision(conn: &Connection, source: &str, path: &str) -> Result<(PathBuf, Value), String> {
    if source == "notes" {
        conn.query_row("SELECT s.value, d.id, d.modified_ts, d.title FROM docs d JOIN settings s ON s.key='root' WHERE d.path=?1 AND length(CAST(s.value AS BLOB))<=32768 AND length(CAST(d.title AS BLOB))<=8192", [path], |row| {
            let root: String = row.get(0)?;
            Ok((PathBuf::from(&root), json!([root, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?, row.get::<_, String>(3)?])))
        }).map_err(|_| "search_stale".into())
    } else {
        if !query::is_indexed_path(conn, path).map_err(|_| "search_stale")? {
            return Err("search_stale".into());
        }
        conn.query_row("SELECT r.path, f.id, f.root_id, f.size, f.modified_ts FROM files f JOIN roots r ON r.id=f.root_id WHERE f.path=?1 AND length(CAST(r.path AS BLOB))<=32768", [path], |row| {
            let root: String = row.get(0)?;
            Ok((PathBuf::from(&root), json!([root, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?, row.get::<_, i64>(4)?])))
        }).map_err(|_| "search_stale".into())
    }
}
fn candidates(
    conn: &Connection,
    request: &Query,
    limit: i64,
    scope: Option<&str>,
) -> Result<Vec<Candidate>, String> {
    if request.query.trim().is_empty() {
        return Ok(Vec::new());
    }
    if request.source == "notes" {
        let rows = knowledge_base_lib::component::search_projection(
            conn,
            &request.query,
            limit.min(100),
            request.mode == "name",
        )
        .map_err(|_| "search_unavailable")?;
        rows.into_iter().map(|(path, title, snippet)| {
            let (root, revision) = revision(conn, "notes", &path)?;
            let absolute = root.join(&path);
            if Path::new(&path).is_absolute() || path.len() > 32768 || title.len() > 8192 { return Err("search_limit".into()); }
            let root_key = format!("notes:{}", crate::core::vault_binding::approval_id(conn, &root.to_string_lossy())?.unwrap_or_else(|| "vault".into()));
            let value = json!({"id": revision[1], "path": absolute, "notePath": path, "name": title, "ext":"md", "size":0, "modified_ts":revision[2], "snippet":snippet, "content_status":"indexed"});
            Ok(Candidate { path:absolute, root, root_key, revision, value, index_stale: false, offline: false })
        }).collect()
    } else {
        let values = if request.mode == "content" {
            serde_json::to_value(
                query::search_content_with_filter_in_scope(
                    conn,
                    &request.query,
                    limit,
                    &request.filter,
                    scope,
                )
                .map_err(|_| "search_unavailable")?,
            )
        } else {
            serde_json::to_value(
                query::search_with_filter_in_scope(
                    conn,
                    &request.query,
                    limit,
                    &request.filter,
                    scope,
                )
                .map_err(|_| "search_unavailable")?,
            )
        }
        .map_err(|_| "search_unavailable")?;
        values
            .as_array()
            .ok_or("search_unavailable")?
            .iter()
            .map(|value| {
                let path = value["path"]
                    .as_str()
                    .filter(|path| path.len() <= 32768)
                    .ok_or("search_limit")?;
                let (root, revision) = revision(conn, "files", path)?;
                Ok(Candidate {
                    path: path.into(),
                    root,
                    root_key: format!("files:{}", revision[2]),
                    revision,
                    value: value.clone(),
                    index_stale: false,
                    offline: false,
                })
            })
            .collect()
    }
}
fn run(
    app: tauri::AppHandle,
    root: PathBuf,
    manifest: stores::Manifest,
    request: Query,
    limit: i64,
    work: Work,
    project: Option<Selection>,
) {
    let result = (|| {
        let component = if request.source == "notes" {
            "notes"
        } else {
            "search"
        };
        let path = stores::directory(&root, &manifest, component)?.join("data.db");
        let conn = read_connection(&path, Some(&work), work.deadline)?;
        let mut rows = candidates(
            &conn,
            &request,
            limit,
            project.as_ref().map(|p| p.project.root.as_str()),
        )?;
        if request.source == "notes" {
            let (offline, stale) = knowledge_base_lib::component::product_index_health(&app);
            for row in &mut rows {
                row.offline = offline;
                row.index_stale = stale;
            }
        } else {
            let health = everything_plus_lib::component::product_root_health(&app);
            for row in &mut rows {
                let key = row.root.to_string_lossy().replace('\\', "/");
                let status = health
                    .iter()
                    .find(|(root, _, _)| root.replace('\\', "/") == key);
                row.offline = status.is_some_and(|(_, offline, _)| *offline);
                row.index_stale = status.is_none_or(|(_, _, stale)| *stale);
            }
        }
        if project
            .as_ref()
            .is_some_and(|p| p.project.availability != Availability::Available)
        {
            for row in &mut rows {
                row.offline = true;
                row.index_stale = true;
            }
        }
        // Release the SQL snapshot before any potentially offline filesystem IO.
        drop(conn);
        work.publish_candidates(&rows, rows.len() >= limit as usize)?;
        // Cached rows are already visible before an unknown/offline project
        // root can block a probe. The same retained source worker owns this IO.
        let verified_project = project
            .as_ref()
            .filter(|p| p.project.availability == Availability::Available)
            .and_then(|p| {
                if work.stopped() {
                    return None;
                }
                let root = PathBuf::from(&p.project.root);
                devbox_filesystem::ensure_no_links(&root).ok()?;
                let (object, identity) =
                    devbox_filesystem::open_filesystem_object(&root, true).ok()?;
                Some(VerifiedProject {
                    reference: ProjectReference {
                        epoch: p.epoch.clone(),
                        root,
                        identity,
                    },
                    object: Arc::new(object),
                })
            });
        // Native roots are checked first. A disconnected WSL probe cannot delay
        // publishing the already-read matches or other source workers.
        let mut order: Vec<_> = rows.iter().enumerate().collect();
        order.sort_by_key(|(_, row)| {
            row.path
                .to_string_lossy()
                .replace('\\', "/")
                .starts_with("//")
        });
        for (index, row) in order {
            if project.is_some() {
                work.verify_with_project(index, row, verified_project.as_ref());
            } else {
                work.verify(index, row);
            }
            if work.stopped() {
                break;
            }
        }
        Ok::<_, String>(())
    })();
    work.finish(if result.is_ok() {
        "complete"
    } else {
        "unavailable"
    });
}
pub fn dispatch(app: &tauri::AppHandle, method: &str, args: Value) -> Result<Value, String> {
    dispatch_for(
        app,
        method,
        args,
        crate::core::source_search::Consumer::Product,
    )
}
pub(crate) fn dispatch_for(
    app: &tauri::AppHandle,
    method: &str,
    args: Value,
    consumer: crate::core::source_search::Consumer,
) -> Result<Value, String> {
    let host = app.try_state::<Host>().ok_or("setup_required")?;
    if stores::read(&host.root)?.as_ref() != Some(&host.manifest) {
        return Err("search_stale".into());
    }
    match method {
        "source_reference" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Input {
                reference: String,
            }
            let input: Input =
                serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
            if uuid::Uuid::parse_str(&input.reference).is_err() {
                return Err("component_args_invalid".into());
            }
            let reference = host.jobs.resolve(&input.reference)?;
            Ok(
                json!({"reference":input.reference,"source":reference.source,"name":reference.candidate.value["name"],"path":reference.candidate.path}),
            )
        }
        "source_query" => {
            let mut request: Query =
                serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
            if !matches!(
                request.source.as_str(),
                "notes" | "files" | "current_project"
            ) || !matches!(request.mode.as_str(), "name" | "content")
                || request.query.len() > 4096
                || request.query.chars().any(char::is_control)
            {
                return Err("component_args_invalid".into());
            }
            request.filter = request
                .filter
                .normalized()
                .map_err(|_| "component_args_invalid")?;
            let project = (request.source == "current_project")
                .then(|| host.projects.current())
                .flatten();
            if (request.source == "current_project" && project.is_none())
                || (request.source == "notes" && !request.filter.is_empty())
            {
                let work =
                    host.jobs
                        .begin_for(&request.source, &host.manifest.generation, consumer)?;
                work.finish("unsupported");
                return serde_json::to_value(host.jobs.snapshot_for(&work.generation, consumer)?)
                    .map_err(|_| "search_unavailable".into());
            }
            let limit = request.limit.unwrap_or(200).clamp(
                1,
                if request.source == "notes" {
                    100
                } else if request.mode == "content" {
                    200
                } else {
                    2000
                },
            );
            let mut work =
                host.jobs
                    .begin_for(&request.source, &host.manifest.generation, consumer)?;
            if let Some(project) = &project {
                work.bind_project(project);
            }
            let snapshot = host.jobs.snapshot_for(&work.generation, consumer)?;
            let root = host.root.clone();
            let manifest = host.manifest.clone();
            let app = app.clone();
            tauri::async_runtime::spawn_blocking(move || {
                run(app, root, manifest, request, limit, work, project)
            });
            serde_json::to_value(snapshot).map_err(|_| "search_unavailable".into())
        }
        "source_poll" | "source_cancel" => {
            let request: Generation =
                serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
            if uuid::Uuid::parse_str(&request.generation).is_err() {
                return Err("component_args_invalid".into());
            }
            if method == "source_cancel" {
                host.jobs.cancel_for(&request.generation, consumer)?;
                Ok(Value::Null)
            } else {
                serde_json::to_value(host.jobs.snapshot_for(&request.generation, consumer)?)
                    .map_err(|_| "search_unavailable".into())
            }
        }
        _ => Err("component_method_invalid".into()),
    }
}
pub(crate) fn federated_reference(
    app: &tauri::AppHandle,
    id: &str,
) -> Result<crate::core::source_search::Reference, String> {
    let host = app.try_state::<Host>().ok_or("setup_required")?;
    if stores::read(&host.root)?.as_ref() != Some(&host.manifest) {
        return Err("search_stale".into());
    }
    host.jobs
        .resolve_consumer(id, Some(crate::core::source_search::Consumer::Federated))
}

struct OpenPermit(Arc<AtomicUsize>);
impl Drop for OpenPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
pub async fn open(app: &tauri::AppHandle, method: &str, args: Value) -> Result<Value, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        reference: String,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    if uuid::Uuid::parse_str(&input.reference).is_err() {
        return Err("component_args_invalid".into());
    }
    let host = app.state::<Host>();
    let reference = host.jobs.resolve(&input.reference)?;
    host.openers
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
            (count < 2).then_some(count + 1)
        })
        .map_err(|_| "search_busy")?;
    let permit = OpenPermit(host.openers.clone());
    let jobs = host.jobs.clone();
    let projects = host.projects.clone();
    let root = host.root.clone();
    let app = app.clone();
    let reveal = method == "reveal_file";
    let native_reference = method == "native_file_reference";
    let deadline = Instant::now() + Duration::from_secs(2);
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        let result = (|| {
            if reference
                .project
                .as_ref()
                .is_some_and(|p| !projects.matches(&p.epoch))
            {
                return Err("search_stale".into());
            }
            let manifest = stores::read(&root)?.ok_or("search_stale")?;
            if manifest.generation != reference.store_generation {
                return Err("search_stale".into());
            }
            let source = reference.source.as_str();
            let database = stores::directory(
                &root,
                &manifest,
                if source == "notes" { "notes" } else { "search" },
            )?
            .join("data.db");
            let conn = read_connection(&database, None, deadline)?;
            let row = &reference.candidate;
            let path = if source == "notes" {
                row.value["notePath"].as_str().ok_or("search_stale")?
            } else {
                row.path.to_str().ok_or("search_stale")?
            };
            let (current_root, current_revision) = revision(&conn, source, path)?;
            if current_root != row.root || current_revision != row.revision {
                return Err("search_stale".into());
            }
            drop(conn);
            devbox_filesystem::ensure_no_links(&row.path).map_err(|_| "search_stale")?;
            if devbox_filesystem::filesystem_identity(&row.root, true)
                .map_err(|_| "search_stale")?
                != reference.root_identity
                || devbox_filesystem::filesystem_identity(&row.path, false)
                    .map_err(|_| "search_stale")?
                    != reference.file_identity
                || Instant::now() >= deadline
            {
                return Err("search_stale".into());
            }
            if let Some(project) = &reference.project {
                if !projects.matches(&project.epoch)
                    || !row.path.starts_with(&project.root)
                    || devbox_filesystem::filesystem_identity(&project.root, true)
                        .map_err(|_| "search_stale")?
                        != project.identity
                {
                    return Err("search_stale".into());
                }
            }
            // Root removal/index replacement may have happened during a slow OS
            // probe. Re-read the current registered row before authorizing launch.
            let current = read_connection(&database, None, deadline)?;
            if revision(&current, source, path)? != (row.root.clone(), row.revision.clone()) {
                return Err("search_stale".into());
            }
            drop(current);
            jobs.resolve(&input.reference)?;
            if reference
                .project
                .as_ref()
                .is_some_and(|p| !projects.matches(&p.epoch))
            {
                return Err("search_stale".into());
            }
            if Instant::now() >= deadline {
                return Err("search_stale".into());
            }
            if native_reference {
                if source == "notes" {
                    return Err("search_source_denied".into());
                }
                let (volume, object) = reference.file_identity.components();
                let proof = product_contract::file_reference::Proof {
                    reference: input.reference.clone(),
                    path: row.path.to_str().ok_or("search_stale")?.into(),
                    volume: format!("{volume:x}"),
                    object: format!("{object:x}"),
                    expires_at_ms: (std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_err(|_| "search_stale")?
                        .as_millis()
                        + reference
                            .expires
                            .saturating_duration_since(Instant::now())
                            .as_millis())
                    .min(u128::from(u64::MAX)) as u64,
                    context: reference.project.as_ref().and_then(|_| {
                        projects
                            .current()
                            .map(|selection| selection.project.context)
                    }),
                };
                proof.validate().map_err(str::to_owned)?;
                return serde_json::to_value(proof).map_err(|_| "search_stale".into());
            }
            if source == "notes" && !reveal {
                knowledge_base_lib::component::offer_product_path(&app, &row.path)?;
            } else {
                use tauri_plugin_opener::OpenerExt;
                if reveal {
                    app.opener().reveal_item_in_dir(&row.path)
                } else {
                    app.opener()
                        .open_path(row.path.to_string_lossy(), None::<&str>)
                }
                .map_err(|_| "search_unavailable")?;
            }
            Ok(json!({"owner": if source == "notes" && !reveal { "notes" } else { "files" }}))
        })();
        let _ = sender.send(result);
    });
    // The OS probe may outlive the response deadline. Its permit stays owned
    // by that worker, and the deadline check prevents a late launch.
    tauri::async_runtime::spawn_blocking(move || {
        receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| "search_stale".to_owned())?
    })
    .await
    .map_err(|_| "search_unavailable")?
}

#[cfg(test)]
mod tests {
    use super::*;
    fn query_request(source: &str) -> Query {
        Query {
            source: source.into(),
            query: "shared".into(),
            mode: "name".into(),
            limit: Some(200),
            filter: SearchFilter::default(),
        }
    }
    #[test]
    fn physical_sources_keep_identity_and_root_removal_invalidates_the_file_row() {
        let directory = tempfile::tempdir().unwrap();
        let vault = directory.path().join("vault");
        std::fs::create_dir(&vault).unwrap();
        let note_db = directory.path().join("notes.db");
        let file_db = directory.path().join("files.db");
        knowledge_base_lib::component::create_empty_store(&note_db, &vault).unwrap();
        everything_plus_lib::component::create_empty_store(&file_db).unwrap();
        let notes = Connection::open(&note_db).unwrap();
        notes.execute("INSERT INTO docs(path,title,body,tags,modified_ts) VALUES('shared.md','shared','onlynote','[]',100)", []).unwrap();
        let files = Connection::open(&file_db).unwrap();
        let root = vault.to_string_lossy().replace('\\', "/");
        let file = format!("{root}/shared.md");
        files
            .execute("INSERT INTO roots(id,path,content) VALUES(7,?1,1)", [&root])
            .unwrap();
        files.execute("INSERT INTO files(id,path,name,ext,size,modified_ts,root_id) VALUES(1,?1,'shared.md','md',12,100,7)", [&file]).unwrap();
        let note_rows = candidates(&notes, &query_request("notes"), 200, None).unwrap();
        let file_rows = candidates(&files, &query_request("files"), 200, None).unwrap();
        assert_eq!(note_rows.len(), 1);
        assert_eq!(file_rows.len(), 1);
        assert_eq!(note_rows[0].path, file_rows[0].path);
        assert_ne!(note_rows[0].root_key, file_rows[0].root_key);
        let mut body_query = query_request("notes");
        body_query.query = "onlynote".into();
        assert!(candidates(&notes, &body_query, 200, None)
            .unwrap()
            .is_empty());
        body_query.mode = "content".into();
        assert!(
            candidates(&notes, &body_query, 200, None).unwrap()[0].value["snippet"]
                .as_str()
                .unwrap()
                .contains("onlynote")
        );
        let mut filtered = query_request("files");
        filtered.filter.source_root_id = Some(99);
        assert!(candidates(&files, &filtered, 200, None).unwrap().is_empty());
        files.execute("DELETE FROM roots WHERE id=7", []).unwrap();
        assert!(revision(&files, "files", &file).is_err());
        assert!(candidates(&files, &query_request("files"), 200, None)
            .unwrap()
            .is_empty());
        assert_eq!(
            candidates(&notes, &query_request("notes"), 200, None)
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    fn reader_cannot_mutate_and_deadline_interrupts_expensive_sql() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("notes.db");
        knowledge_base_lib::component::create_empty_store(&database, directory.path()).unwrap();
        let conn =
            read_connection(&database, None, Instant::now() + Duration::from_secs(1)).unwrap();
        assert!(conn.execute("DELETE FROM settings", []).is_err());
        drop(conn);
        let conn = read_connection(&database, None, Instant::now()).unwrap();
        let error = conn.query_row("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<1000000000) SELECT sum(x) FROM n", [], |row| row.get::<_, i64>(0)).unwrap_err();
        assert_eq!(
            error.sqlite_error_code(),
            Some(rusqlite::ErrorCode::OperationInterrupted)
        );
    }
}

pub(crate) fn current_generation(app: &tauri::AppHandle) -> Result<String, String> {
    let host = app.try_state::<Host>().ok_or("setup_required")?;
    if stores::read(&host.root)?.as_ref() != Some(&host.manifest) {
        return Err("search_stale".into());
    }
    Ok(host.manifest.generation.clone())
}
