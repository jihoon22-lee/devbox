use devbox_git as git;
use std::{path::Path, process::Command};
fn sh(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Owned fixture")
        .env("GIT_COMMITTER_NAME", "Owned fixture")
        .env("GIT_AUTHOR_EMAIL", "owned@example.invalid")
        .env("GIT_COMMITTER_EMAIL", "owned@example.invalid")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_LITERAL_PATHSPECS", "1")
        .output()
        .expect("owned Git");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
async fn case(edited: bool, frozen: bool, reused_directory: bool) {
    let root = tempfile::tempdir().expect("owned repo");
    let dir = root.path();
    sh(dir, &["init", "-q", "-b", "main"]);
    let old = ":(glob)* old\t\n한글\".txt";
    let new = "-new :(glob)*\t\n한글\".txt";
    let contents = (0..40)
        .map(|n| format!("owned stable line {n}\n"))
        .collect::<String>();
    std::fs::write(dir.join(old), &contents).expect("owned source");
    std::fs::write(dir.join("unrelated"), "owned unrelated\n").expect("owned unrelated");
    sh(dir, &["add", "--", old, "unrelated"]);
    sh(dir, &["commit", "-qm", "owned initial"]);
    let base = git::run::resolve(dir, "HEAD").await.expect("base");
    sh(dir, &["mv", "--", old, new]);
    if edited {
        std::fs::write(
            dir.join(new),
            contents.replace("owned stable line 20\n", "owned changed line 20\n"),
        )
        .expect("owned edit");
        sh(dir, &["add", "--", new]);
    }
    if reused_directory {
        std::fs::create_dir(dir.join(old)).expect("owned recreated source directory");
        std::fs::write(
            dir.join(old).join("unrelated"),
            "unapproved source descendant\n",
        )
        .expect("owned descendant");
        sh(dir, &["add", "--", &format!("{old}/unrelated")]);
    }
    let changes = git::changed_files(dir, &base).await.expect("changes");
    let change = changes
        .iter()
        .find(|change| change.path == new)
        .expect("rename");
    assert_eq!(change.old_path.as_deref(), Some(old));
    assert_eq!(change.status, git::ChangeStatus::Renamed);
    let live = git::file_patch(dir, &base, new).await.expect("live patch");
    sh(dir, &["commit", "-qm", "owned rename"]);
    let head = git::run::resolve(dir, "HEAD").await.expect("head");
    std::fs::write(dir.join(new), "unapproved later checkout\n").expect("owned later change");
    std::fs::write(dir.join("unrelated"), "unapproved unrelated\n").expect("owned unrelated edit");
    let snapshot = git::diff::file_patch_at(dir, &base, &head, new)
        .await
        .expect("frozen patch");
    assert_rename(if frozen { &snapshot } else { &live }, edited);
    assert_eq!(snapshot, live);
    assert!(!snapshot.contains("unapproved"));
}
fn assert_rename(patch: &str, edited: bool) {
    assert!(patch.contains("rename from "), "{patch}");
    assert!(patch.contains("rename to "), "{patch}");
    assert!(!patch.contains("new file mode"), "{patch}");
    assert!(!patch.contains("deleted file mode"), "{patch}");
    if edited {
        assert!(patch.contains("-owned stable line 20\n"), "{patch}");
        assert!(patch.contains("+owned changed line 20\n"), "{patch}");
        assert!(
            !patch.contains("+owned stable line 0\n"),
            "unchanged content rendered as additions: {patch}"
        );
    } else {
        assert!(patch.contains("similarity index 100%"), "{patch}");
        assert!(
            !patch.contains("@@"),
            "pure rename has no content delta: {patch}"
        );
    }
}
#[tokio::test]
async fn exact_rename_live_patch_keeps_both_literal_paths() {
    case(false, false, false).await;
}
#[tokio::test]
async fn exact_rename_frozen_patch_keeps_both_literal_paths() {
    case(false, true, false).await;
}
#[tokio::test]
async fn edited_rename_live_patch_shows_only_approved_delta() {
    case(true, false, false).await;
}
#[tokio::test]
async fn edited_rename_frozen_patch_shows_only_approved_delta() {
    case(true, true, false).await;
}
#[tokio::test]
async fn reused_source_directory_live_patch_excludes_added_descendants() {
    case(false, false, true).await;
}
#[tokio::test]
async fn reused_source_directory_frozen_patch_excludes_added_descendants() {
    case(false, true, true).await;
}
async fn related_case(mode: &str) {
    let root = tempfile::tempdir().expect("owned related paths");
    let dir = root.path();
    sh(dir, &["init", "-q", "-b", "main"]);
    let (old, new) = match mode {
        "file-to-descendant" => ("a", "a/b"),
        "descendant-to-file" => ("a/b", "a"),
        _ => ("c", "a"),
    };
    if mode != "file-to-descendant" {
        std::fs::create_dir(dir.join("a")).expect("owned parent");
        std::fs::write(dir.join("a/sibling"), "unrelated source\n").expect("owned sibling");
    }
    std::fs::write(dir.join(old), "selected content\n").expect("owned source");
    sh(dir, &["add", "."]);
    sh(dir, &["commit", "-qm", "owned initial"]);
    let base = git::run::resolve(dir, "HEAD").await.expect("base");
    sh(dir, &["mv", old, "holding"]);
    if mode == "file-to-descendant" {
        std::fs::create_dir(dir.join("a")).expect("owned parent");
        std::fs::write(dir.join("a/sibling"), "unrelated addition\n").expect("owned sibling");
        sh(dir, &["add", "a/sibling"]);
    } else {
        if mode == "other-renamed-source" {
            sh(dir, &["mv", "a/sibling", "elsewhere"]);
        } else {
            sh(dir, &["rm", "-q", "a/sibling"]);
        }
        if dir.join("a").exists() {
            std::fs::remove_dir(dir.join("a")).expect("owned empty parent");
        }
    }
    sh(dir, &["mv", "holding", new]);
    let changes = git::changed_files(dir, &base)
        .await
        .expect("related changes");
    assert_eq!(
        changes
            .iter()
            .find(|change| change.path == new)
            .expect("selected rename")
            .old_path
            .as_deref(),
        Some(old)
    );
    let live = git::file_patch(dir, &base, new)
        .await
        .expect("related live patch");
    assert_rename(&live, false);
    assert_eq!(live.matches("diff --git ").count(), 1);
    assert!(!live.contains("unrelated"));
    assert!(!live.contains("sibling"));
    sh(dir, &["commit", "-qm", "owned related rename"]);
    let head = git::run::resolve(dir, "HEAD").await.expect("head");
    let frozen = git::diff::file_patch_at(dir, &base, &head, new)
        .await
        .expect("related frozen patch");
    assert_eq!(frozen, live);
}
#[tokio::test]
async fn file_to_descendant_rename_preserves_selected_destination() {
    related_case("file-to-descendant").await;
}
#[tokio::test]
async fn descendant_to_file_rename_preserves_selected_source() {
    related_case("descendant-to-file").await;
}
#[tokio::test]
async fn moved_away_descendant_source_is_excluded_from_selected_rename() {
    related_case("other-renamed-source").await;
}
