//! Renderer deadlines use this same native policy; authorization still caps 30s.
use product_ipc::workspace::{DEFAULT_BUDGET_MS, LONG_BUDGET_MS};
pub fn budget(component: &str, method: &str) -> u64 {
    if matches!(
        component,
        "workspace.runtime"
            | "workspace.processes"
            | "workspace.process-actions"
            | "workspace.logs"
            | "workspace.terminal"
            | "workspace.dependencies"
            | "workspace.source"
            | "workspace.lsp"
    ) || (component == "workspace.files"
        && matches!(
            method,
            "reconnect_wsl_files" | "open_file" | "send_editor_selection"
        ))
        || (component == "workspace.registry"
            && matches!(
                method,
                "list_wsl_distros"
                    | "preview_wsl"
                    | "apply_registration"
                    | "cancel_registration"
                    | "select_project"
                    | "clear_project"
            ))
    {
        LONG_BUDGET_MS
    } else {
        DEFAULT_BUDGET_MS
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn long_operations_keep_their_original_budget() {
        for (component, method) in [
            ("workspace.registry", "list_wsl_distros"),
            ("workspace.registry", "preview_wsl"),
            ("workspace.source", "repo_fetch"),
            ("workspace.dependencies", "dependency_enrichment_execute"),
            ("workspace.runtime", "runtime_control"),
        ] {
            assert_eq!(budget(component, method), 29_000);
        }
        assert_eq!(budget("workspace.problems", "snapshot"), 5_000);
        assert_eq!(budget("workspace.files", "load_session"), 5_000);
    }
}
