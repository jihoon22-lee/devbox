//! Select native-generated patch hunks without accepting renderer patch text.
use super::*;
pub use crate::core::hunks::Hunk;
use crate::core::hunks::{parse_file_patch, partial_patch, patch_revision, unsupported_reason};
use std::io::Write;
const FAILED: &str = "hunk_apply_failed";
const PATCH_LIMIT: usize = 2 * 1024 * 1024;
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileHunksRequest {
    pub path: String,
    pub file: String,
    pub staged: bool,
}
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct FileHunks {
    pub file: String,
    pub staged: bool,
    pub supported: bool,
    pub reason: Option<String>,
    pub revision: String,
    pub hunks: Vec<Hunk>,
}
#[derive(Debug, Clone, Copy, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum HunkAction {
    Stage,
    Unstage,
    Discard,
}
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HunksApplyRequest {
    pub path: String,
    pub file: String,
    pub staged: bool,
    pub action: HunkAction,
    pub hunk_ids: Vec<String>,
    pub revision: String,
    pub operation_id: String,
}
struct Observed {
    view: FileHunks,
    patch: String,
}
fn observe(root: &Path, file: &str, staged: bool, cancel: &AtomicBool) -> Result<Observed, String> {
    validate_change_path(file).map_err(|_| FAILED)?;
    source_root_before_io(root).map_err(|_| FAILED)?;
    // Git may follow directories on a diff path: refuse links before invoking it.
    let target_file = root.join(file);
    match devbox_filesystem::ensure_no_links(&target_file) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(_) => {
            return Ok(Observed {
                view: FileHunks {
                    file: file.into(),
                    staged,
                    supported: false,
                    reason: Some("mode_change".into()),
                    revision: String::new(),
                    hunks: vec![],
                },
                patch: String::new(),
            })
        }
    }
    let target = git_target_for_path(root, FAILED)?;
    let mut args = vec![
        "--literal-pathspecs",
        "diff",
        "--no-ext-diff",
        "--no-textconv",
        "--no-color",
        "-U3",
    ];
    if staged {
        args.push("--cached");
    }
    args.extend(["--", file]);
    let (patch, mut reason) = match devbox_git::run_bounded_target_with_cancel(
        &args,
        &target,
        Duration::from_secs(5),
        PATCH_LIMIT,
        cancel,
    ) {
        Ok(patch) => (patch, None),
        Err(error) if error == "git_output_too_large" => (String::new(), Some("too_large")),
        Err(error) if error == "git_output_invalid_utf8" => (String::new(), Some("binary")),
        Err(_) => return Err(FAILED.into()),
    };
    if reason.is_none() {
        reason = unsupported_reason(&patch);
    }
    if reason.is_none() {
        let status = devbox_git::run_bounded_target_with_cancel(
            &[
                "--literal-pathspecs",
                "status",
                "--porcelain",
                "-z",
                "--untracked-files=all",
                "--",
                file,
            ],
            &target,
            Duration::from_secs(5),
            MAX_STATUS_OUTPUT_BYTES,
            cancel,
        )
        .map_err(|_| FAILED)?;
        for entry in parse_status_changes(&status).map_err(|_| FAILED)? {
            if entry.path == file {
                let side = if staged {
                    entry.index_status.as_str()
                } else {
                    entry.worktree_status.as_str()
                };
                if entry.kind == "untracked" || side == "A" {
                    reason = Some("new_file");
                } else if side == "D" {
                    reason = Some("deleted_file");
                } else if matches!(side, "R" | "C") {
                    reason = Some("rename");
                }
            }
        }
    }
    let hunks = if reason.is_none() && !patch.is_empty() {
        parse_file_patch(&patch)?.hunks
    } else {
        vec![]
    };
    let view = FileHunks {
        file: file.into(),
        staged,
        supported: reason.is_none(),
        reason: reason.map(str::to_owned),
        revision: patch_revision(&patch),
        hunks,
    };
    Ok(Observed { view, patch })
}
pub async fn repo_file_hunks(request: FileHunksRequest) -> Result<FileHunks, String> {
    validate_change_path(&request.file).map_err(|_| FAILED)?;
    spawn_git_task(FAILED, move || {
        let context = validated_repository_context(&request.path, FAILED)?;
        revalidate_repository_context(&context, FAILED)?;
        let observed = observe(
            &context.worktree,
            &request.file,
            request.staged,
            &AtomicBool::new(false),
        )?;
        revalidate_repository_context(&context, FAILED)?;
        Ok(observed.view)
    })
    .await
}
struct TemporaryPatch {
    path: PathBuf,
    parent_id: FilesystemIdentity,
    file_id: FilesystemIdentity,
}
impl TemporaryPatch {
    fn create(root: &Path, patch: &str, cancel: &AtomicBool) -> Result<Self, String> {
        let target = git_target_for_path(root, FAILED)?;
        let git = devbox_git::run_bounded_target_with_cancel(
            &["rev-parse", "--absolute-git-dir"],
            &target,
            Duration::from_secs(5),
            MAX_SAFETY_METADATA_OUTPUT_BYTES,
            cancel,
        )
        .map_err(|_| FAILED)?;
        let git = host_path_from_git(root, git.trim(), FAILED)?;
        source_metadata_before_io(root, &git, false).map_err(|_| FAILED)?;
        devbox_filesystem::ensure_no_links(&git).map_err(|_| FAILED)?;
        let parent_id = filesystem_identity(&git, true).map_err(|_| FAILED)?;
        let serial = NEXT_INTERNAL_OPERATION.fetch_add(1, Ordering::Relaxed);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| FAILED)?
            .as_millis();
        let path = git.join(format!(
            "devbox-hunk-{}-{serial}-{now}.patch",
            std::process::id()
        ));
        source_metadata_before_io(root, &path, false).map_err(|_| FAILED)?;
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path).map_err(|_| FAILED)?;
        let file_id = filesystem_identity(&path, false).map_err(|_| FAILED)?;
        let owned = Self {
            path,
            parent_id,
            file_id,
        };
        file.write_all(patch.as_bytes()).map_err(|_| FAILED)?;
        file.flush().map_err(|_| FAILED)?;
        Ok(owned)
    }
}
impl Drop for TemporaryPatch {
    fn drop(&mut self) {
        if self
            .path
            .parent()
            .is_some_and(|parent| filesystem_identity(parent, true).ok() == Some(self.parent_id))
            && filesystem_identity(&self.path, false).ok() == Some(self.file_id)
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}
pub async fn repo_hunks_apply(request: HunksApplyRequest) -> Result<(), String> {
    validate_change_path(&request.file).map_err(|_| FAILED)?;
    if !matches!(
        (request.staged, request.action),
        (false, HunkAction::Stage | HunkAction::Discard) | (true, HunkAction::Unstage)
    ) || request.hunk_ids.is_empty()
        || request.hunk_ids.len() > 200
    {
        return Err("hunk_selection_invalid".into());
    }
    let operation = begin_git_operation(&request.operation_id, FAILED, FAILED)?;
    spawn_git_task(FAILED, move || {
        let mut operation = operation;
        let context = validated_repository_context(&request.path, FAILED)?;
        operation.bind_repository(context.common_git_identity, FAILED, FAILED)?;
        revalidate_repository_context(&context, FAILED)?;
        let cancel = operation.cancellation.as_ref();
        let observed = observe(&context.worktree, &request.file, request.staged, cancel)?;
        if observed.view.revision != request.revision {
            return Err("hunk_stale".into());
        }
        if !observed.view.supported {
            return Err("hunk_unsupported".into());
        }
        let partial = partial_patch(&parse_file_patch(&observed.patch)?, &request.hunk_ids)?;
        let temporary = TemporaryPatch::create(&context.worktree, &partial, cancel)?;
        let path = git_path_from_host(&context.worktree, &temporary.path, FAILED)?;
        revalidate_repository_context(&context, FAILED)?;
        let mut args = vec!["apply".into(), "--whitespace=nowarn".into()];
        if matches!(request.action, HunkAction::Stage | HunkAction::Unstage) {
            args.push("--cached".into());
        }
        if matches!(request.action, HunkAction::Unstage | HunkAction::Discard) {
            args.push("--reverse".into());
        }
        args.push(path);
        run_git_mutation_with_cancel(&args, &context.worktree, cancel).map_err(|_| FAILED.into())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{git, init_repo};
    use std::fs;

    fn block<T>(future: impl std::future::Future<Output = T>) -> T {
        crate::runtime::block_on(future)
    }
    fn path(dir: &std::path::Path) -> String {
        host_path_spelling(&dir.canonicalize().unwrap(), "hunk_apply_failed").unwrap()
    }
    fn lines(n: usize) -> String {
        (1..=n).map(|i| format!("line {i}\n")).collect()
    }

    fn two_hunk_repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        fs::write(tmp.path().join("a.txt"), lines(30)).unwrap();
        git(tmp.path(), &["add", "a.txt"]);
        git(tmp.path(), &["commit", "--quiet", "-m", "thirty lines"]);
        let edited = lines(30)
            .replace("line 2\n", "line two\n")
            .replace("line 28\n", "line twenty-eight\n");
        fs::write(tmp.path().join("a.txt"), edited).unwrap();
        tmp
    }

    #[test]
    fn omitted_earlier_insertions_do_not_shift_the_selected_hunk() {
        let tmp = two_hunk_repo();
        let edited = lines(30)
            .replace("line 2\n", "line 2\ninserted\nextra\n")
            .replace("line 28\n", "changed end\n");
        fs::write(tmp.path().join("a.txt"), &edited).unwrap();
        let view = block(repo_file_hunks(FileHunksRequest {
            path: path(tmp.path()),
            file: "a.txt".into(),
            staged: false,
        }))
        .unwrap();
        assert_eq!(view.hunks.len(), 2);
        block(repo_hunks_apply(HunksApplyRequest {
            path: path(tmp.path()),
            file: "a.txt".into(),
            staged: false,
            action: HunkAction::Stage,
            hunk_ids: vec![view.hunks[1].id.clone()],
            revision: view.revision,
            operation_id: "offset-stage".into(),
        }))
        .unwrap();
        let index = git(tmp.path(), &["show", ":a.txt"]);
        assert!(index.contains("changed end") && !index.contains("inserted"));
        assert_eq!(
            fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
            edited
        );
        git(tmp.path(), &["add", "a.txt"]);
        let view = block(repo_file_hunks(FileHunksRequest {
            path: path(tmp.path()),
            file: "a.txt".into(),
            staged: true,
        }))
        .unwrap();
        block(repo_hunks_apply(HunksApplyRequest {
            path: path(tmp.path()),
            file: "a.txt".into(),
            staged: true,
            action: HunkAction::Unstage,
            hunk_ids: vec![view.hunks[1].id.clone()],
            revision: view.revision,
            operation_id: "offset-unstage".into(),
        }))
        .unwrap();
        let index = git(tmp.path(), &["show", ":a.txt"]);
        assert!(
            index.contains("inserted")
                && index.contains("line 28")
                && !index.contains("changed end")
        );
    }
    #[test]
    fn untracked_binary_and_oversized_patches_are_not_text_hunks() {
        let tmp = two_hunk_repo();
        let read = |file: &str| {
            block(repo_file_hunks(FileHunksRequest {
                path: path(tmp.path()),
                file: file.into(),
                staged: false,
            }))
            .unwrap()
        };
        fs::write(tmp.path().join("new.txt"), "new").unwrap();
        assert_eq!(read("new.txt").reason.as_deref(), Some("new_file"));
        fs::write(tmp.path().join("a.txt"), b"binary\0data").unwrap();
        assert_eq!(read("a.txt").reason.as_deref(), Some("binary"));
        fs::write(tmp.path().join("a.txt"), "x".repeat(PATCH_LIMIT + 1024)).unwrap();
        assert_eq!(read("a.txt").reason.as_deref(), Some("too_large"));
    }
    #[test]
    fn temporary_patch_is_removed_after_git_rejects_it() {
        let tmp = two_hunk_repo();
        let cancelled = AtomicBool::new(false);
        let patch = TemporaryPatch::create(tmp.path(), "invalid patch\n", &cancelled).unwrap();
        let temporary = patch.path.clone();
        let args = vec![
            "apply".into(),
            "--cached".into(),
            git_path_from_host(tmp.path(), &temporary, FAILED).unwrap(),
        ];
        assert!(run_git_mutation_with_cancel(&args, tmp.path(), &cancelled).is_err());
        drop(patch);
        assert!(!temporary.exists());
    }

    #[test]
    fn staging_one_hunk_leaves_the_other_in_the_worktree() {
        let tmp = two_hunk_repo();
        let view = block(repo_file_hunks(FileHunksRequest {
            path: path(tmp.path()),
            file: "a.txt".into(),
            staged: false,
        }))
        .unwrap();
        assert!(view.supported);
        assert_eq!(view.hunks.len(), 2);
        block(repo_hunks_apply(HunksApplyRequest {
            path: path(tmp.path()),
            file: "a.txt".into(),
            staged: false,
            action: HunkAction::Stage,
            hunk_ids: vec![view.hunks[1].id.clone()],
            revision: view.revision.clone(),
            operation_id: "h1".into(),
        }))
        .unwrap();
        let cached = git(tmp.path(), &["diff", "--cached"]);
        assert!(cached.contains("+line twenty-eight") && !cached.contains("+line two"));
        assert!(git(tmp.path(), &["diff"]).contains("+line two"));
    }

    #[test]
    fn unstaging_and_discarding_hunks() {
        let tmp = two_hunk_repo();
        git(tmp.path(), &["add", "a.txt"]);
        let staged = block(repo_file_hunks(FileHunksRequest {
            path: path(tmp.path()),
            file: "a.txt".into(),
            staged: true,
        }))
        .unwrap();
        block(repo_hunks_apply(HunksApplyRequest {
            path: path(tmp.path()),
            file: "a.txt".into(),
            staged: true,
            action: HunkAction::Unstage,
            hunk_ids: vec![staged.hunks[0].id.clone()],
            revision: staged.revision.clone(),
            operation_id: "h2".into(),
        }))
        .unwrap();
        assert!(!git(tmp.path(), &["diff", "--cached"]).contains("+line two"));
        let worktree = block(repo_file_hunks(FileHunksRequest {
            path: path(tmp.path()),
            file: "a.txt".into(),
            staged: false,
        }))
        .unwrap();
        block(repo_hunks_apply(HunksApplyRequest {
            path: path(tmp.path()),
            file: "a.txt".into(),
            staged: false,
            action: HunkAction::Discard,
            hunk_ids: vec![worktree.hunks[0].id.clone()],
            revision: worktree.revision.clone(),
            operation_id: "h3".into(),
        }))
        .unwrap();
        assert!(fs::read_to_string(tmp.path().join("a.txt"))
            .unwrap()
            .contains("line 2\n"));
        let git_dir = tmp.path().join(".git");
        assert!(
            fs::read_dir(&git_dir).unwrap().all(|e| !e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("devbox-hunk-")),
            "temporary patches are removed"
        );
    }

    #[test]
    fn a_changed_file_makes_the_selection_stale() {
        let tmp = two_hunk_repo();
        let view = block(repo_file_hunks(FileHunksRequest {
            path: path(tmp.path()),
            file: "a.txt".into(),
            staged: false,
        }))
        .unwrap();
        fs::write(tmp.path().join("a.txt"), lines(31)).unwrap();
        let error = block(repo_hunks_apply(HunksApplyRequest {
            path: path(tmp.path()),
            file: "a.txt".into(),
            staged: false,
            action: HunkAction::Stage,
            hunk_ids: vec![view.hunks[0].id.clone()],
            revision: view.revision,
            operation_id: "h4".into(),
        }))
        .unwrap_err();
        assert_eq!(error, "hunk_stale");
        assert!(git(tmp.path(), &["diff", "--cached"]).is_empty());
    }

    #[test]
    fn actions_must_match_the_side_and_file_kind() {
        let tmp = two_hunk_repo();
        let view = block(repo_file_hunks(FileHunksRequest {
            path: path(tmp.path()),
            file: "a.txt".into(),
            staged: false,
        }))
        .unwrap();
        let wrong = block(repo_hunks_apply(HunksApplyRequest {
            path: path(tmp.path()),
            file: "a.txt".into(),
            staged: false,
            action: HunkAction::Unstage,
            hunk_ids: vec![view.hunks[0].id.clone()],
            revision: view.revision,
            operation_id: "h5".into(),
        }))
        .unwrap_err();
        assert_eq!(wrong, "hunk_selection_invalid");
        fs::write(tmp.path().join("new.txt"), "new\n").unwrap();
        git(tmp.path(), &["add", "-N", "new.txt"]);
        let new_file = block(repo_file_hunks(FileHunksRequest {
            path: path(tmp.path()),
            file: "new.txt".into(),
            staged: false,
        }))
        .unwrap();
        assert_eq!(
            (new_file.supported, new_file.reason.as_deref()),
            (false, Some("new_file"))
        );
    }
}
