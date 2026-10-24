mod source_support;
use devbox_git::{model::SourceGitError, remote};
use source_support::*;
fn remote_repo() -> (tempfile::TempDir, tempfile::TempDir) {
    let r = repo();
    let bare = tempfile::tempdir().expect("create owned temporary directory");
    git(bare.path(), &["init", "--bare", "-q"]);
    git(
        r.path(),
        &[
            "remote",
            "add",
            "origin",
            bare.path().to_str().expect("owned UTF-8 path"),
        ],
    );
    git(r.path(), &["push", "-qu", "origin", "main"]);
    (r, bare)
}
#[tokio::test]
async fn ar_f01_external_checkout_changes_no_remote_refs() {
    let (r, b) = remote_repo();
    let d = r.path();
    let expect = remote::state(d).await.expect("read owned remote state");
    let before = git(b.path(), &["show-ref"]);
    git(d, &["checkout", "-qb", "other"]);
    std::fs::write(d.join("other"), "other").expect("write owned fixture");
    git(d, &["add", "."]);
    git(d, &["commit", "-qm", "other"]);
    assert!(matches!(
        remote::push(d, &expect, false).await,
        Err(SourceGitError::StateChanged { .. })
    ));
    assert_eq!(git(b.path(), &["show-ref"]), before);
    git(d, &["checkout", "-q", "main"]);
    std::fs::write(d.join("positive"), "positive").expect("write owned fixture");
    git(d, &["add", "."]);
    git(d, &["commit", "-qm", "positive"]);
    let expect = remote::state(d).await.expect("read owned remote state");
    remote::push(d, &expect, false)
        .await
        .expect("push owned remote");
    assert_eq!(
        git(b.path(), &["rev-parse", "refs/heads/main"]).trim(),
        expect.head_oid.expect("owned source result")
    );
}
#[tokio::test]
async fn push_force_with_lease_refuses_when_remote_moved() {
    let (r, b) = remote_repo();
    let expect = remote::state(r.path())
        .await
        .expect("read owned remote state");
    let other = tempfile::tempdir().expect("create owned temporary directory");
    git(
        other.path(),
        &[
            "clone",
            "-q",
            "-b",
            "main",
            b.path().to_str().expect("owned UTF-8 path"),
            "clone",
        ],
    );
    let d = other.path().join("clone");
    std::fs::write(d.join("external"), "external").expect("write owned fixture");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-qm", "external"]);
    git(&d, &["push", "-q", "origin", "main"]);
    let before = git(b.path(), &["show-ref"]);
    assert!(remote::push(r.path(), &expect, true).await.is_err());
    assert_eq!(git(b.path(), &["show-ref"]), before);
}
#[tokio::test]
async fn pull_rejects_changed_head_or_upstream_before_fetch() {
    let (r, _b) = remote_repo();
    let d = r.path();
    let expect = remote::state(d).await.expect("read owned remote state");
    git(d, &["commit", "--allow-empty", "-qm", "head moved"]);
    assert!(matches!(
        remote::pull(d, &expect, false).await,
        Err(SourceGitError::StateChanged { .. })
    ));
    assert!(!d.join(".git/FETCH_HEAD").exists());
    let expect = remote::state(d).await.expect("read owned remote state");
    git(d, &["config", "branch.main.merge", "refs/heads/other"]);
    assert!(matches!(
        remote::pull(d, &expect, true).await,
        Err(SourceGitError::StateChanged { .. })
    ));
    assert!(!d.join(".git/FETCH_HEAD").exists());
}
#[tokio::test]
async fn pull_positive_control_fetches_then_applies_only_to_confirmed_branch() {
    let (r, b) = remote_repo();
    let other = tempfile::tempdir().expect("create owned temporary directory");
    git(
        other.path(),
        &[
            "clone",
            "-q",
            "-b",
            "main",
            b.path().to_str().expect("owned UTF-8 path"),
            "clone",
        ],
    );
    let d = other.path().join("clone");
    std::fs::write(d.join("external"), "external").expect("write owned fixture");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-qm", "external"]);
    git(&d, &["push", "-q", "origin", "main"]);
    let expect = remote::state(r.path())
        .await
        .expect("read owned remote state");
    remote::pull(r.path(), &expect, false)
        .await
        .expect("pull owned remote");
    assert_eq!(
        git(r.path(), &["rev-parse", "HEAD"]),
        git(b.path(), &["rev-parse", "refs/heads/main"])
    );
    assert!(r.path().join("external").exists());
}
#[cfg(unix)]
#[tokio::test]
async fn push_uses_confirmed_oid_even_if_hook_switches_checkout() {
    use std::os::unix::fs::PermissionsExt;
    let (r, b) = remote_repo();
    let d = r.path();
    git(d, &["checkout", "-qb", "other"]);
    git(d, &["commit", "--allow-empty", "-qm", "other"]);
    let other = git(d, &["rev-parse", "HEAD"]);
    git(d, &["checkout", "-q", "main"]);
    git(d, &["commit", "--allow-empty", "-qm", "confirmed"]);
    let expected = remote::state(d).await.expect("read owned remote state");
    let hook = d.join(".git/hooks/pre-push");
    std::fs::write(
        &hook,
        "#!/bin/sh\ncat > .git/owned-push-input\ngit checkout -q other\n",
    )
    .expect("write owned fixture");
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o700))
        .expect("set owned fixture permissions");
    remote::push(d, &expected, false)
        .await
        .expect("push owned remote");
    let refs = git(b.path(), &["show-ref"]);
    assert_eq!(refs.lines().count(), 1);
    assert_eq!(
        git(b.path(), &["rev-parse", "refs/heads/main"]).trim(),
        expected.head_oid.as_deref().expect("owned source result")
    );
    assert_ne!(git(b.path(), &["rev-parse", "refs/heads/main"]), other);
    let input =
        std::fs::read_to_string(d.join(".git/owned-push-input")).expect("read owned fixture text");
    assert!(input.starts_with(expected.head_oid.as_deref().expect("owned source result")));
    assert!(input.contains(" refs/heads/main "));
}
#[tokio::test]
async fn differently_named_upstream_changes_only_the_confirmed_remote_branch() {
    let (r, b) = remote_repo();
    let d = r.path();
    git(b.path(), &["branch", "preserved", "main"]);
    let preserved = git(b.path(), &["rev-parse", "refs/heads/preserved"]);
    git(d, &["branch", "-m", "feature"]);
    git(d, &["commit", "--allow-empty", "-qm", "feature update"]);
    let expected = remote::state(d).await.expect("read owned remote state");
    assert_eq!(expected.branch.as_deref(), Some("feature"));
    assert_eq!(
        expected
            .upstream
            .as_ref()
            .expect("owned optional value")
            .destination_ref,
        "refs/heads/main"
    );
    remote::push(d, &expected, false)
        .await
        .expect("push owned remote");
    let refs = git(b.path(), &["show-ref"]);
    assert_eq!(refs.lines().count(), 2);
    assert!(!refs.contains("refs/heads/feature"));
    assert_eq!(
        git(b.path(), &["rev-parse", "refs/heads/preserved"]),
        preserved
    );
    assert_eq!(
        git(b.path(), &["rev-parse", "refs/heads/main"]).trim(),
        expected.head_oid.expect("owned source result")
    );
}
#[tokio::test]
async fn malformed_destination_is_rejected_before_starting_remote_work() {
    let (r, _b) = remote_repo();
    git(
        r.path(),
        &["config", "branch.main.merge", "refs/heads/not..valid"],
    );
    let expected = remote::state(r.path())
        .await
        .expect("read owned remote state");
    assert!(matches!(
        remote::preflight(r.path(), &expected).await,
        Err(SourceGitError::InvalidSourceInput)
    ));
}
#[tokio::test]
async fn configured_follow_tags_cannot_publish_unconfirmed_refs() {
    let (r, b) = remote_repo();
    let d = r.path();
    git(d, &["commit", "--allow-empty", "-qm", "approved"]);
    git(d, &["tag", "-a", "unapproved-tag", "-m", "owned tag"]);
    git(d, &["config", "push.followTags", "true"]);
    let expected = remote::state(d).await.expect("read owned remote state");
    remote::push(d, &expected, false)
        .await
        .expect("push owned remote");
    let refs = git(b.path(), &["show-ref"]);
    assert_eq!(refs.lines().count(), 1, "{refs}");
    assert!(!refs.contains("refs/tags/"));
}
