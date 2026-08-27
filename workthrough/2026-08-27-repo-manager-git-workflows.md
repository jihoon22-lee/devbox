# Repo Manager Git workflows grouped candidate (#316–#319)

## Overview

최신 origin/main(7a03b9b)에서 전용 worktree와 branch를 만들고, 기존 repo-manager-git-safety
후보의 사용자 변경을 건드리지 않은 채 #316 history/diff, #317 selected stage/unstage/commit,
#318 remote sync, #319 Git safety preflight를 하나의 P2-16 grouped PR 후보로 이식했다.
이 문서는 변경 범위·이슈별 acceptance·검증 경계를 기록하며 #307 handoff와 destructive
recovery는 포함하지 않는다.

## Context and decisions

- 기존 history/stage/remote source worktree와 root safety candidate는 읽기 전용으로 참고했다.
- latest main의 window-state 및 Knowledge 변경과 root Cargo/Cargo.lock 내용을 보존하고, Repo
  Manager와 공용 git crate의 기능 hunk만 새 worktree에 적용했다.
- read-only 조회와 mutation을 분리하되, local stage/commit 및 remote fetch/pull/push는 canonical
  repository별 native single-flight registry와 RAII cleanup 경계를 공유한다.
- Git credential helper/config는 Git에 맡기고 devbox는 credential, remote URL, raw path/stderr/
  commit message를 저장하거나 UI 오류로 반향하지 않는다.

## Changes

### #316 history and diff

- bounded `repo_history`, `repo_commit_detail`, `repo_diff` command/API와 HistoryDiffPanel을
  추가했다.
- hexadecimal revision, fixed argv, NUL/relative-path parser, binary marker, file/patch/stdout
  bounds, `--no-ext-diff`/`--no-textconv`/`--no-color`/`--no-renames`를 적용했다.
- history/detail/diff는 repository와 working tree를 변경하지 않고 storage, remote, telemetry와
  permanent export를 사용하지 않는다.

### #317 selected stage/unstage and commit

- bounded NUL status를 기준으로 selected repository-relative path만 stage/unstage한다.
- `--literal-pathspecs` 및 `--` 뒤에 literal path를 전달하고, unborn repository unstage에서도
  worktree를 보존한다.
- commit은 bounded explicit message와 현재 index만 사용해 unstaged 파일을 자동 stage하지
  않는다. 실패 시 frontend selection/message를 보존한다.

### #318 remote sync

- configured remote의 `fetch --no-tags`, `pull --ff-only --no-rebase`, 검증된 upstream
  destination으로의 exact current-branch push만 허용한다.
- dirty/detached/no-upstream/diverged/in-progress 상태별 preflight와 final revalidation을
  적용하고, push default/refspec이 범위를 확장하지 못하도록 고정 argv를 사용한다.
- opaque operation ID를 첫 await 전에 등록한다. cancel/unmount/timeout은 Unix process group
  또는 Windows kill-on-close Job Object로 root Git과 hook/helper/SSH descendant를 함께
  종료하고, root 종료 뒤에도 process tree를 먼저 정리해 stdout reader가 남지 않게 한다.
- local mutation과 remote mutation 사이의 concurrent operation은 shared registry가 차단하고,
  stale response/post-action refresh 실패는 frontend가 이전 안정 snapshot을 mutation 가능한
  상태로 남기지 않도록 처리한다.

### #319 Git safety preflight

- fixed porcelain-v2 branch status와 `rev-parse --git-path` marker만 read-only로 조회한다.
- dirty, detached, no-upstream, ahead/behind, diverged, rebase/merge marker를 deterministic
  issue ID로 분류하고 malformed/overflow/permission/busy/unmount/race는 고정 오류로 fail-closed한다.
- force push/reset/clean/automatic recovery와 arbitrary shell command는 요청·handler·UI에 없다.

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

Focused verification stayed within the Repo Manager and git crate because the parent agent is running
heavy workspace gates. The following low-load checks passed:

- cargo fmt --all -- --check
- cargo test -p git -p repo-manager --lib -j2 — git 11 tests and repo-manager 55 tests passed
- cargo check -p repo-manager -j2
- cargo clippy -p git -p repo-manager --all-targets -- -D warnings
- pnpm --filter repo-manager test -- --maxWorkers=2 — 8 files and 52 tests passed
- pnpm --filter repo-manager build — TypeScript check and Vite production build passed
- git diff --check

Frontend dependencies were reused from the existing local workspace install for the focused run;
no dependency or lockfile installation was performed. Windows packaged Git/credential-helper/hook
descendant and real bare-remote smoke remain W2 evidence.

## Scope and handoff

- No commit, push, PR, rebase, reset, checkout, or existing worktree deletion was performed.
- Do not merge #307 Life Log→Knowledge handoff, arbitrary shell, force/reset/clean, runtime external
  download, or version bump into this candidate.
- Before PR, rerun latest-main workspace CI and Windows W2 packaged smoke, especially Git for Windows
  Job Object behavior, credential helper descendants, reparse/path identity, cancellation, and
  final status race evidence.
