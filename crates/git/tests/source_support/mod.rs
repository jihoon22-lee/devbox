#![allow(dead_code)]
use std::{path::Path, process::Command};
pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Original Author")
        .env("GIT_AUTHOR_EMAIL", "author@example.invalid")
        .env("GIT_COMMITTER_NAME", "Owned fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .output()
        .expect("run owned Git command");
    assert!(
        out.status.success(),
        "{:?}: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("decode owned Git output")
}
pub fn repo() -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("create owned temporary directory");
    git(temp.path(), &["init", "-q", "-b", "main"]);
    git(temp.path(), &["config", "user.name", "Owned fixture"]);
    git(
        temp.path(),
        &["config", "user.email", "fixture@example.invalid"],
    );
    let text = (0..40).map(|i| format!("line {i}\n")).collect::<String>();
    std::fs::write(temp.path().join("file.txt"), text).expect("write owned fixture");
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-qm", "initial"]);
    temp
}
pub fn edit(dir: &Path) {
    let text = std::fs::read_to_string(dir.join("file.txt")).expect("read owned fixture text");
    std::fs::write(
        dir.join("file.txt"),
        text.replace("line 2\n", "first change\n")
            .replace("line 32\n", "second change\n"),
    )
    .expect("write owned fixture");
}
