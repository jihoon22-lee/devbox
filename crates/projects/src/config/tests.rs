#[cfg(unix)]
use super::copy::copy_collected_file;
use super::*;

#[cfg(unix)]
#[test]
fn collected_copy_rejects_source_replacement_after_ancestor_validation() {
    for replace_ancestor in [false, true] {
        let root = tempfile::tempdir().expect("owned fixture");
        let source = root.path().join("source");
        let external = root.path().join("external");
        let destination = root.path().join("copied");
        std::fs::create_dir(&source).expect("owned fixture");
        std::fs::create_dir(&external).expect("owned fixture");
        std::fs::write(source.join("file"), "project file").expect("owned fixture");
        std::fs::write(external.join("file"), "outside file").expect("owned fixture");
        check_ancestors(&source.join("file")).expect("initial validation");
        if replace_ancestor {
            std::fs::rename(&source, root.path().join("old-source")).expect("owned fixture");
            std::os::unix::fs::symlink(&external, &source).expect("owned fixture");
        } else {
            std::fs::remove_file(source.join("file")).expect("owned fixture");
            std::os::unix::fs::symlink(external.join("file"), source.join("file"))
                .expect("owned fixture");
        }
        assert!(
            copy_collected_file(
                &source.join("file"),
                &destination,
                &crate::copy_target::CopyTarget::open(root.path()).expect("pin destination")
            )
            .is_err()
        );
        assert!(!destination.exists(), "outside source was copied");
    }
}

#[cfg(unix)]
#[test]
fn collected_copy_rejects_destination_parent_replacement() {
    let root = tempfile::tempdir().expect("owned fixture");
    let source = root.path().join("source");
    let target = root.path().join("target");
    let external = root.path().join("external");
    std::fs::write(&source, "project file").expect("owned fixture");
    std::fs::create_dir_all(target.join("dir")).expect("owned fixture");
    std::fs::create_dir(&external).expect("owned fixture");
    let destination = target.join("dir/file");
    check_ancestors(&destination).expect("initial validation");
    let pinned = crate::copy_target::CopyTarget::open(&target).expect("pin destination");
    std::fs::rename(target.join("dir"), target.join("old-dir")).expect("owned fixture");
    std::os::unix::fs::symlink(&external, target.join("dir")).expect("owned fixture");
    let result = copy_collected_file(&source, &destination, &pinned);
    assert!(
        !external.join("file").exists(),
        "copy escaped owned worktree"
    );
    assert!(result.is_err());
}

#[test]
fn launch_sections_and_entries_require_named_toml_tables() {
    assert!(matches!(
        parse("launch=[[],[[\"shell\",\"echo\"]],[]]"),
        Err(ConfigError::LaunchNotTable)
    ));
    for (text, section) in [
        ("[launch]\nterminals=[[\"shell\",\"echo\"]]", "terminals"),
        ("[launch]\nagents=[[\"fake\",false,\"prompt\"]]", "agents"),
        (
            "[launch]\nterminals=[{name='valid'}, ['shell','echo']]",
            "terminals",
        ),
    ] {
        assert!(
            matches!(parse(text), Err(ConfigError::LaunchEntryNotTable { section: actual, .. }) if actual == section),
            "{text}"
        );
    }
    let config = parse("[[launch.terminals]]\nname='shell'\ncommand='echo'\n[[launch.agents]]\ntool='fake'\nworktree=false\nprompt='prompt'").expect("named table entries");
    assert_eq!(config.launch.terminals[0].name, "shell");
    assert_eq!(config.launch.agents[0].tool, "fake");
}

#[test]
fn parses_launch_and_keeps_typed_runs_out_of_legacy_config_wire() {
    let config = parse("[[task]]\nid='test'\ncommand='pnpm test'\n[[service]]\nid='web'\ncommand='serve'\n[[schedule]]\nid='daily'\ntask='test'\ncron='0 0 * * *'\n[launch]\nservices=['web']\nterminals=[{name='shell'}, {name='logs',command='tail -f x.log'}]\nagents=[{tool='claude',worktree=false,prompt='hello'}]").expect("parse fixture");
    assert_eq!(
        config.launch.terminals[1].command.as_deref(),
        Some("tail -f x.log")
    );
    assert!(!config.launch.agents[0].worktree);
    assert_eq!(config.raw_tasks.len(), 1);
    assert_eq!(config.raw_services.len(), 1);
    assert_eq!(config.raw_schedules.len(), 1);
    let wire = serde_json::to_string(&config).expect("serialize config");
    assert_eq!(config.raw_tasks[0].command, "pnpm test");
    assert!(!wire.contains("pnpm test"));
    assert!(!wire.contains("raw_tasks"));
    assert!(parse("[launch]\nterminals=[{name='shell',commnad='x'}]").is_err());
    assert!(parse("[launch]\nagents=[{tool='claude',unknown=true}]").is_err());
}

#[cfg(unix)]
#[test]
fn config_load_rejects_symlinked_config_and_ancestors() {
    let root = tempfile::tempdir().expect("fixture");
    std::fs::create_dir(root.path().join(".devbox")).expect("fixture");
    std::fs::write(root.path().join("outside.toml"), "[agent]\nsetup=[]").expect("fixture");
    std::os::unix::fs::symlink("../outside.toml", root.path().join(".devbox/devbox.toml"))
        .expect("fixture");
    assert!(load(root.path()).is_err());
    std::fs::remove_file(root.path().join(".devbox/devbox.toml")).expect("fixture");
    std::fs::remove_dir(root.path().join(".devbox")).expect("fixture");
    std::os::unix::fs::symlink(root.path(), root.path().join(".devbox")).expect("fixture");
    assert!(load(root.path()).is_err());
}

#[test]
fn strict_agent_config_and_missing_default() {
    let config = parse("[agent]\ncopy=['.env']\nsetup=['install']\nteardown=['stop']\ntest='test'")
        .expect("parse fixture");
    assert_eq!(config.agent.copy, [".env"]);
    assert_eq!(config.agent.test.as_deref(), Some("test"));
    assert!(parse("[agent]\nsetpu=[]").is_err());
    assert!(parse("[agnet]\nsetup=[]").is_err());
    assert!(parse("agent=[[],[],[],\"x\"]").is_err());
    for path in [
        "../secret",
        "/etc/passwd",
        "",
        ".",
        "a/../b",
        "C:\\secret",
        "a\\b",
    ] {
        assert!(
            parse(&format!("[agent]\ncopy=[{path:?}]")).is_err(),
            "{path}"
        );
    }
    let root = tempfile::tempdir().expect("fixture");
    assert!(
        load(root.path())
            .expect("missing config")
            .agent
            .copy
            .is_empty()
    );
    std::fs::create_dir(root.path().join(".devbox")).expect("fixture");
    std::fs::write(root.path().join(".devbox/devbox.toml"), "[agent").expect("fixture");
    assert!(load(root.path()).is_err());
}

#[test]
fn copies_nested_files_and_skips_missing_without_overwriting() {
    let root = tempfile::tempdir().expect("fixture");
    let source = root.path().join("source");
    let target = root.path().join("target");
    std::fs::create_dir_all(source.join("nested")).expect("fixture");
    std::fs::create_dir(&target).expect("fixture");
    std::fs::write(source.join("nested/.env"), "secret").expect("fixture");
    copy_files(&source, &target, &["nested/.env".into(), "missing".into()]).expect("copy");
    assert_eq!(
        std::fs::read_to_string(target.join("nested/.env")).expect("read"),
        "secret"
    );
    assert!(copy_files(&source, &target, &["nested/.env".into()]).is_err());
    assert!(copy_files(&source, &target, &["../escape".into()]).is_err());
}

#[cfg(unix)]
#[test]
fn preserves_file_permissions_without_privileged_bits() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().expect("fixture");
    let source = root.path().join("source");
    let target = root.path().join("target");
    std::fs::create_dir(&source).expect("fixture");
    std::fs::create_dir(&target).expect("fixture");
    for mode in [0o600, 0o700, 0o6700] {
        let name = format!("file-{mode}");
        std::fs::write(source.join(&name), "secret").expect("fixture");
        std::fs::set_permissions(source.join(&name), std::fs::Permissions::from_mode(mode))
            .expect("fixture");
        copy_files(&source, &target, std::slice::from_ref(&name)).expect("copy");
        assert_eq!(
            std::fs::metadata(target.join(name))
                .expect("fixture")
                .permissions()
                .mode()
                & 0o7777,
            mode & 0o777
        );
    }
}

#[cfg(unix)]
#[test]
fn rejects_symlink_files_and_ancestors_on_both_sides() {
    let root = tempfile::tempdir().expect("fixture");
    let source = root.path().join("source");
    let target = root.path().join("target");
    std::fs::create_dir_all(source.join("dir")).expect("fixture");
    std::fs::create_dir(&target).expect("fixture");
    std::fs::write(source.join("dir/file"), "x").expect("fixture");
    std::os::unix::fs::symlink("dir", source.join("alias")).expect("fixture");
    std::os::unix::fs::symlink("dir/file", source.join("link")).expect("fixture");
    for path in ["alias/file", "link"] {
        assert!(copy_files(&source, &target, &[path.into()]).is_err());
    }
    std::os::unix::fs::symlink(&source, target.join("dir")).expect("fixture");
    assert!(copy_files(&source, &target, &["dir/file".into()]).is_err());
    std::fs::write(source.join("plain"), "source").expect("fixture");
    std::os::unix::fs::symlink(source.join("plain"), target.join("plain")).expect("fixture");
    assert!(copy_files(&source, &target, &["plain".into()]).is_err());
    std::os::unix::fs::symlink(&source, root.path().join("root-alias")).expect("fixture");
    assert!(
        copy_files(
            &root.path().join("root-alias"),
            &target,
            &["dir/file".into()]
        )
        .is_err()
    );
}
