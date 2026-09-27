pub mod plan;
pub mod store;
product_ipc::issue_codes! { pub enum AgentIssue {
    TaskMissing = "agent_task_missing", TaskChanged = "agent_task_changed", StateInvalid = "agent_task_state_invalid",
    TaskLimit = "agent_task_limit", TitleInvalid = "agent_title_invalid", CommandInvalid = "agent_command_invalid",
    TargetInvalid = "agent_target_invalid", SlugExhausted = "agent_slug_exhausted", WslRequired = "agent_wsl_required",
    ContextMismatch = "agent_task_context_mismatch", StoreUnavailable = "agent_store_unavailable"
} }
pub(crate) fn tasks(host: &crate::host::Host) -> Result<store::AgentTaskStore, AgentIssue> {
    Ok(store::AgentTaskStore::open(
        &host
            .component("agents")
            .map_err(|_| AgentIssue::StoreUnavailable)?,
    ))
}

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|t| t.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) fn distro_name(
    context: &product_contract::ProjectContext,
) -> Result<String, &'static str> {
    let product_contract::ExecutionTarget::Wsl { distro_id } = &context.target else {
        return Err(AgentIssue::WslRequired.code());
    };
    crate::platform::wsl_distro::list()
        .map_err(|_| "terminal_distro_unavailable")?
        .into_iter()
        .find(|distro| distro.id == *distro_id)
        .map(|distro| distro.name)
        .ok_or("terminal_distro_unavailable")
}
