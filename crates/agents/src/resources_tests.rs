use super::*;

const SCOPE: &str = "devbox-test-agent-owned.scope";
const INVOCATION: &str = "11111111111111111111111111111111";

fn accounting(cpu: Option<u64>) -> Accounting {
    Accounting {
        invocation: Some(INVOCATION.into()),
        cpu_ns: cpu,
        memory: Some(123),
    }
}

#[test]
fn cpu_uses_elapsed_delta_and_resets_on_counter_restart_or_missing_accounting() {
    let mut sampler = ResourceSampler::default();
    let at = Instant::now();
    assert_eq!(
        sampler.record(SCOPE, accounting(Some(1_000_000_000)), at),
        (None, Some(123))
    );
    assert_eq!(
        sampler.record(
            SCOPE,
            accounting(Some(2_000_000_000)),
            at + Duration::from_secs(2)
        ),
        (Some(50.0), Some(123))
    );
    assert_eq!(
        sampler
            .record(SCOPE, accounting(Some(1)), at + Duration::from_secs(3))
            .0,
        None
    );
    assert_eq!(
        sampler
            .record(SCOPE, accounting(None), at + Duration::from_secs(4))
            .0,
        None
    );
    assert_eq!(
        sampler
            .record(SCOPE, accounting(Some(10)), at + Duration::from_secs(5))
            .0,
        None
    );
}

#[test]
fn same_timestamp_and_reused_or_departed_units_never_reuse_cpu_baseline() {
    let mut sampler = ResourceSampler::default();
    let at = Instant::now();
    sampler.record(SCOPE, accounting(Some(10)), at);
    assert_eq!(sampler.record(SCOPE, accounting(Some(20)), at).0, None);
    let mut reused = accounting(Some(1_000_000_000));
    reused.invocation = Some("22222222222222222222222222222222".into());
    assert_eq!(
        sampler.record(SCOPE, reused, at + Duration::from_secs(1)).0,
        None
    );
    sampler.retain(&[]);
    assert_eq!(
        sampler
            .record(SCOPE, accounting(Some(30)), at + Duration::from_secs(2))
            .0,
        None
    );
}

#[test]
fn named_properties_ignore_order_and_preserve_unavailable_values() {
    let parsed = parse(&format!("MemoryCurrent=123\nInvocationID={INVOCATION}\nActiveState=active\nCPUUsageNSec=456\nLoadState=loaded\n")).expect("named fields");
    assert_eq!((parsed.cpu_ns, parsed.memory), (Some(456), Some(123)));
    for unavailable in [
        "",
        "[not set]",
        "infinity",
        "18446744073709551615",
        "garbage",
    ] {
        let parsed = parse(&format!("LoadState=loaded\nActiveState=active\nCPUUsageNSec={unavailable}\nMemoryCurrent={unavailable}\n")).expect("unavailable counters");
        assert_eq!((parsed.cpu_ns, parsed.memory), (None, None));
    }
    assert!(
        parse("LoadState=not-found\nActiveState=inactive\nCPUUsageNSec=0\nMemoryCurrent=0\n")
            .is_err()
    );
    assert!(
        parse("LoadState=loaded\nActiveState=active\nMemoryCurrent=1\nMemoryCurrent=2\n").is_err()
    );
}

#[cfg(unix)]
fn provider(script: &str) -> (tempfile::TempDir, ResourceSampler) {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("owned provider fixture");
    let path = dir.path().join("systemctl");
    std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).expect("provider script");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).expect("script mode");
    (
        dir,
        ResourceSampler {
            program: path,
            timeout: Duration::from_millis(100),
            previous: HashMap::new(),
            launches: HashMap::new(),
        },
    )
}

#[cfg(unix)]
#[tokio::test]
async fn fake_provider_queries_only_exact_named_properties_and_invalid_scopes_are_rejected() {
    let (_dir, mut sampler) = provider(
        "[ \"$*\" = '--user show --no-pager -p CPUUsageNSec -p MemoryCurrent -p LoadState -p ActiveState -p InvocationID -- devbox-test-agent-owned.scope' ] || exit 2\nprintf 'MemoryCurrent=77\\nLoadState=loaded\\nActiveState=active\\nCPUUsageNSec=0\\n'",
    );
    assert_eq!(
        sampler
            .sample(
                "owned",
                SCOPE,
                1,
                &tokio_util::sync::CancellationToken::new()
            )
            .await,
        (None, Some(77))
    );
    assert!(matches!(
        sampler.query("--foreign.scope").await,
        Err(ResourceError::InvalidScope)
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn timeout_output_overflow_nonzero_and_missing_unit_clear_baselines() {
    for script in [
        "exec sleep 5",
        "head -c 20000 /dev/zero",
        "head -c 20000 /dev/zero >&2",
        "exit 3",
        "printf 'LoadState=not-found\\nActiveState=inactive\\nMemoryCurrent=0\\nCPUUsageNSec=0\\n'",
    ] {
        let (_dir, mut sampler) = provider(script);
        sampler.launches.insert(SCOPE.into(), 1);
        sampler.record(SCOPE, accounting(Some(5)), Instant::now());
        let started = Instant::now();
        assert_eq!(
            sampler
                .sample(
                    "owned",
                    SCOPE,
                    1,
                    &tokio_util::sync::CancellationToken::new()
                )
                .await,
            (None, None)
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(sampler.previous.is_empty());
    }
}

#[cfg(unix)]
#[tokio::test]
async fn launch_change_resets_same_scope_even_when_provider_invocation_is_reused() {
    let (_dir, mut sampler) = provider(&format!(
        "printf 'LoadState=loaded\\nActiveState=active\\nInvocationID={INVOCATION}\\nCPUUsageNSec=1000000000\\nMemoryCurrent=9\\n'"
    ));
    let cancel = tokio_util::sync::CancellationToken::new();
    assert_eq!(sampler.sample("owned", SCOPE, 1, &cancel).await.0, None);
    assert_eq!(
        sampler.sample("owned", SCOPE, 1, &cancel).await.0,
        Some(0.0)
    );
    assert_eq!(sampler.sample("owned", SCOPE, 2, &cancel).await.0, None);
    cancel.cancel();
    assert_eq!(
        sampler.sample("owned", SCOPE, 2, &cancel).await,
        (None, None)
    );
    assert!(sampler.previous.is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn canceled_query_kills_its_owned_subprocess_group() {
    let (dir, mut sampler) =
        provider("sleep 30 &\nprintf '%s %s' \"$$\" \"$!\" > \"$0.pid\"\nwait");
    sampler.timeout = Duration::from_secs(5);
    let pid_path = dir.path().join("systemctl.pid");
    let task = tokio::spawn(async move { sampler.query(SCOPE).await });
    let (pid, descendant) = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Ok(contents) = std::fs::read_to_string(&pid_path) {
                let pids: Vec<i32> = contents
                    .split_whitespace()
                    .filter_map(|part| part.parse().ok())
                    .collect();
                if pids.len() == 2 {
                    break (pids[0], pids[1]);
                }
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("provider entered");
    task.abort();
    assert!(task.await.expect_err("canceled").is_cancelled());
    tokio::time::timeout(Duration::from_secs(2), async {
        while nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_ok() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("owned provider killed and reaped");
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let stat = std::fs::read_to_string(format!("/proc/{descendant}/stat"));
            if stat.is_err()
                || stat.is_ok_and(|stat| {
                    stat.rsplit_once(')')
                        .is_some_and(|(_, fields)| fields.trim_start().starts_with('Z'))
                })
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("owned pipe-inheriting descendant terminated");
}

#[cfg(unix)]
#[tokio::test]
async fn review_regression_inline_live_agent_is_included_in_resources() {
    use crate::{model::AgentTask, service::AgentsService, store, tracker::Binding};
    use devbox_core::{Db, Hub, Instance, Migration, Paths, paths::Env};
    use std::sync::Arc;
    struct TestEnv(PathBuf);
    impl Env for TestEnv {
        fn var(&self, key: &str) -> Option<String> {
            match key {
                "HOME" => Some(self.0.to_string_lossy().into_owned()),
                "XDG_RUNTIME_DIR" => Some(self.0.join("run").to_string_lossy().into_owned()),
                _ => None,
            }
        }
        fn uid(&self) -> u32 {
            1000
        }
    }
    let root = tempfile::tempdir().unwrap();
    let instance = Instance::parse(&format!(
        "test-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    ))
    .unwrap();
    let paths = Paths::resolve(instance, &TestEnv(root.path().into())).unwrap();
    paths.ensure_dirs().unwrap();
    std::fs::write(
        paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .unwrap();
    let migrations: Vec<Migration> = crate::MIGRATIONS
        .iter()
        .chain(devbox_projects::MIGRATIONS)
        .chain(devbox_terminal::MIGRATIONS)
        .copied()
        .collect();
    let db = Db::open(&paths.db_path, &paths.backups_dir, "test", &migrations).unwrap();
    let hub = Arc::new(Hub::new());
    let projects = Arc::new(devbox_projects::ProjectsService::new(
        db.clone(),
        hub.clone(),
        paths.home.clone(),
        vec![],
    ));
    let terminal = Arc::new(devbox_terminal::TerminalService::new(
        db.clone(),
        hub.clone(),
        paths.clone(),
        projects.clone(),
        devbox_core::config::TerminalConfig::default(),
    ));
    let svc = AgentsService::new(
        db,
        hub,
        paths,
        projects,
        terminal.clone(),
        vec![],
        "/owned/devbox".into(),
        true,
        false,
    );
    let (_provider_dir, sampler) = provider(&format!(
        "printf 'LoadState=loaded\\nActiveState=active\\nInvocationID={INVOCATION}\\nCPUUsageNSec=1000000000\\nMemoryCurrent=77\\n'"
    ));
    *svc.resource_samples.lock().await = sampler;
    let task: AgentTask = serde_json::from_str(r#"{"id":"a","projectId":"p","title":"inline","profile":"codex","baseBranch":"main","branch":null,"worktreePath":null,"workdir":"/owned","sessionId":null,"terminalId":"owned-term","state":"running","reason":null,"attention":null,"port":20000,"prompt":"","followUps":[],"tokens":null,"prUrl":null,"closedHow":null,"createdMs":0,"updatedMs":0,"closedMs":null}"#).unwrap();
    svc.db
        .write(move |tx| {
            store::put(tx, &task)?;
            // Inline preparation retains Planned; this enum records Git worktree ownership.
            store::put_binding(tx, "a", &Binding::new(0))
        })
        .await
        .unwrap();
    terminal
        .tmux()
        .new_session("owned-term", root.path(), Some("sleep 60"), &[])
        .await
        .unwrap();
    let result = svc.resources().await;
    terminal.tmux().kill_server().await;
    let items = result.unwrap().items;
    assert_eq!(items.len(), 1, "live inline agent scope must be sampled");
    assert_eq!(items[0].memory_bytes, Some(77));
}
