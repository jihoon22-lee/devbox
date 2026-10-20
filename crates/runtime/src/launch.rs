//! Pure argv construction. Callers validate identities, env, cwd and executable before launch.
use crate::{RestartPolicy, RunDef};
use devbox_core::{
    Paths,
    systemd::{path_environment, transient_path, transient_quote},
};
use std::path::Path;

pub fn unit_name(run_id: &str, paths: &Paths) -> String {
    format!("devbox-{}-run-{run_id}.service", paths.instance.as_str())
}
pub fn unit_args(def: &RunDef, run_id: &str, paths: &Paths, slice: Option<&str>) -> Vec<String> {
    unit_args_with_executable(
        def,
        run_id,
        paths,
        slice,
        &std::env::current_exe().expect("running executable is available"),
    )
}
pub fn unit_args_with_executable(
    def: &RunDef,
    run_id: &str,
    paths: &Paths,
    slice: Option<&str>,
    executable: &Path,
) -> Vec<String> {
    let (cwd, environment) = match def {
        RunDef::Task(def) => (&def.cwd, &def.env),
        RunDef::Service(def) => (&def.cwd, &def.env),
        RunDef::Schedule(_) => panic!("resolve schedule to its task before launch"),
    };
    let instance = paths.instance.as_str();
    let mut args = vec![
        "--user".into(),
        "--unit".into(),
        unit_name(run_id, paths),
        "--collect".into(),
        "--property=Type=exec".into(),
        "--expand-environment=no".into(),
    ];
    if let Some(slice) = slice {
        args.push(format!("--slice={slice}"));
    }
    let mut property = |value: String| args.push(format!("--property={value}"));
    if slice.is_some() {
        property("Nice=10".into());
    }
    let cwd = Path::new(cwd.as_deref().expect("preflight resolved cwd"));
    property(format!(
        "WorkingDirectory={}",
        transient_path(cwd).expect("preflight validated cwd")
    ));
    property(format!(
        "EnvironmentFile={}",
        transient_path(&paths.instance_config_dir.join("env")).expect("resolved environment path")
    ));
    property("KillMode=control-group".into());
    property("MemoryAccounting=yes".into());
    for (key, value) in [("PYTHONUNBUFFERED", "1"), ("FORCE_COLOR", "1")] {
        property(format!(
            "Environment={}",
            transient_quote(&format!("{key}={value}"))
        ));
    }
    for (key, path) in path_environment(paths) {
        property(format!(
            "Environment={}",
            transient_quote(&format!("{key}={}", path.display()))
        ));
    }
    property(format!(
        "Environment={}",
        transient_quote(&format!("DEVBOX_INSTANCE={instance}"))
    ));
    if let RunDef::Service(service) = def
        && let Some(port) = service.ports_hint.first()
    {
        property(format!(
            "Environment={}",
            transient_quote(&format!("DEVBOX_PORT={port}"))
        ));
    }
    for (key, value) in environment
        .iter()
        .filter(|(key, value)| !crate::secret_input::injected_env(def, key, value))
    {
        property(format!(
            "Environment={}",
            transient_quote(&format!("{key}={value}"))
        ));
    }
    property(format!(
        "ExecStopPost=:{} --instance {instance} run-exited --run {run_id}",
        transient_quote(executable.to_str().expect("preflight validated executable"))
    ));
    if let RunDef::Service(service) = def {
        let restart = match service.restart {
            RestartPolicy::No => "no",
            RestartPolicy::OnFailure => "on-failure",
            RestartPolicy::Always => "always",
        };
        // Retain failed services so reconciliation can read start-limit state.
        property("CollectMode=inactive".into());
        property(format!("Restart={restart}"));
        property(format!("RestartSec={}s", service.restart_sec));
        property("RestartSteps=5".into());
        property("RestartMaxDelaySec=30s".into());
        property(format!("StartLimitBurst={}", service.start_limit.burst));
        property(format!(
            "StartLimitIntervalSec={}s",
            service.start_limit.interval_sec
        ));
    }
    args.extend([
        "--".into(),
        executable.display().to_string(),
        "--instance".into(),
        instance.into(),
        if crate::secret_input::requires(def) {
            "exec-attempt".into()
        } else {
            "exec".into()
        },
        "--run".into(),
        run_id.into(),
        "--".into(),
    ]);
    args.extend(child_args(def));
    args
}
/// Exactly the child argv admitted by preflight and passed to the exec wrapper.
pub fn child_args(def: &RunDef) -> Vec<String> {
    let environment = match def {
        RunDef::Task(d) => &d.env,
        RunDef::Service(d) => &d.env,
        _ => panic!("resolve executable definition"),
    };
    let mut args = vec!["/usr/bin/env".into(), "--".into()];
    // EnvironmentFile wins over systemd Environment=. Apply definition values
    // at child exec so PATH/LANG overrides follow the definition's semantics.
    args.extend(
        environment
            .iter()
            .filter(|(key, value)| !crate::secret_input::injected_env(def, key, value))
            .map(|(key, value)| format!("{key}={value}")),
    );
    args.extend([
        "/bin/bash".into(),
        "-c".into(),
        def.command().expect("executable definition").into(),
    ]);
    args
}
