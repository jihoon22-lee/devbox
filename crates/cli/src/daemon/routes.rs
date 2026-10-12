use super::state::Services;
use devbox_projects as projects;
use devbox_protocol::system::{DaemonInfo, Info, Ping, Pong};
use devbox_protocol::{NameCollector, Router};
use std::sync::Arc;

pub fn router(s: Arc<Services>) -> Router<Services> {
    Router::new(s)
        .add::<Ping, _, _>(|s, _, _| async move {
            Ok(Pong {
                daemon_id: s.daemon_id.clone(),
                version: s.version.into(),
                uptime_ms: s.started.elapsed().as_millis() as i64,
            })
        })
        .add::<Info, _, _>(|s, _, _| async move {
            Ok(DaemonInfo {
                version: s.version.into(),
                instance: s.paths.instance.as_str().into(),
                pid: std::process::id(),
                started_ms: s.started_ms,
                data_dir: s.paths.data_dir.display().to_string(),
            })
        })
        .add::<projects::ProjectsList, _, _>(|s, _, p| async move { s.projects.list(p).await })
        .add::<projects::ProjectsAdd, _, _>(|s, _, p| async move { s.projects.add(p).await })
        .add::<projects::ProjectsRemove, _, _>(|s, _, p| async move { s.projects.remove(p).await })
        .add::<projects::ProjectsScan, _, _>(|s, _, p| async move { s.projects.scan(p).await })
}

/// 선언된 메서드·주제 전체(라우터 완전성 시험과 주제 검증에 쓴다).
pub fn declared() -> NameCollector {
    let mut n = NameCollector::default();
    devbox_protocol::system::export(&mut n);
    projects::export(&mut n);
    n
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_declared_method_is_routed() {
        let declared = super::declared();
        let t = tempfile::tempdir().unwrap();
        let s = crate::daemon::build_services_for_test(t.path());
        let r = super::router(s);
        let mut want = declared.methods.clone();
        want.sort_unstable();
        assert_eq!(r.names(), want);
    }
}
