pub mod agent_event;
pub mod bridge;
pub mod client;
pub mod daemon;
pub mod doctor;
pub mod exec;
pub mod gateway;
pub mod mcp;
pub mod run_exited;
pub mod setup;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

static INSTANCE: std::sync::OnceLock<devbox_core::Instance> = std::sync::OnceLock::new();

pub fn set_instance(name: &str) -> anyhow::Result<()> {
    INSTANCE
        .set(devbox_core::Instance::parse(name)?)
        .map_err(|_| anyhow::anyhow!("instance already configured"))
}

pub fn paths() -> anyhow::Result<devbox_core::Paths> {
    let env = devbox_core::OsEnv;
    Ok(devbox_core::Paths::resolve(
        INSTANCE
            .get()
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| devbox_core::Instance::from_env(&env))?,
        &env,
    )?)
}

pub mod exec_attempt;
pub mod secrets;
