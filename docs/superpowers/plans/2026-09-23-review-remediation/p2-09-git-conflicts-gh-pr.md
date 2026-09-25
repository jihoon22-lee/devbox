# P2-09 Git 보강 3: 충돌 해결·GitHub PR — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4와 `CONVENTIONS.md`의 "메서드 추가 절차"(P1-11)를 읽는다.

**Goal:** Source 화면에서 병합·rebase·cherry-pick·revert·stash 적용 중 생긴 충돌을 파일별로 세 버전(공통 조상·우리 쪽·상대 쪽)을 보며 해결하고 계속·중단할 수 있게 하고, 현재 branch의 GitHub PR 상태 보기와 PR 만들기를 `gh` CLI로 제공한다(D25 3순위, `review.md` §8 표 6번). Agent Hub 병합 충돌에서 "충돌 해결하기"로 이 화면에 이어진다.

**Architecture:**
- 충돌: `crates/repositories-engine/src/core/conflicts.rs`(순수: porcelain v2 `u` 기록 파싱, 진행 중 작업 판정, 충돌 표시 검사)와 `src/commands/conflicts.rs`(실행). 파일 내용 쓰기는 engine이 작업 폴더에 직접 한다(WSL 프로젝트는 helper 안에서 같은 코드가 돈다). 계속은 `git -c core.editor=true <op> --continue`로 편집기 없이 진행한다.
- PR: `src/commands/pull_requests.rs`가 PATH의 `gh`를 실행한다(Windows 프로젝트는 Windows의 `gh.exe`, WSL 프로젝트는 helper 안에서 WSL의 `gh`). 환경 `GH_PROMPT_DISABLED=1`, `GH_NO_UPDATE_NOTIFIER=1`, `NO_COLOR=1`, `GH_PAGER=cat`, stdin 닫음, 30초 제한, 출력 1MiB. `gh`가 없거나 로그인하지 않았으면 오류가 아니라 `available: false`와 이유를 돌려준다.
- 화면: `ConflictPanel`(충돌이 있을 때 `SourcePanel` 맨 위), `PullRequestPanel`(`RemoteSyncPanel` 뒤). Agent Hub(P2-05)의 병합에 `keepConflicts` 선택을 더한다.
- 메서드 등록은 P2-07 Task 4와 같은 경로.

**Tech Stack:** Rust(git·gh CLI), React 19, Vitest

**Spec:** `review.md` §8 신규 기능 표 6번 · `00-roadmap.md` D13·D25

## Global Constraints

- `00-roadmap.md` §3 전부 적용. 새 의존성 없음.
- 충돌 파일 경로는 `validate_change_path`를 통과하고, 작업 폴더 안의 링크가 아닌 파일이어야 한다. 버전 텍스트는 파일마다 1MiB(넘거나 NUL이 있으면 `binary: true`로 텍스트 없이).
- "편집한 결과로 해결"은 줄 시작의 충돌 표시(`<<<<<<< `, `=======`, `>>>>>>> `)가 남아 있으면 거부한다.
- 중단(`--abort`, stash 충돌은 `reset --merge`)은 진행 중인 해결을 버리므로 화면 안 확인 한 번을 둔다. 나머지는 확인 없이.
- `gh`에는 remote·저장소 이름을 넘기지 않는다(현재 작업 폴더의 git remote를 `gh`가 스스로 고른다). PR URL은 `https://`로 시작할 때만 연다.

## Review Focus

1. 충돌이 남은 파일이 있으면 "계속"은 `conflict_unresolved`로 거부되고 아무것도 커밋하지 않는다. (Task 2 테스트)
2. 한쪽이 삭제한 충돌(`DU`/`UD`)은 "파일 삭제로 해결"과 남은 쪽 사용만 제시한다. (Task 1·5 테스트)
3. stash 적용 충돌(진행 중 작업 표시 없음)도 목록에 나오고, "중단"은 `reset --merge`로 작업 폴더를 적용 전으로 되돌리며 stash는 남는다. (Task 2 테스트)
4. `gh`가 없거나 로그인하지 않은 PC에서는 PR 절이 설치·로그인 안내만 보이고 오류 알림이 뜨지 않는다. (Task 3·5 테스트)
5. push하지 않은 branch로 PR을 만들면 "먼저 push해 주세요"와 push 버튼이 보인다. (Task 3·5 테스트)

## Branch · PR

- 묶음: **B11** — 브랜치 `feat/devbox-workspace/git-workflows`, PR 제목 `feat(devbox-workspace): branches, stash, hunks, amend, blame, conflicts and pull requests`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `feat(devbox-workspace): resolve conflicts and manage GitHub pull requests`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

---

### Task 1: 충돌 파서

**Files:** Create `crates/repositories-engine/src/core/conflicts.rs`; Modify `core/mod.rs`

**Interfaces (Produces):** `ConflictKind { BothModified, BothAdded, BothDeleted, AddedByUs, AddedByThem, DeletedByUs, DeletedByThem }`, `ConflictFile { path: String, kind: ConflictKind }`, `ConflictOperation { Merge, Rebase, CherryPick, Revert }`(serde camelCase, TS), `parse_unmerged(status_v2_z: &str) -> Result<Vec<ConflictFile>, String>`, `operation_from_markers(git_dir_entries: &[&str]) -> Option<ConflictOperation>`, `has_conflict_markers(text: &str) -> bool`, `ConflictKind::choices(self) -> &'static [&'static str]`(`"ours"`, `"theirs"`, `"content"`, `"delete"` 중 가능한 것)

- [ ] **Step 1: 실패하는 테스트**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unmerged_records_map_to_kinds() {
        let status = concat!(
            "u UU N... 100644 100644 100644 100644 a1 b1 c1 src/app.rs\0",
            "u DU N... 100644 000000 100644 100644 a2 b2 c2 gone.txt\0",
            "1 .M N... 100644 100644 100644 d e clean.txt\0",
            "u AA N... 000000 100644 100644 100644 a3 b3 c3 both new.txt\0",
        );
        let files = parse_unmerged(status).unwrap();
        assert_eq!(files, vec![
            ConflictFile { path: "src/app.rs".into(), kind: ConflictKind::BothModified },
            ConflictFile { path: "gone.txt".into(), kind: ConflictKind::DeletedByUs },
            ConflictFile { path: "both new.txt".into(), kind: ConflictKind::BothAdded },
        ]);
        assert!(parse_unmerged("u ZZ N... 1 2 3 4 a b c x\0").is_err());
    }

    #[test]
    fn operations_come_from_git_dir_markers() {
        assert_eq!(operation_from_markers(&["MERGE_HEAD", "ORIG_HEAD"]), Some(ConflictOperation::Merge));
        assert_eq!(operation_from_markers(&["rebase-merge"]), Some(ConflictOperation::Rebase));
        assert_eq!(operation_from_markers(&["rebase-apply"]), Some(ConflictOperation::Rebase));
        assert_eq!(operation_from_markers(&["CHERRY_PICK_HEAD"]), Some(ConflictOperation::CherryPick));
        assert_eq!(operation_from_markers(&["REVERT_HEAD"]), Some(ConflictOperation::Revert));
        assert_eq!(operation_from_markers(&["ORIG_HEAD"]), None);
    }

    #[test]
    fn markers_must_start_a_line() {
        assert!(has_conflict_markers("a\n<<<<<<< HEAD\nb\n=======\nc\n>>>>>>> other\n"));
        assert!(!has_conflict_markers("let arrow = \"<<<<<<< not a marker\";\n"));
        assert!(!has_conflict_markers("plain text\n"));
    }

    #[test]
    fn deletion_conflicts_offer_delete_and_the_surviving_side() {
        assert_eq!(ConflictKind::DeletedByUs.choices(), &["theirs", "delete"]);
        assert_eq!(ConflictKind::DeletedByThem.choices(), &["ours", "delete"]);
        assert_eq!(ConflictKind::BothModified.choices(), &["ours", "theirs", "content"]);
    }
}
```

- [ ] **Step 2: 실패 확인** — Run: `source ~/.cargo/env && cargo test -p devbox-repositories-engine --lib -- core::conflicts` → FAIL
- [ ] **Step 3: 구현** — `u` 기록은 `u XY sub m1 m2 m3 mW h1 h2 h3 path`(경로에 공백 가능 — 앞 10개 필드 뒤 나머지 전체가 경로). XY 표: `UU` BothModified, `AA` BothAdded, `DD` BothDeleted, `AU` AddedByUs, `UA` AddedByThem, `DU` DeletedByUs, `UD` DeletedByThem, 그 밖 오류. `u`가 아닌 기록(`1`, `2`, `?`, `!`, `#`)은 건너뛴다(`2`는 이름 변경 기록이라 다음 NUL 필드도 건너뛴다). choices: BothModified·BothAdded → ours·theirs·content, AddedByUs → ours·delete, AddedByThem → theirs·delete, DeletedByUs → theirs·delete, DeletedByThem → ours·delete, BothDeleted → delete.
- [ ] **Step 4: 확인·커밋** — Run: Step 2 명령 → PASS. `git add -A && git commit -m "feat(devbox-workspace): parse merge conflicts"`

---

### Task 2: 충돌 명령

**Files:** Create `crates/repositories-engine/src/commands/conflicts.rs`; Modify P2-05의 `repo_merge`(`keep_conflicts`)와 그 테스트(`keep_conflicts: false` 명시), `src/test_support.rs`(P2-05의 `repo_with_agent_branch`를 `commands.rs` 테스트에서 옮김)

**Interfaces (Produces):** `repo_conflicts(PathRequest) -> Result<ConflictState { operation: Option<ConflictOperation>, files: Vec<ConflictFile> }, String>`, `repo_conflict_versions(ConflictFileRequest { path, file }) -> Result<ConflictVersions { base: Option<String>, ours: Option<String>, theirs: Option<String>, current: Option<String>, binary: bool }, String>`, `repo_conflict_resolve(ConflictResolveRequest { path, file, resolution: Resolution, operation_id }) -> Result<(), String>`(`Resolution { Ours, Theirs, Delete, Content { text: String } }`, serde `tag = "kind"`), `repo_operation_continue(OperationRequest { path, operation_id }) -> Result<(), String>`, `repo_operation_abort(OperationRequest) -> Result<(), String>`; `MergeRequest { …, #[serde(default)] keep_conflicts: bool }`; 오류 `conflict_path_invalid`, `conflict_choice_invalid`, `conflict_markers_left`, `conflict_unresolved`, `conflict_no_operation`, `conflict_operation_failed`

- [ ] **Step 1: 실패하는 테스트**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{git, init_repo};
    use std::fs;

    fn block<T>(future: impl std::future::Future<Output = T>) -> T { crate::runtime::block_on(future) }
    fn path(dir: &std::path::Path) -> String { dir.to_string_lossy().into_owned() }

    fn conflicted_merge() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        git(tmp.path(), &["switch", "--quiet", "-c", "other"]);
        fs::write(tmp.path().join("README.md"), "theirs\n").unwrap();
        git(tmp.path(), &["commit", "--quiet", "-am", "theirs"]);
        git(tmp.path(), &["switch", "--quiet", "main"]);
        fs::write(tmp.path().join("README.md"), "ours\n").unwrap();
        git(tmp.path(), &["commit", "--quiet", "-am", "ours"]);
        let merge = std::process::Command::new("git").args(["merge", "--quiet", "other"]).current_dir(tmp.path()).status().unwrap();
        assert!(!merge.success());
        tmp
    }

    #[test]
    fn conflicts_and_versions_are_visible() {
        let tmp = conflicted_merge();
        let state = block(repo_conflicts(PathRequest { path: path(tmp.path()) })).unwrap();
        assert_eq!(state.operation, Some(ConflictOperation::Merge));
        assert_eq!(state.files[0].path, "README.md");
        let versions = block(repo_conflict_versions(ConflictFileRequest { path: path(tmp.path()), file: "README.md".into() })).unwrap();
        assert_eq!((versions.base.as_deref(), versions.ours.as_deref(), versions.theirs.as_deref()), (Some("base\n"), Some("ours\n"), Some("theirs\n")));
        assert!(versions.current.unwrap().contains("<<<<<<<"));
    }

    #[test]
    fn continue_needs_every_file_resolved() {
        let tmp = conflicted_merge();
        assert_eq!(block(repo_operation_continue(OperationRequest { path: path(tmp.path()), operation_id: "c1".into() })).unwrap_err(), "conflict_unresolved");
        assert_eq!(
            block(repo_conflict_resolve(ConflictResolveRequest { path: path(tmp.path()), file: "README.md".into(), resolution: Resolution::Content { text: "<<<<<<< HEAD\nx\n".into() }, operation_id: "c2".into() })).unwrap_err(),
            "conflict_markers_left"
        );
        block(repo_conflict_resolve(ConflictResolveRequest { path: path(tmp.path()), file: "README.md".into(), resolution: Resolution::Content { text: "ours and theirs\n".into() }, operation_id: "c3".into() })).unwrap();
        block(repo_operation_continue(OperationRequest { path: path(tmp.path()), operation_id: "c4".into() })).unwrap();
        assert_eq!(git(tmp.path(), &["rev-list", "--count", "--merges", "HEAD"]).trim(), "1");
        assert_eq!(fs::read_to_string(tmp.path().join("README.md")).unwrap(), "ours and theirs\n");
    }

    #[test]
    fn choosing_a_side_and_aborting() {
        let tmp = conflicted_merge();
        block(repo_conflict_resolve(ConflictResolveRequest { path: path(tmp.path()), file: "README.md".into(), resolution: Resolution::Theirs, operation_id: "t1".into() })).unwrap();
        assert_eq!(fs::read_to_string(tmp.path().join("README.md")).unwrap(), "theirs\n");
        assert_eq!(block(repo_conflict_resolve(ConflictResolveRequest { path: path(tmp.path()), file: "../x".into(), resolution: Resolution::Ours, operation_id: "t2".into() })).unwrap_err(), "conflict_path_invalid");
        block(repo_operation_abort(OperationRequest { path: path(tmp.path()), operation_id: "t3".into() })).unwrap();
        assert_eq!(fs::read_to_string(tmp.path().join("README.md")).unwrap(), "ours\n");
        assert!(block(repo_conflicts(PathRequest { path: path(tmp.path()) })).unwrap().files.is_empty());
    }

    #[test]
    fn stash_conflicts_have_no_operation_and_abort_with_reset_merge() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        fs::write(tmp.path().join("README.md"), "stashed\n").unwrap();
        git(tmp.path(), &["stash", "push", "--quiet"]);
        fs::write(tmp.path().join("README.md"), "committed\n").unwrap();
        git(tmp.path(), &["commit", "--quiet", "-am", "c"]);
        let _ = std::process::Command::new("git").args(["stash", "apply", "--quiet"]).current_dir(tmp.path()).status();
        let state = block(repo_conflicts(PathRequest { path: path(tmp.path()) })).unwrap();
        assert_eq!((state.operation, state.files.len()), (None, 1));
        assert_eq!(block(repo_operation_continue(OperationRequest { path: path(tmp.path()), operation_id: "s1".into() })).unwrap_err(), "conflict_no_operation");
        block(repo_operation_abort(OperationRequest { path: path(tmp.path()), operation_id: "s2".into() })).unwrap();
        assert_eq!(fs::read_to_string(tmp.path().join("README.md")).unwrap(), "committed\n");
        assert_eq!(git(tmp.path(), &["stash", "list"]).lines().count(), 1);
    }

    #[test]
    fn merge_can_keep_conflicts_for_resolution() {
        let (_tmp, main, _agent) = crate::test_support::repo_with_agent_branch("agent\n", Some("main\n"));
        let result = block(crate::commands::repo_merge(crate::commands::MergeRequest {
            path: main.to_string_lossy().into(), branch: "agent/fix".into(), operation_id: "k1".into(), keep_conflicts: true,
        })).unwrap();
        assert!(!result.merged);
        assert!(main.join(".git/MERGE_HEAD").exists());
    }
}
```

  (`repo_with_agent_branch`는 P2-05 테스트 도우미를 `test_support`로 옮긴 것. P2-05의 `repo_merge` 테스트들은 `keep_conflicts: false`를 명시하도록 고친다.)
- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-repositories-engine --lib -- commands::conflicts` → FAIL
- [ ] **Step 3: 구현**
  - `repo_conflicts`: `git status --porcelain=v2 -z` → `parse_unmerged`; `git rev-parse --absolute-git-dir` 폴더의 항목 이름 중 `MERGE_HEAD`·`CHERRY_PICK_HEAD`·`REVERT_HEAD`·`rebase-merge`·`rebase-apply`를 모아 `operation_from_markers`.
  - `repo_conflict_versions`: 경로 검증 → `git show :1:<file>`·`:2:`·`:3:`(없는 stage는 `None`), `current`는 작업 폴더 파일(없으면 `None`). 1MiB 초과나 NUL 포함이면 모두 `None`, `binary: true`.
  - `repo_conflict_resolve`: 파일이 현재 충돌 목록에 있고 선택이 `kind.choices()`에 있어야 한다(아니면 `conflict_choice_invalid`). Ours/Theirs는 해당 stage가 있으면 `git checkout --ours|--theirs -- <file>` 후 `git add -- <file>`. Delete는 `git rm --quiet -- <file>`. Content는 `has_conflict_markers`면 `conflict_markers_left`, 아니면 대상이 링크가 아닌지 확인한 뒤 같은 폴더 임시 파일 → rename으로 쓰고 `git add -- <file>`.
  - `repo_operation_continue`: 진행 중 작업이 없으면 먼저 `conflict_no_operation`(stash 충돌은 계속할 작업이 없다), 충돌 파일이 남아 있으면 `conflict_unresolved`, 그 밖에는 `git -c core.editor=true merge|rebase|cherry-pick|revert --continue`(실패 → `conflict_operation_failed`).
  - `repo_operation_abort`: 작업이 있으면 `git <op> --abort`, 없고 충돌 파일이 있으면 `git reset --merge`, 둘 다 없으면 `conflict_no_operation`.
  - `repo_merge`: `keep_conflicts`면 충돌 때 `merge --abort`를 하지 않고 `merged: false, conflicts`를 돌려준다.
- [ ] **Step 4: 확인·커밋** — Run: `cargo test -p devbox-repositories-engine --lib -- commands::conflicts merge_` → PASS. `git add -A && git commit -m "feat(devbox-workspace): resolve conflicts and continue or abort"`

---

### Task 3: GitHub PR 명령

**Files:** Create `crates/repositories-engine/src/commands/pull_requests.rs`, `crates/repositories-engine/src/core/pull_requests.rs`

**Interfaces (Produces):**
- `core::pull_requests`: `PullRequest { number: u64, title: String, state: String, url: String, is_draft: bool, head: String, base: String, review_decision: Option<String>, checks: CheckSummary }`, `CheckSummary { passed: u32, failed: u32, pending: u32 }`, `PrListItem { number, title, head, author: String, updated_at: String, is_draft: bool, url }`(camelCase, TS), `parse_view(json: &str) -> Result<PullRequest, String>`, `parse_list(json: &str) -> Result<Vec<PrListItem>, String>`, `classify_gh_failure(stderr: &str) -> &'static str`
- `commands::pull_requests`: `repo_pr_status(PathRequest) -> Result<PrStatus { available: bool, reason: Option<String>, pr: Option<PullRequest> }, String>`, `repo_pr_list(PrListRequest { path, limit: u32 }) -> Result<Vec<PrListItem>, String>`, `repo_pr_create(PrCreateRequest { path, title, body, base: Option<String>, draft: bool, operation_id }) -> Result<PrCreated { url }, String>`
- 순수 실행 경계: `trait GhRunner { fn run(&self, cwd: &Path, args: &[&str]) -> Result<GhOutput { status: i32, stdout: String, stderr: String }, GhSpawnError> }`, 실제 구현 `SystemGh`, 테스트용 `FakeGh`. `GhSpawnError::Missing`(PATH에 없음), `Timeout`, `Io`.
- 오류 코드: `pr_branch_not_pushed`, `pr_exists`, `pr_input_invalid`, `pr_failed`

- [ ] **Step 1: 실패하는 테스트** — `core/pull_requests.rs`와 `commands/pull_requests.rs`(`FakeGh`로 `gh` 없이)

```rust
// core/pull_requests.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_json_summarizes_checks() {
        let json = r#"{"number":42,"title":"Fix login","state":"OPEN","url":"https://github.com/me/devbox/pull/42","isDraft":false,
            "headRefName":"agent/fix-login","baseRefName":"main","reviewDecision":"REVIEW_REQUIRED",
            "statusCheckRollup":[{"conclusion":"SUCCESS"},{"conclusion":"FAILURE"},{"status":"IN_PROGRESS","conclusion":""},{"conclusion":"SKIPPED"}]}"#;
        let pr = parse_view(json).unwrap();
        assert_eq!((pr.number, pr.head.as_str(), pr.base.as_str()), (42, "agent/fix-login", "main"));
        assert_eq!(pr.checks, CheckSummary { passed: 2, failed: 1, pending: 1 });
    }

    #[test]
    fn gh_failures_map_to_codes() {
        assert_eq!(classify_gh_failure("aborted: you must first push the current branch to a remote"), "pr_branch_not_pushed");
        assert_eq!(classify_gh_failure("a pull request for branch \"x\" into branch \"main\" already exists:\nhttps://github.com/me/r/pull/1"), "pr_exists");
        assert_eq!(classify_gh_failure("something else"), "pr_failed");
    }
}
```

```rust
// commands/pull_requests.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_logged_out_gh_is_reported_as_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let missing = FakeGh::missing();
        assert_eq!(pr_status_with(&missing, dir.path()).unwrap(), PrStatus { available: false, reason: Some("gh_missing".into()), pr: None });
        let logged_out = FakeGh::with(vec![(vec!["auth", "status"], 1, "", "You are not logged into any GitHub hosts.")]);
        assert_eq!(pr_status_with(&logged_out, dir.path()).unwrap().reason.as_deref(), Some("gh_unauthenticated"));
    }

    #[test]
    fn a_branch_without_a_pr_is_available_with_none() {
        let dir = tempfile::tempdir().unwrap();
        let gh = FakeGh::with(vec![
            (vec!["auth", "status"], 0, "", ""),
            (vec!["pr", "view"], 1, "", "no pull requests found for branch \"agent/fix\""),
        ]);
        assert_eq!(pr_status_with(&gh, dir.path()).unwrap(), PrStatus { available: true, reason: None, pr: None });
    }

    #[test]
    fn create_passes_only_title_body_base_and_draft() {
        let dir = tempfile::tempdir().unwrap();
        let gh = FakeGh::with(vec![(vec!["pr", "create"], 0, "https://github.com/me/devbox/pull/43\n", "")]);
        let created = pr_create_with(&gh, dir.path(), "Fix login", "Body", Some("main"), true).unwrap();
        assert_eq!(created.url, "https://github.com/me/devbox/pull/43");
        assert_eq!(gh.last_args(), vec!["pr", "create", "--title", "Fix login", "--body", "Body", "--base", "main", "--draft"]);
        assert_eq!(pr_create_with(&gh, dir.path(), "", "Body", None, false).unwrap_err(), "pr_input_invalid");
    }
}
```

  (`FakeGh::with`는 인자 앞부분이 일치하는 첫 항목의 (종료 코드, stdout, stderr)를 돌려주고 받은 인자를 기록한다.)
- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-repositories-engine --lib -- pull_requests` → FAIL
- [ ] **Step 3: 구현**
  - `SystemGh::run`: `std::process::Command::new("gh")`(Windows는 PATH에서 `gh.exe`), `current_dir(cwd)`, 위 환경 변수, stdin null, stdout·stderr pipe. 별도 스레드 두 개로 각 1MiB까지 읽고, 30초 동안 `try_wait`(50ms 간격) 후 넘으면 `kill` → `Timeout`. 실행 파일이 없으면(`ErrorKind::NotFound`) `Missing`.
  - `pr_status_with`: `Missing` → `available: false, reason: "gh_missing"`. `gh auth status` 종료 코드가 0이 아니면 `gh_unauthenticated`. `gh pr view --json number,title,state,url,isDraft,headRefName,baseRefName,reviewDecision,statusCheckRollup` 종료 코드 0 → `parse_view`, stderr에 `no pull requests found`면 `pr: None`, 그 밖 `pr_failed`.
  - `repo_pr_list`: `gh pr list --json number,title,headRefName,author,updatedAt,isDraft,url --limit <1–30>`(author는 `author.login`).
  - `pr_create_with`: 제목 1–256자·제어 문자 없음, 본문 64KiB 이하, base는 `valid_worktree_branch`. `gh pr create --title T --body B [--base X] [--draft]`, stdout 마지막 줄이 `https://`로 시작해야 한다. 실패는 `classify_gh_failure`.
  - 세 명령 모두 `validated_repository_context`로 경로를 확인하고 `spawn_git_task` 안에서 `SystemGh`를 쓴다. `repo_pr_create`만 `begin_git_operation`.
  - `checks`: `conclusion`이 `SUCCESS`·`NEUTRAL`·`SKIPPED`면 passed, `FAILURE`·`CANCELLED`·`TIMED_OUT`·`ACTION_REQUIRED`·`STARTUP_FAILURE`면 failed, 그 밖(빈 값 포함)은 pending.
- [ ] **Step 4: 확인·커밋** — Run: Step 2 명령 → PASS. `git add -A && git commit -m "feat(devbox-workspace): pull request status, list and create through gh"`

---

### Task 4: 메서드 등록

- [ ] P2-07 Task 4와 같은 순서로 `repo_conflicts`, `repo_conflict_versions`, `repo_conflict_resolve`, `repo_operation_continue`, `repo_operation_abort`, `repo_pr_status`, `repo_pr_list`, `repo_pr_create`를 등록한다(`SourceCall`, `SOURCE_COMMANDS`, helper `source_method`, 생성 TS, 허용 표 fixture, 이슈 카탈로그). 예산: `repo_operation_continue`·`repo_operation_abort`·`repo_pr_*`는 `LONG_BUDGET_MS`. 문구: `conflict_path_invalid: "충돌 파일 경로를 확인해 주세요."`, `conflict_choice_invalid: "이 충돌에는 사용할 수 없는 해결 방법입니다."`, `conflict_markers_left: "충돌 표시(<<<<<<<, =======, >>>>>>>)가 남아 있습니다. 모두 정리한 뒤 해결해 주세요."`, `conflict_unresolved: "아직 해결하지 않은 충돌 파일이 있습니다."`, `conflict_no_operation: "계속할 병합·rebase 작업이 없습니다."`, `conflict_operation_failed: "Git 작업을 계속하지 못했습니다. 상태를 새로 고쳐 확인해 주세요."`, `pr_branch_not_pushed: "PR을 만들기 전에 현재 branch를 push해 주세요."`, `pr_exists: "이 branch의 PR이 이미 있습니다."`, `pr_input_invalid: "PR 제목과 내용을 확인해 주세요."`, `pr_failed: "GitHub CLI 작업을 완료하지 못했습니다."`
- [ ] Run: `cargo test -p devbox-repositories-engine --lib api && cargo test -p workspace-wsl --lib source_method && cargo test -p devbox-workspace --lib allow_table && bash .github/scripts/check-generated-bindings.sh && pnpm --filter @devbox/workspace-features exec vitest run src/issues` → PASS. `git add -A && git commit -m "feat(devbox-workspace): route conflict and pull request methods"`

---

### Task 5: 화면

**Files:** Create `packages/workspace-features/src/source/components/{ConflictPanel.tsx,ConflictPanel.test.tsx,PullRequestPanel.tsx,PullRequestPanel.test.tsx}`, `packages/workspace-features/src/lib/openUrl.ts`; Modify `source/{api.ts,SourcePanel.tsx}`, `packages/workspace-features/src/terminal/api.ts`(URL 열기를 `lib/openUrl.ts`로 옮겨 재사용), `apps/devbox-workspace/src/agents/AgentTaskRow.tsx`(+test)

- [ ] **Step 1: 실패하는 테스트**
  - `ConflictPanel.test.tsx`: `repo_conflicts`가 파일을 주면 "병합 중 충돌 2개"와 목록. 파일을 고르면 `repo_conflict_versions` → "공통 조상"·"우리 쪽"·"상대 쪽" 읽기 전용 영역과 "결과" 편집칸(현재 파일 내용). 버튼은 `choices`에 맞게("우리 쪽 사용", "상대 쪽 사용", "편집한 결과로 해결", "파일 삭제로 해결"). 결과에 충돌 표시가 남으면 "편집한 결과로 해결"이 비활성이고 이유 문구가 보인다. 모두 해결하면 "계속"이 활성, "중단"은 "진행 중인 해결을 모두 버리고 작업 전으로 돌아갑니다." 확인을 거친다. `binary: true`면 편집 대신 쪽 선택만. axe 위반 0.
  - `PullRequestPanel.test.tsx`: `available: false, reason: "gh_missing"` → "GitHub CLI(gh)를 설치하고 터미널에서 `gh auth login`을 실행해 주세요." / `gh_unauthenticated` → "`gh auth login`으로 로그인해 주세요." / PR 있음 → "#42 Fix login · 열림 · 검토 필요 · 검사 통과 2 · 실패 1 · 진행 중 1"과 "GitHub에서 열기"(`openUrl` mock 호출) / PR 없음 → "PR 만들기" 폼(제목은 `repo_last_commit` 요약으로 채움, 본문, base(원격 branch 목록에서 선택, 기본 비움 = 저장소 기본 branch), "초안" 체크) → `repo_pr_create` → 결과 URL 링크. `pr_branch_not_pushed` → 안내와 "push" 버튼(`repo_push` 호출). axe 위반 0.
  - `AgentTaskRow.test.tsx`(P2-05)에 추가: 병합 충돌 결과에서 "충돌 해결하기"를 누르면 `repo_merge {…, keepConflicts: true}` 후 `navigate("source")`. `running` 작업 행에 "PR 만들기"(작업 worktree 선택 → `navigate("source")`)가 있다.
- [ ] **Step 2: 실패 확인** — Run: `pnpm --filter @devbox/workspace-features exec vitest run src/source/components/ConflictPanel.test.tsx src/source/components/PullRequestPanel.test.tsx && pnpm --filter devbox-workspace exec vitest run src/agents` → FAIL
- [ ] **Step 3: 구현** — `api.ts` wrapper 8개. `SourcePanel`: 새로 고침 때 `repo_conflicts`도 부르고 파일이 있으면 `ConflictPanel`을 맨 위에 둔다(해결·계속·중단 뒤 `onChanged`로 다시 읽음). `PullRequestPanel`은 `RemoteSyncPanel` 뒤. `lib/openUrl.ts`: `https://`로 시작할 때만 `@tauri-apps/plugin-opener`의 `openUrl`(제품 밖 개발 모드는 `window.open`) — 터미널 `api.ts`의 기존 URL 열기 코드를 옮겨 둘이 함께 쓴다.
- [ ] **Step 4: 확인·커밋** — Run: Step 2 명령 → PASS, `pnpm --filter @devbox/workspace-features exec vitest run src/source` → PASS. `git add -A && git commit -m "feat(devbox-workspace): conflict and pull request panels"`

---

### Task 6: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9.
- [ ] PR 본문 "Windows 실기 확인"(사용자 확인 대기): (1) WSL 프로젝트에서 일부러 충돌을 만든 병합을 화면에서 해결하고 계속. (2) Agent Hub 병합 충돌 → "충돌 해결하기" → 해결 → 계속. (3) WSL에서 `gh auth login`한 상태로 PR 만들기·상태 보기·GitHub에서 열기. (4) `gh`가 없는 Windows 프로젝트에서 설치 안내만 보임.
