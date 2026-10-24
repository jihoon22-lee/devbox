mod source_support;
use source_support::*;
#[tokio::test]
async fn config_warning_lists_keys_without_values_or_synthetic_override() {
    let r = repo();
    let d = r.path();
    assert!(
        devbox_git::safety::warnings(d)
            .await
            .expect("owned source result")
            .is_empty()
    );
    for key in [
        "core.fsmonitor",
        "core.hooksPath",
        "filter.owned.clean",
        "diff.external",
    ] {
        git(d, &["config", key, "SECRET-SENTINEL-NEVER-RETURN"]);
    }
    let warnings = devbox_git::safety::warnings(d)
        .await
        .expect("owned source result");
    assert_eq!(warnings.len(), 4);
    assert!(warnings.iter().all(|w| !w.key.contains("SECRET")));
}
#[tokio::test]
async fn cleanup_preserves_user_main_locked_dirty_unmerged_and_unowned() {
    let r = repo();
    let d = r.path();
    git(d, &["branch", "user"]);
    git(d, &["branch", "agent/clean"]);
    assert!(
        devbox_git::cleanup::candidates(d, &[], &[])
            .await
            .expect("owned source result")
            .is_empty()
    );
    let names = vec!["main".into(), "user".into(), "agent/clean".into()];
    let result = devbox_git::cleanup::candidates(d, &[], &names)
        .await
        .expect("owned source result");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].branch, "agent/clean");
    let parent = tempfile::tempdir().expect("create owned temporary directory");
    let tree = parent.path().join("tree");
    git(
        d,
        &[
            "worktree",
            "add",
            "-q",
            tree.to_str().expect("owned UTF-8 path"),
            "agent/clean",
        ],
    );
    let paths = vec![tree.to_string_lossy().into_owned()];
    git(
        d,
        &["worktree", "lock", tree.to_str().expect("owned UTF-8 path")],
    );
    assert!(
        devbox_git::cleanup::candidates(d, &paths, &names)
            .await
            .expect("owned source result")
            .is_empty()
    );
    git(
        d,
        &[
            "worktree",
            "unlock",
            tree.to_str().expect("owned UTF-8 path"),
        ],
    );
    std::fs::write(tree.join("dirty"), "dirty").expect("write owned fixture");
    assert!(
        devbox_git::cleanup::candidates(d, &paths, &names)
            .await
            .expect("owned source result")
            .is_empty()
    );
    git(&tree, &["add", "."]);
    git(&tree, &["commit", "-qm", "unmerged"]);
    assert!(
        devbox_git::cleanup::candidates(d, &paths, &names)
            .await
            .expect("owned source result")
            .is_empty()
    );
}
