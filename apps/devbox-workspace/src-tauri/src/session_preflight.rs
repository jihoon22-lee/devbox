//! Read-only preflight: native project/definition evidence and the existing port collector.
use crate::{
    definitions::{Definitions, SessionDefinitions},
    host::Host,
};
use product_contract::{ExecutionTarget, ProjectContext};
use run_manager_lib::{component::sessions::PreparedJob, core::models::JobKind};
use serde::Serialize;
use std::{collections::BTreeSet, sync::Mutex};
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Entry {
    kind: &'static str,
    key: String,
    state: &'static str,
    blocking: bool,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Report {
    pub definitions_revision: Option<String>,
    pub restore_blocked: bool,
    pub execution_blocked: bool,
    pub entries: Vec<Entry>,
}
impl Report {
    fn add(
        &mut self,
        kind: &'static str,
        key: impl Into<String>,
        state: &'static str,
        blocking: bool,
    ) {
        self.execution_blocked |= blocking;
        self.entries.push(Entry {
            kind,
            key: key.into(),
            state,
            blocking,
        });
    }
}
pub(crate) async fn capture(
    app: &tauri::AppHandle,
    host: &Host,
    definitions: &Mutex<Definitions>,
    context: &ProjectContext,
    jobs: &[PreparedJob],
    deadline: u64,
) -> Result<Report, &'static str> {
    let mut report = Report {
        definitions_revision: None,
        restore_blocked: false,
        execution_blocked: false,
        entries: vec![],
    };
    crate::files_host::current_deadline(deadline)?;
    let projects = host.projects()?;
    let metadata = projects.binding(context)?;
    let binding = projects.admit_selection(host.helper_directory()?, context);
    if binding.is_err() {
        report.restore_blocked = true;
        report.add("project", "root", "unavailable-or-changed", true);
        if let ExecutionTarget::Wsl { distro_id } = &context.target {
            #[cfg(windows)]
            let state = crate::platform::wsl_distro::list()
                .ok()
                .and_then(|list| list.into_iter().find(|distro| distro.id == *distro_id))
                .map(|distro| if distro.running { "running" } else { "stopped" })
                .unwrap_or("unavailable");
            #[cfg(not(windows))]
            let state = "unavailable";
            report.add("distro", distro_id, state, true);
        }
        return Ok(report);
    }
    report.add(
        "project",
        "root",
        if metadata.repository_object.is_some() {
            "repository-verified"
        } else {
            "folder-verified"
        },
        false,
    );
    if let ExecutionTarget::Wsl { distro_id } = &context.target {
        report.add("distro", distro_id, "running", false);
    }
    let declared = definitions
        .lock()
        .map_err(|_| "session_preflight_busy")?
        .session_preflight(host, context, deadline);
    let declared = match declared {
        Ok(declared) => {
            report.definitions_revision = Some(declared.revision.clone());
            Some(declared)
        }
        Err(_) => {
            report.add("definitions", "project", "unavailable", true);
            None
        }
    };
    if let Some(declared) = &declared {
        describe_definitions(&mut report, declared);
    }
    let mut ports = declared
        .as_ref()
        .map(|declared| {
            declared
                .expected_ports
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let mut borrowable = BTreeSet::new();
    for prepared in jobs {
        let job = prepared.job();
        let current = prepared.revalidate(app).is_ok();
        report.add(
            "task",
            job.id.clone(),
            if current {
                "definition-verified"
            } else {
                "definition-changed-or-unavailable"
            },
            !current,
        );
        if let Some(task) = prepared.task() {
            let same = task.source_root == metadata.root;
            report.add(
                "taskContext",
                job.id.clone(),
                if same {
                    "current-worktree"
                } else {
                    "different-worktree"
                },
                !same,
            );
            report.add(
                "taskTrust",
                job.id.clone(),
                if task.trusted
                    && (task.task_kind
                        != run_manager_lib::core::workspace_tasks::WorkspaceTaskKind::Shell
                        || task.shell_trusted)
                {
                    "reviewed"
                } else {
                    "review-required"
                },
                !task.trusted
                    || task.task_kind
                        == run_manager_lib::core::workspace_tasks::WorkspaceTaskKind::Shell
                        && !task.shell_trusted,
            );
        }
        // Presence is metadata; preflight never decrypts or exports environment values.
        report.add(
            "environment",
            job.id.clone(),
            if job.env_configured {
                "configured"
            } else {
                "not-configured"
            },
            false,
        );
        if let Some(port) = job.health_tcp_port {
            ports.insert(port);
        }
        if job.kind == JobKind::Service {
            borrowable.insert(job.id.clone());
        }
    }
    if !ports.is_empty() {
        match crate::runtime_host::session_ports(app, host, definitions, context, deadline).await {
            Ok(snapshot) => {
                let distro_name = match &context.target {
                    ExecutionTarget::Windows => None,
                    ExecutionTarget::Wsl { distro_id } => {
                        #[cfg(windows)]
                        {
                            crate::platform::wsl_distro::list()
                                .ok()
                                .and_then(|list| {
                                    list.into_iter().find(|distro| distro.id == *distro_id)
                                })
                                .map(|distro| distro.name)
                        }
                        #[cfg(not(windows))]
                        {
                            let _ = distro_id;
                            None
                        }
                    }
                };
                let unavailable = matches!(context.target, ExecutionTarget::Wsl { .. })
                    && (distro_name.is_none()
                        || distro_name
                            .as_ref()
                            .is_some_and(|name| snapshot.unavailable_wsl.contains(name)));
                for port in ports {
                    let rows =
                        snapshot
                            .rows
                            .iter()
                            .filter(|row| {
                                row.row.port.port == port
                                    && row.row.port.proto.to_ascii_uppercase().starts_with("TCP")
                                    && match context.target {
                                        ExecutionTarget::Windows => row.row.source
                                            == port_manager_lib::component::ListenerSource::Windows,
                                        ExecutionTarget::Wsl { .. } => {
                                            row.row.wsl_distro.as_ref() == distro_name.as_ref()
                                        }
                                    }
                            })
                            .collect::<Vec<_>>();
                    let owned = !rows.is_empty()
                        && rows.iter().all(|row| {
                            row.row.identity.is_some()
                                && row.correlations.iter().any(|correlation| {
                                    correlation.source_app == "run-manager"
                                && borrowable.contains(&correlation.target_id)
                                && correlation.confidence
                                    != port_manager_lib::component::CorrelationConfidence::Expected
                                })
                        });
                    let (state, blocking) = if unavailable {
                        ("observation-unavailable", true)
                    } else if rows.is_empty() {
                        ("available", false)
                    } else if owned {
                        ("existing-selected-service", false)
                    } else {
                        ("occupied", true)
                    };
                    report.add("port", port.to_string(), state, blocking);
                }
            }
            Err(_) => report.add("port", "collection", "observation-unavailable", true),
        }
    }
    crate::files_host::current_deadline(deadline)?;
    Ok(report)
}
fn describe_definitions(report: &mut Report, declared: &SessionDefinitions) {
    report.add(
        "definitions",
        "project",
        if declared.trusted {
            "reviewed"
        } else {
            "not-reviewed"
        },
        false,
    );
    for source in &declared.unavailable_sources {
        report.add("definitionSource", source.clone(), "unavailable", true);
    }
    for (name, tool) in &declared.toolchains {
        report.add(
            "toolchain",
            format!("{name}: {} {}", tool.tool, tool.version),
            "declared-version-unverified",
            false,
        );
    }
    for reference in &declared.secret_references {
        report.add(
            "secretReference",
            reference.clone(),
            "owner-reference-configured",
            false,
        );
    }
    if declared.environment_reference {
        report.add(
            "environmentReference",
            "api",
            "owner-reference-configured",
            false,
        );
    }
}
