use crate::{RestartPolicy, RunDef, ServiceDef, launch::unit_args};
use devbox_core::{Env, Instance, Paths};

struct FixtureEnv;
impl Env for FixtureEnv {
    fn var(&self, key: &str) -> Option<String> {
        let path = match key {
            "HOME" => "/owned/home",
            "XDG_CONFIG_HOME" => "/owned/config",
            "XDG_DATA_HOME" => "/owned/data",
            "XDG_STATE_HOME" => "/owned/state",
            "XDG_RUNTIME_DIR" => "/owned/runtime",
            _ => return None,
        };
        Some(path.into())
    }
    fn uid(&self) -> u32 {
        1000
    }
}
fn paths(instance: &str) -> Paths {
    Paths::resolve(Instance::parse(instance).expect("instance"), &FixtureEnv).expect("paths")
}
fn service(restart: RestartPolicy) -> RunDef {
    RunDef::Service(ServiceDef {
        id: "api".into(),
        command: "printf '%s' \"$HOME\"".into(),
        cwd: Some("/owned/project".into()),
        restart,
        ports_hint: vec![3000],
        ..Default::default()
    })
}
#[test]
fn unit_args_for_service_with_restart_policy() {
    let paths = paths("test-args");
    let exe = std::env::current_exe().expect("exe").display().to_string();
    let expected: Vec<String> = vec![
        "--user".into(),
        "--unit".into(),
        "devbox-test-args-run-r1.service".into(),
        "--collect".into(),
        "--property=Type=exec".into(),
        "--expand-environment=no".into(),
        "--slice=owned.slice".into(),
        "--property=Nice=10".into(),
        "--property=WorkingDirectory=/owned/project".into(),
        "--property=EnvironmentFile=/owned/config/devbox/test-args/env".into(),
        "--property=KillMode=control-group".into(),
        "--property=MemoryAccounting=yes".into(),
        "--property=Environment=\"PYTHONUNBUFFERED=1\"".into(),
        "--property=Environment=\"FORCE_COLOR=1\"".into(),
        "--property=Environment=\"HOME=/owned/home\"".into(),
        "--property=Environment=\"XDG_CONFIG_HOME=/owned/config\"".into(),
        "--property=Environment=\"XDG_DATA_HOME=/owned/data\"".into(),
        "--property=Environment=\"XDG_STATE_HOME=/owned/state\"".into(),
        "--property=Environment=\"XDG_RUNTIME_DIR=/owned/runtime\"".into(),
        "--property=Environment=\"DEVBOX_INSTANCE=test-args\"".into(),
        "--property=Environment=\"DEVBOX_PORT=3000\"".into(),
        format!("--property=ExecStopPost=:\"{exe}\" --instance test-args run-exited --run r1"),
        "--property=CollectMode=inactive".into(),
        "--property=Restart=on-failure".into(),
        "--property=RestartSec=1s".into(),
        "--property=RestartSteps=5".into(),
        "--property=RestartMaxDelaySec=30s".into(),
        "--property=StartLimitBurst=5".into(),
        "--property=StartLimitIntervalSec=60s".into(),
        "--".into(),
        exe,
        "--instance".into(),
        "test-args".into(),
        "exec".into(),
        "--run".into(),
        "r1".into(),
        "--".into(),
        "/usr/bin/env".into(),
        "--".into(),
        "/bin/bash".into(),
        "-c".into(),
        "printf '%s' \"$HOME\"".into(),
    ];
    assert_eq!(
        unit_args(
            &service(RestartPolicy::OnFailure),
            "r1",
            &paths,
            Some("owned.slice")
        ),
        expected
    );
    let args = unit_args(&service(RestartPolicy::No), "r1", &paths, None);
    assert!(args.contains(&"--property=Restart=no".into()));
    assert!(!args.iter().any(|arg| arg.starts_with("--slice")));
    assert!(
        unit_args(&service(RestartPolicy::Always), "r1", &paths, None)
            .contains(&"--property=Restart=always".into())
    );
}
#[test]
fn unit_args_preserve_instance_environment_file() {
    for instance in ["test-args", "accept"] {
        let mut paths = paths(instance);
        paths.instance_config_dir = "/owned/config space/%special/\"quoted\"".into();
        let args = unit_args(&service(RestartPolicy::No), "r1", &paths, None);
        assert!(args.contains(
            &"--property=EnvironmentFile=/owned/config space/%special/\"quoted\"/env".into()
        ));
        assert!(!args.iter().any(|arg| arg.contains("prod")));
    }
}
#[test]
fn exec_helper_and_stop_post_share_instance_and_xdg_paths() {
    for instance in ["test-args", "accept"] {
        let paths = paths(instance);
        let args = unit_args(&service(RestartPolicy::No), "r1", &paths, None);
        let exe = std::env::current_exe().expect("exe").display().to_string();
        assert!(args.contains(&format!(
            "--property=ExecStopPost=:\"{exe}\" --instance {instance} run-exited --run r1"
        )));
        let main = args.iter().position(|arg| arg == "--").expect("main");
        assert_eq!(
            &args[main + 1..main + 4],
            &[exe, "--instance".into(), instance.into()]
        );
        for (key, path) in [
            ("HOME", "/owned/home"),
            ("XDG_CONFIG_HOME", "/owned/config"),
            ("XDG_DATA_HOME", "/owned/data"),
            ("XDG_STATE_HOME", "/owned/state"),
            ("XDG_RUNTIME_DIR", "/owned/runtime"),
        ] {
            assert!(args.contains(&format!("--property=Environment=\"{key}={path}\"")));
        }
    }
}
#[test]
fn unit_args_preserve_literal_command_environment_and_working_directory() {
    let mut def = service(RestartPolicy::No);
    let RunDef::Service(service) = &mut def else {
        unreachable!()
    };
    service.cwd = Some("/owned/project % space".into());
    service
        .env
        .insert("LITERAL".into(), "space \"quote\" \\ $HOME %h".into());
    let expected_command = service.command.clone();
    let args = unit_args(&def, "r1", &paths("test-args"), None);
    assert!(args.contains(&"--property=WorkingDirectory=/owned/project % space".into()));
    assert!(
        args.contains(
            &"--property=Environment=\"LITERAL=space \\\"quote\\\" \\\\ $HOME %h\"".into()
        )
    );
    assert_eq!(args.last().expect("command"), &expected_command);
}
