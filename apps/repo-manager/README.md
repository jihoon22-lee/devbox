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
- **정리 후보** — merged/stale branch와 안전한 linked/detached worktree 후보를 bounded
  preview로 확인하고, 대상·근거·차단 사유를 검토한 뒤 선택 항목만 정리
- **일상 Git 흐름 (#317)** — 변경 파일을 읽어 선택한 경로만 stage/unstage하고, 현재
  index에 올라간 파일만 명시적으로 commit한다. Commit은 별도 확인 후에만 실행된다.
- **원격 Git 흐름 (#318)** — Git 기본 remote를 fetch하고, clean·attached·upstream 상태에서
  fast-forward-only/no-rebase pull과 configured upstream destination으로 제한한 현재 branch
  push를 실행한다. Pull/push는 별도 확인 후에만 실행된다.

## 안전 경계

- force delete·reset·clean을 기본 동작으로 제공하지 않음
- worktree remove 전 uncommitted/untracked 확인·안내
- Windows/WSL path가 같은 저장소를 중복 등록하지 않음 (`crates/wsl` canonical_project_key)
- inbound Path는 절대 경로·traversal·존재·Git repository 여부를 backend에서 검증하며, 실패 오류와 로그에 원문을 반향하지 않음
- 등록 초안은 자동 저장·Git 명령·임의 경로 쓰기를 수행하지 않고 사용자의 명시적 탐색 전까지 UI state로만 유지
- 경로 복사와 탐색기 열기는 action 시점에 존재하는 절대 Git repository인지 backend에서 다시
  확인한다. copy 외에는 새 path DTO를 만들지 않으며 opener 상세 오류나 거부된 raw path를 반향하지 않는다.
- 실제 worktree/branch 제거는 safe cleanup(#364)의 bounded preview·정확한 대상 확인·명시적
  확인·eligible 차단을 통과한 경우에만 수행한다. dirty/untracked/ignored/locked/main/current/
  prunable/state-unavailable 대상은 항상 mutation 전에 차단된다. 확인창에는 선택한 branch 이름과
  worktree 경로를 그대로 나열하고, mutation 직전 branch object/upstream/current HEAD와 worktree
  HEAD·branch·registration·filesystem identity·status를 다시 읽는다. 한 항목이라도 바뀌면
  해당 batch를 실행하지 않으며 취소·timeout·실패 뒤에는 이전 preview를 폐기해 재검사를 요구한다.
- stage/unstage는 porcelain-v1 NUL status와 검증된 repository-relative path만 사용하며,
  `git add`/`git restore --staged`에 선택 경로를 명시적으로 전달한다. commit은 사용자가 입력한
  bounded message로 현재 index만 실행하고 unstaged 파일을 자동 추가하지 않는다.
- Git의 기본 credential helper/config 경계를 그대로 사용하되 devbox가 credential을 읽거나 저장하지
  않는다. stdin/stderr는 bounded runner에서 차단하고 실패는 고정 오류로 표시한다.
- Git child에는 선택 repository를 바꿀 수 있는 `GIT_DIR`, `GIT_COMMON_DIR`, `GIT_WORK_TREE`,
  `GIT_INDEX_FILE`, `GIT_OBJECT_DIRECTORY`, `GIT_ALTERNATE_OBJECT_DIRECTORIES`,
  `GIT_CEILING_DIRECTORIES`, `GIT_DISCOVERY_ACROSS_FILESYSTEM`, `GIT_PREFIX`,
  `GIT_QUARANTINE_PATH`, `GIT_CONFIG_PARAMETERS`, `GIT_CONFIG_COUNT` 및 일반적인
  `GIT_CONFIG_KEY_n`/`GIT_CONFIG_VALUE_n` override를 전달하지 않는다. `GIT_ASKPASS`를 포함한
  credential/SSH/askpass 환경과 사용자의 일반 Git config는 유지해 configured credential
  helper가 계속 동작하며, devbox는 그 값을 읽거나 저장하지 않는다.
- 원격 작업은 remote URL·refspec·credential을 frontend에서 받지 않으며, force push·reset·clean·
  merge/rebase 자동화를 제공하지 않는다. dirty/detached/no-upstream/diverged/in-progress 상태는
  pull/push 전 고정 오류로 차단하고, fetch도 진행 중인 merge/rebase에서는 차단한다.
- stage/unstage/commit과 fetch/pull/push는 canonical repository별 native single-flight lock을
  공유한다. 서로 다른 UI panel에서 동시에 실행해도 한 작업만 진입하며, blocking Git/파일시스템
  호출은 Tauri async runtime 밖의 blocking worker에서 실행한다.
- 이 lock의 키는 표시 경로나 문자열이 아니라 Git의 `--git-common-dir` filesystem identity다.
  따라서 linked worktree와 `create_worktree`도 같은 common Git directory를 공유하면 서로
  차단된다. Unix에서는 열린 디렉터리의 `dev/inode`, Windows에서는 native handle의
  `volume serial/file index`를 비교하고, 최종 symlink/reparse point는 따라가지 않는다.
  worktree/common directory와 worktree-create target parent identity는 Git mutation 직전에
  다시 확인하며 바뀌면 fixed error로 child를 생성하지 않는다.
- scan과 각 panel은 mounted 상태와 monotonically increasing request sequence를 함께 확인해
  늦은 응답이 새 root/repository 상태를 덮지 못하게 한다. backend와 UI는 raw path, Git stderr,
  remote URL, credential, commit message를 오류에 반향하지 않고 작업별 고정 오류만 표시한다.

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

Remote status의 branch/upstream metadata와 exact push ref는 bounded parser로 확인하며
control/whitespace, URL·userinfo, 경로 traversal 및 Git ref syntax가 섞인 값은 fail-closed한다.
Preflight branch label도 bounded/control 검사를 통과해야 한다. `issues`는
`dirty`, `detached`, `noUpstream`, `diverged`, `rebaseInProgress`, `mergeInProgress` 순서의
안정적인 ID다. `safe`는 이 read-only snapshot에 알려진 차단 사유가 없다는 뜻일 뿐 mutation
권한이나 자동 복구 승인이 아니다.

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
- `repo_stage({ request: { path, paths, operationId } })`와
  `repo_unstage({ request: { path, paths, operationId } })` —
  검증된 상대 경로를 선택한 순서대로 `git add` 또는 `git restore --staged`에 전달한다.
  아직 HEAD가 없는 저장소의 unstage는 `git rm --cached`로 index만 되돌리고 worktree 파일은
  보존한다. `operationId`는 local cancel이 주소 지정하는 bounded opaque ID다.
- `repo_commit({ request: { path, message, operationId } })` — bounded message를
  `git commit --message`로 전달하며 현재 index만 commit한다. unstaged 파일을 자동 stage하지
  않는다.
- `repo_local_cancel({ request: { operationId } })` — path를 다시 열지 않고 해당 local
  stage/unstage/commit child의 취소 token만 설정하며, 반환값은 실제 in-flight ID를 찾았는지다.

local panel에서 사용자가 취소를 누르면 native 응답이 취소 직후 성공하더라도 해당 요청
sequence를 즉시 폐기한다. UI는 변경 파일·선택·commit 확인 snapshot을 비우고 고정된 취소
오류와 `최신 변경 파일을 다시 불러오세요` 안내만 남긴다. stage/unstage/commit은 Git의
여러 파일 동작 또는 hook을 원자적으로 되돌릴 수 없으므로 취소 시 자동 rollback을 시도하지
않으며, 다음 명시적 새로고침이 실제 index/working tree 상태의 유일한 기준이다.

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

`%aI` timestamp는 표시 전에 strict ISO 형태, 실제 calendar 날짜/시간, year zero 금지,
`Z` 또는 `±14:00` 이내 offset을 모두 검사한다. Commit ID는 7..64 hexadecimal 입력만,
history object ID는 실제 40..64 hexadecimal만 허용한다. Body·subject·author metadata와
stdout/파일 patch는 각각 byte cap을 넘으면 원문 없이 고정 오류 또는 `truncated`로 처리한다.

Frontend는 선택 repository에만 panel을 표시하고, history limit 입력의 Enter를 포함한 모든
action을 explicit하게 실행한다. busy 상태에서 field/action을 잠그고 duplicate submit을
무시하며, request sequence와 unmount cleanup으로 늦은 결과가 새 repository를 덮지 못하게
한다. IME composition 중 Enter는 실행하지 않고, `aria-busy`, `role=status`, `role=alert`,
keyboard-focusable commit buttons와 binary/oversize empty state를 제공한다. Commit 확인창은
검토한 repository identity·staged 개수·message snapshot이 바뀌면 무효화되고, message·경로·
remote URL·credential 원문을 표시하지 않는다. 열릴 때 취소 버튼에 focus하고 Tab을 dialog 안에
가두며 Escape 취소와 닫힌 뒤 원래 trigger로 focus 복원을 지원한다.

## Git remote sync (#318)

선택한 repository의 remote 상태를 bounded porcelain status와 Git metadata marker로 확인한 뒤
아래의 고정 명령만 실행한다.

- `repo_remote_status({ request: { path } })` — 현재 branch, upstream, ahead/behind, dirty,
  detached, diverged, merge/rebase 진행 여부를 반환한다. change filename, remote URL, stderr,
  credential helper 정보는 반환하지 않는다.
- `repo_fetch({ request: { path, operationId } })` — `git --no-pager --no-optional-locks fetch
  --no-tags`. remote/refspec을 argv로 받지 않는다. 따라서 Git의 기본 선택 규칙, 즉 현재
  branch에 configured remote가 있으면 그것을 사용하고 없으면 `origin`을 fallback으로 사용하며,
  절대로 `--all`을 사용하지 않는다. working tree를 변경하지 않으므로 dirty/detached/
  no-upstream/diverged에서도 사용할 수 있지만 merge/rebase 또는 다른 Git 작업 진행 중에는
  차단한다.
- `repo_pull({ request: { path, operationId } })` — clean·attached·upstream·non-diverged 상태에서
  `git --no-pager --no-optional-locks pull --ff-only --no-rebase`만 실행한다. 사용자 Git config가
  `pull.rebase=true`여도 rebase로 바뀌지 않는다. `operationId`는 remote cancel용 bounded
  opaque ID이며, pull은 확인창에서 승인한 동일 status snapshot에 대해서만 시작한다.
- `repo_push({ request: { path, operationId } })` — clean·attached·upstream·non-diverged 상태에서
  native가 현재 branch의 configured remote와 upstream destination을 읽은 뒤
  `git --no-pager --no-optional-locks push -- <remote> HEAD:refs/heads/<destination>`을 실행한다.
  따라서 `push.default`나 추가 push refspec이 다른 branch를 함께 전송하지 못하며,
  remote/refspec을 frontend가 지정하지 않고 force push도 제공하지 않는다. upstream보다 local
  branch가 뒤처져 있거나 diverged이면 차단하며, push는 확인창에서 승인한 동일 status snapshot에
  대해서만 시작한다.
- `repo_remote_cancel({ request: { operationId } })` — path와 분리된 bounded opaque ID로 정확한
  in-flight child의 cancellation token을 설정한다. repository가 unmount/deleted된 뒤에도 path를
  다시 열지 않는다. ID는 첫 async await 전에 등록하고 canonical path는 blocking validation 뒤
  같은 RAII guard에 bind하므로 즉시 취소도 유실되지 않으며, 늦은 결과는 frontend sequence
  guard가 폐기한다. 사용자가 취소를 누른 순간 sequence도 무효화하므로 native 작업이 그 직후
  성공해도 UI에 성공 상태를 반영하지 않고, 최신 상태를 다시 읽으라는 고정 안내와 함께
  remote snapshot을 비운다.

모든 remote command는 absolute/existing Git repository를 다시 검증하고, status 512 KiB,
marker 4 KiB, mutation stdout 64 KiB, 30초 timeout을 적용한다. stdin/stderr를 닫고 Git의
기본 credential helper만 사용하며, devbox는 credential을 읽거나 저장하지 않는다. 실패·취소·
preflight 차단은 remote URL, raw path, credential, Git stderr를 포함하지 않는 고정 메시지다.
Windows에서는 `git.exe`를 suspended로 생성해 kill-on-close Job Object에 편입한 뒤에만
primary thread를 resume하고, Linux/WSL에서는 독립 process group을 사용해 hook·credential helper·SSH/transport 하위 프로세스까지
timeout/cancel/drop 수명 경계에 둔다. root Git이 먼저 종료돼도 tree를 닫은 뒤 stdout reader를
회수해 inherited pipe가 timeout을 우회하지 못한다.
frontend는 상태별 pull/push 비활성화, busy 중 중복 방지, 취소 버튼, stale/unmount 폐기와
`aria-busy`/`role=status`/`role=alert`를 제공한다.
remote mutation은 native lock을 잡은 상태에서 status를 두 번 읽고, push는 configured remote를
읽은 뒤 status를 한 번 더 확인한다. 안전 관련 snapshot이 달라지면 child를 생성하지 않으며,
RAII operation guard가 성공·실패·panic에서 registry를 정리한다.

local과 remote의 `operationId`는 128 bytes 이하 ASCII `[A-Za-z0-9._-]`로 제한되고 첫 async
await 전에 registry에 등록된다. 취소·timeout은 Unix process group, Windows suspended 생성→
kill-on-close Job Object 편입→resume 경계를 통해 Git root뿐 아니라 hook, credential helper, SSH/transport descendant까지
종료하고, root가 먼저 끝나도 owned tree와 bounded stdout reader를 정리한 뒤 결과를 반환한다.
Fetch는 pull/push와 달리 confirmation 없이 버튼 action으로 시작되지만 동일한 operation ID,
lock, preflight 및 cancellation 경계를 사용한다.

## Safe branch · worktree cleanup (#364)

정리는 항상 read-only preview를 먼저 읽고, preview에서 선택한 대상만 명시적 확인 뒤 실행한다.
Preview는 고정된 bounded Git 출력에서 local branch의 `mergedIntoCurrent`, `upstreamGone`,
`inactive`(90일) 근거를 계산하고, worktree의 linked/detached 상태와 dirty/untracked/ignored/
locked/prunable 상태를 함께 표시한다. 기본(main) worktree, 현재 열려 있는 worktree, 현재 branch,
`main` branch, worktree에서 사용 중인 branch, locked 또는 상태를 확인하지 못한 worktree는 `eligible=false`로
닫힌다. stale/prunable metadata를 자동 prune하지 않는다.

Native contract는 다음과 같다.

- `repo_cleanup_preview({ request: { path, operationId } })` — `{ revision, currentBranch, currentHead,
  branches, worktrees }`를 반환한다. branch/worktree 항목에는 `candidate`, `eligible`,
  stable `reasons`, `blocked` ID가 있으며 raw Git stderr·credential·잠금 사유 원문은 반환하지
  않는다. `operationId`는 preview 관찰을 취소할 때만 사용하는 bounded opaque ID다.
- `repo_cleanup({ request: { path, branchNames, worktreePaths, previewRevision, operationId } })`
  — fresh preview의 opaque revision·common-directory identity와 선택 대상의
  filesystem identity·status가 일치하고 eligible일 때만 `git branch --delete -- <branch>` 또는
  `git worktree remove -- <path>`를 실행한다. 실행 직전에도 branch는 name/object/upstream/
  merged·stale·current·checked-out 판정을, worktree는 canonical path/identity·HEAD·branch/
  main/bare/locked/prunable/status 판정을 다시 읽어 preview와 완전히 비교한다. `-D/--force`,
  `reset`, `clean`, `worktree prune`은 argv/UI에 존재하지 않는다. blocked selection은 mutation
  없이 per-item 결과로 반환하며, stale preview·path exchange·ref/worktree registration/
  dirty/untracked/ignored 상태 변화는 고정 오류로 중단한다.
- `repo_cleanup_cancel({ request: { operationId } })` — path를 다시 열지 않고 bounded
  operation의 cancellation token만 설정한다. preview/재검증 read와 선택 batch mutation에는
  각각 cancellation-aware bounded deadline을 적용하며, mutation batch도 120초 total budget을
  넘기지 않는다. cleanup은 local/remote/stage/create와 동일한 canonical Git common-directory
  single-flight lock과 bounded process-tree runner를 사용한다.

Git object format에 따른 unborn HEAD 표기는 40자리 SHA-1과 64자리 SHA-256 all-zero object ID를
모두 `currentHead: null`로 처리한다. cleanup context의 canonicalize/filesystem identity 단계도
Git 조회와 같은 cancellation token·남은 deadline 경계를 앞뒤로 확인하며, cleanup command 전체는
Tauri의 bounded `spawn_blocking` worker 안에서 실행한다. 개별 OS filesystem syscall은 진입 후
강제 중단할 수 없으므로, 완료 직후 경계를 다시 확인해 만료·취소된 작업이 다음 Git child를
생성하지 않도록 닫는다.

모든 요청 DTO는 unknown field를 거부하고 selection 수·branch/path·revision·operation ID를
상한/문자 집합으로 검증한다. Preview와 실행 결과는 raw Git diagnostic을 오류에 반향하지
않으며, path는 명시적 preview/result와 사용자가 승인하는 대상 목록에서만 확인할 수 있다.
Frontend는 candidate rationale와 block reason을 보여 주고, confirmation snapshot·repository
identity·선택 집합이 바뀌면 native 호출을 생략한다. native cleanup 실패·취소·state-change는
기존 preview와 선택을 폐기해 다음 실행 전에 명시적인 `정리 후보 검사`를 요구하며, 성공 응답의
`previewRevision`도 승인 snapshot과 일치하지 않으면 같은 방식으로 폐기한다.

cleanup batch는 여러 branch/worktree mutation을 하나의 OS/Git transaction으로 되돌릴 수 없다.
앞선 항목이 이미 제거된 뒤 후속 항목의 재검증·Git 실행이 실패할 수 있으므로 자동 rollback이나
강제 복구를 시도하지 않으며, 결과가 불확실하거나 부분 적용이면 UI가 전체 preview와 선택을
폐기하고 새 preview에서 실제 상태를 다시 확인하게 한다. 취소 직후 도착한 native 성공 응답도
현재 sequence에 반영하지 않는다.

## 기술

- 공용 크레이트 `crates/wsl`·`crates/launch`(`installed_targets`, `launch_open`)·`crates/filesystem`(`is_ignored_dir`, scan_root)
- git 출력 파싱·탐색은 순수 `core/` 로직

## 개발

- 순수 로직: `src-tauri/src/core/` → `cargo test`
- 실행/빌드(Windows): `pnpm tauri dev` / `pnpm tauri build`

설계 문서: `docs/superpowers/specs/2026-08-14-repo-manager-design.md`
