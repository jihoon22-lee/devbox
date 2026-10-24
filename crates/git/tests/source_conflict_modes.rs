#![cfg(target_os = "linux")]
mod source_support;
use devbox_git::{
    conflict_model::ConflictApply,
    conflicts::{apply, preview},
    model::{SourceGitError, SourceTarget},
};
use source_support::*;
use std::{os::unix::fs::PermissionsExt, path::Path, process::Command};
fn write_mode(d: &Path, text: &str, mode: u32) {
    std::fs::write(d.join("file.txt"), text).expect("write owned fixture");
    std::fs::set_permissions(d.join("file.txt"), std::fs::Permissions::from_mode(mode))
        .expect("set owned fixture permissions");
}
fn fixture(base: u32, ours: Option<u32>, theirs: u32) -> tempfile::TempDir {
    let r = repo();
    let d = r.path();
    write_mode(d, "base\n", base);
    git(d, &["commit", "-am", "base mode"]);
    git(d, &["checkout", "-b", "side"]);
    write_mode(d, "theirs\n", theirs);
    git(d, &["commit", "-am", "theirs"]);
    git(d, &["checkout", "main"]);
    if let Some(mode) = ours {
        write_mode(d, "ours\n", mode);
    } else {
        git(d, &["rm", "file.txt"]);
    }
    git(d, &["commit", "-am", "ours"]);
    assert!(
        !Command::new("git")
            .arg("-C")
            .arg(d)
            .args(["merge", "side"])
            .output()
            .expect("run owned Git command")
            .status
            .success()
    );
    std::fs::remove_file(d.join("file.txt")).expect("remove owned fixture file");
    r
}
async fn request(d: &Path, resolution: Option<&str>) -> ConflictApply {
    let target = SourceTarget {
        project_id: "fixture".into(),
        worktree_id: None,
    };
    ConflictApply {
        token: preview(d, &target, "file.txt")
            .await
            .expect("preview owned source")
            .token,
        target,
        path: "file.txt".into(),
        resolution: resolution.map(str::to_owned),
    }
}
fn assert_mode(d: &Path, mode: u32, index_mode: &str) {
    assert_eq!(
        std::fs::metadata(d.join("file.txt"))
            .expect("inspect owned fixture metadata")
            .permissions()
            .mode()
            & 0o777,
        mode
    );
    assert!(git(d, &["ls-files", "--stage", "file.txt"]).starts_with(index_mode));
    assert_eq!(git(d, &["show", ":file.txt"]), "resolved\n");
}
#[tokio::test]
async fn absent_executable_conflict_preserves_consistent_stage_modes() {
    let r = fixture(0o755, Some(0o755), 0o755);
    let d = r.path();
    assert!(
        git(d, &["ls-files", "--unmerged"])
            .lines()
            .all(|l| l.starts_with("100755"))
    );
    apply(d, &request(d, Some("resolved\n")).await)
        .await
        .expect("apply owned resolution");
    assert_mode(d, 0o755, "100755");
}
#[tokio::test]
async fn absent_delete_modify_preserves_surviving_executable_mode() {
    let r = fixture(0o755, None, 0o755);
    let d = r.path();
    let p = request(d, Some("resolved\n")).await;
    assert!(p.token.stage_oids[1].is_none());
    apply(d, &p).await.expect("apply owned resolution");
    assert_mode(d, 0o755, "100755");
}
#[tokio::test]
async fn absent_regular_conflict_remains_regular() {
    let r = fixture(0o644, Some(0o644), 0o644);
    let d = r.path();
    apply(d, &request(d, Some("resolved\n")).await)
        .await
        .expect("apply owned resolution");
    assert_mode(d, 0o644, "100644");
}
#[tokio::test]
async fn explicit_deletion_does_not_require_agreement_on_stage_modes() {
    for modes in [(0o755, Some(0o755), 0o755), (0o644, Some(0o755), 0o644)] {
        let r = fixture(modes.0, modes.1, modes.2);
        let d = r.path();
        apply(d, &request(d, None).await)
            .await
            .expect("apply owned resolution");
        assert!(!d.join("file.txt").exists());
        assert!(git(d, &["ls-files", "--stage", "file.txt"]).is_empty());
    }
}
#[tokio::test]
async fn absent_mixed_stage_modes_refuse_text_without_changing_index() {
    for modes in [
        (0o644, Some(0o755), 0o644),
        (0o644, Some(0o755), 0o755),
        (0o644, None, 0o755),
    ] {
        let r = fixture(modes.0, modes.1, modes.2);
        let d = r.path();
        let p = request(d, Some("resolved\n")).await;
        let index = std::fs::read(d.join(".git/index")).expect("read owned fixture");
        assert!(matches!(
            apply(d, &p).await,
            Err(SourceGitError::InvalidSourceInput)
        ));
        assert!(!d.join("file.txt").exists());
        assert_eq!(
            std::fs::read(d.join(".git/index")).expect("read owned fixture"),
            index
        );
    }
}
#[tokio::test]
async fn existing_worktree_mode_wins_over_mixed_stage_modes() {
    let r = fixture(0o644, Some(0o755), 0o644);
    let d = r.path();
    write_mode(d, "working resolution", 0o750);
    apply(d, &request(d, Some("resolved\n")).await)
        .await
        .expect("apply owned resolution");
    assert_mode(d, 0o750, "100755");
}
#[tokio::test]
async fn stage_mode_changed_after_preview_is_stale_and_preserved() {
    use std::io::Write;
    let r = fixture(0o755, Some(0o755), 0o755);
    let d = r.path();
    let p = request(d, Some("resolved\n")).await;
    let oid = p.token.stage_oids[1]
        .as_ref()
        .expect("owned optional value");
    let mut child = Command::new("git")
        .arg("-C")
        .arg(d)
        .args(["update-index", "--index-info"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("create owned fixture");
    writeln!(
        child.stdin.take().expect("owned fixture value"),
        "100644 {oid} 2\tfile.txt"
    )
    .expect("owned fixture value");
    assert!(child.wait().expect("wait for owned child").success());
    let index = std::fs::read(d.join(".git/index")).expect("read owned fixture");
    assert!(matches!(
        apply(d, &p).await,
        Err(SourceGitError::ConflictStale { .. })
    ));
    assert!(!d.join("file.txt").exists());
    assert_eq!(
        std::fs::read(d.join(".git/index")).expect("read owned fixture"),
        index
    );
}
#[tokio::test]
async fn worktree_mode_changed_after_preview_is_stale_and_preserved() {
    let r = fixture(0o755, Some(0o755), 0o755);
    let d = r.path();
    write_mode(d, "working resolution", 0o750);
    let p = request(d, Some("resolved\n")).await;
    std::fs::set_permissions(d.join("file.txt"), std::fs::Permissions::from_mode(0o640))
        .expect("set owned fixture permissions");
    let index = std::fs::read(d.join(".git/index")).expect("read owned fixture");
    assert!(matches!(
        apply(d, &p).await,
        Err(SourceGitError::ConflictStale { .. })
    ));
    assert_eq!(
        std::fs::metadata(d.join("file.txt"))
            .expect("inspect owned fixture metadata")
            .permissions()
            .mode()
            & 0o777,
        0o640
    );
    assert_eq!(
        std::fs::read_to_string(d.join("file.txt")).expect("read owned fixture text"),
        "working resolution"
    );
    assert_eq!(
        std::fs::read(d.join(".git/index")).expect("read owned fixture"),
        index
    );
}
