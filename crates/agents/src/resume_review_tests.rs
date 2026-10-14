use super::*;
use crate::{prepare::PreparationFailure, tracker::Binding};
use devbox_protocol::Fail;

async fn seed(f: &Fixture, marker: Option<&str>) -> AgentTask {
    let task: AgentTask = serde_json::from_value(serde_json::json!({
        "id":"resume-review","projectId":f.project,"title":"Owned resume","profile":"missing-profile","baseBranch":"main",
        "branch":null,"worktreePath":null,"workdir":f.repo,"sessionId":null,"terminalId":null,
        "state":"failed","toolExited":true,"reason":"arbitrary-reason","attention":null,"port":20000,"prompt":"owned",
        "followUps":[],"tokens":null,"prUrl":null,"closedHow":null,"createdMs":0,"updatedMs":0,"closedMs":null
    })).expect("owned task");
    let row = task.clone();
    f.svc
        .db
        .write(move |tx| {
            crate::store::put(tx, &row)?;
            let mut binding = Binding::new(0);
            binding.launch_id = Some("current".into());
            crate::store::put_binding(tx, &row.id, &binding)
        })
        .await
        .expect("owned launch");
    let phase = f.svc.phase_path(&task, "current");
    crate::prepare::private_dir(phase.parent().expect("data directory")).expect("owned data");
    if let Some(marker) = marker {
        fs::write(phase, marker).expect("owned phase");
    }
    task
}

async fn unchanged(f: &Fixture, before: &AgentTask) {
    let current = f.svc.get(&before.id).await.expect("task");
    assert_eq!(current, *before, "rejected resume leaves task unchanged");
    assert_eq!(
        f.svc
            .launch_binding(&before.id)
            .await
            .expect("binding")
            .launch_id
            .as_deref(),
        Some("current")
    );
    let files =
        fs::read_dir(f.svc.paths.data_dir.join("agents").join(&before.id)).expect("owned data");
    assert!(
        !files
            .filter_map(std::result::Result::ok)
            .any(|e| e.file_name().to_string_lossy().starts_with("resume-")),
        "reject before creating resume files"
    );
}

#[tokio::test]
async fn resume_requires_current_launch_tool_evidence() {
    for marker in [None, Some("starting"), Some("setup:1"), Some("unknown")] {
        let f = Fixture::new().await;
        let task = seed(&f, marker).await;
        let error = f
            .svc
            .resume(&task.id)
            .await
            .expect_err("unproven tool launch");
        assert_eq!(
            serde_json::to_value(match &error {
                Fail::Domain(e) => e,
                _ => panic!("expected domain error: {error:?}"),
            })
            .expect("error")["code"],
            "agents.resume_unavailable",
            "{marker:?}"
        );
        unchanged(&f, &task).await;
    }
    for marker in ["tool", "exit:0", "exit:137"] {
        let f = Fixture::new().await;
        let task = seed(&f, Some(marker)).await;
        assert!(
            matches!(
                f.svc.resume(&task.id).await,
                Err(Fail::Domain(AgentsError::ProfileNotFound { .. }))
            ),
            "valid tool evidence reaches profile resolution"
        );
        unchanged(&f, &task).await;
    }
}

#[tokio::test]
async fn preparation_failure_cannot_convert_worktree_resume_to_inline() {
    let f = Fixture::new().await;
    let task = seed(&f, Some("starting")).await;
    let id = task.id.clone();
    f.svc
        .db
        .write(move |tx| {
            let mut task = crate::store::get(tx, &id)?.expect("task");
            task.state = AgentState::Preparing;
            task.tool_exited = false;
            task.branch = Some("agent/never-created".into());
            task.worktree_path = Some(format!("{}/never-created", task.workdir));
            task.workdir = task.worktree_path.clone().expect("planned path");
            crate::store::put(tx, &task)
        })
        .await
        .expect("planned worktree");
    f.svc
        .preparation_failed(
            &task.id,
            &PreparationFailure::Failed {
                error: Box::new(
                    AgentsError::BranchExists {
                        branch: "agent/never-created".into(),
                    }
                    .into(),
                ),
                launch_id: Some("current".into()),
            },
        )
        .await
        .expect("preparation compensation");
    let failed = f.svc.get(&task.id).await.expect("failed task");
    assert!(failed.worktree_path.is_none() && failed.branch.is_none());
    assert_eq!(failed.workdir, f.repo.to_string_lossy());
    let error = f
        .svc
        .resume(&task.id)
        .await
        .expect_err("failed preparation cannot resume inline");
    assert_eq!(
        serde_json::to_value(match &error {
            Fail::Domain(e) => e,
            _ => panic!("expected domain error: {error:?}"),
        })
        .expect("error")["code"],
        "agents.resume_unavailable"
    );
    unchanged(&f, &failed).await;
}

#[tokio::test]
async fn inline_resume_rejects_symlink_missing_and_noncanonical_workdirs() {
    for mode in [
        "symlink", "ancestor", "missing", "file", "alias", "mismatch",
    ] {
        let f = Fixture::new().await;
        let mut task = seed(&f, Some("exit:0")).await;
        match mode {
            "symlink" => {
                let moved = f.root.path().join("moved");
                fs::rename(&f.repo, &moved).expect("move owned project");
                std::os::unix::fs::symlink(moved, &f.repo).expect("replace with symlink");
            }
            "ancestor" => {
                let link = f.root.path().join("linked");
                std::os::unix::fs::symlink(f.root.path(), &link).expect("symlink ancestor");
                task.workdir = link.join("project").to_string_lossy().into_owned();
            }
            "missing" => fs::remove_dir_all(&f.repo).expect("remove owned project"),
            "file" => {
                fs::remove_dir_all(&f.repo).expect("remove owned project");
                fs::write(&f.repo, "owned").expect("replace with file");
            }
            "alias" => task.workdir = format!("{}/../project", task.workdir),
            "mismatch" => task.workdir = f.root.path().to_string_lossy().into_owned(),
            _ => unreachable!(),
        }
        if mode == "ancestor" || mode == "alias" {
            let project = f.project.clone();
            let path = task.workdir.clone();
            f.svc
                .db
                .write(move |tx| {
                    tx.execute(
                        "UPDATE projects_projects SET path=?1 WHERE id=?2",
                        rusqlite::params![path, project],
                    )
                })
                .await
                .expect("owned alias project");
        }
        let row = task.clone();
        f.svc
            .db
            .write(move |tx| crate::store::put(tx, &row))
            .await
            .expect("owned saved directory");
        assert!(
            matches!(
                f.svc.resume(&task.id).await,
                Err(Fail::Domain(AgentsError::UnsafeWorktree))
            ),
            "{mode} must fail before profile resolution"
        );
        unchanged(&f, &task).await;
    }
}
