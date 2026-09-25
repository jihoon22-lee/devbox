# P2-07 Git 보강 1: branch·stash — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4와 `CONVENTIONS.md`의 "메서드 추가 절차"(P1-11)를 읽는다.

**Goal:** Workspace Source 화면에서 branch 목록·만들기·전환·이름 바꾸기·지우기와 stash 목록·저장·적용·꺼내기·지우기를 할 수 있게 한다(D25 3순위, `review.md` §8 표 6번). 지우기는 확인 대화 대신 8초 되돌리기를 준다(D13).

**Architecture:**
- engine: `crates/repositories-engine`에 새 파일 `src/core/branches.rs`·`src/core/stash.rs`(순수 파서)와 `src/commands/branches.rs`·`src/commands/stash.rs`(git 실행)를 둔다. `commands.rs`(5,500줄)에는 더하지 않는다. 실행은 기존 도우미(`begin_git_operation`, `spawn_git_task`, `validated_repository_context`, `run_git_mutation_with_cancel`, `run_git_status_bounded`)를 쓰고, 이 도우미들이 `commands.rs`의 private 함수면 `pub(crate)`로 연다.
- 새 메서드는 P1-14의 `SourceCall` variant, `SOURCE_COMMANDS`, WSL helper `control::source_method` 허용 목록에 모두 넣어 Windows·WSL 프로젝트가 같은 경로로 동작한다(helper가 WSL 안에서 같은 engine 코드를 실행한다).
- 되돌리기: branch 지우기는 지운 branch의 마지막 commit id를, stash 지우기는 지운 stash commit id를 돌려준다. 프런트는 P1-18 `useUndo`로 8초 동안 "되돌리기"를 보이고, 누르면 `repo_branch_create {name, startPoint: commit}`·`repo_stash_store {commit, message}`를 부른다.
- 화면: `SourcePanel`에 `BranchPanel`·`StashPanel`을 추가한다(`HistoryDiffPanel` 앞).

**Tech Stack:** Rust(git CLI), React 19, Vitest

**Spec:** `review.md` §8 신규 기능 표 6번 · `00-roadmap.md` D13·D25

## Global Constraints

- `00-roadmap.md` §3 전부 적용. 새 의존성 없음.
- 원격 이름·URL·force push는 다루지 않는다(기존 규칙: remote 설정은 Git이 소유). 원격 branch는 목록과 "추적 branch 만들며 전환"만.
- 전환은 `git switch`만 쓰고 `--discard-changes`·`--force`는 쓰지 않는다. 로컬 변경이 막으면 `switch_blocked_by_changes`로 알리고 stash를 권한다.
- branch 이름 검증은 기존 `valid_worktree_branch`(길이·`-` 시작 금지·ref 조각 규칙). stash 번호는 0–999.
- 출력 한도: ref 2,000개(넘으면 `truncated`), stash 500개.

## Review Focus

1. 현재 branch·다른 worktree가 체크아웃한 branch는 지우지 않는다(Git이 거부하는 것을 `branch_in_use`로 보인다). (Task 2 테스트)
2. 병합되지 않은 branch 지우기는 되돌리기로 같은 commit에서 다시 만들어진다. (Task 2·5 테스트)
3. 로컬 변경 때문에 전환이 막히면 작업 폴더가 그대로다(아무 파일도 바뀌지 않음). (Task 2 테스트)
4. stash 적용이 충돌하면 결과에 충돌 파일이 나오고 stash는 목록에 남는다(`pop`이어도). (Task 3 테스트)
5. detached HEAD에서도 목록이 뜨고 "현재 branch 없음(detached)"이 보인다. (Task 1 테스트)

## Branch · PR

- 묶음: **B11** — 브랜치 `feat/devbox-workspace/git-workflows`, PR 제목 `feat(devbox-workspace): branches, stash, hunks, amend, blame, conflicts and pull requests`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `feat(devbox-workspace): branches and stash in Source`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

---

### Task 1: 파서

**Files:** Create `crates/repositories-engine/src/core/{branches.rs,stash.rs}`, `crates/repositories-engine/src/test_support.rs`; Modify `crates/repositories-engine/src/core/mod.rs`, `src/lib.rs`(`#[cfg(test)] pub(crate) mod test_support;`)

**Interfaces (Produces):**
- `branches::FOR_EACH_REF_FORMAT: &str` = `"%(refname)%00%(objectname)%00%(upstream:short)%00%(upstream:track,nobracket)%00%(worktreepath)%00%(committerdate:unix)%00%(contents:subject)"`
- `branches::Branch { name: String, remote: bool, commit: String, upstream: Option<String>, ahead: u32, behind: u32, worktree: Option<String>, committed_at: i64, subject: String }`(camelCase, TS), `branches::BranchList { current: Option<String>, detached: bool, branches: Vec<Branch>, truncated: bool }`
- `branches::parse_refs(output: &str, current: Option<&str>) -> Result<Vec<Branch>, String>`, `branches::parse_track(value: &str) -> (u32, u32)`
- `stash::STASH_FORMAT` = `"%gd%x00%H%x00%ct%x00%gs"`, `stash::StashEntry { index: u32, commit: String, created_at: i64, message: String }`, `stash::parse_list(output: &str) -> Result<Vec<StashEntry>, String>`
- `test_support::{git(repo: &Path, args: &[&str]) -> String, init_repo(dir: &Path)}`(P2-05가 `commands.rs` 테스트에 둔 `git` 도우미를 이리로 옮긴다)

- [ ] **Step 1: 실패하는 테스트** — `core/branches.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const REFS: &str = concat!(
        "refs/heads/main\0aaaa\0origin/main\0ahead 2, behind 1\0/home/me/devbox\01790000000\0Merge agent/fix\n",
        "refs/heads/agent/fix\0bbbb\0\0\0/home/me/devbox-fix\01790000100\0agent change\n",
        "refs/remotes/origin/main\0cccc\0\0\0\01789999000\0upstream\n",
        "refs/remotes/origin/HEAD\0cccc\0\0\0\01789999000\0upstream\n",
    );

    #[test]
    fn refs_parse_into_local_and_remote_branches() {
        let branches = parse_refs(REFS, Some("main")).unwrap();
        assert_eq!(branches.len(), 3, "origin/HEAD is a symbolic alias and is skipped");
        assert_eq!((branches[0].name.as_str(), branches[0].ahead, branches[0].behind), ("main", 2, 1));
        assert_eq!(branches[0].upstream.as_deref(), Some("origin/main"));
        assert_eq!(branches[1].worktree.as_deref(), Some("/home/me/devbox-fix"));
        assert!(branches[2].remote && branches[2].name == "origin/main");
    }

    #[test]
    fn tracking_text_variants() {
        assert_eq!(parse_track(""), (0, 0));
        assert_eq!(parse_track("ahead 3"), (3, 0));
        assert_eq!(parse_track("behind 4"), (0, 4));
        assert_eq!(parse_track("gone"), (0, 0));
    }

    #[test]
    fn malformed_records_are_rejected() {
        assert!(parse_refs("refs/heads/main\0only-two-fields\n", None).is_err());
        assert!(parse_refs("refs/tags/v1\0a\0\0\0\01\0s\n", None).is_err());
    }
}
```

  `core/stash.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stash_list_parses_index_commit_time_and_message() {
        let output = "stash@{0}\0dddd\01790000000\0On main: wip login\nstash@{1}\0eeee\01789990000\0WIP on main: 1234567 base\n";
        let entries = parse_list(output).unwrap();
        assert_eq!(entries[0], StashEntry { index: 0, commit: "dddd".into(), created_at: 1_790_000_000, message: "On main: wip login".into() });
        assert_eq!(entries[1].index, 1);
        assert!(parse_list("stash@{x}\0d\01\0m\n").is_err());
        assert!(parse_list("").unwrap().is_empty());
    }
}
```

- [ ] **Step 2: 실패 확인** — Run: `source ~/.cargo/env && cargo test -p devbox-repositories-engine --lib -- core::branches core::stash` → FAIL
- [ ] **Step 3: 구현** — 기록은 `\n`, 필드는 `\0`으로 나눈다(필드 수가 7이 아니면 오류). `refs/heads/` → 로컬, `refs/remotes/` → 원격(`<remote>/HEAD`는 건너뜀), 그 밖의 ref는 오류. `parse_track`: `ahead N`, `behind N`, `ahead N, behind M`, `gone`, 빈 값. stash 기록은 4필드, `%gd`는 `stash@{N}` 모양이어야 한다. `test_support`에 `git()`(P2-05와 같은 모양)과 `init_repo(dir)`(`git init --quiet -b main`, `user.email`·`user.name` 설정, `README.md`에 `"base\n"`을 쓰고 `base`로 커밋)를 둔다. `Branch`·`StashEntry`는 `Debug, Clone, PartialEq, Eq, Serialize, TS`를 derive한다.
- [ ] **Step 4: 확인·커밋** — Run: Step 2 명령 → PASS. `git add -A && git commit -m "feat(devbox-workspace): parse branch and stash listings"`

---

### Task 2: branch 명령

**Files:** Create `crates/repositories-engine/src/commands/branches.rs`; Modify `src/commands.rs`(`mod branches;` 재수출, 도우미 `pub(crate)`)

**Interfaces (Produces):**
- `repo_branches(request: PathRequest) -> Result<BranchList, String>` — `PathRequest`는 새 구조체가 아니라 `commands.rs`의 `pub type PathRequest = RepoChangesRequest;`(필드 `path` 하나, 별칭으로도 `PathRequest { path }` 구조체 문법을 쓸 수 있다). P2-08도 이 별칭을 쓴다.
- `repo_branch_create(request: BranchCreateRequest { path, name, start_point: Option<String>, checkout: bool, operation_id }) -> Result<(), String>`
- `repo_switch(request: SwitchRequest { path, branch, operation_id }) -> Result<(), String>`(원격 branch 이름이면 `git switch --track <remote>/<name>`)
- `repo_branch_rename(request: BranchRenameRequest { path, from, to, operation_id }) -> Result<(), String>`
- `repo_branch_delete(request: BranchDeleteRequest { path, name, operation_id }) -> Result<DeletedBranch { name, commit }, String>`
- 오류 코드: `branch_name_invalid`, `branch_exists`, `branch_missing`, `branch_in_use`, `switch_blocked_by_changes`, `branch_operation_failed`

- [ ] **Step 1: 실패하는 테스트** — `commands/branches.rs` 테스트 모듈(`crate::test_support::{git, init_repo}`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{git, init_repo};
    use std::fs;

    fn block<T>(future: impl std::future::Future<Output = T>) -> T { crate::runtime::block_on(future) }
    fn path(dir: &std::path::Path) -> String { dir.to_string_lossy().into_owned() }

    #[test]
    fn create_switch_rename_and_list() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        block(repo_branch_create(BranchCreateRequest { path: path(tmp.path()), name: "feature/a".into(), start_point: None, checkout: true, operation_id: "b1".into() })).unwrap();
        assert_eq!(git(tmp.path(), &["branch", "--show-current"]).trim(), "feature/a");
        block(repo_branch_rename(BranchRenameRequest { path: path(tmp.path()), from: "feature/a".into(), to: "feature/b".into(), operation_id: "b2".into() })).unwrap();
        block(repo_switch(SwitchRequest { path: path(tmp.path()), branch: "main".into(), operation_id: "b3".into() })).unwrap();
        let list = block(repo_branches(PathRequest { path: path(tmp.path()) })).unwrap();
        assert_eq!(list.current.as_deref(), Some("main"));
        assert!(list.branches.iter().any(|b| b.name == "feature/b"));
        assert_eq!(
            block(repo_branch_create(BranchCreateRequest { path: path(tmp.path()), name: "main".into(), start_point: None, checkout: false, operation_id: "b4".into() })).unwrap_err(),
            "branch_exists"
        );
        assert_eq!(
            block(repo_branch_create(BranchCreateRequest { path: path(tmp.path()), name: "-bad".into(), start_point: None, checkout: false, operation_id: "b5".into() })).unwrap_err(),
            "branch_name_invalid"
        );
    }

    #[test]
    fn switching_is_refused_when_local_changes_would_be_overwritten() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        git(tmp.path(), &["switch", "--quiet", "-c", "other"]);
        fs::write(tmp.path().join("README.md"), "other\n").unwrap();
        git(tmp.path(), &["commit", "--quiet", "-am", "other"]);
        git(tmp.path(), &["switch", "--quiet", "main"]);
        fs::write(tmp.path().join("README.md"), "local edit\n").unwrap();
        let error = block(repo_switch(SwitchRequest { path: path(tmp.path()), branch: "other".into(), operation_id: "s1".into() })).unwrap_err();
        assert_eq!(error, "switch_blocked_by_changes");
        assert_eq!(fs::read_to_string(tmp.path().join("README.md")).unwrap(), "local edit\n");
        assert_eq!(git(tmp.path(), &["branch", "--show-current"]).trim(), "main");
    }

    #[test]
    fn delete_returns_the_tip_for_undo_and_refuses_branches_in_use() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        git(tmp.path(), &["switch", "--quiet", "-c", "unmerged"]);
        fs::write(tmp.path().join("new.txt"), "x\n").unwrap();
        git(tmp.path(), &["add", "new.txt"]);
        git(tmp.path(), &["commit", "--quiet", "-m", "unmerged work"]);
        let tip = git(tmp.path(), &["rev-parse", "HEAD"]).trim().to_string();
        assert_eq!(block(repo_branch_delete(BranchDeleteRequest { path: path(tmp.path()), name: "unmerged".into(), operation_id: "d1".into() })).unwrap_err(), "branch_in_use");
        git(tmp.path(), &["switch", "--quiet", "main"]);
        let deleted = block(repo_branch_delete(BranchDeleteRequest { path: path(tmp.path()), name: "unmerged".into(), operation_id: "d2".into() })).unwrap();
        assert_eq!(deleted.commit, tip);
        block(repo_branch_create(BranchCreateRequest { path: path(tmp.path()), name: "unmerged".into(), start_point: Some(tip.clone()), checkout: false, operation_id: "d3".into() })).unwrap();
        assert_eq!(git(tmp.path(), &["rev-parse", "unmerged"]).trim(), tip);
    }

    #[test]
    fn detached_head_lists_without_a_current_branch() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        git(tmp.path(), &["switch", "--quiet", "--detach", "HEAD"]);
        let list = block(repo_branches(PathRequest { path: path(tmp.path()) })).unwrap();
        assert!(list.detached && list.current.is_none());
    }
}
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-repositories-engine --lib -- commands::branches` → FAIL
- [ ] **Step 3: 구현**
  - `repo_branches`: `git for-each-ref --format=<FOR_EACH_REF_FORMAT> --count=2001 refs/heads refs/remotes`(2,000개 넘으면 `truncated: true`로 2,000개만), 현재 branch는 `git branch --show-current`(빈 값이면 detached). 읽기 작업이라 `begin_git_operation` 없이 `spawn_git_task` + `run_git_status_bounded`.
  - 쓰기 네 개: `begin_git_operation(&operation_id, …)` → `validated_repository_context` → `bind_repository` → `revalidate_repository_context` → `run_git_mutation_with_cancel`. 인자: 만들기 `branch <name> [<start>]`(checkout이면 `switch -c <name> [<start>]`), 전환 `switch <branch>`(이름이 `refs/remotes`에만 있으면 `switch --track <branch>`), 이름 바꾸기 `branch -m <from> <to>`, 지우기는 먼저 `rev-parse --verify refs/heads/<name>`로 tip을 읽고(없으면 `branch_missing`) `branch -D <name>`. `start_point`는 40·64자리 hex commit id이거나 `valid_worktree_branch`를 통과한 ref여야 한다.
  - git 실패 메시지를 오류 코드로 바꾸는 순수 함수 `classify_branch_failure(stderr: &str) -> &'static str`: `already exists` → `branch_exists`, `checked out at` 또는 `Cannot delete branch` → `branch_in_use`, `would be overwritten by checkout`·`Please commit your changes or stash them` → `switch_blocked_by_changes`, `not found`·`invalid reference` → `branch_missing`, 그 밖 → `branch_operation_failed`. 기존 mutation 도우미가 stderr를 버린다면, 이 파일 전용으로 stderr 앞 4KiB를 돌려주는 변형(`run_git_mutation_capture`)을 `commands.rs`에 추가한다(같은 제한·환경·취소 규칙). `classify_branch_failure`는 단위 테스트를 따로 둔다.
- [ ] **Step 4: 확인·커밋** — Run: Step 2 명령 → PASS. `git add -A && git commit -m "feat(devbox-workspace): create, switch, rename and delete branches"`

---

### Task 3: stash 명령

**Files:** Create `crates/repositories-engine/src/commands/stash.rs`

**Interfaces (Produces):** `repo_stash_list(PathRequest) -> Result<Vec<StashEntry>, String>`, `repo_stash_push(StashPushRequest { path, message: Option<String>, include_untracked: bool, operation_id }) -> Result<(), String>`, `repo_stash_apply(StashApplyRequest { path, index: u32, pop: bool, operation_id }) -> Result<StashApplyResult { applied: bool, conflicts: Vec<String> }, String>`, `repo_stash_drop(StashDropRequest { path, index: u32, operation_id }) -> Result<DroppedStash { commit, message }, String>`, `repo_stash_store(StashStoreRequest { path, commit, message, operation_id }) -> Result<(), String>`; 오류 `stash_empty`(저장할 변경 없음), `stash_missing`, `stash_operation_failed`

- [ ] **Step 1: 실패하는 테스트**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{git, init_repo};
    use std::fs;

    fn block<T>(future: impl std::future::Future<Output = T>) -> T { crate::runtime::block_on(future) }
    fn path(dir: &std::path::Path) -> String { dir.to_string_lossy().into_owned() }

    #[test]
    fn push_list_pop_round_trip_including_untracked_files() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        fs::write(tmp.path().join("README.md"), "edit\n").unwrap();
        fs::write(tmp.path().join("new.txt"), "untracked\n").unwrap();
        block(repo_stash_push(StashPushRequest { path: path(tmp.path()), message: Some("wip login".into()), include_untracked: true, operation_id: "s1".into() })).unwrap();
        assert!(git(tmp.path(), &["status", "--porcelain"]).is_empty());
        let list = block(repo_stash_list(PathRequest { path: path(tmp.path()) })).unwrap();
        assert!(list[0].message.ends_with("wip login"));
        let result = block(repo_stash_apply(StashApplyRequest { path: path(tmp.path()), index: 0, pop: true, operation_id: "s2".into() })).unwrap();
        assert!(result.applied && result.conflicts.is_empty());
        assert_eq!(fs::read_to_string(tmp.path().join("new.txt")).unwrap(), "untracked\n");
        assert!(block(repo_stash_list(PathRequest { path: path(tmp.path()) })).unwrap().is_empty());
        assert_eq!(
            block(repo_stash_push(StashPushRequest { path: path(tmp.path()), message: None, include_untracked: false, operation_id: "s3".into() })).map(|_| ()),
            Ok(())
        );
    }

    #[test]
    fn clean_tree_has_nothing_to_stash() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        assert_eq!(block(repo_stash_push(StashPushRequest { path: path(tmp.path()), message: None, include_untracked: false, operation_id: "c1".into() })).unwrap_err(), "stash_empty");
    }

    #[test]
    fn conflicting_pop_keeps_the_stash_and_reports_files() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        fs::write(tmp.path().join("README.md"), "stashed\n").unwrap();
        git(tmp.path(), &["stash", "push", "--quiet"]);
        fs::write(tmp.path().join("README.md"), "committed\n").unwrap();
        git(tmp.path(), &["commit", "--quiet", "-am", "conflicting"]);
        let result = block(repo_stash_apply(StashApplyRequest { path: path(tmp.path()), index: 0, pop: true, operation_id: "p1".into() })).unwrap();
        assert!(!result.applied);
        assert_eq!(result.conflicts, vec!["README.md".to_string()]);
        assert_eq!(block(repo_stash_list(PathRequest { path: path(tmp.path()) })).unwrap().len(), 1);
    }

    #[test]
    fn dropped_stash_can_be_stored_back() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        fs::write(tmp.path().join("README.md"), "keep me\n").unwrap();
        git(tmp.path(), &["stash", "push", "--quiet", "-m", "keep"]);
        let dropped = block(repo_stash_drop(StashDropRequest { path: path(tmp.path()), index: 0, operation_id: "r1".into() })).unwrap();
        assert!(block(repo_stash_list(PathRequest { path: path(tmp.path()) })).unwrap().is_empty());
        block(repo_stash_store(StashStoreRequest { path: path(tmp.path()), commit: dropped.commit.clone(), message: dropped.message.clone(), operation_id: "r2".into() })).unwrap();
        let list = block(repo_stash_list(PathRequest { path: path(tmp.path()) })).unwrap();
        assert_eq!(list[0].commit, dropped.commit);
        assert_eq!(block(repo_stash_drop(StashDropRequest { path: path(tmp.path()), index: 5, operation_id: "r3".into() })).unwrap_err(), "stash_missing");
    }
}
```

  (두 번째 `push` 호출은 첫 `pop` 뒤 남은 변경을 다시 저장하므로 성공해야 한다.)
- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-repositories-engine --lib -- commands::stash` → FAIL
- [ ] **Step 3: 구현** — 목록: `git stash list --format=<STASH_FORMAT>`(최대 500개). 저장: 먼저 `git status --porcelain`(untracked 포함 여부에 맞춰)이 비면 `stash_empty`, 아니면 `stash push [--include-untracked] [--message <m>]`(메시지는 `validate_commit_message` 규칙, 200바이트 한도). 적용: 목록에서 `index`가 없으면 `stash_missing`; `stash apply|pop stash@{N}` 실패 시 `git diff --name-only --diff-filter=U -z`로 충돌 파일을 모아 `applied: false`로 돌려준다(충돌이 없는 실패는 `stash_operation_failed`). 지우기: 목록에서 commit·message를 읽은 뒤 `stash drop stash@{N}`. 되살리기: `stash store --message <m> <commit>`(commit은 40·64자리 hex).
- [ ] **Step 4: 확인·커밋** — Run: Step 2 명령 → PASS. `git add -A && git commit -m "feat(devbox-workspace): stash push, apply, pop, drop and restore"`

---

### Task 4: 메서드 등록

**Files:** Modify `crates/repositories-engine/src/{api.rs,component.rs}`(P1-14 `SourceCall`·`SOURCE_COMMANDS`), `crates/wsl-helper/src/control.rs`, `apps/devbox-workspace/src-tauri/tests/{typescript.rs,fixtures/allow-table.json}`, `packages/workspace-features/src/issues/source.ts`

- [ ] **Step 1: 실패하는 테스트** — `api.rs` 테스트: 새 메서드 10개(`repo_branches`, `repo_branch_create`, `repo_switch`, `repo_branch_rename`, `repo_branch_delete`, `repo_stash_list`, `repo_stash_push`, `repo_stash_apply`, `repo_stash_drop`, `repo_stash_store`)가 `{"method": …, "args": {"request": {...}}}` 모양으로 파싱되고 lane `Source`, 예산은 목록 두 개만 `DEFAULT_BUDGET_MS`, 나머지 `LONG_BUDGET_MS`. `control.rs` 테스트: 10개 모두 `source_method`가 true. 이슈 카탈로그 테스트(P1-12 모양)가 새 코드 전부에 문구가 있는지 확인한다.
- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-repositories-engine --lib api && cargo test -p workspace-wsl --lib source_method && pnpm --filter @devbox/workspace-features exec vitest run src/issues` → FAIL
- [ ] **Step 3: 구현** — variant·목록·허용 목록 추가, `tests/typescript.rs`에 새 요청·결과 타입 추가, 허용 표 fixture 다시 쓰기(`UPDATE_ALLOW_TABLE=1`, diff가 새 10행뿐인지 확인). 문구: `branch_name_invalid: "branch 이름을 확인해 주세요."`, `branch_exists: "같은 이름의 branch가 이미 있습니다."`, `branch_missing: "branch를 찾지 못했습니다. 목록을 새로 고쳐 주세요."`, `branch_in_use: "현재 branch이거나 다른 작업 폴더에서 사용 중인 branch입니다."`, `switch_blocked_by_changes: "커밋하지 않은 변경이 전환할 branch와 겹칩니다. 변경을 커밋하거나 stash에 저장한 뒤 전환해 주세요."`, `branch_operation_failed: "branch 작업을 완료하지 못했습니다."`, `stash_empty: "저장할 변경이 없습니다."`, `stash_missing: "stash를 찾지 못했습니다. 목록을 새로 고쳐 주세요."`, `stash_operation_failed: "stash 작업을 완료하지 못했습니다."`
- [ ] **Step 4: 확인·커밋** — Run: Step 2 명령과 `cargo test -p devbox-workspace --lib allow_table && bash .github/scripts/check-generated-bindings.sh` → PASS. `git add -A && git commit -m "feat(devbox-workspace): route branch and stash methods"`

---

### Task 5: 화면

**Files:** Create `packages/workspace-features/src/source/components/{BranchPanel.tsx,BranchPanel.test.tsx,StashPanel.tsx,StashPanel.test.tsx}`; Modify `packages/workspace-features/src/source/{api.ts,SourcePanel.tsx}`

- [ ] **Step 1: 실패하는 테스트**
  - `BranchPanel.test.tsx`(기존 `StageCommitPanel.test.tsx`의 transport mock 방식):
    - 목록: 현재 branch에 "현재" 표시, upstream이 있으면 "앞섬 2 · 뒤처짐 1", 다른 worktree에서 쓰는 branch는 "다른 작업 폴더에서 사용 중"이고 전환·삭제 버튼이 없다. 원격 branch는 접힌 "원격 branch" 안에 있고 버튼은 "추적 branch로 전환".
    - "새 branch" 폼(이름, "만든 뒤 전환" 체크 기본 켬) 제출 → `repo_branch_create {request: {path, name, startPoint: null, checkout: true, operationId}}` 후 목록 새로 고침.
    - "삭제" → 확인 없이 `repo_branch_delete` → "branch 'x'를 삭제했습니다." 토스트와 "되돌리기" → 누르면 `repo_branch_create {name: "x", startPoint: <commit>, checkout: false}`. (P1-18 `useUndo`, 가짜 타이머로 8초 뒤 버튼이 사라지는 것도 확인)
    - `switch_blocked_by_changes`면 경고 문구와 "변경을 stash에 저장" 버튼(누르면 StashPanel의 저장 흐름과 같은 `repo_stash_push`)이 보인다.
    - axe 위반 0.
  - `StashPanel.test.tsx`: 목록 표시, "변경 임시 저장"(메시지, "추적하지 않는 파일 포함") → `repo_stash_push`, "적용"·"꺼내기" → `repo_stash_apply {pop: false|true}`, 충돌 결과면 "충돌 파일 1개: README.md" 경고, "삭제" → 되돌리기 토스트 → `repo_stash_store`. axe 위반 0.
- [ ] **Step 2: 실패 확인** — Run: `pnpm --filter @devbox/workspace-features exec vitest run src/source/components/BranchPanel.test.tsx src/source/components/StashPanel.test.tsx` → FAIL
- [ ] **Step 3: 구현** — `api.ts`에 10개 wrapper(`repoBranches`, `repoBranchCreate`, `repoSwitch`, `repoBranchRename`, `repoBranchDelete`, `repoStashList`, `repoStashPush`, `repoStashApply`, `repoStashDrop`, `repoStashStore`; 모두 `request: {path, …, operationId: crypto.randomUUID()}`). 패널은 `useOperation`(P1-15)으로 busy·오류를 다루고 `onBusyChange`를 올린다(다른 패널과 같은 props: `repo`, `onBusyChange`). `SourcePanel.tsx`의 callbacks 이름 목록에 `"branches","stash"`를 더하고 `HistoryDiffPanel` 앞에 두 패널을 넣는다. 전환·stash 뒤에는 `SourcePanel`의 저장소 상태를 새로 고치도록 `onChanged` prop을 둔다.
- [ ] **Step 4: 확인·커밋** — Run: Step 2 명령 → PASS, `pnpm --filter @devbox/workspace-features exec vitest run src/source` → PASS. `git add -A && git commit -m "feat(devbox-workspace): branch and stash panels"`

---

### Task 6: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9.
- [ ] PR 본문 "Windows 실기 확인"(사용자 확인 대기): WSL 프로젝트와 Windows 프로젝트 각각에서 branch 만들기·전환·삭제·되돌리기, stash 저장·꺼내기·삭제·되돌리기.
