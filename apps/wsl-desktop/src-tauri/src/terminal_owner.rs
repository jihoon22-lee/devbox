//! Product PTY principal created by the native Workspace companion factory.
//! Window labels from JSON are never accepted as caller identity.
use crate::commands::terminal::{self, OwnedOutput, SessionState, StartedSession};
use crate::core::workspace::MultiplexerKind;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::Manager;

const MAX_PANES: usize = 32;

#[derive(Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Start {
    distro: String,
    cwd: Option<String>,
    pane_key: String,
    multiplexer: MultiplexerKind,
}

enum Pane {
    Starting {
        lease: Option<Arc<dyn crate::component::TerminalLaunchLease>>,
    },
    Failed {
        lease: Option<Arc<dyn crate::component::TerminalLaunchLease>>,
    },
    Active {
        config: Start,
        started: StartedSession,
        output: Arc<OwnedOutput>,
    },
}

/// Immutable binding. Workspace retains this object across hide/reload and checks
/// Registry context authority before executing any request through it.
pub struct TerminalOwner {
    window: String,
    panes: Mutex<HashMap<String, Pane>>,
    starts: Arc<tokio::sync::Semaphore>,
    initial_commands: Mutex<HashMap<String, (String, bool)>>,
    restore_only: bool,
}

struct Starting<'a> {
    panes: &'a Mutex<HashMap<String, Pane>>,
    key: String,
}
impl Drop for Starting<'_> {
    fn drop(&mut self) {
        if let Ok(mut panes) = self.panes.lock() {
            if matches!(panes.get(&self.key), Some(Pane::Starting { .. })) {
                let lease = match panes.remove(&self.key) {
                    Some(Pane::Starting { lease }) => lease,
                    _ => None,
                };
                panes.insert(self.key.clone(), Pane::Failed { lease });
            }
        }
    }
}

impl TerminalOwner {
    pub fn new(window: String) -> Result<Self, String> {
        Self::with_restore_only(window, false)
    }

    pub fn with_restore_only(window: String, restore_only: bool) -> Result<Self, String> {
        if !window.starts_with("terminal-")
            || window.len() > 96
            || !window
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err("terminal_window_invalid".into());
        }
        Ok(Self {
            window,
            panes: Mutex::default(),
            starts: Arc::new(tokio::sync::Semaphore::new(2)),
            initial_commands: Mutex::default(),
            restore_only,
        })
    }

    pub fn window(&self) -> &str {
        &self.window
    }

    /// A native Development Session observes the exact saved pane keys. A
    /// companion window existing is not proof that its PTYs restored.
    pub fn restored(&self, keys: &[String]) -> Result<bool, String> {
        if keys.is_empty() || keys.len() > 32 {
            return Err("terminal_layout_invalid".into());
        }
        let panes = self
            .panes
            .lock()
            .map_err(|_| "terminal_state_unavailable")?;
        let mut ready = true;
        for key in keys {
            match panes.get(key) {
                Some(Pane::Failed { .. }) => return Err("terminal_restore_failed".into()),
                Some(Pane::Active { output, .. }) => {
                    if output
                        .buffer
                        .lock()
                        .map_err(|_| "terminal_state_unavailable")?
                        .is_closed()
                    {
                        return Err("terminal_restore_closed".into());
                    }
                }
                _ => ready = false,
            }
        }
        Ok(ready)
    }

    pub async fn dispatch(
        &self,
        app: &tauri::AppHandle,
        actual_window: &str,
        method: &str,
        args: Value,
        launch_factory: Option<&dyn crate::component::TerminalLaunchFactory>,
    ) -> Result<Value, String> {
        if actual_window != self.window || !crate::component::is_product(app) {
            return Err("terminal_peer_denied".into());
        }
        crate::component::data_root(app)?;
        let state = app
            .try_state::<Arc<SessionState>>()
            .ok_or("terminal_state_unavailable")?;
        match method {
            "start_session" => {
                self.start(
                    app,
                    state.inner(),
                    args,
                    launch_factory.ok_or("terminal_launch_unavailable")?,
                )
                .await
            }
            "reset_failed_pane" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Reset {
                    pane_key: String,
                }
                let input: Reset = parse(args)?;
                let mut panes = self
                    .panes
                    .lock()
                    .map_err(|_| "terminal_state_unavailable")?;
                match panes.get(&input.pane_key) {
                    Some(Pane::Failed { lease }) => {
                        if let Some(lease) = lease {
                            lease.retire()?;
                        }
                    }
                    Some(Pane::Active {
                        started, output, ..
                    }) if output
                        .buffer
                        .lock()
                        .map_err(|_| "terminal_state_unavailable")?
                        .is_closed() =>
                    {
                        self.initial_commands
                            .lock()
                            .map_err(|_| "terminal_state_unavailable")?
                            .remove(&started.session_id);
                    }
                    None => return Ok(Value::Null),
                    _ => return Err("terminal_pane_active".into()),
                }
                panes.remove(&input.pane_key);
                Ok(Value::Null)
            }
            "list_sessions" => {
                empty(args)?;
                let panes = self
                    .panes
                    .lock()
                    .map_err(|_| "terminal_state_unavailable")?;
                Ok(Value::Array(
                    panes
                        .iter()
                        .filter_map(|(key, pane)| match pane {
                            Pane::Active {
                                config, started, ..
                            } => Some(json!({
                                "id": started.session_id, "distro": config.distro, "paneKey": key, "multiplexer": started.multiplexer, "resumed": started.resumed,
                            })),
                            _ => None,
                        })
                        .collect(),
                ))
            }
            "terminal_output" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Read {
                    session_id: String,
                    after: u64,
                }
                let input: Read = parse(args)?;
                let output = self.output(&input.session_id)?;
                let buffer = output
                    .buffer
                    .lock()
                    .map_err(|_| "terminal_state_unavailable")?;
                serde_json::to_value(buffer.read(input.after)?)
                    .map_err(|_| "terminal_response_invalid".into())
            }
            "attach_session" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Attach {
                    session_id: String,
                }
                let input: Attach = parse(args)?;
                self.output(&input.session_id)?;
                // The native reader is already running. A new renderer starts its own cursor.
                Ok(Value::Null)
            }
            "close_session" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Close {
                    session_id: String,
                }
                let input: Close = parse(args)?;
                self.output(&input.session_id)?;
                terminal::retire_owned(&state, &input.session_id, true)?;
                self.initial_commands
                    .lock()
                    .map_err(|_| "terminal_state_unavailable")?
                    .remove(&input.session_id);
                self.panes.lock().map_err(|_| "terminal_state_unavailable")?.retain(|_, pane| {
                    !matches!(pane, Pane::Active { started, .. } if started.session_id == input.session_id)
                });
                Ok(Value::Null)
            }
            "write_initial_command" => {
                if self.restore_only {
                    return Err("terminal_restore_only".into());
                }
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Initial {
                    session_id: String,
                    data: String,
                }
                let input: Initial = parse(args.clone())?;
                self.output(&input.session_id)?;
                if input.data.len() > 16 * 1024 {
                    return Err("terminal_args_invalid".into());
                }
                {
                    let mut commands = self
                        .initial_commands
                        .lock()
                        .map_err(|_| "terminal_state_unavailable")?;
                    if let Some((original, completed)) = commands.get(&input.session_id) {
                        return if original != &input.data {
                            Err("terminal_initial_command_conflict".into())
                        } else if *completed {
                            Ok(Value::Null)
                        } else {
                            Err("terminal_initial_command_interrupted".into())
                        };
                    }
                    // Reserve before any write; failure or renderer loss never resends input.
                    commands.insert(input.session_id.clone(), (input.data, false));
                }
                let result = terminal::__component_write_session(app, args).await?;
                if let Some((_, completed)) = self
                    .initial_commands
                    .lock()
                    .map_err(|_| "terminal_state_unavailable")?
                    .get_mut(&input.session_id)
                {
                    *completed = true;
                }
                Ok(result)
            }
            "write_session" | "resize_session" | "broadcast" => {
                // Recheck every exact target before the old bounded input/resize implementation.
                // Session IDs are never recycled, so closing after this check cannot redirect IO.
                if method == "broadcast" {
                    let ids = args
                        .get("sessionIds")
                        .and_then(Value::as_array)
                        .ok_or("terminal_args_invalid")?;
                    if !(2..=MAX_PANES).contains(&ids.len()) {
                        return Err("terminal_args_invalid".into());
                    }
                    for id in ids {
                        self.output(id.as_str().ok_or("terminal_args_invalid")?)?;
                    }
                } else {
                    self.output(
                        args.get("sessionId")
                            .and_then(Value::as_str)
                            .ok_or("terminal_args_invalid")?,
                    )?;
                }
                match method {
                    "write_session" => terminal::__component_write_session(app, args).await,
                    "resize_session" => terminal::__component_resize_session(app, args).await,
                    _ => terminal::__component_broadcast(app, args).await,
                }
            }
            _ => Err("terminal_method_invalid".into()),
        }
    }

    fn output(&self, session_id: &str) -> Result<Arc<OwnedOutput>, String> {
        self.panes
            .lock()
            .map_err(|_| "terminal_state_unavailable")?
            .values()
            .find_map(|pane| match pane {
                Pane::Active {
                    started, output, ..
                } if started.session_id == session_id => Some(output.clone()),
                _ => None,
            })
            .ok_or("terminal_session_denied".into())
    }

    async fn start(
        &self,
        app: &tauri::AppHandle,
        state: &Arc<SessionState>,
        args: Value,
        launch_factory: &dyn crate::component::TerminalLaunchFactory,
    ) -> Result<Value, String> {
        let config: Start = parse(args)?;
        if config.pane_key.is_empty()
            || config.pane_key.len() > 128
            || !config
                .pane_key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err("terminal_pane_invalid".into());
        }
        let _permit = self
            .starts
            .clone()
            .try_acquire_owned()
            .map_err(|_| "terminal_start_busy")?;
        {
            let mut panes = self
                .panes
                .lock()
                .map_err(|_| "terminal_state_unavailable")?;
            if let Some(pane) = panes.get(&config.pane_key) {
                return match pane {
                    Pane::Active {
                        config: original,
                        started,
                        ..
                    } if original.distro == config.distro
                        && original.multiplexer == config.multiplexer =>
                    {
                        Ok(json!({
                            "sessionId": started.session_id, "resumed": true, "multiplexer": started.multiplexer,
                        }))
                    }
                    Pane::Starting { .. } => Err("terminal_start_pending".into()),
                    Pane::Failed { .. } => Err("terminal_start_interrupted".into()),
                    _ => Err("terminal_pane_conflict".into()),
                };
            }
            if panes.len() >= MAX_PANES {
                return Err("terminal_pane_limit".into());
            }
            panes.insert(config.pane_key.clone(), Pane::Starting { lease: None });
        }
        let _starting = Starting {
            panes: &self.panes,
            key: config.pane_key.clone(),
        };
        let launch_lease = launch_factory.capture(&config.distro)?;
        if let Some(Pane::Starting { lease }) = self
            .panes
            .lock()
            .map_err(|_| "terminal_state_unavailable")?
            .get_mut(&config.pane_key)
        {
            *lease = Some(launch_lease.clone());
        }
        let output = Arc::new(OwnedOutput::default());
        // Scope stable multiplexer keys to the native companion identity too. Two
        // worktrees restoring the same profile must not silently share a tmux session.
        let native_key = crate::core::multiplexer::session_name(&self.window, &config.pane_key)?;
        let started = terminal::start_owned_or_legacy(
            state,
            config.distro.clone(),
            config.cwd.clone(),
            native_key,
            config.multiplexer,
            Some(output.clone()),
            Some(launch_lease.clone()),
        )
        .await?;
        let result = serde_json::to_value(&started).map_err(|_| "terminal_response_invalid")?;
        let id = started.session_id.clone();
        // Register the owner before starting the reader; EOF can arrive immediately.
        self.panes
            .lock()
            .map_err(|_| "terminal_state_unavailable")?
            .insert(
                config.pane_key.clone(),
                Pane::Active {
                    config,
                    started,
                    output,
                },
            );
        terminal::attach_native(app.clone(), state, id.clone())?;
        if let Err(error) = launch_lease.revalidate() {
            terminal::retire_owned(state, &id, true)?;
            return Err(error);
        }
        Ok(result)
    }

    /// Called by a bounded native cleanup worker, never by a renderer unmount.
    /// Failure keeps the owner and exact handles available for another stop attempt.
    pub fn stop(&self, app: &tauri::AppHandle) -> Result<(), String> {
        let state = app
            .try_state::<Arc<SessionState>>()
            .ok_or("terminal_state_unavailable")?;
        self.starts.close();
        if self.starts.available_permits() != 2 {
            return Err("terminal_start_pending".into());
        }
        let ids: Vec<_> = self
            .panes
            .lock()
            .map_err(|_| "terminal_state_unavailable")?
            .values()
            .filter_map(|pane| match pane {
                Pane::Active { started, .. } => Some(started.session_id.clone()),
                _ => None,
            })
            .collect();
        let mut failure = None;
        for id in ids {
            if let Err(error) = terminal::retire_owned(&state, &id, true) {
                failure = Some(error);
            }
        }
        {
            let panes = self
                .panes
                .lock()
                .map_err(|_| "terminal_state_unavailable")?;
            for pane in panes.values() {
                if let Pane::Failed { lease: Some(lease) } = pane {
                    if let Err(error) = lease.retire() {
                        failure = Some(error);
                    }
                }
            }
        }
        if let Some(error) = failure {
            return Err(error);
        }
        self.panes
            .lock()
            .map_err(|_| "terminal_state_unavailable")?
            .clear();
        Ok(())
    }
}

fn parse<T: serde::de::DeserializeOwned>(args: Value) -> Result<T, String> {
    if !args.is_object() {
        return Err("terminal_args_invalid".into());
    }
    serde_json::from_value(args).map_err(|_| "terminal_args_invalid".into())
}
fn empty(args: Value) -> Result<(), String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Empty {}
    parse::<Empty>(args).map(|_| ())
}
