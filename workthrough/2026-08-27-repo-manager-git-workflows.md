# Repo Manager Git workflows grouped candidate (#316–#319)

## Overview

`origin/main` `5719285`를 기준으로 한 전용 worktree와 branch에서 `6d637d5` grouped
baseline을 만들고, 기존 repo-manager-git-safety 후보의 사용자 변경을 건드리지 않은 채 #316
history/diff, #317 selected stage/unstage/commit, #318 remote sync, #319 Git safety preflight를
하나의 P2-16 grouped PR 후보로 이식했다. 이후 review follow-up에서는 linked worktree identity
lock, local operation cancellation, strict timestamp/ref parser와 confirmation/a11y 경계를
보강했다. 이 문서는 변경 범위·이슈별 acceptance·검증 경계를 기록하며 #307 handoff와
destructive recovery는 포함하지 않는다.

## Context and decisions

- 기존 history/stage/remote source worktree와 root safety candidate는 읽기 전용으로 참고했다.
- latest main의 window-state 및 Knowledge 변경과 root Cargo/Cargo.lock 내용을 보존하고, Repo
  Manager와 공용 git crate의 기능 hunk만 새 worktree에 적용했다.
- read-only 조회와 mutation을 분리하되, local stage/commit 및 remote fetch/pull/push는 canonical
  repository별 native single-flight registry와 RAII cleanup 경계를 공유한다.
- Git credential helper/config는 Git에 맡기고 devbox는 credential, remote URL, raw path/stderr/
  commit message를 저장하거나 UI 오류로 반향하지 않는다.
- `git --git-common-dir`를 canonical repository 권한의 기준으로 삼아 linked worktree도 같은
  filesystem identity lock을 공유한다. Unix는 `dev/inode`, Windows는 native handle의 volume
  serial/file index를 비교하며 worktree/common directory와 worktree-create target parent를
  mutation 직전에 재검증한다.
- Git child에서는 repository-selection override 환경만 제거하고(`GIT_DIR`, `GIT_COMMON_DIR`,
  `GIT_WORK_TREE`, `GIT_INDEX_FILE`, object/discovery/prefix/quarantine 계열), credential/SSH/
  askpass 환경과 Git config는 보존해 사용자의 configured credential helper를 유지한다.

## Changes

### #316 history and diff

- bounded `repo_history`, `repo_commit_detail`, `repo_diff` command/API와 HistoryDiffPanel을
  추가했다.
- hexadecimal revision, fixed argv, NUL/relative-path parser, binary marker, file/patch/stdout
  bounds, `--no-ext-diff`/`--no-textconv`/`--no-color`/`--no-renames`를 적용했다.
- History는 기본 50개·최대 100개, detail 128KiB, 전체 diff 2MiB·파일당 patch 512KiB·최대
  256파일로 제한한다. `%aI` authored timestamp는 calendar/time과 year zero, `Z` 또는
  `±14:00` offset까지 strict ISO로 검사하고, 실제 object ID·경로·UTF-8이 맞지 않으면 fixed
  error로 전체 결과를 버린다.
- history/detail/diff는 repository와 working tree를 변경하지 않고 storage, remote, telemetry와
  permanent export를 사용하지 않는다.

### #317 selected stage/unstage and commit

- bounded NUL status를 기준으로 selected repository-relative path만 stage/unstage한다.
- `--literal-pathspecs` 및 `--` 뒤에 literal path를 전달하고, unborn repository unstage에서도
  worktree를 보존한다. rename 선택은 new/old path를 함께 검증해 전달한다.
- commit은 bounded explicit message와 현재 index만 사용해 unstaged 파일을 자동 stage하지
  않는다. 실패 시 frontend selection/message를 보존한다.
- `repo_stage`/`repo_unstage`/`repo_commit`은 frontend가 만든 bounded opaque `operationId`를
  받고, `repo_local_cancel({ request: { operationId } })`가 path 없이 해당 local child를
  취소한다. Commit은 staged path/message snapshot을 확인창에서 승인한 뒤에만 native 호출하며,
  message/path 변경 또는 status refresh가 승인 상태를 무효화한다.

### #318 remote sync

- remote/refspec 없이 Git 기본 선택 규칙(현재 branch configured remote, 없으면 `origin` fallback)을
  따르는 `fetch --no-tags`, `pull --ff-only --no-rebase`, 검증된 upstream destination으로의
  exact current-branch push만 허용한다. `fetch --all`, force push, reset, clean, 자동 merge/
  rebase는 없다.
- dirty/detached/no-upstream/diverged/in-progress 상태별 preflight와 final revalidation을
  적용하고, push는 behind도 차단하며 push default/refspec이 범위를 확장하지 못하도록 고정
  argv를 사용한다.
- opaque operation ID를 첫 await 전에 등록한다. `repo_remote_cancel({ request: { operationId } })`
  는 path 재검증 없이 정확한 in-flight child를 취소한다. cancel/unmount/timeout은 Unix process group
  또는 Windows kill-on-close Job Object로 root Git과 hook/helper/SSH descendant를 함께
  종료하고, root 종료 뒤에도 process tree를 먼저 정리해 stdout reader가 남지 않게 한다.
- local mutation과 remote mutation 사이의 concurrent operation은 shared registry가 차단하고,
  stale response/post-action refresh 실패는 frontend가 이전 안정 snapshot을 mutation 가능한
  상태로 남기지 않도록 처리한다.
- Pull/push는 동일 status snapshot 확인창(취소 focus, Tab trap, Escape, trigger focus restore)을
  거친다. Fetch는 working tree read-only 경계로 확인 없이 시작하지만 같은 ID/lock/cancel
  경계를 사용한다.

### #319 Git safety preflight

- fixed porcelain-v2 branch status와 `rev-parse --git-path` marker만 read-only로 조회한다.
- dirty, detached, no-upstream, ahead/behind, diverged, rebase/merge marker를 deterministic
  issue ID로 분류하고 malformed/overflow/permission/busy/unmount/race는 고정 오류로 fail-closed한다.
- force push/reset/clean/automatic recovery와 arbitrary shell command는 요청·handler·UI에 없다.
- scan과 panel은 mounted/request sequence guard로 stale 응답을 버리고, backend/UI 오류는 raw
  path·stderr·remote URL·credential·commit message가 없는 fixed error로 제한한다. Remote
  branch/upstream·push ref parser는 bounded control/whitespace/URL/userinfo/traversal/ref
  syntax를 fail-closed한다.

## Fixture and acceptance matrix

| Issue | Native acceptance | UI/fixture acceptance |
|---|---|---|
| #316 | graph/root/multi-parent, full/short revision, text/binary diff, unsafe path, malformed and output caps | exact read-only calls, keyboard/IME, busy/stale/unmount, empty/binary/oversize and a11y states |
| #317 | selected-only stage/unstage, unborn handling, index-only commit, multiline message, bounded path/message and no credential storage | explicit selection, failure preservation, keyboard commit, duplicate/stale/unmount guard |
| #318 | bare remote FF-only/no-rebase pull, exact current-branch push, no-upstream/dirty/detached/diverged/in-progress blocking, cancellation and child cleanup | exact action calls, cancel/busy/stale/unmount, fixed redacted errors and stable snapshot on failure |
| #319 | clean/dirty/detached/upstream/ahead/behind/diverged and rebase/merge marker parser, malformed/overflow fixed errors | explicit read-only preflight, state-based disabled actions, no destructive action calls |

## Files

- `apps/repo-manager/src-tauri/src/commands.rs` and `src-tauri/src/core/{history_diff,stage_commit,remote_sync,git_safety}.rs`
- `apps/repo-manager/src/{App.tsx,App.css,api.ts,components/*Panel*}` and focused component tests
- `crates/git/{Cargo.toml,src/lib.rs}` for bounded process-tree cancellation
- `apps/repo-manager/README.md`, architecture/roadmap/spec documentation

## Verification

The complete local workspace gates and focused regression suites passed in the dedicated worktree:

- `cargo fmt --all -- --check` — passed.
- `cargo test -p filesystem -p git -p repo-manager --lib -j2` — filesystem 17 tests, git 13
  tests, and repo-manager 61 tests passed (0 failed).
- `cargo clippy -p filesystem -p git -p repo-manager --all-targets -- -D warnings` — passed.
- `cargo test -j2` — complete Rust workspace passed, including doc tests.
- `cargo check -j2` — complete Rust workspace passed.
- `pnpm --filter repo-manager test` — 9 test files and 66 tests passed (0 failed).
- `pnpm --filter repo-manager build` — TypeScript check and Vite production build passed.
- `pnpm build` — all frontend workspace projects passed.
- `cargo check -p filesystem -p git --target x86_64-pc-windows-gnu` — Windows-specific
  filesystem identity and Job Object code compiled successfully.
- `cargo test -p git runner_does_not_wait_for_an_escaped_descendant_holding_stdout` — CI 부하에서도
  fixture가 먼저 escaped child PID를 게시하도록 handshake를 고정한 뒤 5회 연속 통과했다.
- `cargo test -p repo-manager remote_real_git_fixture_covers_ff_pull_push_and_diverged_block` —
  Git for Windows의 CRLF checkout도 같은 logical content로 검증하도록 정규화한 뒤 통과했다.
- `cargo test -p code-pad --lib lsp::runtime::tests::managed_node_rejects_old_missing_and_hanging_runtimes` —
  output-limit fixture를 느린 9,000회 shell loop에서 단일 POSIX `printf` 호출로 바꿨다. 부하가
  큰 Linux CI에서도 8 KiB output 경계가 5초 timeout보다 먼저 결정되도록 고정했다.
- `git diff --check` — passed.

The worktree restored frontend dependencies from the existing pnpm store with
`pnpm install --offline --frozen-lockfile --filter repo-manager...`; it downloaded nothing and did
not change the lockfile. A full `repo-manager` cross-target check reached the Tauri Windows resource
step and then stopped because this WSL environment has no `x86_64-w64-mingw32-windres`; the shared
Windows-specific crates had already compiled. Windows packaged Git/credential-helper/hook descendant,
reparse/path identity and real bare-remote smoke therefore remain W2 evidence. PR merge에는 GitHub
Actions의 Windows compile/Clippy/test gate까지 모두 통과해야 하며, 이 문서는 실행 중인 CI
결과를 미리 성공으로 기록하지 않는다.

## Scope and handoff

- 기능 브랜치의 commit/push와 grouped PR 생성만 수행했다. rebase/reset/destructive checkout 또는
  기존 사용자·source worktree 삭제는 수행하지 않았다.
- Do not merge #307 Life Log→Knowledge handoff, arbitrary shell, force/reset/clean, runtime external
  download, or version bump into this candidate.
- Merge 전에 latest-main workspace CI를 통과하고, Windows W2 packaged smoke에서는 Git for
  Windows Job Object behavior, credential helper descendants, reparse/path identity, cancellation,
  final status race evidence를 확인한다.
