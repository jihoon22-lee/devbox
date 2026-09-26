//! Generate only from native call enums, producer DTOs and deadline policy.
use devbox_workspace_lib::ipc;
use std::{collections::BTreeMap, path::PathBuf};
#[test]
fn export_typescript_bindings() {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../packages/workspace-features/src/generated");
    if out.exists() {
        std::fs::remove_dir_all(&out).unwrap();
    }
    std::fs::create_dir_all(&out).unwrap();
    let cfg = ts_rs::Config::new()
        .with_large_int("number")
        .with_out_dir(&out);
    let mut export = product_ipc::TypeExporter::new(&cfg);
    export.register::<ipc::issues::WorkspaceIssue>().unwrap();
    let companion = ipc::companion::result_types(&mut export).unwrap();
    let companion_budgets: BTreeMap<_, _> = companion
        .iter()
        .map(|(method, _)| (*method, ipc::companion::deadline_budget_for(method)))
        .collect();
    for (file, name, results) in [
        (
            "runtime-results.ts",
            "RuntimeResults",
            ipc::runtime::result_types(&mut export).unwrap(),
        ),
        (
            "processes-results.ts",
            "ProcessesResults",
            ipc::processes::result_types(&mut export).unwrap(),
        ),
        (
            "process-actions-results.ts",
            "ProcessActionsResults",
            ipc::process_actions::result_types(&mut export).unwrap(),
        ),
        (
            "logs-results.ts",
            "LogsResults",
            ipc::logs::result_types(&mut export).unwrap(),
        ),
        (
            "terminal-results.ts",
            "TerminalResults",
            ipc::terminal::result_types(&mut export).unwrap(),
        ),
        (
            "problems-results.ts",
            "ProblemsResults",
            ipc::problems::result_types(&mut export).unwrap(),
        ),
        (
            "commands-results.ts",
            "CommandsResults",
            ipc::commands::result_types(&mut export).unwrap(),
        ),
        ("companion-results.ts", "CompanionResults", companion),
        (
            "control-results.ts",
            "ControlResults",
            runtime_engine::api_control::result_types(&mut export).unwrap(),
        ),
    ] {
        std::fs::write(out.join(file), export.results(name, &results)).unwrap();
    }
    // The pinned baseline also covers components being migrated in the second half.
    let rows: Vec<(String, String, String)> =
        serde_json::from_str(include_str!("fixtures/allow-table.json")).unwrap();
    let mut budgets: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    for (component, _, method) in rows {
        let budget = ipc::deadlines::budget(&component, &method);
        if budget != product_ipc::workspace::DEFAULT_BUDGET_MS {
            budgets.entry(component).or_default().insert(method, budget);
        }
    }
    let commands: BTreeMap<_, _> = [
        ("workspace.runtime", "runtime"),
        ("workspace.processes", "processes"),
        ("workspace.process-actions", "process_actions"),
        ("workspace.logs", "logs"),
        ("workspace.terminal", "terminal"),
        ("workspace.problems", "problems"),
        ("workspace.commands", "commands"),
    ]
    .into_iter()
    .collect();
    let text=format!("// Generated from native Workspace scheduling policy.\nexport const deadlineBudgets: Readonly<Record<string, Readonly<Record<string, number>>>> = {};\nexport const companionDeadlineBudgets: Readonly<Record<string, number>> = {};\nexport const componentCommands: Readonly<Record<string, string>> = {};\n",serde_json::to_string_pretty(&budgets).unwrap(),serde_json::to_string_pretty(&companion_budgets).unwrap(),serde_json::to_string_pretty(&commands).unwrap());
    std::fs::write(out.join("deadline-budgets.ts"), text).unwrap();
}
