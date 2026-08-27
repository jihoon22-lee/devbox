# Repo Manager 설계 — Git Worktree Manager

- 상태: 제안(Proposal) — Stage 5
- 작성일: 2026-08-14
- 근거: `docs/product-opportunities.md` §15.6, §17.9

## 1. 제품 정의

지정 root 아래 Git repository를 탐색해 브랜치·worktree·상태를 목록화하고,
worktree 생성과 열기를 제공한다. 파괴적 동작(force delete·reset·clean)은 기본 제공하지 않는다.

## 2. MVP

- 지정 root 아래 Git repository 탐색
- branch·dirty·ahead/behind·worktree 목록
- worktree 생성
- Code Pad·WSL Desktop·Workbench로 열기
- merged/stale branch 후보
- remove 전 uncommitted/untracked 검사

## 3. 안전 경계

- force delete·reset·clean을 기본 동작으로 제공하지 않는다.
- worktree remove 전 uncommitted/untracked를 확인하고 안내한다.
- Windows/WSL path가 같은 repository를 중복 등록하지 않는다 —
  `crates/wsl`(devbox_wsl) `canonical_project_key` 재사용 (§7.1).

## 4. 아키텍처

```
apps/repo-manager/
├─ src-tauri/src/
│  ├─ core/
│  │  ├─ git.rs        # git 출력 파싱 (branch·dirty·ahead/behind, 순수)
│  │  └─ discover.rs   # root 아래 repository 탐색 (순수: .git 존재 판정)
│  └─ commands.rs      # scan_root, repo_status, create_worktree, open_in
└─ src/
   ├─ App.tsx          # 저장소 목록·상태·worktree·열기
   └─ api.ts
```

## 4.1 P2-16 — Git history·diff 읽기 경계 (#316)

Repo Manager는 Git을 매번 별도 GUI에서 열어 파일을 찾아 복사하는 흐름을 줄이기 위해
선택 repository의 최근 history·commit detail·변경 diff를 앱 안에서 확인한다. 이 PR의
기능은 **읽기 전용**이다. stage/unstage, commit, fetch, pull, push, branch/worktree
cleanup, reset, clean, force 동작은 별도 issue에서 다루며 이 명령의 request/handler에
존재하지 않는다.

### Request와 응답

- `repo_history({ path, limit })`는 `limit` 1..100으로 제한된 최근 commit 목록과
  `hasMore`를 반환한다. 각 entry는 full object ID, 12자 short ID, parent IDs, strict ISO
  authored timestamp, author/name/email, subject를 갖는다.
- `repo_commit_detail({ path, commitId })`는 bounded subject/body와 위 metadata를
  반환한다. `commitId`는 7..64자 hexadecimal object ID/short ID만 허용하고 arbitrary
  rev expression, `--`, pathspec, shell 문자열은 거부한다.
- `repo_diff({ path, commitId })`에서 `commitId: null`은 `HEAD` 대 현재 index/worktree의
  추적 파일 변경, 값은 selected commit patch다. 응답은 scope, selected ID, file별 relative path,
  oldPath, status, binary 여부, bounded patch, truncated를 반환한다.
- 모든 DTO는 camelCase와 `deny_unknown_fields`를 사용한다. 요청 path는 absolute,
  existing Git repository, `crates/wsl` canonical identity 경계로 재검증하며 control
  character·`.`/`..` traversal·device path를 허용하지 않는다.
- history UI는 parent ID와 root 표시가 있는 간결한 graph lane으로 목록의 commit 관계를
  보여 주되, 고정된 `--topo-order`와 Git parent metadata에 없는 priority나 가상의 branch를
  만들지 않는다.

### Native 실행과 안전 상한

- `crates/git::run_bounded(args, cwd, timeout, max_stdout_bytes)`를 사용한다. stdin/stderr와
  Git optional lock을
  닫고 Git child를 timeout 안에 종료하며, argument·UTF-8·stdout overflow·non-zero
  status는 안정적인 내부 error code로만 반환한다. Repo Manager command는 이를 하나의
  고정 UI 오류 `Git history 또는 diff를 불러올 수 없습니다.`로 매핑하고 raw stderr,
  repository path, remote URL, credential helper 출력, OS detail을 버린다.
- History stdout는 512 KiB, detail은 128 KiB, diff는 2 MiB와 5초 실행 상한을 갖는다.
  diff parser는 file 256개, file patch 512 KiB, relative path 16 KiB를 추가로 제한한다.
  상한 초과 결과는 partial raw output을 넘기지 않고 고정 오류 또는 명시적 `truncated`
  metadata로 처리한다.
- Git은 `--no-ext-diff`, `--no-textconv`, `--no-color`, `--no-renames`로 호출하고
  `--binary`를 사용하지 않는다. selected merge commit은 `-m` parent별 표준 patch로
  표시한다. binary 파일은 `Binary files differ` marker와 안전한
  path metadata만 반환하며 원본 bytes를 읽거나 UI에 렌더링하지 않는다. parser가 invalid
  UTF-8, malformed NUL record, unsafe relative path를 발견하면 전체 응답을 폐기한다.
- history/detail/diff는 working tree나 repository를 쓰지 않고, remote/network, credential
  storage, telemetry, permanent export를 호출하지 않는다. 화면에 표시되는 commit/diff
  원문은 사용자 명시 조회의 결과이며 새 localStorage/history를 만들지 않는다.

### Frontend interaction

선택된 repository 카드 아래에 History·diff panel을 표시한다. limit 입력과 `History 불러오기`,
commit 선택, `Working tree diff`/`Selected commit diff` action을 keyboard로 사용할 수 있고,
commit detail·binary·empty·oversize 상태를 명시적으로 표시한다. 작업 중에는 input/button을
잠그고 `busyRef`로 double-submit을 막는다. request sequence와 unmount cleanup은 이전
repository 또는 늦은 command response가 새 선택을 덮지 않도록 한다. limit Enter는 IME
composition 중 실행하지 않으며 `aria-busy`, `role=status`, `role=alert`, focus-visible
commit button을 제공한다.

## 4.2 P2-16 — selected stage·unstage·commit 경계 (#317)

Repo Manager는 선택 repository의 working tree status를 bounded porcelain-v1 NUL 출력으로
읽고, 사용자가 선택한 repository-relative path만 stage 또는 unstage한다. `repo_stage`는
`git add`, `repo_unstage`는 HEAD가 있는 저장소에서 `git restore --staged`, unborn 저장소에서
worktree를 보존하는 `git rm --cached`를 사용한다. 모든 path는 control/traversal/absolute/
pathspec magic을 거부하고 `--literal-pathspecs`와 `--` 뒤에 전달한다. status snapshot에 없는
path가 섞인 batch는 Git을 호출하지 않고 전체를 고정 오류로 거절한다.

History/diff header는 fixed `core.quotePath=false`·`--no-renames` 계약에서 old/new path가 같은
유일한 중앙 구분자를 검증한다. 따라서 공백 또는 ` b/` segment를 포함한 UTF-8 path를 임의의
마지막 separator로 잘못 자르지 않으며, quoted control/non-UTF-8 path는 fail-closed한다.

`repo_commit`은 non-empty bounded message를 받아 현재 index만 `git commit --message`로
commit하며 unstaged 파일을 자동 stage하지 않는다. native child는 `crates/git::run_mutating`의
bounded timeout/stdout, 닫힌 stdin/stderr를 사용한다. Git config와 credential helper는 Git
자체에 맡기고 devbox는 credential을 읽거나 저장하지 않으며, raw stderr/remote/path/message와
OS exception은 UI 오류에 포함하지 않는다. frontend는 explicit refresh/selection/action,
busyRef 중복 방지, sequence·unmount stale 폐기, 실패 시 selection/message 보존을 제공한다.

## 4.3 P2-16 — Git remote sync 경계 (#318)

Repo Manager는 선택 repository에서 configured remote의 최신 상태를 확인하고, fetch·
fast-forward-only/no-rebase pull·configured upstream destination으로 제한한 현재 branch push를
제공한다. frontend는 remote name, URL,
refspec, credential을 입력하지 않으며, force push·merge/rebase 자동화·reset·clean은 이 기능에
포함하지 않는다.

### Request와 preflight

- `repo_remote_status({ request: { path } })`는 bounded `git status --porcelain=v1 --branch`
  결과와 `MERGE_HEAD`, `CHERRY_PICK_HEAD`, `REVERT_HEAD`, `BISECT_LOG`, `rebase-merge`,
  `rebase-apply` marker를 사용해 `currentBranch`, `upstream`, `ahead`, `behind`, `dirty`,
  `detached`, `diverged`, `operationInProgress`를 반환한다. filename, remote URL, stderr,
  credential helper 정보는 DTO에 포함하지 않는다.
- fetch는 `git --no-pager --no-optional-locks fetch --no-tags`를 실행한다. working tree를
  변경하지 않으므로 dirty/detached/no-upstream/diverged 상태에서도 허용하지만 in-progress
  merge/rebase에서는 고정 오류로 차단한다.
- pull은 clean·attached·upstream·non-diverged 상태에서 `git --no-pager --no-optional-locks
  pull --ff-only --no-rebase`만 실행한다. push는 같은 preflight에서 native가 현재 branch의
  configured remote/upstream destination을 검증해 `git --no-pager --no-optional-locks push --
  <remote> HEAD:refs/heads/<destination>`만 실행한다. `push.default`나 별도 push refspec이 작업
  범위를 확장하지 못한다.
- detached, no-upstream, dirty, diverged, in-progress는 고정된 상태 오류로 표시하고, ambiguous
  status 또는 Git failure는 raw output 없는 공통 오류로 매핑한다. fetch/pull/push 중 merge,
  rebase, force, reset, clean argv는 생성하지 않는다.

### Native 실행과 취소 경계

- `repo_remote_status`는 512 KiB status/4 KiB marker output, 5초 read timeout을 사용하고,
  mutation은 64 KiB stdout·30초 timeout과 닫힌 stdin/stderr를 사용한다. Git credential helper
  해석은 Git에 맡기되 devbox는 credential을 읽거나 저장하지 않고 remote URL·stderr·raw path를
  반환하지 않는다. Windows에서는 kill-on-close Job Object에 root Git을 fail-closed로 편입하고
  Linux/WSL에서는 독립 process group을 사용해 hook·credential helper·SSH/transport descendant도
  cancel/timeout/drop 때 함께 종료한다. root 종료 뒤 tree를 먼저 닫아 inherited stdout pipe가
  reader join을 무기한 유지하지 못하게 한다.
- `repo_remote_cancel({ request: { operationId } })`는 path와 분리된 opaque ID로 in-memory
  cancellation token을 설정한다. frontend는 busyRef로 duplicate action을 막고 cancel/unmount/
  repository 교체 시 ID를 취소하며, sequence guard로 늦은 결과를 폐기한다. local Git mutation과
  remote mutation은 canonical repository별 single-flight registry를 공유하고 RAII guard로 정리한다.
  ID는 첫 await 전에 등록하고 canonical path는 blocking validation 뒤 bind해 즉시 취소와 path
  validation 사이의 race를 닫는다.

### Fixture와 완료 조건

Pure parser fixture는 upstream/no-upstream, unborn/detached, dirty, ahead/behind/diverged,
in-progress marker와 malformed/oversized output을 고정한다. temporary bare-remote integration
fixture는 fast-forward/no-rebase pull, exact current-branch push, no-upstream/dirty/detached/diverged
차단과 status preservation을 검증하며 reset/clean/merge/rebase/force command를 실행하지 않는다.
frontend fixture는 exact action calls, state-based disabled controls, fixed error/redaction,
busy/double-action, cancel, stale response와 unmount cleanup을 검증한다.


## 4.4 P2-16 — Git 상태 사전 검사 경계 (#319)

Repo Manager는 선택된 repository의 remote 작업 전 상태를 별도 read-only preflight로 확인한다.
`repo_preflight({ request: { path } })`는 다음 고정 argv만 사용한다.

- `git --no-pager --no-optional-locks status --porcelain=v2 --branch --untracked-files=all -z --`
  — branch head/upstream/ahead/behind와 dirty status를 읽는다.
- `git --no-pager --no-optional-locks rev-parse --git-path rebase-merge --git-path rebase-apply
  --git-path MERGE_HEAD` — operation marker의 실제 repository 경로를 bounded output으로
  얻어 rebase·merge 진행 여부를 읽는다.

status parser는 branch metadata와 ordinary/rename/unmerged/untracked record를 모두 bounded
검증하고 malformed NUL, unknown status, invalid UTF-8, overflow를 전체 고정 오류로 폐기한다.
marker 조회는 고정 파일명·경로 형식만 허용하며 missing marker만 false로, 권한·busy·race·unmount
오류는 `Git 상태를 확인하지 못했습니다.`로 fail-closed한다. 응답은 dirty, detached,
noUpstream, diverged, rebaseInProgress, mergeInProgress와 ahead/behind, safe, deterministic
issue IDs만 포함하고 raw stderr·remote URL·credential·OS/path detail을 노출하지 않는다.

이 기능은 repository/index/ref/remote/credential를 변경하지 않는다. force push, reset, clean,
force delete 및 automatic recovery는 request/handler/UI에 존재하지 않는다. frontend는 선택
repository별 explicit 검사 버튼만 제공하고 busy 중 중복 요청을 무시하며, repository 교체와
unmount 뒤 늦은 결과를 폐기한다. state matrix는 clean/upstream, dirty, detached, no-upstream,
ahead/behind/diverged, rebase marker, merge marker와 조합을 pure parser·real Git fixture로
검증한다.

## 5. 완료 조건

- root 아래 repository를 중복 없이 나열한다 (canonical identity).
- branch·dirty·ahead/behind·worktree 상태를 표시한다.
- worktree 생성과 열기가 동작한다.
- remove 전 검사가 동작하고, 파괴적 기본 동작이 없다.
- history/detail과 working-tree/selected-commit diff가 위 bounded read-only contract를
  충족한다. binary/oversize/unsafe path/invalid revision과 Git failure가 raw 값 없이
  재현 가능하고, native command 및 frontend parity fixture가 통과한다.
- selected stage/unstage와 explicit index-only commit이 검증된 상대 경로·bounded message,
  credential-helper/no-storage·fixed-error 경계를 충족하고, selected-only real Git fixture와
  frontend stale/unmount/double-action/failure-isolation fixture가 통과한다.
- remote status와 fetch/FF-only pull/current-branch push가 upstream/no-upstream, dirty/detached,
  diverged/in-progress preflight, bounded/redacted child execution, cancel/stale/unmount fixture와
  함께 동작하며 force/merge/rebase/reset/clean/credential storage 경계가 유지된다.
