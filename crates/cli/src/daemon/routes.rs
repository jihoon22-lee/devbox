use super::state::Services;
use devbox_projects as projects;
use devbox_protocol::system::{DaemonInfo, Info, Ping, Pong};
use devbox_protocol::{NameCollector, Router};
use std::sync::Arc;

pub fn router(s: Arc<Services>) -> Router<Services> {
    let test_instance = s.paths.instance.as_str().starts_with("test-");
    let router = Router::new(s)
        .register::<Ping, _, _>(|s, _, _| async move {
            Ok(Pong {
                daemon_id: s.daemon_id.clone(),
                version: s.version.into(),
                uptime_ms: s.started.elapsed().as_millis() as i64,
            })
        })
        .register::<Info, _, _>(|s, _, _| async move {
            Ok(DaemonInfo {
                version: s.version.into(),
                instance: s.paths.instance.as_str().into(),
                pid: std::process::id(),
                started_ms: s.started_ms,
                data_dir: s.paths.data_dir.display().to_string(),
            })
        })
        .register::<devbox_protocol::system::Doctor, _, _>(|s, _, _| async move {
            let probe = crate::doctor::OsProbe {
                env_file: s.paths.instance_config_dir.join("env"),
            };
            let checks = tokio::task::spawn_blocking(move || crate::doctor::checks(&probe))
                .await
                .map_err(|e| devbox_protocol::Fail::Internal(e.to_string()))?;
            Ok(devbox_protocol::system::DoctorReport { checks })
        })
        .register::<projects::ProjectsList, _, _>(|s, _, p| async move { s.projects.list(p).await })
        .register::<projects::ProjectsAdd, _, _>(|s, _, p| async move { s.projects.add(p).await })
        .register::<projects::ProjectsRemove, _, _>(
            |s, _, p| async move { s.projects.remove(p).await },
        )
        .register::<projects::ProjectsScan, _, _>(
            |s, _, p| async move { s.projects.scan(p).await },
        );
    let router = router
        .register::<devbox_terminal::TerminalAttach, _, _>(|s, ctx, p| async move {
            s.terminal.attach(ctx, p).await
        })
        .register::<devbox_terminal::TerminalResize, _, _>(|s, ctx, p| async move {
            s.terminal.resize(ctx, p).await
        })
        .register::<devbox_terminal::TerminalList, _, _>(|s, _, p| async move {
            s.terminal.list(p).await
        })
        .register::<devbox_terminal::TerminalCreate, _, _>(|s, _, p| async move {
            s.terminal.create(p).await
        })
        .register::<devbox_terminal::TerminalRename, _, _>(|s, _, p| async move {
            s.terminal.rename(p).await
        })
        .register::<devbox_terminal::TerminalClose, _, _>(|s, _, p| async move {
            s.terminal.close(p).await
        })
        .register::<devbox_terminal::TerminalZellijList, _, _>(|s, _, _| async move {
            Ok(s.terminal.zellij_list().await)
        })
        .register::<devbox_terminal::Profiles, _, _>(
            |s, _, _| async move { Ok(s.terminal.profiles()) },
        );
    let router = router
        .register::<devbox_agents::AgentsCreate, _, _>(
            |s, _, p| async move { s.agents.create(p).await },
        )
        .register::<devbox_agents::AgentsListMethod, _, _>(|s, _, p| async move {
            s.agents.list(p).await
        })
        .register::<devbox_agents::AgentsGet, _, _>(
            |s, _, p| async move { s.agents.get(&p.id).await },
        )
        .register::<devbox_agents::AgentsProfiles, _, _>(|s, _, _| async move {
            Ok(s.agents.profiles())
        })
        .register::<devbox_agents::AgentsEvent, _, _>(
            |s, _, p| async move { s.agents.event(p).await },
        );
    let router = router
        .register::<devbox_agents::AgentsSend, _, _>(
            |s, _, p| async move { s.agents.send(p).await },
        )
        .register::<devbox_agents::AgentsResume, _, _>(|s, _, p| async move {
            s.agents.resume(&p.id).await
        })
        .register::<devbox_agents::AgentsReview, _, _>(|s, _, p| async move {
            s.agents.review(&p.id).await
        })
        .register::<devbox_agents::AgentsRebase, _, _>(|s, _, p| async move {
            s.agents.rebase(&p.id).await
        })
        .register::<devbox_agents::AgentsRebaseAbort, _, _>(|s, _, p| async move {
            s.agents.rebase_abort(&p.id).await
        })
        .register::<devbox_agents::AgentsMerge, _, _>(
            |s, _, p| async move { s.agents.merge(p).await },
        )
        .register::<devbox_agents::AgentsPr, _, _>(|s, _, p| async move { s.agents.pr(p).await })
        .register::<devbox_agents::AgentsDiscardCheck, _, _>(|s, _, p| async move {
            s.agents.discard_check(&p.id).await
        })
        .register::<devbox_agents::AgentsDiscard, _, _>(|s, _, p| async move {
            s.agents.discard(p).await
        })
        .register::<devbox_agents::AgentsTest, _, _>(
            |s, _, p| async move { s.agents.test(&p.id).await },
        )
        .register::<devbox_agents::AgentsCleanupCandidates, _, _>(|s, _, p| async move {
            s.agents.cleanup_candidates(p).await
        })
        .register::<devbox_agents::AgentsCleanup, _, _>(|s, _, p| async move {
            s.agents.cleanup(p).await
        })
        .register::<devbox_agents::AgentsChanges, _, _>(|s, _, p| async move {
            s.agents.changes(&p.id).await
        })
        .register::<devbox_agents::AgentsPatch, _, _>(
            |s, _, p| async move { s.agents.patch(p).await },
        )
        .register::<devbox_agents::AgentsCommits, _, _>(|s, _, p| async move {
            s.agents.commits(&p.id).await
        });
    if test_instance {
        router.register::<devbox_protocol::debug::DebugEcho, _, _>(|_, ctx, params| {
            super::debug::echo(ctx, params)
        })
    } else {
        router
    }
}

/// 선언된 메서드·주제 전체(라우터 완전성 시험과 주제 검증에 쓴다).
pub fn declared() -> NameCollector {
    let mut n = NameCollector::default();
    devbox_protocol::system::export(&mut n);
    projects::export(&mut n);
    devbox_terminal::export(&mut n);
    devbox_agents::export(&mut n);
    n
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_declared_method_is_routed() {
        let declared = super::declared();
        let t = tempfile::tempdir().expect("create fixture directory");
        let s = crate::daemon::build_services_for_test(t.path());
        let r = super::router(s);
        let mut want = declared.methods.clone();
        want.push("debug.echo");
        want.sort_unstable();
        assert_eq!(r.names(), want);
    }
}
