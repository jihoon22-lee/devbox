//! A branch name a person typed is used as it is, or refused. One made from the title is numbered.
use super::*;
use devbox_protocol::Fail;

/// Rows a start leaves behind: tasks, launch bindings and terminals.
async fn rows(f: &Fixture) -> Vec<u32> {
    f.svc
        .db
        .read(|tx| {
            let mut counts = vec![];
            for table in ["agents_tasks", "agents_runtime", "terminal_sessions"] {
                counts.push(
                    tx.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                        row.get::<_, u32>(0)
                    })?,
                );
            }
            Ok(counts)
        })
        .await
        .expect("side effects")
}

fn named(f: &Fixture, name: &str) -> CreateAgentTask {
    CreateAgentTask {
        branch: Some(name.into()),
        ..f.request()
    }
}

/// A finished task of the fixture project that used `branch`, whose Git branch is long gone.
fn recorded(project: &str, branch: &str) -> AgentTask {
    serde_json::from_value(serde_json::json!({
        "id":"a-recorded","projectId":project,"title":"Recorded","profile":"claude",
        "baseBranch":"main","branch":branch,"worktreePath":null,"workdir":"/owned/fixture",
        "sessionId":null,"terminalId":null,"state":"closed","reason":null,"attention":null,
        "port":20000,"prompt":"","followUps":[],"tokens":null,"prUrl":null,"closedHow":null,
        "createdMs":0,"updatedMs":0,"closedMs":0
    }))
    .expect("owned task")
}

#[tokio::test]
async fn an_invalid_branch_name_is_refused_before_anything_is_reserved() {
    let f = Fixture::new().await;
    let before = devbox_git::branches(&f.repo).await.expect("branches");
    let too_long = "x".repeat(41);
    for bad in [
        "",
        "Fix",
        "fix login",
        "fix--login",
        "-fix",
        "agent/fix",
        "가나다",
        too_long.as_str(),
    ] {
        let refused = f.svc.create(named(&f, bad)).await;
        assert!(
            matches!(refused, Err(Fail::Domain(AgentsError::InvalidInput))),
            "{bad:?}: {refused:?}"
        );
    }
    // A name needs a branch to name: without a work folder there is none.
    let inline = CreateAgentTask {
        worktree: false,
        ..named(&f, "fine")
    };
    let refused = f.svc.create(inline).await;
    assert!(
        matches!(refused, Err(Fail::Domain(AgentsError::InvalidInput))),
        "{refused:?}"
    );
    assert_eq!(
        rows(&f).await,
        vec![0, 0, 0],
        "no task, binding or terminal"
    );
    assert_eq!(
        devbox_git::branches(&f.repo).await.expect("branches"),
        before
    );
    assert!(!f.root.path().join("project-agents").exists());
}

#[tokio::test]
async fn a_taken_branch_name_is_refused_not_numbered() {
    let f = Fixture::new().await;
    git(&f.repo, &["branch", "agent/taken"]);
    let row = recorded(&f.project, "agent/recorded");
    f.svc
        .db
        .write(move |tx| {
            store::put(tx, &row)?;
            Ok(())
        })
        .await
        .expect("owned row");
    for taken in ["taken", "recorded"] {
        let refused = f.svc.create(named(&f, taken)).await;
        assert!(
            matches!(&refused, Err(Fail::Domain(AgentsError::BranchExists { branch }))
                if *branch == format!("agent/{taken}")),
            "{taken}: {refused:?}"
        );
    }
    assert_eq!(rows(&f).await, vec![1, 0, 0], "only the seeded record");
    assert!(!f.root.path().join("project-agents").exists());
}

#[tokio::test]
async fn a_typed_branch_name_names_the_branch_and_its_work_folder() {
    if checkout_fixture::run_child(
        "service::resource_tests::branch_name_tests::a_typed_branch_name_names_the_branch_and_its_work_folder",
    ) {
        return;
    }
    let f = Fixture::new().await;
    // The title is "owned"; the typed name decides the branch.
    let task = f
        .svc
        .create(named(&f, "fix-login"))
        .await
        .expect("typed name");
    assert_eq!(task.branch.as_deref(), Some("agent/fix-login"));
    let folder = f.root.path().join("project-agents/fix-login");
    assert_eq!(task.worktree_path.as_deref(), folder.to_str());
    f.drain().await;
    let branches = devbox_git::branches(&f.repo).await.expect("branches");
    assert!(branches.contains(&"agent/fix-login".into()), "{branches:?}");
    assert!(!branches.contains(&"agent/owned".into()), "{branches:?}");
}

#[tokio::test]
async fn a_nested_branch_takes_the_name_it_lives_under() {
    if checkout_fixture::run_child(
        "service::resource_tests::branch_name_tests::a_nested_branch_takes_the_name_it_lives_under",
    ) {
        return;
    }
    let f = Fixture::new().await;
    // Git cannot make `agent/login` while `agent/login/v2` exists, so `login` is taken.
    git(&f.repo, &["branch", "agent/login/v2"]);
    let refused = f.svc.create(named(&f, "login")).await;
    assert!(
        matches!(&refused, Err(Fail::Domain(AgentsError::BranchExists { branch }))
            if branch == "agent/login"),
        "{refused:?}"
    );
    assert_eq!(
        rows(&f).await,
        vec![0, 0, 0],
        "nothing reserved to fail later"
    );
    assert!(!f.root.path().join("project-agents").exists());
    // A name made from the title skips it like any taken name.
    let derived = CreateAgentTask {
        title: "login".into(),
        ..f.request()
    };
    let task = f.svc.create(derived).await.expect("numbered name");
    assert_eq!(task.branch.as_deref(), Some("agent/login-2"));
    f.drain().await;
    let branches = devbox_git::branches(&f.repo).await.expect("branches");
    assert!(branches.contains(&"agent/login-2".into()), "{branches:?}");
    assert!(branches.contains(&"agent/login/v2".into()), "{branches:?}");
}
