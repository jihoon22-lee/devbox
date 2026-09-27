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
