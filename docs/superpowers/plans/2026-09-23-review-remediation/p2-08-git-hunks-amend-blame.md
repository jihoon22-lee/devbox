# P2-08 Git 보강 2: hunk 단위 작업·amend·blame — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4와 `CONVENTIONS.md`의 "메서드 추가 절차"(P1-11)를 읽는다.

**Goal:** Source 화면에서 파일의 변경 덩어리(hunk)를 골라 stage·unstage·버리기, 마지막 커밋 수정(amend), 파일 줄별 작성 이력(blame)을 할 수 있게 한다(D25 3순위, `review.md` §8 표 6번).

**Architecture:**
- hunk: 서버가 `git diff`로 파일 patch를 다시 만들고, 프런트는 hunk id(내용 해시)와 patch revision만 보낸다. 서버는 revision이 같을 때만 자기 patch에서 고른 hunk로 부분 patch를 만들어 `git apply`(`--cached`/`--reverse`)한다. 프런트가 보낸 patch 텍스트는 쓰지 않는다. 부분 patch는 저장소 git 폴더의 임시 파일(`<git-dir>/devbox-hunk-<nonce>.patch`, 작업 후 삭제)로 넘긴다(새 의존성·stdin 경로 없이).
- 지원 범위: 수정(M)된 텍스트 파일. 새 파일·삭제·이름 변경·모드 변경·binary는 `supported: false`로 알리고 기존 파일 단위 stage를 쓰게 한다.
- amend: 기존 `repo_commit` 요청에 `amend: bool`(serde 기본 false)을 더한다. 새 읽기 메서드 `repo_last_commit`이 마지막 커밋 메시지와 "이미 push됨" 여부를 준다.
- blame: `git blame --porcelain`을 파싱해 줄마다 commit, commit마다 작성자·시각·요약을 준다(최대 10,000줄).
- 새 코드는 `crates/repositories-engine/src/core/{hunks.rs,blame.rs}`(순수)와 `src/commands/{hunks.rs,blame.rs}`(실행)에 둔다. 메서드 등록은 P2-07 Task 4와 같은 경로(`SourceCall`, `SOURCE_COMMANDS`, helper `source_method`, 생성 TS, 허용 표, 이슈 카탈로그).

**Tech Stack:** Rust(git CLI), React 19, Vitest

**Spec:** `review.md` §8 신규 기능 표 6번 · `00-roadmap.md` D13·D25

## Global Constraints

- `00-roadmap.md` §3 전부 적용. 새 의존성 없음.
- diff는 항상 `--no-ext-diff --no-textconv --no-color -U3`(기존 규칙). 파일 경로는 기존 `validate_change_path`를 통과해야 한다.
- 한도: 파일 patch 2MiB(넘으면 `supported: false, reason: "too_large"`), hunk 선택 200개, blame 출력 4MiB·10,000줄.
- hunk 버리기는 되돌릴 수 없으므로 화면 안 확인 한 번을 둔다(D13의 삭제 계열). stage·unstage는 확인 없이 바로.

## Review Focus

1. 화면을 연 뒤 파일이 바뀌었으면(편집기 저장 등) hunk 작업은 `hunk_stale`로 거부되고 파일이 그대로다. (Task 2 테스트)
2. 두 hunk 중 두 번째만 stage하면 index에는 두 번째 변경만 들어가고 작업 폴더는 그대로다. (Task 2 테스트)
3. 새 파일·binary·이름 변경 파일은 hunk 버튼 대신 "파일 단위로만 처리할 수 있습니다"가 보인다. (Task 2·6 테스트)
4. 이미 push한 커밋을 amend하려 하면 경고가 보이고, 그래도 진행할 수 있다. (Task 3·6 테스트)
5. 커밋이 없는 새 저장소에서 amend는 `amend_no_commit`, blame은 `blame_unavailable`로 끝난다. (Task 3·4 테스트)

## Branch · PR

- 묶음: **B11** — 브랜치 `feat/devbox-workspace/git-workflows`, PR 제목 `feat(devbox-workspace): branches, stash, hunks, amend, blame, conflicts and pull requests`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `feat(devbox-workspace): hunk staging, amend and blame in Source`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

---

### Task 1: hunk 파서와 부분 patch

**Files:** Create `crates/repositories-engine/src/core/hunks.rs`; Modify `core/mod.rs`

**Interfaces (Produces):** `HunkLineKind { Context, Add, Remove, NoNewline }`, `HunkLine { kind, text: String }`, `Hunk { id: String, header: String, old_start: u32, old_count: u32, new_start: u32, new_count: u32, lines: Vec<HunkLine> }`, `FilePatch { header: String, hunks: Vec<Hunk> }`(serde camelCase, TS), `parse_file_patch(patch: &str) -> Result<FilePatch, String>`, `unsupported_reason(patch: &str) -> Option<&'static str>`(`"new_file"`, `"deleted_file"`, `"rename"`, `"mode_change"`, `"binary"`), `partial_patch(file: &FilePatch, ids: &[String]) -> Result<String, String>`, `patch_revision(patch: &str) -> String`(SHA-256 hex)

- [ ] **Step 1: 실패하는 테스트**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const PATCH: &str = "diff --git a/src/app.rs b/src/app.rs\nindex 1111111..2222222 100644\n--- a/src/app.rs\n+++ b/src/app.rs\n@@ -1,3 +1,3 @@ fn main() {\n line one\n-line two\n+line 2\n line three\n@@ -10,2 +10,3 @@\n ten\n+ten and a half\n eleven\n\\ No newline at end of file\n";

    #[test]
    fn hunks_parse_with_ranges_and_stable_ids() {
        let file = parse_file_patch(PATCH).unwrap();
        assert!(file.header.starts_with("diff --git a/src/app.rs b/src/app.rs\n"));
        assert!(file.header.ends_with("+++ b/src/app.rs\n"));
        assert_eq!(file.hunks.len(), 2);
        assert_eq!((file.hunks[0].old_start, file.hunks[0].old_count, file.hunks[0].new_start, file.hunks[0].new_count), (1, 3, 1, 3));
        assert_eq!(file.hunks[1].lines.last().unwrap().kind, HunkLineKind::NoNewline);
        assert_eq!(parse_file_patch(PATCH).unwrap().hunks[1].id, file.hunks[1].id);
        assert_ne!(file.hunks[0].id, file.hunks[1].id);
    }

    #[test]
    fn partial_patch_keeps_the_header_and_only_selected_hunks() {
        let file = parse_file_patch(PATCH).unwrap();
        let partial = partial_patch(&file, &[file.hunks[1].id.clone()]).unwrap();
        assert!(partial.starts_with(&file.header));
        assert!(partial.contains("+ten and a half\n"));
        assert!(!partial.contains("+line 2\n"));
        assert!(partial.ends_with("\\ No newline at end of file\n"));
        assert_eq!(partial_patch(&file, &["missing".into()]).unwrap_err(), "hunk_stale");
        assert_eq!(partial_patch(&file, &[]).unwrap_err(), "hunk_selection_invalid");
    }

    #[test]
    fn special_files_are_reported_as_unsupported() {
        assert_eq!(unsupported_reason("diff --git a/n b/n\nnew file mode 100644\n"), Some("new_file"));
        assert_eq!(unsupported_reason("diff --git a/d b/d\ndeleted file mode 100644\n"), Some("deleted_file"));
        assert_eq!(unsupported_reason("diff --git a/a b/b\nsimilarity index 90%\nrename from a\nrename to b\n"), Some("rename"));
        assert_eq!(unsupported_reason("diff --git a/x b/x\nold mode 100644\nnew mode 100755\n"), Some("mode_change"));
        assert_eq!(unsupported_reason("diff --git a/i b/i\nBinary files a/i and b/i differ\n"), Some("binary"));
        assert_eq!(unsupported_reason(PATCH), None);
    }
}
```

- [ ] **Step 2: 실패 확인** — Run: `source ~/.cargo/env && cargo test -p devbox-repositories-engine --lib -- core::hunks` → FAIL
- [ ] **Step 3: 구현** — header는 첫 `@@ ` 줄 앞까지 전부. hunk 머리 `@@ -a[,b] +c[,d] @@[ 문맥]`(개수 생략은 1). 본문 줄은 첫 글자 ` `·`+`·`-`·`\`로 종류를 정하고 그 밖은 오류. `id` = `patch_revision(header 줄 + 본문)`의 앞 16자. `partial_patch`는 header + 선택된 hunk의 원문 줄을 원래 순서대로 이어 붙인다(선택 중 없는 id가 있으면 `hunk_stale`, 빈 선택·201개 이상은 `hunk_selection_invalid`).
- [ ] **Step 4: 확인·커밋** — Run: Step 2 명령 → PASS. `git add -A && git commit -m "feat(devbox-workspace): parse hunks and build partial patches"`

---

### Task 2: hunk 명령

**Files:** Create `crates/repositories-engine/src/commands/hunks.rs`

**Interfaces (Produces):** `repo_file_hunks(FileHunksRequest { path, file, staged: bool }) -> Result<FileHunks { file, staged, supported: bool, reason: Option<String>, revision: String, hunks: Vec<Hunk> }, String>`, `repo_hunks_apply(HunksApplyRequest { path, file, staged: bool, action: HunkAction, hunk_ids: Vec<String>, revision: String, operation_id }) -> Result<(), String>`, `HunkAction { Stage, Unstage, Discard }`; 오류 `hunk_stale`, `hunk_selection_invalid`, `hunk_unsupported`, `hunk_apply_failed`

- [ ] **Step 1: 실패하는 테스트**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{git, init_repo};
    use std::fs;

    fn block<T>(future: impl std::future::Future<Output = T>) -> T { crate::runtime::block_on(future) }
    fn path(dir: &std::path::Path) -> String { dir.to_string_lossy().into_owned() }
    fn lines(n: usize) -> String { (1..=n).map(|i| format!("line {i}\n")).collect() }

    fn two_hunk_repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        fs::write(tmp.path().join("a.txt"), lines(30)).unwrap();
        git(tmp.path(), &["add", "a.txt"]);
        git(tmp.path(), &["commit", "--quiet", "-m", "thirty lines"]);
        let edited = lines(30).replace("line 2\n", "line two\n").replace("line 28\n", "line twenty-eight\n");
        fs::write(tmp.path().join("a.txt"), edited).unwrap();
        tmp
    }

    #[test]
    fn staging_one_hunk_leaves_the_other_in_the_worktree() {
        let tmp = two_hunk_repo();
        let view = block(repo_file_hunks(FileHunksRequest { path: path(tmp.path()), file: "a.txt".into(), staged: false })).unwrap();
        assert!(view.supported);
        assert_eq!(view.hunks.len(), 2);
        block(repo_hunks_apply(HunksApplyRequest {
            path: path(tmp.path()), file: "a.txt".into(), staged: false, action: HunkAction::Stage,
            hunk_ids: vec![view.hunks[1].id.clone()], revision: view.revision.clone(), operation_id: "h1".into(),
        })).unwrap();
        let cached = git(tmp.path(), &["diff", "--cached"]);
        assert!(cached.contains("+line twenty-eight") && !cached.contains("+line two"));
        assert!(git(tmp.path(), &["diff"]).contains("+line two"));
    }

    #[test]
    fn unstaging_and_discarding_hunks() {
        let tmp = two_hunk_repo();
        git(tmp.path(), &["add", "a.txt"]);
        let staged = block(repo_file_hunks(FileHunksRequest { path: path(tmp.path()), file: "a.txt".into(), staged: true })).unwrap();
        block(repo_hunks_apply(HunksApplyRequest {
            path: path(tmp.path()), file: "a.txt".into(), staged: true, action: HunkAction::Unstage,
            hunk_ids: vec![staged.hunks[0].id.clone()], revision: staged.revision.clone(), operation_id: "h2".into(),
        })).unwrap();
        assert!(!git(tmp.path(), &["diff", "--cached"]).contains("+line two"));
        let worktree = block(repo_file_hunks(FileHunksRequest { path: path(tmp.path()), file: "a.txt".into(), staged: false })).unwrap();
        block(repo_hunks_apply(HunksApplyRequest {
            path: path(tmp.path()), file: "a.txt".into(), staged: false, action: HunkAction::Discard,
            hunk_ids: vec![worktree.hunks[0].id.clone()], revision: worktree.revision.clone(), operation_id: "h3".into(),
        })).unwrap();
        assert!(fs::read_to_string(tmp.path().join("a.txt")).unwrap().contains("line 2\n"));
        let git_dir = tmp.path().join(".git");
        assert!(fs::read_dir(&git_dir).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().starts_with("devbox-hunk-")), "temporary patches are removed");
    }

    #[test]
    fn a_changed_file_makes_the_selection_stale() {
        let tmp = two_hunk_repo();
        let view = block(repo_file_hunks(FileHunksRequest { path: path(tmp.path()), file: "a.txt".into(), staged: false })).unwrap();
        fs::write(tmp.path().join("a.txt"), lines(31)).unwrap();
        let error = block(repo_hunks_apply(HunksApplyRequest {
            path: path(tmp.path()), file: "a.txt".into(), staged: false, action: HunkAction::Stage,
            hunk_ids: vec![view.hunks[0].id.clone()], revision: view.revision, operation_id: "h4".into(),
        })).unwrap_err();
        assert_eq!(error, "hunk_stale");
        assert!(git(tmp.path(), &["diff", "--cached"]).is_empty());
    }

    #[test]
    fn actions_must_match_the_side_and_file_kind() {
        let tmp = two_hunk_repo();
        let view = block(repo_file_hunks(FileHunksRequest { path: path(tmp.path()), file: "a.txt".into(), staged: false })).unwrap();
        let wrong = block(repo_hunks_apply(HunksApplyRequest {
            path: path(tmp.path()), file: "a.txt".into(), staged: false, action: HunkAction::Unstage,
            hunk_ids: vec![view.hunks[0].id.clone()], revision: view.revision, operation_id: "h5".into(),
        })).unwrap_err();
        assert_eq!(wrong, "hunk_selection_invalid");
        fs::write(tmp.path().join("new.txt"), "new\n").unwrap();
        git(tmp.path(), &["add", "-N", "new.txt"]);
        let new_file = block(repo_file_hunks(FileHunksRequest { path: path(tmp.path()), file: "new.txt".into(), staged: false })).unwrap();
        assert_eq!((new_file.supported, new_file.reason.as_deref()), (false, Some("new_file")));
    }
}
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-repositories-engine --lib -- commands::hunks` → FAIL
- [ ] **Step 3: 구현**
  - `repo_file_hunks`: `validate_change_path(&file)`; `git diff [--cached] --no-ext-diff --no-textconv --no-color -U3 -- <file>`(출력 2MiB 넘으면 `supported: false, reason: "too_large"`). 빈 출력이면 `supported: true, hunks: []`. `unsupported_reason`이 있으면 hunk 없이 그 이유. revision은 `patch_revision(patch)`.
  - `repo_hunks_apply`: 허용 조합은 `(staged=false, Stage|Discard)`, `(staged=true, Unstage)`(그 밖 `hunk_selection_invalid`). `begin_git_operation` → 같은 diff를 다시 만들어 revision이 다르면 `hunk_stale` → `unsupported_reason`이 있으면 `hunk_unsupported` → `partial_patch` → `git rev-parse --absolute-git-dir`로 얻은 폴더에 `devbox-hunk-<pid>-<counter>-<millis>.patch`를 `create_new`로 쓰고, `git apply --cached --whitespace=nowarn <file>`(Stage) / `git apply --cached --reverse --whitespace=nowarn <file>`(Unstage) / `git apply --reverse --whitespace=nowarn <file>`(Discard)를 `run_git_mutation_with_cancel`로 실행 → 성공·실패와 관계없이 임시 파일 삭제(`Drop` guard). git 실패는 `hunk_apply_failed`.
- [ ] **Step 4: 확인·커밋** — Run: Step 2 명령 → PASS. `git add -A && git commit -m "feat(devbox-workspace): stage, unstage and discard hunks"`

---

### Task 3: amend

**Files:** Modify `crates/repositories-engine/src/commands.rs`(`CommitRequest`, `git_commit_args`, `repo_commit`), `src/commands/commit_review.rs`(빈 stage 허용 조건); Create `src/commands/last_commit.rs`

**Interfaces (Produces):** `CommitRequest { …, #[serde(default)] amend: bool }`, `git_commit_args(message: &str, amend: bool) -> Vec<String>`, `repo_last_commit(PathRequest) -> Result<LastCommit { id, message, pushed: bool, merge: bool }, String>`; 오류 `amend_no_commit`

- [ ] **Step 1: 실패하는 테스트** — `last_commit.rs` 테스트와 `commands.rs` 테스트

```rust
    #[test]
    fn amend_rewrites_the_last_commit_message_and_contents() {
        let tmp = tempfile::tempdir().unwrap();
        crate::test_support::init_repo(tmp.path());
        let root = tmp.path().to_string_lossy().into_owned();
        let before = crate::test_support::git(tmp.path(), &["rev-parse", "HEAD"]);
        std::fs::write(tmp.path().join("README.md"), "amended\n").unwrap();
        crate::test_support::git(tmp.path(), &["add", "README.md"]);
        let review = crate::runtime::block_on(repo_commit_preview(RepoChangesRequest { path: root.clone() })).unwrap();
        crate::runtime::block_on(repo_commit(CommitRequest {
            path: root.clone(), message: "base (amended)".into(), operation_id: "a1".into(), index_revision: review.revision, amend: true,
        })).unwrap();
        assert_ne!(crate::test_support::git(tmp.path(), &["rev-parse", "HEAD"]), before);
        assert_eq!(crate::test_support::git(tmp.path(), &["rev-list", "--count", "HEAD"]).trim(), "1");
        let last = crate::runtime::block_on(repo_last_commit(PathRequest { path: root })).unwrap();
        assert_eq!(last.message.trim(), "base (amended)");
        assert!(!last.pushed);
    }

    #[test]
    fn message_only_amend_needs_no_staged_changes() {
        let tmp = tempfile::tempdir().unwrap();
        crate::test_support::init_repo(tmp.path());
        let root = tmp.path().to_string_lossy().into_owned();
        let review = crate::runtime::block_on(repo_commit_preview(RepoChangesRequest { path: root.clone() })).unwrap();
        crate::runtime::block_on(repo_commit(CommitRequest {
            path: root, message: "better message".into(), operation_id: "a2".into(), index_revision: review.revision, amend: true,
        })).unwrap();
        assert_eq!(crate::test_support::git(tmp.path(), &["log", "-1", "--format=%s"]).trim(), "better message");
    }

    #[test]
    fn a_repository_without_commits_cannot_amend() {
        let tmp = tempfile::tempdir().unwrap();
        crate::test_support::git(tmp.path(), &["init", "--quiet", "-b", "main"]);
        let error = crate::runtime::block_on(repo_last_commit(PathRequest { path: tmp.path().to_string_lossy().into_owned() })).unwrap_err();
        assert_eq!(error, "amend_no_commit");
    }
```

  `last_commit.rs`에 upstream 테스트: bare 저장소를 원격으로 추가해 push한 뒤 `pushed == true`, 새 커밋을 더 만들면 `false`.
- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-repositories-engine --lib -- amend last_commit` → FAIL
- [ ] **Step 3: 구현** — `git_commit_args`에 `amend`면 `--amend`를 `commit` 뒤에 넣는다(기존 호출부는 `false`). amend일 때 `commit_review::require`의 "stage된 경로가 있어야 한다" 조건을 건너뛰고(있다면), index revision 비교는 그대로 한다. `repo_last_commit`: `git log -1 --format=%H%x00%P%x00%B`(실패 → `amend_no_commit`), `merge` = 부모 2개 이상, `pushed` = `git rev-parse --abbrev-ref --symbolic-full-name @{upstream}`가 성공하고 `git merge-base --is-ancestor HEAD @{upstream}`의 종료 코드가 0.
- [ ] **Step 4: 확인·커밋** — Run: Step 2 명령 → PASS. `git add -A && git commit -m "feat(devbox-workspace): amend the last commit"`

---

### Task 4: blame

**Files:** Create `crates/repositories-engine/src/core/blame.rs`, `src/commands/blame.rs`

**Interfaces (Produces):** `BlameLine { line: u32, commit: String, text: String }`, `BlameCommit { author: String, author_time: i64, summary: String }`, `Blame { file: String, lines: Vec<BlameLine>, commits: BTreeMap<String, BlameCommit>, truncated: bool }`(camelCase, TS), `parse_porcelain(output: &str, max_lines: usize) -> Result<Blame, String>`, `repo_blame(BlameRequest { path, file, commit_id: Option<String> }) -> Result<Blame, String>`; 오류 `blame_unavailable`

- [ ] **Step 1: 실패하는 테스트** — `core/blame.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const PORCELAIN: &str = concat!(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa 1 1 2\n",
        "author Kim\nauthor-mail <kim@example.test>\nauthor-time 1790000000\nauthor-tz +0900\n",
        "committer Kim\ncommitter-mail <kim@example.test>\ncommitter-time 1790000000\ncommitter-tz +0900\n",
        "summary first commit\nfilename a.txt\n\tline one\n",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa 2 2\n\tline two\n",
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb 3 3 1\n",
        "author Lee\nauthor-mail <lee@example.test>\nauthor-time 1790000500\nauthor-tz +0900\n",
        "committer Lee\ncommitter-mail <lee@example.test>\ncommitter-time 1790000500\ncommitter-tz +0900\n",
        "summary second\nprevious aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa a.txt\nfilename a.txt\n\tline three\n",
    );

    #[test]
    fn porcelain_blame_reuses_commit_headers() {
        let blame = parse_porcelain(PORCELAIN, 10_000).unwrap();
        assert_eq!(blame.lines.len(), 3);
        assert_eq!((blame.lines[1].line, blame.lines[1].text.as_str()), (2, "line two"));
        assert_eq!(blame.lines[1].commit, blame.lines[0].commit);
        assert_eq!(blame.commits[&blame.lines[2].commit].author, "Lee");
        assert_eq!(blame.commits[&blame.lines[0].commit].summary, "first commit");
        assert!(!blame.truncated);
    }

    #[test]
    fn line_limit_truncates() {
        let blame = parse_porcelain(PORCELAIN, 2).unwrap();
        assert_eq!(blame.lines.len(), 2);
        assert!(blame.truncated);
    }
}
```

  `commands/blame.rs`: `init_repo` 뒤 README 두 번째 줄을 다른 커밋으로 추가하고 `repo_blame`가 줄 2개·commit 2개를 돌려준다. 커밋 없는 저장소는 `blame_unavailable`.
- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-repositories-engine --lib -- blame` → FAIL
- [ ] **Step 3: 구현** — porcelain 블록: `<40|64 hex> <orig> <final> [<count>]` 줄로 시작하고, 그 commit을 처음 볼 때만 header 줄(`author`, `author-time`, `summary` 등)이 온 뒤 `\t<내용>` 줄로 끝난다. 모르는 header는 무시. 명령: `git blame --porcelain [<commit_id>] -- <file>`(`validate_change_path`, commit id는 hex만, 출력 4MiB). 실패 → `blame_unavailable`.
- [ ] **Step 4: 확인·커밋** — Run: Step 2 명령 → PASS. `git add -A && git commit -m "feat(devbox-workspace): blame files"`

---

### Task 5: 메서드 등록

**Files:** `crates/repositories-engine/src/{api.rs,component.rs}`, `crates/wsl-helper/src/control.rs`, `apps/devbox-workspace/src-tauri/tests/{typescript.rs,fixtures/allow-table.json}`, `packages/workspace-features/src/issues/source.ts`

- [ ] P2-07 Task 4와 같은 순서(실패하는 테스트 → 구현 → 확인)로 `repo_file_hunks`, `repo_hunks_apply`, `repo_last_commit`, `repo_blame`를 등록한다(`repo_commit`의 `amend` 필드는 기존 variant에 더해지는 것이라 허용 표 변화 없음). lane `Source`, 예산: `repo_hunks_apply`만 `LONG_BUDGET_MS`. 문구: `hunk_stale: "파일이 바뀌었습니다. 변경 덩어리를 다시 불러와 주세요."`, `hunk_selection_invalid: "선택한 변경 덩어리를 확인해 주세요."`, `hunk_unsupported: "이 파일은 파일 단위로만 처리할 수 있습니다."`, `hunk_apply_failed: "변경 덩어리를 적용하지 못했습니다."`, `amend_no_commit: "수정할 커밋이 없습니다."`, `blame_unavailable: "이 파일의 작성 이력을 읽지 못했습니다."`
- [ ] Run: `cargo test -p devbox-repositories-engine --lib api && cargo test -p workspace-wsl --lib source_method && cargo test -p devbox-workspace --lib allow_table && bash .github/scripts/check-generated-bindings.sh && pnpm --filter @devbox/workspace-features exec vitest run src/issues` → PASS. `git add -A && git commit -m "feat(devbox-workspace): route hunk, amend and blame methods"`

---

### Task 6: 화면

**Files:** Create `packages/workspace-features/src/source/components/{HunkList.tsx,HunkList.test.tsx,BlamePanel.tsx,BlamePanel.test.tsx}`; Modify `StageCommitPanel.tsx`(+test), `HistoryDiffPanel.tsx`, `SourcePanel.tsx`, `source/api.ts`

- [ ] **Step 1: 실패하는 테스트**
  - `HunkList.test.tsx`: 파일 행의 "변경 덩어리 보기"를 누르면 `repo_file_hunks`가 불리고 hunk마다 `@@` 머리와 +/- 줄(색만이 아니라 `+`/`-` 기호로도 구분)이 보인다. 작업 폴더 쪽 hunk에는 "이 덩어리 stage"·"이 덩어리 버리기", stage 쪽에는 "이 덩어리 unstage". "버리기"는 "이 변경을 버립니다. 되돌릴 수 없습니다." 확인 영역을 거친다. `hunk_stale` 오류면 목록을 다시 불러온다. `supported: false`면 "이 파일은 파일 단위로만 처리할 수 있습니다."(이유 `binary`면 "binary 파일"). axe 위반 0.
  - `StageCommitPanel.test.tsx`에 추가: "마지막 커밋 수정(amend)" 체크 → `repo_last_commit` 결과 메시지가 빈 입력칸에 채워지고, `pushed: true`면 "이미 push한 커밋입니다. 수정하면 다음 push가 거부될 수 있습니다." 경고. 커밋 버튼 이름이 "마지막 커밋 수정"이 되고 `repo_commit {request: {…, amend: true}}`를 보낸다. stage된 변경이 없어도 amend면 버튼이 활성이다.
  - `BlamePanel.test.tsx`: 경로 입력(또는 `HistoryDiffPanel`·`StageCommitPanel` 파일 행의 "작성 이력" 버튼에서 받은 경로) → `repo_blame` → 같은 commit이 이어지는 줄은 첫 줄에만 "짧은 id · 작성자 · 날짜 · 요약"이 보인다. commit id를 누르면 `onShowCommit(id)`가 불린다(HistoryDiffPanel의 상세 보기로 연결). `truncated`면 "처음 10,000줄만 보여 줍니다.". axe 위반 0.
- [ ] **Step 2: 실패 확인** — Run: `pnpm --filter @devbox/workspace-features exec vitest run src/source/components` → FAIL
- [ ] **Step 3: 구현** — `api.ts` wrapper 4개(`repoFileHunks`, `repoHunksApply`, `repoLastCommit`, `repoBlame`)와 `repoCommit`에 `amend` 인자. `HunkList`는 `StageCommitPanel`의 파일 행 아래에 펼쳐지는 부품(props: `repo`, `file`, `staged`, `onChanged`). `BlamePanel`은 `SourcePanel`의 `HistoryDiffPanel` 뒤에 둔다. `HistoryDiffPanel`에 `selectCommit(id)`를 부모가 부를 수 있는 prop(`focusCommit`)을 더해 blame에서 commit 상세로 이동한다. 날짜는 `Intl.DateTimeFormat("ko-KR", { dateStyle: "medium" })`.
- [ ] **Step 4: 확인·커밋** — Run: Step 2 명령과 `pnpm --filter @devbox/workspace-features exec vitest run src/source` → PASS. `git add -A && git commit -m "feat(devbox-workspace): hunk, amend and blame views"`

---

### Task 7: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9.
- [ ] PR 본문 "Windows 실기 확인"(사용자 확인 대기): WSL·Windows 프로젝트에서 한 파일의 두 변경 중 하나만 stage해 커밋, 다른 하나 버리기, 마지막 커밋 메시지 수정, 파일 blame에서 commit 상세로 이동.
