# repo-manager — Repo Manager (Git Worktree 관리)

지정 root 아래 Git 저장소를 탐색해 브랜치·worktree·상태를 목록화하고, worktree 생성·열기를 제공한다.
산출물: `RepoManager.exe` (`apps/repo-manager`).

## 주요 기능

- **저장소 탐색** — root 아래 Git repository 중복 없이 나열 (canonical identity)
- **앱 간 repository 선택** — catalog `Path`를 cold start와 실행 중 재호출에서 수신해 기존 항목을 선택하거나, 검증된 미등록 경로를 저장 전 초안으로 표시
- **상태 목록** — branch·dirty·ahead/behind·worktree
- **worktree 생성** — 새 작업 트리 생성
- **열기** — catalog에서 `path` capability와 실제 설치 executable이 모두 확인된 앱만 자동 노출하고, `workspace`도 받는 앱에는 더 구체적인 `Workspace` payload를 전달한다 (설계: [`docs/superpowers/specs/2026-08-17-app-interop-design.md`](../../docs/superpowers/specs/2026-08-17-app-interop-design.md))
- **repository 컨텍스트 메뉴** — 다른 앱으로 열기, worktree 생성 입력으로 이동, backend에서
  재검증한 경로 복사, 탐색기에서 열기. 우클릭과 `Shift+F10`/Menu 키를 지원하고 닫은 뒤 원래
  repository 카드로 포커스를 돌려보낸다. 카드 안의 텍스트 입력은 기본 우클릭·IME 동작을 유지한다.
- **정리 후보** — merged/stale branch 후보, remove 전 uncommitted/untracked 검사
- **일상 Git 흐름 (#317)** — 변경 파일을 읽어 선택한 경로만 stage/unstage하고, 현재
  index에 올라간 파일만 명시적으로 commit
- **원격 Git 흐름 (#318)** — configured remote를 fetch하고, clean·attached·upstream 상태에서
  fast-forward-only/no-rebase pull과 configured upstream destination으로 제한한 현재 branch
  push를 실행

## 안전 경계

- force delete·reset·clean을 기본 동작으로 제공하지 않음
- worktree remove 전 uncommitted/untracked 확인·안내
- Windows/WSL path가 같은 저장소를 중복 등록하지 않음 (`crates/wsl` canonical_project_key)
- inbound Path는 절대 경로·traversal·존재·Git repository 여부를 backend에서 검증하며, 실패 오류와 로그에 원문을 반향하지 않음
- 등록 초안은 자동 저장·Git 명령·임의 경로 쓰기를 수행하지 않고 사용자의 명시적 탐색 전까지 UI state로만 유지
- 경로 복사와 탐색기 열기는 action 시점에 존재하는 절대 Git repository인지 backend에서 다시
  확인한다. copy 외에는 새 path DTO를 만들지 않으며 opener 상세 오류나 거부된 raw path를 반향하지 않는다.
- 실제 worktree/branch 제거는 이 메뉴 PR에 포함하지 않는다. 현재는 read-only clean 검사만 유지하고,
  dirty/untracked/locked/main 차단과 preview를 갖춘 safe cleanup(#364)에서만 파괴 action을 추가한다.
- stage/unstage는 porcelain-v1 NUL status와 검증된 repository-relative path만 사용하며,
  `git add`/`git restore --staged`에 선택 경로를 명시적으로 전달한다. commit은 사용자가 입력한
  bounded message로 현재 index만 실행하고 unstaged 파일을 자동 추가하지 않는다.
- Git의 기본 credential helper/config 경계를 그대로 사용하되 devbox가 credential을 읽거나 저장하지
  않는다. stdin/stderr는 bounded runner에서 차단하고 실패는 고정 오류로 표시한다.
- 원격 작업은 remote URL·refspec·credential을 frontend에서 받지 않으며, force push·reset·clean·
  merge/rebase 자동화를 제공하지 않는다. dirty/detached/no-upstream/diverged/in-progress 상태는
  pull/push 전 고정 오류로 차단하고, fetch도 진행 중인 merge/rebase에서는 차단한다.
- stage/unstage/commit과 fetch/pull/push는 canonical repository별 native single-flight lock을
  공유한다. 서로 다른 UI panel에서 동시에 실행해도 한 작업만 진입하며, blocking Git/파일시스템
  호출은 Tauri async runtime 밖의 blocking worker에서 실행한다.

## Git 상태 사전 검사 (#319)

선택한 repository의 remote 작업 전 상태를 읽기 전용으로 확인한다. `repo_preflight({ request:
{ path } })`는 고정된 `git status --porcelain=v2 --branch --untracked-files=all -z --` argv로
dirty tree, detached HEAD, upstream 없음, ahead/behind와 diverged 상태를 판정한다. Git의
`rev-parse --git-path rebase-merge --git-path rebase-apply --git-path MERGE_HEAD` 결과에서
현재 rebase/merge marker도 확인한다. status와 marker 출력은 각각 bounded/UTF-8 검증을 거치고,
실패·경합·권한 오류·마운트 해제는 `Git 상태를 확인하지 못했습니다.` 하나로 닫힌다.

응답은 branch/upstream metadata, ahead/behind, 상태 boolean, deterministic `issues` 목록과
`safe`만 반환하며 raw stderr, remote URL, credential helper 출력, 원문 경로를 오류에 포함하지
않는다. UI는 상태 검사·새 repository 선택·unmount 사이의 늦은 응답을 버리고 duplicate 검사를
무시한다. 이 기능은 상태만 읽으며 force push, reset, clean, force delete와 자동 복구 action을
제공하지 않는다.

## Git history · diff (#316)

선택한 repository에서 별도 Git GUI나 온라인 서비스를 열지 않고, 최근 commit
history·detail과 diff를 확인한다. History는 기본 50개(최대 100개)를 보여 주고, 각 commit의
object ID·부모 ID·author·author email·strict ISO timestamp·subject를 유지한다. commit을
선택하면 bounded body와 selected-commit diff를 함께 가져오며, `Working tree diff`는
`HEAD`와 현재 index/worktree의 추적 파일 변경을 합쳐 보여 준다(미추적 파일은 Git diff의
기본 범위 밖이다). Stage/unstage와 explicit commit은 현재 index를 대상으로 하며,
unstaged 파일을 자동으로 추가하지 않는다. 이 기능은
fetch/pull/push, branch mutation, cleanup/force action을 호출하지 않는다.
history 목록은 parent ID와 root 표시를 포함한 간결한 graph lane으로 commit 관계를 드러내며,
Git의 고정 `--topo-order` 순서와 parent metadata를 사용하고 임의 priority를 만들지 않는다.

native command contract는 다음 read-only request와 명시적 mutation request로 고정한다.

- `repo_history({ request: { path, limit } })` — `limit` 1..100, 반환은
  `{ entries, hasMore }`.
- `repo_commit_detail({ request: { path, commitId } })` — `commitId`는 7..64자의 hexadecimal
  object ID 또는 short ID만 허용한다.
- `repo_diff({ request: { path, commitId } })` — `commitId: null`은 working tree,
  hexadecimal `commitId`는 해당 commit patch다. 반환은 `{ scope, commitId, files,
  truncated }`이고 각 file은 repository-relative `path`, optional `oldPath`, status,
  `binary`, bounded `patch`, `truncated`를 가진다.
- `repo_changes({ request: { path } })` — porcelain-v1 NUL status를 `{ path, oldPath,
  indexStatus, worktreeStatus, kind, staged, unstaged }`로 반환한다.
- `repo_stage({ request: { path, paths } })`와 `repo_unstage({ request: { path, paths } })` —
  검증된 상대 경로를 선택한 순서대로 `git add` 또는 `git restore --staged`에 전달한다.
  아직 HEAD가 없는 저장소의 unstage는 `git rm --cached`로 index만 되돌리고 worktree 파일은
  보존한다.
- `repo_commit({ request: { path, message } })` — bounded message를 `git commit --message`로
  전달하며 현재 index만 commit한다. unstaged 파일을 자동 stage하지 않는다.

모든 request DTO는 unknown field를 거부한다. path는 absolute·existing Git repository·canonical
identity로 재검증하며 control character, traversal, device path를 허용하지 않는다. commit
revision은 arbitrary rev expression/pathspec으로 전달하지 않는다. Git invocation은 shared
`crates/git::run_bounded`/`run_mutating`의 bounded timeout, stdin/stderr 차단, argument/UTF-8 검증,
optional lock 차단, command별
stdout 상한(History 512 KiB, Detail 128 KiB, Diff 2 MiB)을 사용한다. 실패는 Git stderr,
raw path, remote URL, credential helper, OS exception을 포함하지 않는
`Git history 또는 diff를 불러올 수 없습니다.`로만 반환한다.

Diff는 `--no-ext-diff`, `--no-textconv`, `--no-color`, `--no-renames`로 실행하며 `--binary`를
사용하지 않는다. selected merge commit은 `-m`으로 부모별 표준 patch를 사용한다. binary 파일은
marker와 metadata만 표시하고 원본 bytes를 읽거나 렌더링하지 않는다. file당 patch도 512 KiB로
제한하며 file 수는 256개로 제한한다. 안전한 parser가
repository-relative path를 확인하지 못하거나 UTF-8이 아닌 command output이면 전체 결과를
고정 오류로 폐기한다. History/detail/diff는 filesystem write, repository mutation, telemetry,
remote network, persistent history를 사용하지 않는다.
`core.quotePath=false`와 `--no-renames` 계약 아래 old/new가 같은 유일한 header 경계를 찾아
공백과 ` b/`를 포함한 일반 UTF-8 경로도 손실 없이 표시하며, control/non-UTF-8 path는 계속
fail-closed한다.

Frontend는 선택 repository에만 panel을 표시하고, history limit 입력의 Enter를 포함한 모든
action을 explicit하게 실행한다. busy 상태에서 field/action을 잠그고 duplicate submit을
무시하며, request sequence와 unmount cleanup으로 늦은 결과가 새 repository를 덮지 못하게
한다. IME composition 중 Enter는 실행하지 않고, `aria-busy`, `role=status`, `role=alert`,
keyboard-focusable commit buttons와 binary/oversize empty state를 제공한다.

## Git remote sync (#318)

선택한 repository의 remote 상태를 bounded porcelain status와 Git metadata marker로 확인한 뒤
아래의 고정 명령만 실행한다.

- `repo_remote_status({ request: { path } })` — 현재 branch, upstream, ahead/behind, dirty,
  detached, diverged, merge/rebase 진행 여부를 반환한다. change filename, remote URL, stderr,
  credential helper 정보는 반환하지 않는다.
- `repo_fetch({ request: { path } })` — `git --no-pager --no-optional-locks fetch --no-tags`.
  working tree를 변경하지 않으므로 dirty/detached/no-upstream/diverged에서도 사용할 수 있지만,
  merge/rebase 진행 중에는 차단한다.
- `repo_pull({ request: { path } })` — clean·attached·upstream·non-diverged 상태에서
  `git --no-pager --no-optional-locks pull --ff-only --no-rebase`만 실행한다. 사용자 Git config가
  `pull.rebase=true`여도 rebase로 바뀌지 않는다.
- `repo_push({ request: { path } })` — clean·attached·upstream·non-diverged 상태에서
  native가 현재 branch의 configured remote와 upstream destination을 읽은 뒤
  `git --no-pager --no-optional-locks push -- <remote> HEAD:refs/heads/<destination>`을 실행한다.
  따라서 `push.default`나 추가 push refspec이 다른 branch를 함께 전송하지 못하며,
  remote/refspec을 frontend가 지정하지 않고 force push도 제공하지 않는다.
- `repo_remote_cancel({ request: { operationId } })` — path와 분리된 bounded opaque ID로 정확한
  in-flight child의 cancellation token을 설정한다. repository가 unmount/deleted된 뒤에도 path를
  다시 열지 않는다. ID는 첫 async await 전에 등록하고 canonical path는 blocking validation 뒤
  같은 RAII guard에 bind하므로 즉시 취소도 유실되지 않으며, 늦은 결과는 frontend sequence
  guard가 폐기한다.

모든 remote command는 absolute/existing Git repository를 다시 검증하고, status 512 KiB,
marker 4 KiB, mutation stdout 64 KiB, 30초 timeout을 적용한다. stdin/stderr를 닫고 Git의
기본 credential helper만 사용하며, devbox는 credential을 읽거나 저장하지 않는다. 실패·취소·
preflight 차단은 remote URL, raw path, credential, Git stderr를 포함하지 않는 고정 메시지다.
Windows에서는 kill-on-close Job Object에 `git.exe`를 fail-closed로 편입하고 Linux/WSL에서는
독립 process group을 사용해 hook·credential helper·SSH/transport 하위 프로세스까지
timeout/cancel/drop 수명 경계에 둔다. root Git이 먼저 종료돼도 tree를 닫은 뒤 stdout reader를
회수해 inherited pipe가 timeout을 우회하지 못한다.
frontend는 상태별 pull/push 비활성화, busy 중 중복 방지, 취소 버튼, stale/unmount 폐기와
`aria-busy`/`role=status`/`role=alert`를 제공한다.
remote mutation은 native lock을 잡은 상태에서 status를 두 번 읽고, push는 configured remote를
읽은 뒤 status를 한 번 더 확인한다. 안전 관련 snapshot이 달라지면 child를 생성하지 않으며,
RAII operation guard가 성공·실패·panic에서 registry를 정리한다.

## 기술

- 공용 크레이트 `crates/wsl`·`crates/launch`(`installed_targets`, `launch_open`)·`crates/filesystem`(`is_ignored_dir`, scan_root)
- git 출력 파싱·탐색은 순수 `core/` 로직

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`

설계 문서: `docs/superpowers/specs/2026-08-14-repo-manager-design.md`
