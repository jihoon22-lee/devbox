//! Native-only producer bridge. Renderer requests can read/select, never publish.
use crate::core::problems::{Item, Severity, Snapshot, Store, Target, Ticket};
use product_contract::ProjectContext;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tauri::Manager;
type Result<T> = std::result::Result<T, &'static str>;
#[derive(Default)]
struct State {
    store: Store,
    epoch: u64,
    lsp: HashMap<String, u64>,
    versions: HashMap<(String, String), i32>,
    encodings: HashMap<(String, String), bool>,
    summaries: HashMap<String, Value>,
    run_generations: HashMap<(String, String), i64>,
}
#[derive(Default)]
pub(crate) struct Problems(Mutex<State>);
#[derive(Clone)]
pub(crate) struct LspEpoch {
    context: ProjectContext,
    key: String,
    id: u64,
}
fn context_key(context: &ProjectContext) -> String {
    serde_json::to_string(context).expect("typed context")
}
impl Problems {
    pub(crate) fn lsp_epoch(&self, context: &ProjectContext) -> Result<LspEpoch> {
        let mut state = self.0.lock().map_err(|_| "problems_busy")?;
        let key = context_key(context);
        state.epoch = state.epoch.checked_add(1).ok_or("problem_source_limit")?;
        let id = state.epoch;
        // The native LSP host owns one active project actor at a time.
        state.lsp.clear();
        state.versions.clear();
        state.encodings.clear();
        state.store.clear_all("lsp");
        state.lsp.insert(key.clone(), id);
        state.versions.retain(|(scope, _), _| scope != &key);
        state.encodings.retain(|(scope, _), _| scope != &key);
        state.store.clear(context, "lsp");
        Ok(LspEpoch {
            context: context.clone(),
            key,
            id,
        })
    }
    pub(crate) fn lsp(
        &self,
        host: &crate::host::Host,
        epoch: &LspEpoch,
        name: &str,
        value: &Value,
    ) {
        if name == "lsp/status" {
            if matches!(
                value.pointer("/status/status").and_then(Value::as_str),
                Some("stopped" | "crashed" | "degraded")
            ) {
                if let Ok(mut state) = self.0.lock() {
                    if state.lsp.get(&epoch.key) == Some(&epoch.id) {
                        state.store.source_unavailable(&epoch.context, "lsp");
                        if let Some(language) = value.get("languageId").and_then(Value::as_str) {
                            state
                                .encodings
                                .remove(&(epoch.key.clone(), language.into()));
                        }
                    }
                }
            }
            if value.pointer("/status/status").and_then(Value::as_str) != Some("ready") {
                return;
            }
            if let (Some(language), Some(encoding)) = (
                value.get("languageId").and_then(Value::as_str),
                value
                    .pointer("/status/capabilities/positionEncoding")
                    .and_then(Value::as_str),
            ) {
                if let Ok(mut state) = self.0.lock() {
                    if state.lsp.get(&epoch.key) == Some(&epoch.id) {
                        state
                            .encodings
                            .insert((epoch.key.clone(), language.into()), encoding == "utf-16");
                    }
                }
            }
            return;
        }
        if name != "lsp/diagnostics" {
            return;
        }
        let result = (|| -> Result<()> {
            let context: ProjectContext = serde_json::from_value(
                value
                    .get("nativeContext")
                    .cloned()
                    .ok_or("problem_invalid")?,
            )
            .map_err(|_| "problem_invalid")?;
            if context != epoch.context {
                return Err("problem_stale");
            }
            let response = value.get("response").ok_or("problem_invalid")?;
            if response.get("stale").and_then(Value::as_bool) != Some(false) {
                return Err("problem_stale");
            }
            let metadata = response.get("metadata").ok_or("problem_invalid")?;
            let uri = metadata
                .get("uri")
                .and_then(Value::as_str)
                .ok_or("problem_invalid")?;
            let version = metadata
                .get("version")
                .and_then(Value::as_i64)
                .and_then(|value| i32::try_from(value).ok())
                .filter(|value| *value >= 0)
                .ok_or("problem_invalid")?;
            let data = response.get("value").ok_or("problem_invalid")?;
            if data.get("unchanged").and_then(Value::as_bool) == Some(true) {
                return Ok(());
            }
            let language = value
                .get("languageId")
                .and_then(Value::as_str)
                .ok_or("problem_invalid")?;
            let utf16 = self
                .0
                .lock()
                .map_err(|_| "problems_busy")?
                .encodings
                .get(&(epoch.key.clone(), language.into()))
                .copied()
                .ok_or("problem_source_unavailable")?;
            let diagnostics = data
                .get("diagnostics")
                .and_then(Value::as_array)
                .ok_or("problem_invalid")?;
            let root = host.projects()?.binding(&context)?.root;
            let path = relative_uri(
                uri,
                &root,
                matches!(context.target, product_contract::ExecutionTarget::Windows),
            );
            let mut items = Vec::new();
            for diagnostic in diagnostics.iter().take(512) {
                let Some(message) =
                    diagnostic
                        .get("message")
                        .and_then(Value::as_str)
                        .filter(|message| {
                            !message.is_empty() && message.len() <= 4096 && !message.contains('\0')
                        })
                else {
                    continue;
                };
                let severity = match diagnostic.get("severity").and_then(Value::as_u64) {
                    Some(1) => Severity::Error,
                    Some(2) => Severity::Warning,
                    _ => Severity::Information,
                };
                let start = diagnostic.pointer("/range/start");
                let line = start
                    .and_then(|start| start.get("line"))
                    .and_then(Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok())
                    .and_then(|value| value.checked_add(1));
                let column = start
                    .and_then(|start| start.get("character"))
                    .and_then(Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok())
                    .and_then(|value| value.checked_add(1));
                let target = match (&path, line) {
                    (Some(path), Some(line)) => Target::File {
                        relative_path: path.clone(),
                        line,
                        column: if utf16 { column } else { None },
                        document_version: Some(version),
                    },
                    _ => Target::Route {
                        route: "files".into(),
                    },
                };
                items.push(Item {
                    severity,
                    message: message.into(),
                    target,
                    log: None,
                });
            }
            let mut state = self.0.lock().map_err(|_| "problems_busy")?;
            if state.lsp.get(&epoch.key) != Some(&epoch.id) {
                return Err("problem_stale");
            }
            let key = (epoch.key.clone(), uri.into());
            if state
                .versions
                .get(&key)
                .is_some_and(|previous| *previous > version)
            {
                return Err("problem_stale");
            }
            if state.versions.len() >= 512 && !state.versions.contains_key(&key) {
                return Err("problem_source_limit");
            }
            state.versions.insert(key, version);
            let ticket = state.store.begin(&context, "lsp", uri)?;
            state.store.finish(
                &ticket,
                &format!("{}-{version}", epoch.id),
                items,
                diagnostics.len() > 512,
            )
        })();
        let _ = result;
    }
    pub(crate) fn document_changed(
        &self,
        context: &ProjectContext,
        uri: &str,
        version: Option<i32>,
        closed: bool,
    ) {
        if uri.len() > 4096 {
            return;
        }
        if let Ok(mut state) = self.0.lock() {
            let key = (context_key(context), uri.into());
            if closed {
                state.versions.remove(&key);
                state.store.document_changed(context, uri, true);
            } else if let Some(version) = version {
                if state
                    .versions
                    .get(&key)
                    .is_none_or(|previous| version > *previous)
                {
                    if state.versions.len() >= 512 && !state.versions.contains_key(&key) {
                        return;
                    }
                    state.versions.insert(key, version);
                    state.store.document_changed(context, uri, false);
                }
            }
        }
    }
    pub(crate) fn summary(&self, context: &ProjectContext, value: Value) {
        if let Ok(mut state) = self.0.lock() {
            let key = context_key(context);
            if state.summaries.len() >= 64 && !state.summaries.contains_key(&key) {
                state.summaries.clear();
            }
            state.summaries.insert(key, value);
        }
    }
    fn begin_run(&self, context: &ProjectContext, job: &str, generation: i64) -> Result<Ticket> {
        let mut state = self.0.lock().map_err(|_| "problems_busy")?;
        let key = (context_key(context), job.into());
        if state
            .run_generations
            .get(&key)
            .is_some_and(|previous| generation < *previous)
        {
            return Err("problem_stale");
        }
        if state.run_generations.len() >= 256 && !state.run_generations.contains_key(&key) {
            return Err("problem_source_limit");
        }
        state.run_generations.insert(key, generation);
        state.store.begin(context, "matcher", job)
    }
    pub(crate) fn begin(
        &self,
        context: &ProjectContext,
        source: &str,
        identity: &str,
    ) -> Result<Ticket> {
        self.0
            .lock()
            .map_err(|_| "problems_busy")?
            .store
            .begin(context, source, identity)
    }
    pub(crate) fn finish(
        &self,
        ticket: &Ticket,
        revision: &str,
        items: Vec<Item>,
        truncated: bool,
    ) -> Result<()> {
        self.0
            .lock()
            .map_err(|_| "problems_busy")?
            .store
            .finish(ticket, revision, items, truncated)
    }
    pub(crate) fn unavailable(&self, ticket: &Ticket) {
        if let Ok(mut state) = self.0.lock() {
            state.store.unavailable(ticket);
        }
    }
    pub(crate) fn snapshot(&self, context: &ProjectContext) -> Result<Snapshot> {
        Ok(self
            .0
            .lock()
            .map_err(|_| "problems_busy")?
            .store
            .snapshot(context))
    }
    pub(crate) fn select(
        &self,
        context: &ProjectContext,
        id: &str,
        revision: &str,
        log: bool,
    ) -> Result<Target> {
        let item = self
            .0
            .lock()
            .map_err(|_| "problems_busy")?
            .store
            .select(context, id, revision)?
            .item;
        if log {
            item.log.ok_or("problem_target_missing")
        } else {
            Ok(item.target)
        }
    }
}
fn relative_uri(uri: &str, root: &str, windows: bool) -> Option<String> {
    let url = tauri::Url::parse(uri).ok()?;
    if url.scheme() != "file" {
        return None;
    }
    let path = url
        .to_file_path()
        .ok()?
        .to_string_lossy()
        .replace('\\', "/");
    let root = root.replace('\\', "/").trim_end_matches('/').to_string();
    let path = path.trim_start_matches("//?/");
    let root = root.trim_start_matches("//?/").to_string() + "/";
    let relative = if windows {
        if !path
            .to_ascii_lowercase()
            .starts_with(&root.to_ascii_lowercase())
        {
            return None;
        }
        path.get(root.len()..)?
    } else {
        path.strip_prefix(&root)?
    };
    if relative.is_empty()
        || relative.len() > 4096
        || relative.chars().any(char::is_control)
        || relative.contains(':')
        || relative
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return None;
    }
    Some(relative.into())
}
pub(crate) fn owner(app: &tauri::AppHandle) -> Result<Arc<Problems>> {
    app.try_state::<Arc<Problems>>()
        .map(|state| state.inner().clone())
        .ok_or("problems_unavailable")
}
pub(crate) fn manage(
    app: &tauri::AppHandle,
    host: &crate::host::Host,
    context: Option<&ProjectContext>,
    method: &str,
    args: Value,
) -> Result<Value> {
    let context = context.ok_or("problem_context_required")?;
    host.projects()?.binding(context)?;
    let owner = owner(app)?;
    if let Some(sessions) = app.try_state::<Arc<crate::development_host::Sessions>>() {
        if let Ok(ticket) = owner.begin(context, "session", "current") {
            match sessions.problem_snapshots(context) {
                Ok(snapshots) if !snapshots.iter().any(|row| row.3) => {
                    let revisions = snapshots
                        .iter()
                        .map(|row| (&row.0, &row.1))
                        .collect::<Vec<_>>();
                    let revision = crate::definitions::digest(
                        &serde_json::to_vec(&revisions).map_err(|_| "problem_invalid")?,
                    );
                    let items = snapshots.into_iter().flat_map(|row| row.2).collect();
                    let _ = owner.finish(&ticket, &revision, items, false);
                }
                _ => owner.unavailable(&ticket),
            }
        }
    }
    if method == "snapshot" {
        if args.as_object().is_none_or(|args| !args.is_empty()) {
            return Err("problem_invalid");
        }
        let mut value =
            serde_json::to_value(owner.snapshot(context)?).map_err(|_| "problem_invalid")?;
        let root = host.projects()?.binding(context)?.root;
        let running = (|| -> Result<usize> {
            let identity = if matches!(
                context.target,
                product_contract::ExecutionTarget::Wsl { .. }
            ) {
                Some(crate::platform::task_sources::context_identity(
                    host, context,
                )?)
            } else {
                None
            };
            let mut ids = run_manager_lib::component::sessions::running_project_runs(
                app,
                &root,
                identity.as_deref(),
            )
            .map_err(|_| "session_runtime_unavailable")?
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
            if let Some(sessions) = app.try_state::<Arc<crate::development_host::Sessions>>() {
                ids.extend(sessions.running_runs(context)?);
            }
            Ok(ids.len())
        })()
        .ok();
        value["runningTasks"] = json!(running);
        value["summary"] = owner
            .0
            .lock()
            .ok()
            .and_then(|state| state.summaries.get(&context_key(context)).cloned())
            .unwrap_or(Value::Null);
        return Ok(value);
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        id: String,
        revision: String,
        #[serde(default)]
        log: bool,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "problem_invalid")?;
    if method != "resolve" {
        return Err("problem_invalid");
    }
    let target = owner.select(context, &input.id, &input.revision, input.log)?;
    let target = match target {
        Target::Matcher { run_id, index } => {
            let binding = host
                .projects()?
                .admit_selection(host.helper_directory()?, context)?;
            if !crate::platform::task_sources::diagnostic_matches(app, host, context, &run_id) {
                return Err("problem_stale");
            }
            let value = tauri::async_runtime::block_on(run_manager_lib::component::dispatch(
                app,
                "list_workspace_task_diagnostics",
                json!({"runId":run_id}),
            ))
            .map_err(|_| "problem_log_expired")?;
            if crate::definitions::digest(
                &serde_json::to_vec(&value).map_err(|_| "problem_invalid")?,
            ) != input.revision
            {
                return Err("problem_stale");
            }
            let row = value
                .get("items")
                .and_then(Value::as_array)
                .and_then(|items| items.get(index as usize))
                .ok_or("problem_stale")?;
            let relative = row
                .get("file")
                .and_then(Value::as_str)
                .ok_or("problem_target_missing")?;
            if matches!(
                context.target,
                product_contract::ExecutionTarget::Wsl { .. }
            ) {
                run_manager_lib::core::workspace_diagnostics::relative_diagnostic_file(relative)
                    .map_err(|_| "problem_target_missing")?;
            } else {
                run_manager_lib::core::workspace_diagnostics::resolve_workspace_diagnostic_path(
                    &binding.root,
                    relative,
                )
                .map_err(|_| "problem_target_missing")?;
            }
            Target::File {
                relative_path: relative.into(),
                line: row
                    .get("line")
                    .and_then(Value::as_u64)
                    .and_then(|line| u32::try_from(line).ok())
                    .ok_or("problem_target_missing")?,
                column: None,
                document_version: None,
            }
        }
        Target::Run {
            run_id,
            stream,
            offset,
        } => {
            let from_project =
                crate::platform::task_sources::diagnostic_matches(app, host, context, &run_id);
            let from_session = app
                .try_state::<Arc<crate::development_host::Sessions>>()
                .is_some_and(|sessions| sessions.problem_run(context, &run_id));
            if !from_project && !from_session {
                return Err("problem_stale");
            }
            if offset.is_some() {
                let diagnostics =
                    tauri::async_runtime::block_on(run_manager_lib::component::dispatch(
                        app,
                        "list_workspace_task_diagnostics",
                        json!({"runId":run_id}),
                    ))
                    .map_err(|_| "problem_log_expired")?;
                if crate::definitions::digest(
                    &serde_json::to_vec(&diagnostics).map_err(|_| "problem_invalid")?,
                ) != input.revision
                {
                    return Err("problem_stale");
                }
            }
            let lease = run_manager_lib::component::log_descriptor(app, &run_id)
                .map_err(|_| "problem_log_expired")?;
            lease.revalidate().map_err(|_| "problem_log_expired")?;
            return Ok(
                json!({"context":context,"target":{"kind":"log","request":{"id":uuid::Uuid::new_v4().simple().to_string(),"source":{"kind":"runtimeRun","runId":run_id,"stream":stream,"revision":lease.revision()},"offset":offset}}}),
            );
        }
        Target::SessionResource {
            session_id,
            resource_key,
        } => {
            let sessions = app
                .try_state::<Arc<crate::development_host::Sessions>>()
                .ok_or("problem_stale")?;
            let job = sessions.problem_resource(context, &session_id, &resource_key)?;
            return Ok(json!({"context":context,"target":{"kind":"task","jobId":job}}));
        }
        target => target,
    };
    Ok(json!({"context":context,"target":target}))
}

pub(crate) struct Observation {
    ticket: Ticket,
    context: ProjectContext,
    source: &'static str,
    route: &'static str,
    run_id: Option<String>,
}
pub(crate) fn begin_observation(
    app: &tauri::AppHandle,
    host: Option<&crate::host::Host>,
    context: Option<&ProjectContext>,
    component: &str,
    method: &str,
    args: &Value,
) -> Option<Observation> {
    let context = context?;
    let (source, route) = match (component, method) {
        ("workspace.definitions", "load") => ("definitions", "overview"),
        ("workspace.dependencies", "dependency_inventory") => ("dependencies", "dependencies"),
        ("workspace.source", "repo_preflight") => ("git", "source"),
        ("workspace.runtime", "list_workspace_task_diagnostics") => ("matcher", "tasks"),
        _ => return None,
    };
    let run_id = if source == "matcher" {
        Some(args.get("runId")?.as_str()?.to_owned())
    } else {
        None
    };
    let owner = owner(app).ok()?;
    let ticket = if let Some(run) = &run_id {
        let (source_root, job, generation) =
            run_manager_lib::component::diagnostic_identity(app, run).ok()?;
        let _ = source_root;
        if !crate::platform::task_sources::diagnostic_matches(app, host?, context, run) {
            return None;
        }
        owner.begin_run(context, &job, generation).ok()?
    } else {
        owner.begin(context, source, "current").ok()?
    };
    Some(Observation {
        ticket,
        context: context.clone(),
        source,
        route,
        run_id,
    })
}
pub(crate) fn finish_observation(
    app: &tauri::AppHandle,
    host: Option<&crate::host::Host>,
    observation: Option<Observation>,
    result: &Result<Value>,
) {
    let Some(observation) = observation else {
        return;
    };
    let Ok(owner) = owner(app) else {
        return;
    };
    let Ok(value) = result else {
        owner.unavailable(&observation.ticket);
        return;
    };
    let Some(host) = host else {
        owner.unavailable(&observation.ticket);
        return;
    };
    let mut items = Vec::new();
    if observation.source == "matcher" {
        let Some(run_id) = observation.run_id else {
            return;
        };
        if !crate::platform::task_sources::diagnostic_matches(
            app,
            host,
            &observation.context,
            &run_id,
        ) {
            owner.unavailable(&observation.ticket);
            return;
        }
        for row in value
            .get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .take(500)
        {
            let Some(index) = row
                .get("index")
                .and_then(Value::as_u64)
                .and_then(|index| u32::try_from(index).ok())
            else {
                continue;
            };
            let Some(message) = row
                .get("message")
                .and_then(Value::as_str)
                .filter(|message| message.len() <= 4096)
            else {
                continue;
            };
            let severity = match row.get("severity").and_then(Value::as_str) {
                Some("warning") => Severity::Warning,
                Some("info" | "information") => Severity::Information,
                _ => Severity::Error,
            };
            let stream = row
                .get("stream")
                .and_then(Value::as_str)
                .unwrap_or("stdout");
            items.push(Item {
                severity,
                message: message.into(),
                target: Target::Matcher {
                    run_id: run_id.clone(),
                    index,
                },
                log: Some(Target::Run {
                    run_id: run_id.clone(),
                    stream: stream.into(),
                    offset: row.get("offset").and_then(Value::as_str).map(str::to_owned),
                }),
            });
        }
    } else if observation.source == "definitions" {
        let tools = value
            .pointer("/effective/toolchains")
            .and_then(Value::as_object)
            .map(|tools| {
                tools
                    .iter()
                    .take(16)
                    .map(|(name, tool)| {
                        format!(
                            "{} {}",
                            name,
                            tool.get("version").and_then(Value::as_str).unwrap_or("?")
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let references = value
            .pointer("/local/secrets")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        let environment = value
            .pointer("/local/apiEnvironmentId")
            .is_some_and(Value::is_string);
        owner.summary(&observation.context,json!({"toolchains":tools,"secretReferences":references,"environmentReference":environment}));
        for source in value
            .get("unavailableSources")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .take(128)
        {
            items.push(Item {
                severity: Severity::Warning,
                message: format!("정의 원본을 읽을 수 없습니다: {source}"),
                target: Target::Route {
                    route: "overview".into(),
                },
                log: None,
            });
        }
    } else if observation.source == "dependencies" {
        for (key, label) in [
            ("missingLockfileCount", "잠금 파일 없음"),
            ("staleLockfileCount", "잠금 파일 갱신 필요"),
            ("unsupportedCount", "지원하지 않는 의존성 원본"),
            ("invalidCount", "읽을 수 없는 의존성 원본"),
            ("unresolvedDependencyCount", "확인되지 않은 의존성 연결"),
        ] {
            if let Some(count) = value
                .get(key)
                .and_then(Value::as_u64)
                .filter(|count| *count > 0)
            {
                items.push(Item {
                    severity: Severity::Warning,
                    message: format!("{label}: {count}개"),
                    target: Target::Route {
                        route: observation.route.into(),
                    },
                    log: None,
                });
            }
        }
    } else {
        for issue in value
            .get("issues")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .take(16)
        {
            let label = match issue {
                "dirty" => "미커밋 변경이 있습니다.",
                "detached" => "분리된 HEAD 상태입니다.",
                "noUpstream" => "추적할 원격 브랜치가 없습니다.",
                "diverged" => "로컬·원격 브랜치가 갈라졌습니다.",
                "rebaseInProgress" => "리베이스가 진행 중입니다.",
                "mergeInProgress" => "병합이 진행 중입니다.",
                _ => continue,
            };
            items.push(Item {
                severity: Severity::Warning,
                message: label.into(),
                target: Target::Route {
                    route: observation.route.into(),
                },
                log: None,
            });
        }
    }
    if let Ok(bytes) = serde_json::to_vec(value) {
        let _ = owner.finish(
            &observation.ticket,
            &crate::definitions::digest(&bytes),
            items,
            value
                .get("truncated")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        );
    }
}

#[cfg(test)]
mod native_uri_tests {
    #[test]
    fn linux_problem_paths_preserve_case_even_on_a_windows_host() {
        assert_eq!(
            super::relative_uri("file:///home/Fixture/a.rs", "/home/fixture", false),
            None
        );
        assert_eq!(
            super::relative_uri("file:///home/fixture/a.rs", "/home/fixture", false),
            Some("a.rs".into())
        );
    }
}
