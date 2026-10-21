use crate::{DefinitionSource, RunDef, RuntimeError, ScheduleDef, ServiceDef, TaskDef, merge};

fn task(id: &str, command: &str) -> RunDef {
    RunDef::Task(TaskDef {
        id: id.into(),
        command: command.into(),
        ..Default::default()
    })
}

#[test]
fn merge_prefers_project_file_and_reports_shadowed() {
    let merged = merge(
        vec![task("build", "echo file")],
        vec![task("build", "echo local")],
    )
    .expect("valid definitions");
    assert_eq!(merged.defs.len(), 1);
    assert_eq!(merged.defs[0].source, DefinitionSource::ProjectFile);
    assert_eq!(merged.defs[0].def.command(), Some("echo file"));
    assert_eq!(merged.shadowed.len(), 1);
    assert_eq!(merged.shadowed[0].def.command(), Some("echo local"));
}

#[test]
fn rejects_dependency_cycles_with_the_cycle_path() {
    let a = RunDef::Task(TaskDef {
        depends_on: vec!["b".into()],
        ..task_value("a")
    });
    let b = RunDef::Service(ServiceDef {
        id: "b".into(),
        command: "echo b".into(),
        depends_on: vec!["a".into()],
        ..Default::default()
    });
    let error = merge(vec![a, b], vec![]).expect_err("cycle");
    assert!(matches!(error, RuntimeError::DefInvalid { id, reason }
        if id == "a" && reason.contains("a → b → a")));
}

fn task_value(id: &str) -> TaskDef {
    TaskDef {
        id: id.into(),
        command: "echo test".into(),
        ..Default::default()
    }
}

#[test]
fn rejects_missing_dependencies_and_schedule_targets_that_are_not_tasks() {
    let missing = RunDef::Task(TaskDef {
        depends_on: vec!["missing".into()],
        ..task_value("a")
    });
    assert!(
        matches!(merge(vec![missing], vec![]), Err(RuntimeError::DefInvalid { reason, .. }) if reason.contains("missing"))
    );
    let service = RunDef::Service(ServiceDef {
        id: "service".into(),
        command: "true".into(),
        ..Default::default()
    });
    let schedule = RunDef::Schedule(ScheduleDef {
        id: "nightly".into(),
        task: "service".into(),
        cron: "0 0 * * *".into(),
        ..Default::default()
    });
    assert!(merge(vec![service, schedule], vec![]).is_err());
}

#[test]
fn rejects_empty_oversized_and_nul_commands_before_storage() {
    for command in ["  ".into(), "x".repeat(65_537), "echo\0bad".into()] {
        assert!(matches!(
            merge(vec![task("bad", &command)], vec![]),
            Err(RuntimeError::DefInvalid { id, .. }) if id == "bad"
        ));
    }
    assert!(merge(vec![task("ok", &"😀".repeat(16_384))], vec![]).is_ok());
}

#[test]
fn cron_validation_preserves_the_existing_parser_contract() {
    for expression in ["0 0 * * *", "30 */15 * * * *", "0 0 13 * 5"] {
        assert!(merge(vec![task("a", "true"), schedule(expression, None)], vec![]).is_ok());
    }
    for expression in [
        "",
        "@daily",
        "@reboot",
        "0 25 * * *",
        "0 0 13 * +5",
        "0 0 0 1 1 2026 1",
    ] {
        assert!(
            merge(vec![task("a", "true"), schedule(expression, None)], vec![]).is_err(),
            "{expression}"
        );
    }
    assert!(
        merge(
            vec![
                task("a", "true"),
                schedule("0 0 * * *", Some("invalid/zone"))
            ],
            vec![]
        )
        .is_err()
    );
    assert!(
        merge(
            vec![task("a", "true"), schedule("0 0 * * *", Some("Asia/Seoul"))],
            vec![]
        )
        .is_ok()
    );
}

fn schedule(cron: &str, tz: Option<&str>) -> RunDef {
    RunDef::Schedule(ScheduleDef {
        id: "nightly".into(),
        task: "a".into(),
        cron: cron.into(),
        tz: tz.map(str::to_owned),
        ..Default::default()
    })
}

#[test]
fn dependency_order_uses_the_effective_source_and_preserves_sibling_order() {
    let root = RunDef::Task(TaskDef {
        depends_on: vec!["b".into(), "a".into()],
        ..task_value("root")
    });
    let local_hidden = RunDef::Task(TaskDef {
        depends_on: vec!["missing".into()],
        ..task_value("a")
    });
    let merged = merge(
        vec![task("a", "true"), task("b", "true"), root],
        vec![local_hidden],
    )
    .expect("effective graph");
    assert_eq!(merged.order_for("root"), ["b", "a", "root"]);
    assert!(merge(vec![task("a", "true"), task("a", "false")], vec![]).is_err());
}
