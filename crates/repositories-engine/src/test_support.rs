use std::{fs, path::Path, process::Command};
pub(crate) fn git(repo: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

pub(crate) fn init_repo(dir: &Path) {
    git(dir, &["init", "--quiet", "-b", "main"]);
    for (key, value) in [
        ("user.email", "fixture@example.test"),
        ("user.name", "Fixture"),
        ("core.autocrlf", "false"),
    ] {
        git(dir, &["config", key, value]);
    }
    fs::write(dir.join("README.md"), "base\n").unwrap();
    git(dir, &["add", "README.md"]);
    git(dir, &["commit", "--quiet", "-m", "base"]);
}
#[test]
fn initializes_a_clean_fixture_with_a_commit() {
    let root = tempfile::tempdir().unwrap();
    init_repo(root.path());
    assert!(git(root.path(), &["status", "--porcelain"]).is_empty());
    assert_eq!(
        git(root.path(), &["branch", "--show-current"]).trim(),
        "main"
    );
}

pub(crate) fn repo_with_agent_branch(
    file_on_agent: &str,
    file_on_main: Option<&str>,
) -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    // Use the same canonical DOS spelling as native Registry admission;
    // Windows temporary directories may otherwise use an 8.3 alias.
    let root = std::path::PathBuf::from(
        crate::commands::host_path_spelling(
            &tmp.path().canonicalize().unwrap(),
            "worktree_not_agent",
        )
        .unwrap(),
    );
    let main = root.join("devbox");
    fs::create_dir(&main).unwrap();
    git(&main, &["init", "--quiet", "-b", "main"]);
    for (key, value) in [
        ("user.email", "hub@example.test"),
        ("user.name", "Hub"),
        ("core.autocrlf", "false"),
    ] {
        git(&main, &["config", key, value]);
    }
    fs::write(main.join("shared.txt"), "base\n").unwrap();
    git(&main, &["add", "shared.txt"]);
    git(&main, &["commit", "--quiet", "-m", "base"]);
    let agent = root.join("devbox-fix");
    git(
        &main,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "agent/fix",
            agent.to_str().unwrap(),
        ],
    );
    fs::write(agent.join("shared.txt"), file_on_agent).unwrap();
    git(&agent, &["commit", "--quiet", "-am", "agent change"]);
    if let Some(content) = file_on_main {
        fs::write(main.join("shared.txt"), content).unwrap();
        git(&main, &["commit", "--quiet", "-am", "main change"]);
    }
    (tmp, main, agent)
}
