use crate::{service_tests::Fixture, *};
use devbox_protocol::Fail;
async fn consent(f: &Fixture) -> devbox_projects::trust::AuthorizedConfig {
    let preview = f
        .projects
        .trust_preview(
            devbox_projects::TrustPreviewParams {
                id: f.project.clone(),
                operation: devbox_projects::TrustOperation::Test,
                agent_id: None,
                worktrees: vec![],
                worktree: None,
            },
            true,
            vec![],
        )
        .await
        .expect("fixture result");
    f.projects
        .trust_approve(devbox_projects::TrustApprove {
            id: f.project.clone(),
            expected_canonical_path: preview.canonical_path,
            expected_digest: preview.digest.expect("fixture value"),
            expected_scope_version: preview.scope_version,
        })
        .await
        .expect("fixture result");
    f.projects
        .test_snapshot(&f.project)
        .await
        .expect("fixture result")
        .expect("fixture value")
}
#[tokio::test]
async fn agent_test_rejects_stale_or_reapproved_changed_snapshot_and_command_substitution_without_resources()
 {
    for mode in ["stale", "reapproved", "command", "project"] {
        let f = Fixture::new().await;
        f.write("[agent]\ntest='true'\n");
        let snapshot = consent(&f).await;
        match mode {
            "stale" | "reapproved" => {
                f.write("# whole-byte digest changed\n[agent]\ntest='true'\n");
                if mode == "reapproved" {
                    consent(&f).await;
                }
            }
            _ => {}
        }
        let result = f
            .runs
            .start_agent_test(AgentTestRequest {
                agent_id: "fixture".into(),
                project_id: if mode == "project" {
                    "foreign".into()
                } else {
                    f.project.clone()
                },
                cwd: f.root.path().join("repo"),
                command: if mode == "command" {
                    "evil".into()
                } else {
                    "true".into()
                },
                snapshot,
            })
            .await;
        assert!(
            matches!(
                result,
                Err(Fail::Domain(RuntimeError::ProjectTrustRequired { .. }))
            ),
            "{mode}: {result:?}"
        );
        assert!(
            f.runs
                .runs(RunsQuery {
                    project_id: None,
                    limit: 100
                })
                .await
                .expect("fixture result")
                .items
                .is_empty()
        );
    }
}
