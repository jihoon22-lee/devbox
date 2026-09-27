//! Native conflict resolution under the shared repository operation lock.
use super::*;
use crate::core::conflicts::{has_conflict_markers, operation_from_markers, parse_unmerged};
pub use crate::core::conflicts::{ConflictFile, ConflictOperation};
use std::io::Read;
const FAILED: &str = "conflict_operation_failed";
const PATH_INVALID: &str = "conflict_path_invalid";
const LIMIT: usize = 1024 * 1024;
#[derive(Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ConflictState {
    pub operation: Option<ConflictOperation>,
    pub files: Vec<ConflictFile>,
}
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConflictFileRequest {
    pub path: String,
    pub file: String,
}
#[derive(Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ConflictVersions {
    pub base: Option<String>,
    pub ours: Option<String>,
    pub theirs: Option<String>,
    pub current: Option<String>,
    pub binary: bool,
}
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Resolution {
    Ours,
    Theirs,
    Delete,
    Content { text: String },
}
impl Resolution {
    fn choice(&self) -> &'static str {
        match self {
            Self::Ours => "ours",
            Self::Theirs => "theirs",
            Self::Delete => "delete",
            Self::Content { .. } => "content",
        }
    }
}
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConflictResolveRequest {
    pub path: String,
    pub file: String,
    pub resolution: Resolution,
    pub operation_id: String,
}
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationRequest {
    pub path: String,
    pub operation_id: String,
}
fn read(root: &Path, args: &[&str], cancel: &AtomicBool, limit: usize) -> Result<String, String> {
    let target = git_target_for_path(root, FAILED)?;
    devbox_git::run_bounded_target_with_cancel(args, &target, Duration::from_secs(5), limit, cancel)
}
fn mutate(root: &Path, args: &[&str], cancel: &AtomicBool) -> Result<(), String> {
    run_git_mutation_with_cancel(
        &args.iter().map(|s| (*s).into()).collect::<Vec<_>>(),
        root,
        cancel,
    )
    .map_err(|_| FAILED.into())
}
fn state(context: &RepositoryContext, cancel: &AtomicBool) -> Result<ConflictState, String> {
    revalidate_repository_context(context, FAILED)?;
    let output = read(
        &context.worktree,
        &[
            "--no-optional-locks",
            "-c",
            "core.fsmonitor=false",
            "status",
            "--porcelain=v2",
            "-z",
            "--untracked-files=no",
        ],
        cancel,
        MAX_STATUS_OUTPUT_BYTES,
    )
    .map_err(|_| FAILED)?;
    let files = parse_unmerged(&output)?;
    let mut markers = vec![];
    for marker in [
        "MERGE_HEAD",
        "rebase-merge",
        "rebase-apply",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
    ] {
        if remote_marker_exists(&context.worktree, marker, Some(cancel)).map_err(|_| FAILED)? {
            markers.push(marker);
        }
    }
    Ok(ConflictState {
        operation: operation_from_markers(&markers),
        files,
    })
}
fn file_path(context: &RepositoryContext, file: &str) -> Result<PathBuf, String> {
    validate_change_path(file).map_err(|_| PATH_INVALID)?;
    revalidate_repository_context(context, PATH_INVALID)?;
    source_root_before_io(&context.worktree).map_err(|_| PATH_INVALID)?;
    let target = context.worktree.join(file);
    match devbox_filesystem::ensure_no_links(&target) {
        Ok(()) => {
            if !target.is_file() {
                return Err(PATH_INVALID.into());
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(_) => return Err(PATH_INVALID.into()),
    }
    Ok(target)
}
fn versions(
    context: &RepositoryContext,
    file: &str,
    cancel: &AtomicBool,
) -> Result<ConflictVersions, String> {
    let target = file_path(context, file)?;
    let entries = read(
        &context.worktree,
        &[
            "--literal-pathspecs",
            "ls-files",
            "--unmerged",
            "-z",
            "--",
            file,
        ],
        cancel,
        16 * 1024,
    )
    .map_err(|_| FAILED)?;
    if entries.is_empty() {
        return Err("conflict_choice_invalid".into());
    }
    let mut texts: [Option<String>; 4] = [None, None, None, None];
    let mut binary = false;
    for entry in entries.split_terminator('\0') {
        let (metadata, name) = entry.split_once('\t').ok_or(FAILED)?;
        let fields = metadata.split_whitespace().collect::<Vec<_>>();
        if fields.len() != 3 || name != file {
            return Err(FAILED.into());
        }
        let stage: usize = fields[2].parse().map_err(|_| FAILED)?;
        if !(1..=3).contains(&stage) {
            return Err(FAILED.into());
        }
        if !matches!(fields[0], "100644" | "100755") {
            binary = true;
            continue;
        }
        let spec = format!(":{stage}:{file}");
        match read(
            &context.worktree,
            &[
                "--no-pager",
                "show",
                "--no-ext-diff",
                "--no-textconv",
                &spec,
            ],
            cancel,
            LIMIT,
        ) {
            Ok(text) if !text.contains('\0') => texts[stage - 1] = Some(text),
            Ok(_) => binary = true,
            Err(error)
                if matches!(
                    error.as_str(),
                    "git_output_too_large" | "git_output_invalid_utf8"
                ) =>
            {
                binary = true
            }
            Err(_) => return Err(FAILED.into()),
        }
    }
    match fs::File::open(&target) {
        Ok(file) => {
            if !file.metadata().map_err(|_| PATH_INVALID)?.is_file() {
                return Err(PATH_INVALID.into());
            }
            let before = filesystem_identity(&target, false).map_err(|_| PATH_INVALID)?;
            let mut bytes = Vec::new();
            file.take((LIMIT + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|_| FAILED)?;
            file_path(
                context,
                target
                    .strip_prefix(&context.worktree)
                    .map_err(|_| PATH_INVALID)?
                    .to_str()
                    .ok_or(PATH_INVALID)?,
            )?;
            if filesystem_identity(&target, false).map_err(|_| PATH_INVALID)? != before {
                return Err(PATH_INVALID.into());
            }
            if bytes.len() > LIMIT || bytes.contains(&0) {
                binary = true;
            } else {
                match String::from_utf8(bytes) {
                    Ok(text) => texts[3] = Some(text),
                    Err(_) => binary = true,
                }
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(_) => return Err(PATH_INVALID.into()),
    }
    revalidate_repository_context(context, FAILED)?;
    let [base, ours, theirs, current] = if binary {
        [None, None, None, None]
    } else {
        texts
    };
    Ok(ConflictVersions {
        base,
        ours,
        theirs,
        current,
        binary,
    })
}
pub async fn repo_conflicts(request: PathRequest) -> Result<ConflictState, String> {
    spawn_git_task(FAILED, move || {
        let context = validated_repository_context(&request.path, FAILED)?;
        state(&context, &AtomicBool::new(false))
    })
    .await
}
pub async fn repo_conflict_versions(
    request: ConflictFileRequest,
) -> Result<ConflictVersions, String> {
    validate_change_path(&request.file).map_err(|_| PATH_INVALID)?;
    spawn_git_task(FAILED, move || {
        let context = validated_repository_context(&request.path, FAILED)?;
        versions(&context, &request.file, &AtomicBool::new(false))
    })
    .await
}
pub async fn repo_conflict_resolve(request: ConflictResolveRequest) -> Result<(), String> {
    validate_change_path(&request.file).map_err(|_| PATH_INVALID)?;
    if let Resolution::Content { text } = &request.resolution {
        if text.len() > LIMIT || text.contains('\0') {
            return Err("conflict_choice_invalid".into());
        }
        if has_conflict_markers(text) {
            return Err("conflict_markers_left".into());
        }
    }
    let operation = begin_git_operation(&request.operation_id, FAILED, FAILED)?;
    spawn_git_task(FAILED, move || {
        let mut operation = operation;
        let context = validated_repository_context(&request.path, FAILED)?;
        operation.bind_repository(context.common_git_identity, FAILED, FAILED)?;
        let cancel = operation.cancellation.as_ref();
        let current = state(&context, cancel)?;
        let selected = current
            .files
            .iter()
            .find(|entry| entry.path == request.file)
            .ok_or("conflict_choice_invalid")?;
        if !selected
            .kind
            .choices()
            .contains(&request.resolution.choice())
        {
            return Err("conflict_choice_invalid".into());
        }
        let target = file_path(&context, &request.file)?;
        match &request.resolution {
            Resolution::Ours | Resolution::Theirs => {
                // Do not materialize a staged symlink into the worktree.
                let entries = read(
                    &context.worktree,
                    &[
                        "--literal-pathspecs",
                        "ls-files",
                        "--unmerged",
                        "-z",
                        "--",
                        &request.file,
                    ],
                    cancel,
                    16 * 1024,
                )
                .map_err(|_| FAILED)?;
                if entries
                    .split_terminator('\0')
                    .any(|entry| !entry.starts_with("100644 ") && !entry.starts_with("100755 "))
                {
                    return Err(PATH_INVALID.into());
                }
                let side = if matches!(request.resolution, Resolution::Ours) {
                    "--ours"
                } else {
                    "--theirs"
                };
                mutate(
                    &context.worktree,
                    &["--literal-pathspecs", "checkout", side, "--", &request.file],
                    cancel,
                )?;
            }
            Resolution::Delete => {
                return mutate(
                    &context.worktree,
                    &["--literal-pathspecs", "rm", "--quiet", "--", &request.file],
                    cancel,
                )
            }
            Resolution::Content { text } => {
                if versions(&context, &request.file, cancel)?.binary {
                    return Err("conflict_choice_invalid".into());
                }
                if cancel.load(Ordering::Acquire) {
                    return Err(FAILED.into());
                }
                file_path(&context, &request.file)?;
                devbox_filesystem::atomic_write(&target, text.as_bytes()).map_err(|_| FAILED)?;
            }
        }
        file_path(&context, &request.file)?;
        mutate(
            &context.worktree,
            &["--literal-pathspecs", "add", "--", &request.file],
            cancel,
        )
    })
    .await
}
fn operation_name(operation: ConflictOperation) -> &'static str {
    match operation {
        ConflictOperation::Merge => "merge",
        ConflictOperation::Rebase => "rebase",
        ConflictOperation::CherryPick => "cherry-pick",
        ConflictOperation::Revert => "revert",
    }
}
async fn finish_operation(request: OperationRequest, abort: bool) -> Result<(), String> {
    let operation = begin_git_operation(&request.operation_id, FAILED, FAILED)?;
    spawn_git_task(FAILED, move || {
        let mut operation = operation;
        let context = validated_repository_context(&request.path, FAILED)?;
        operation.bind_repository(context.common_git_identity, FAILED, FAILED)?;
        let cancel = operation.cancellation.as_ref();
        let state = state(&context, cancel)?;
        if abort {
            if let Some(operation) = state.operation {
                mutate(
                    &context.worktree,
                    &[operation_name(operation), "--abort"],
                    cancel,
                )
            } else if !state.files.is_empty() {
                mutate(&context.worktree, &["reset", "--merge"], cancel)
            } else {
                Err("conflict_no_operation".into())
            }
        } else {
            let operation = state.operation.ok_or("conflict_no_operation")?;
            if !state.files.is_empty() {
                return Err("conflict_unresolved".into());
            }
            mutate(
                &context.worktree,
                &[
                    "-c",
                    "core.editor=true",
                    operation_name(operation),
                    "--continue",
                ],
                cancel,
            )
        }
    })
    .await
}
pub async fn repo_operation_continue(request: OperationRequest) -> Result<(), String> {
    finish_operation(request, false).await
}
pub async fn repo_operation_abort(request: OperationRequest) -> Result<(), String> {
    finish_operation(request, true).await
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
        host_path_spelling(&dir.canonicalize().unwrap(), "conflict_operation_failed").unwrap()
    }

    fn conflicted_merge() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        git(tmp.path(), &["switch", "--quiet", "-c", "other"]);
        fs::write(tmp.path().join("README.md"), "theirs\n").unwrap();
        git(tmp.path(), &["commit", "--quiet", "-am", "theirs"]);
        git(tmp.path(), &["switch", "--quiet", "main"]);
        fs::write(tmp.path().join("README.md"), "ours\n").unwrap();
        git(tmp.path(), &["commit", "--quiet", "-am", "ours"]);
        let merge = std::process::Command::new("git")
            .args(["merge", "--quiet", "other"])
            .current_dir(tmp.path())
            .status()
            .unwrap();
        assert!(!merge.success());
        tmp
    }

    #[test]
    fn automatic_conflict_state_does_not_execute_fsmonitor() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        let hook = tmp.path().join(".git/hooks/fsmonitor-test");
        fs::write(
            &hook,
            "#!/bin/sh\nprintf called > monitor-marker.txt\nprintf 'fixture-token\\0/\\0'\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&hook, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let spelling = hook.to_string_lossy().replace('\\', "/");
        git(
            tmp.path(),
            &["config", "core.fsmonitor", &format!("\"{spelling}\"")],
        );
        let state = block(repo_conflicts(PathRequest {
            path: path(tmp.path()),
        }))
        .unwrap();
        assert!(state.files.is_empty());
        assert!(!tmp.path().join("monitor-marker.txt").exists());
    }

    #[test]
    fn conflicts_and_versions_are_visible() {
        let tmp = conflicted_merge();
        let state = block(repo_conflicts(PathRequest {
            path: path(tmp.path()),
        }))
        .unwrap();
        assert_eq!(state.operation, Some(ConflictOperation::Merge));
        assert_eq!(state.files[0].path, "README.md");
        let versions = block(repo_conflict_versions(ConflictFileRequest {
            path: path(tmp.path()),
            file: "README.md".into(),
        }))
        .unwrap();
        assert_eq!(
            (
                versions.base.as_deref(),
                versions.ours.as_deref(),
                versions.theirs.as_deref()
            ),
            (Some("base\n"), Some("ours\n"), Some("theirs\n"))
        );
        assert!(versions.current.unwrap().contains("<<<<<<<"));
    }

    #[test]
    fn continue_needs_every_file_resolved() {
        let tmp = conflicted_merge();
        assert_eq!(
            block(repo_operation_continue(OperationRequest {
                path: path(tmp.path()),
                operation_id: "c1".into()
            }))
            .unwrap_err(),
            "conflict_unresolved"
        );
        assert_eq!(
            block(repo_conflict_resolve(ConflictResolveRequest {
                path: path(tmp.path()),
                file: "README.md".into(),
                resolution: Resolution::Content {
                    text: "<<<<<<< HEAD\nx\n".into()
                },
                operation_id: "c2".into()
            }))
            .unwrap_err(),
            "conflict_markers_left"
        );
        block(repo_conflict_resolve(ConflictResolveRequest {
            path: path(tmp.path()),
            file: "README.md".into(),
            resolution: Resolution::Content {
                text: "ours and theirs\n".into(),
            },
            operation_id: "c3".into(),
        }))
        .unwrap();
        block(repo_operation_continue(OperationRequest {
            path: path(tmp.path()),
            operation_id: "c4".into(),
        }))
        .unwrap();
        assert_eq!(
            git(tmp.path(), &["rev-list", "--count", "--merges", "HEAD"]).trim(),
            "1"
        );
        assert_eq!(
            fs::read_to_string(tmp.path().join("README.md")).unwrap(),
            "ours and theirs\n"
        );
    }

    #[test]
    fn choosing_a_side_and_aborting() {
        let tmp = conflicted_merge();
        block(repo_conflict_resolve(ConflictResolveRequest {
            path: path(tmp.path()),
            file: "README.md".into(),
            resolution: Resolution::Theirs,
            operation_id: "t1".into(),
        }))
        .unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("README.md")).unwrap(),
            "theirs\n"
        );
        assert_eq!(
            block(repo_conflict_resolve(ConflictResolveRequest {
                path: path(tmp.path()),
                file: "../x".into(),
                resolution: Resolution::Ours,
                operation_id: "t2".into()
            }))
            .unwrap_err(),
            "conflict_path_invalid"
        );
        block(repo_operation_abort(OperationRequest {
            path: path(tmp.path()),
            operation_id: "t3".into(),
        }))
        .unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("README.md")).unwrap(),
            "ours\n"
        );
        assert!(block(repo_conflicts(PathRequest {
            path: path(tmp.path())
        }))
        .unwrap()
        .files
        .is_empty());
    }

    #[test]
    fn stash_conflicts_have_no_operation_and_abort_with_reset_merge() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        fs::write(tmp.path().join("README.md"), "stashed\n").unwrap();
        git(tmp.path(), &["stash", "push", "--quiet"]);
        fs::write(tmp.path().join("README.md"), "committed\n").unwrap();
        git(tmp.path(), &["commit", "--quiet", "-am", "c"]);
        let _ = std::process::Command::new("git")
            .args(["stash", "apply", "--quiet"])
            .current_dir(tmp.path())
            .status();
        let state = block(repo_conflicts(PathRequest {
            path: path(tmp.path()),
        }))
        .unwrap();
        assert_eq!((state.operation, state.files.len()), (None, 1));
        assert_eq!(
            block(repo_operation_continue(OperationRequest {
                path: path(tmp.path()),
                operation_id: "s1".into()
            }))
            .unwrap_err(),
            "conflict_no_operation"
        );
        block(repo_operation_abort(OperationRequest {
            path: path(tmp.path()),
            operation_id: "s2".into(),
        }))
        .unwrap();
        assert_eq!(
            fs::read_to_string(tmp.path().join("README.md")).unwrap(),
            "committed\n"
        );
        assert_eq!(git(tmp.path(), &["stash", "list"]).lines().count(), 1);
    }

    #[test]
    fn merge_can_keep_conflicts_for_resolution() {
        let (_tmp, main, _agent) =
            crate::test_support::repo_with_agent_branch("agent\n", Some("main\n"));
        let result = block(crate::commands::repo_merge(crate::commands::MergeRequest {
            path: main.to_string_lossy().into(),
            branch: "agent/fix".into(),
            operation_id: "k1".into(),
            keep_conflicts: true,
        }))
        .unwrap();
        assert!(!result.merged);
        assert!(main.join(".git/MERGE_HEAD").exists());
    }
    #[test]
    fn deleted_side_rejects_the_missing_version_and_allows_delete() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        git(tmp.path(), &["switch", "--quiet", "-c", "other"]);
        fs::write(tmp.path().join("README.md"), "theirs\n").unwrap();
        git(tmp.path(), &["commit", "--quiet", "-am", "theirs"]);
        git(tmp.path(), &["switch", "--quiet", "main"]);
        git(tmp.path(), &["rm", "README.md"]);
        git(tmp.path(), &["commit", "--quiet", "-m", "delete"]);
        assert!(!std::process::Command::new("git")
            .args(["merge", "--quiet", "other"])
            .current_dir(tmp.path())
            .output()
            .unwrap()
            .status
            .success());
        assert_eq!(
            block(repo_conflict_resolve(ConflictResolveRequest {
                path: path(tmp.path()),
                file: "README.md".into(),
                resolution: Resolution::Ours,
                operation_id: "missing-side".into()
            }))
            .unwrap_err(),
            "conflict_choice_invalid"
        );
        block(repo_conflict_resolve(ConflictResolveRequest {
            path: path(tmp.path()),
            file: "README.md".into(),
            resolution: Resolution::Delete,
            operation_id: "delete-side".into(),
        }))
        .unwrap();
        assert!(!tmp.path().join("README.md").exists());
        assert!(block(repo_conflicts(PathRequest {
            path: path(tmp.path())
        }))
        .unwrap()
        .files
        .is_empty());
    }
    #[test]
    fn oversized_or_binary_content_is_not_returned_or_replaced_with_text() {
        for bytes in [vec![b'x'; LIMIT + 1], b"binary\0content".to_vec()] {
            let tmp = conflicted_merge();
            fs::write(tmp.path().join("README.md"), &bytes).unwrap();
            let view = block(repo_conflict_versions(ConflictFileRequest {
                path: path(tmp.path()),
                file: "README.md".into(),
            }))
            .unwrap();
            assert!(view.binary && view.base.is_none() && view.current.is_none());
            assert_eq!(
                block(repo_conflict_resolve(ConflictResolveRequest {
                    path: path(tmp.path()),
                    file: "README.md".into(),
                    resolution: Resolution::Content {
                        text: "replacement".into()
                    },
                    operation_id: "binary".into()
                }))
                .unwrap_err(),
                "conflict_choice_invalid"
            );
            assert_eq!(fs::read(tmp.path().join("README.md")).unwrap(), bytes);
        }
    }
    #[test]
    #[cfg(unix)]
    fn linked_conflict_files_never_read_or_write_the_destination() {
        let tmp = conflicted_merge();
        let outside = tempfile::NamedTempFile::new().unwrap();
        fs::write(outside.path(), "preserved").unwrap();
        fs::remove_file(tmp.path().join("README.md")).unwrap();
        std::os::unix::fs::symlink(outside.path(), tmp.path().join("README.md")).unwrap();
        assert_eq!(
            block(repo_conflict_versions(ConflictFileRequest {
                path: path(tmp.path()),
                file: "README.md".into()
            }))
            .unwrap_err(),
            "conflict_path_invalid"
        );
        assert_eq!(
            block(repo_conflict_resolve(ConflictResolveRequest {
                path: path(tmp.path()),
                file: "README.md".into(),
                resolution: Resolution::Theirs,
                operation_id: "linked".into()
            }))
            .unwrap_err(),
            "conflict_path_invalid"
        );
        assert_eq!(fs::read_to_string(outside.path()).unwrap(), "preserved");
    }
}
