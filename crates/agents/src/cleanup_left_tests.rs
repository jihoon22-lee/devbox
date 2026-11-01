//! What a closed task's cleanup that stopped leaves behind, and how the cleanup list offers it again.
use super::{teardown_harness::*, *};

/// The folder went, then the branch deletion failed: another Git process held the branch's ref
/// lock. The branch is still at the closed head, so the list offers it alone and cleanup removes
/// it. The lock is the Git wrapper's refusal, so this holds whatever the ref backend.
#[test]
fn the_branch_a_failed_deletion_left_is_listed_and_removable() {
    cases(
        "cleanup_left_tests::the_branch_a_failed_deletion_left_is_listed_and_removable",
        &["ref-lock-files", "ref-lock-reftable"],
        case,
    );
}

async fn case(mode: String) {
    let mode = mode.as_str();
    let (f, task, wt) = action_fixture().await;
    match mode {
        "ref-lock-files" | "ref-lock-reftable" => {
            let format = devbox_git::run::git(&f.repo, &["rev-parse", "--show-ref-format"])
                .await
                .expect("ref backend");
            assert_eq!(format.trim(), ref_format());
            let closed = close(&f, &task, MergeKind::Squash).await;
            // Automatic cleanup removed the folder, then its branch deletion was refused once.
            assert_eq!(receipt("locked"), ["refused"]);
            assert!(!wt.exists(), "the folder went first");
            assert!(closed.cleanup_error.is_some(), "{closed:?}");
            assert!(has_branch(&f).await);
            let options = listed(&f).await;
            assert!(options.worktrees.is_empty(), "{options:?}");
            assert_eq!(options.branches, ["agent/review"]);
            assert_eq!(options.off_base, ["agent/review"]);
            f.svc
                .cleanup(offered(&f, &options))
                .await
                .expect("branch cleanup");
            assert!(!has_branch(&f).await);
            assert_eq!(reservation(&f, &task).await, Resources::Planned);
            assert_eq!(f.svc.get(&task.id).await.expect("task").cleanup_error, None);
        }
        other => panic!("unknown mode {other}"),
    }
}
