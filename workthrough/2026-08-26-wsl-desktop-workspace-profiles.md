# WSL Desktop Native Workspaces and Profiles

## Overview

Issue #263의 P1-08 범위로 WSL Desktop에 외부 멀티플렉서 없이 완전하게 동작하는 터미널
workspace/profile을 추가했다. runtime PTY session ID와 영속 pane key를 분리하고, 마지막
레이아웃과 이름 있는 profile이 tab/pane/distro/cwd/layout/start command를 저장·복원한다.
`OpenTarget::Profile`의 cold-start와 running-instance 요청은 같은 profile 전환 경로로 수렴한다.

native 모드는 기본값이며 설치·download·network 없이 동작한다. 사용자가 이미 설치한 tmux 또는
zellij는 배포판별로 읽기 전용 감지한 뒤에만 opt-in할 수 있다. adapter는 shell command string을
만들지 않고 exact argv를 사용하며, 도구가 없거나 감지에 실패하면 backend가 native로 내린다.

```text
runtime PTY session ID ─┐
                       ├─ Pane(runtime) ── xterm attach/output/input
stable pane key ───────┘
         │
         ├─ localStorage version 1: 마지막 레이아웃
         ├─ app_local_data_dir/terminal-profiles.json: 이름 있는 profile
         ├─ devbox://open Profile target: cold/hot profile 전환
         └─ wsld-* mux session name: optional tmux/zellij 재연결
```

safe broadcast는 기본 OFF다. 활성 탭에서 사용자가 팬을 2개 이상 직접 고른 뒤에만 켤 수 있고,
대상 수를 계속 표시한다. 여러 줄 입력과 재귀 삭제 등 위험 명령의 Enter는 raw command를
confirmation에 반향하지 않고 대상 수와 실행 위험을 다시 확인한다.

Windows packaged build smoke는 issue의 W1 checkpoint 정책에 따라 P1 묶음 checkpoint에 남겼다.

## Context and Constraints

- v0.4.1까지의 WSL Desktop은 runtime session ID를 tab ownership과 UI identity에 함께 사용해
  프로세스 재시작 뒤 레이아웃을 같은 identity로 복원할 수 없었다.
- profile은 WSL Desktop 소유 데이터다. Workbench의 프로젝트 profile이나 Run Manager의
  예약·background service 책임을 복제하지 않는다.
- 시작 명령은 편리하지만 shell input이므로 저장 전 평문 credential과 control character를
  거부하고, 실행 직전에 최종 문자열을 사용자에게 보여 줘야 한다.
- tmux/zellij 전체를 내장하지 않는다. native workspace가 제품 기능의 본체이고, 외부 도구는
  이미 설치된 경우의 process persistence adapter일 뿐이다.
- profile 전환은 최대 32개 WSL 세션을 만들 수 있으므로 순차 실행해 순간적인 process/memory
  spike를 피한다.
- running workspace를 새 profile로 바꿀 때는 새 세션을 먼저 준비하고 UI identity를 한 번에
  교체한 다음 이전 세션을 닫는다. 시작 실패로 기존 작업 공간까지 먼저 잃지 않는다.
- terminal output, selection, clipboard 내용과 runtime session ID는 저장하지 않는다.

## Changes Made

### 1. Versioned workspace schema and fail-closed validation

Files:

- `apps/wsl-desktop/src-tauri/src/core/workspace.rs`
- `apps/wsl-desktop/src/lib/workspace.ts`
- `apps/wsl-desktop/src/types.ts`

version 1 schema는 profile, tab, pane과 active identity를 저장한다. pane은 stable key, distro,
optional cwd/start command와 실제 multiplexer kind를 가진다. tab은 stable ID, title,
custom-title 우선권, layout과 pane-key ownership을 가진다.

Rust store와 frontend last-layout parser가 같은 경계를 적용한다.

- profile 100개, tab 16개, pane 32개
- ASCII stable ID 128 bytes/characters
- 이름 120 bytes, control character 금지
- 시작 명령 한 줄 4,096자, control character 금지
- 공용 filesystem parser와 같은 4,096-byte absolute POSIX/Windows drive/UNC path 정책
- duplicate tab/pane ID, duplicate reference, missing reference, orphan pane 거부
- active tab 존재와 active pane의 active-tab membership 검증
- unknown multiplexer와 unsupported store version 거부
- 손상·unsafe store는 데이터를 실행하지 않고 빈 collection으로 fail closed

시작 명령 credential guard는 private-key header 변형, 알려진 token prefix,
Authorization Bearer, password/token/API key/client secret/access token의 literal 값을 거부한다.
`$TOKEN`, `%TOKEN%`, `{{token_ref}}` 같은 reference는 허용한다. 모든 marker occurrence를
검사하므로 먼저 안전한 reference를 둔 뒤 같은 종류의 literal을 붙이는 우회를 허용하지 않는다.

last layout은 localStorage의 별도 versioned document이고 named profile은 Tauri
`app_local_data_dir/terminal-profiles.json`에 저장한다. runtime session ID는 stable pane key로
역매핑한 뒤 제거한다.

### 2. Atomic named-profile persistence

Files:

- `apps/wsl-desktop/src-tauri/src/commands/workspace.rs`
- `apps/wsl-desktop/src-tauri/src/lib.rs`
- `apps/wsl-desktop/src/api.ts`

list/save/delete Tauri command를 추가했다. 새 profile ID는 backend UUID v4로 발급해 browser/UI가
저장 identity를 신뢰 경계 밖에서 정하지 않게 한다. upsert는 기존 순서를 유지하며 같은 ID만
교체한다. 쓰기는 공용 filesystem crate의 unique sibling + sync + replace atomic write를 사용한다.

UI는 현재 runtime workspace가 전체 validation을 통과할 때만 이름을 받아 저장한다. 삭제는
profile definition만 지우며 실행 중 terminal을 닫지 않는다는 사실을 confirmation에 명시한다.

### 3. Transaction-like workspace restore and profile applink

Files:

- `apps/wsl-desktop/src/App.tsx`
- `apps/wsl-desktop/src/lib/applink.ts`
- `apps/wsl-desktop/src/App.applink.test.tsx`

앱 시작 시 distro hydration 뒤 마지막 workspace를 한 번만 복원한다. profile 목록과 마지막
workspace가 준비되기 전에는 cold/hot applink를 소비하지 않아 빈 default distro 또는 오래된
profile state와 경쟁하지 않는다.

profile 전환은 다음 순서를 따른다.

1. 이미 전환 중이면 새 요청을 거부한다.
2. 기존 terminal을 대체할 때 정확한 종료 개수를 확인한다.
3. 모든 start command를 최종 문자열과 pane identity로 보여 주고 실행 여부를 선택한다.
4. 최대 32개 pane을 순차 시작하고 성공한 session을 stable pane key로 매핑한다.
5. 하나 이상 성공한 경우에만 tab ownership과 active identity를 한 번에 교체한다.
6. 요청한 active pane이 실패하면 성공한 active tab의 마지막 pane으로 안전하게 내린다.
7. 새 workspace가 commit된 뒤 이전 PTY session을 닫는다.
8. 일부 시작/닫기 실패는 성공한 terminal을 보존하고 고정된 개수 요약만 표시한다.

`stateRef`를 React commit보다 먼저 새 identity로 갱신해, 늦게 도착한 이전
`terminal-closed` event가 새 tab/pane을 제거하지 못하게 했다. context menu, tab/pane action,
layout button과 command palette는 profile 전환 중 닫히거나 비활성화된다.

### 4. Optional tmux/zellij adapter

Files:

- `apps/wsl-desktop/src-tauri/src/core/multiplexer.rs`
- `apps/wsl-desktop/src-tauri/src/commands/multiplexer.rs`
- `apps/wsl-desktop/src-tauri/src/commands/terminal.rs`

배포판 안에서 `tmux -V`와 `zellij --version`을 exact argv로 실행한다. 각 probe는 3초 timeout과
kill-on-drop을 사용하고 실패·invalid UTF-8·과도하거나 control character가 든 version output은
available로 보지 않는다. native는 probe 없이 항상 제공된다.

stable mux name은 validated distro와 pane key의 bounded slug 및 stable hash로 만든
`wsld-*` 이름이다. session probe는 tmux `has-session`과 zellij
`list-sessions --short --no-formatting`만 사용한다. zellij의 `EXITED` fixture는 running으로
오판하지 않는다.

tmux create/attach는 `new-session -A -s`와 optional `-c`를 쓴다. status와 mouse는 `-g` 없이
해당 devbox session target에만 `off`를 설정한다. `mouse`가 session option이라는 공식 범위에
맞춰 `-w`를 사용하지 않는다. zellij는 attach `--create`의 optional `options` subcommand로
내장 `disable-status` layout, `pane-frames false`, `mouse-mode false`를 전달한다.

adapter는 tool 설치, download, config file 수정, global tmux option 변경을 하지 않는다.
frontend 감지와 별개로 backend가 매 start 요청에서 availability를 다시 확인하고 unavailable이면
native argv로 내린다. 기존 mux session이면 `resumed=true`를 반환해 start command 재실행을 막는다.

### 5. Stable pane identity and one-time start command

Files:

- `apps/wsl-desktop/src/types.ts`
- `apps/wsl-desktop/src/components/PaneCanvas.tsx`
- `apps/wsl-desktop/src/components/TermPane.tsx`

runtime Pane은 stable `key`와 backend `sessionId`를 별도로 가진다. 저장된 `startCommand`와 이번
새 session에만 전달할 `initialCommand`도 분리한다. backend가 기존 mux session 재연결을 알리면
`initialCommand`는 비워 둔다.

TermPane은 output listener와 PTY attachment를 준비한 뒤 initial command에 carriage return을
붙여 한 번만 쓴다. font/active 상태 rerender에서는 xterm을 재생성하거나 명령을 다시 실행하지
않는다. PaneCanvas 회귀 테스트는 저장된 command가 있어도 resumed pane의 undefined
`initialCommand`만 전달되는 것을 직접 검증한다.

### 6. Native action palette and profile panel

Files:

- `apps/wsl-desktop/src/components/ActionPalette.tsx`
- `apps/wsl-desktop/src/components/WorkspacePanel.tsx`
- `apps/wsl-desktop/src/lib/shortcuts.ts`
- `apps/wsl-desktop/src/App.css`

`Ctrl+Shift+P`와 toolbar button이 app-owned action palette를 연다. 검색, 위/아래 이동, Enter,
Escape와 backdrop close를 지원한다. 활성 pane vertical/horizontal split, close, output search,
validated cwd copy 및 named profile 전환을 제공한다. split은 현재 pane의 distro/cwd/실제 mux
kind를 복제하지만 start command는 복제하지 않는다.

side panel은 profile save/open/delete와 선택 distro의 tmux/zellij available/version 상태를
보여 준다. external tool이 없으면 `없음 · native 사용`을 명시하며 native 선택은 계속 활성이다.

### 7. Explicit-target safe broadcast

Files:

- `apps/wsl-desktop/src/lib/broadcastSafety.ts`
- `apps/wsl-desktop/src/components/TermPane.tsx`
- `apps/wsl-desktop/src-tauri/src/commands/terminal.rs`

broadcast target은 현재 active tab의 session ID 가운데 사용자가 checkbox로 직접 선택한 값만
사용한다. 2개 미만이면 toggle이 꺼지고, tab/pane identity가 바뀌면 존재하지 않는 target을
즉시 제거한다.

입력 parser는 최근 logical command를 최대 4,096자로 추적한다. multiline input은 줄 내용을
보이지 않고 대상 수와 즉시 실행 가능성만 확인한다. shutdown/reboot/poweroff, recursive rm의
플래그 순서 변형, mkfs, output dd, Docker prune, kubectl delete, SQL drop/truncate, forced git
clean 등은 Enter 때 재확인한다. 취소하면 pending command를 지우지 않으므로 다음 Enter도 다시
확인한다.

backend도 2~32개 unique existing session, 최대 1,000,000-byte data를 강제한다. 모든 target의
존재를 쓰기 전에 확인하고 write/flush 실패를 무시하지 않는다. 오류에는 session ID나 raw input을
넣지 않는다.

### 8. Documentation and dependency inventory

Files:

- `apps/wsl-desktop/README.md`
- `docs/roadmap.md`
- `docs/superpowers/specs/2026-08-17-wsl-desktop-terminal-design.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`
- `Cargo.lock`
- `THIRD_PARTY_NOTICES.md`

P1-07 #262 merge와 P1-08 #263 구현 상태, 저장 위치·상한·offline/native 책임, optional mux
tradeoff, broadcast safety와 W1 pending evidence를 문서에 동기화했다.

직접 dependency는 backend profile ID 발급을 위한 기존 lock graph의 `uuid`뿐이다. tokio는 기존
dependency의 process feature에 timeout/test runtime features를 추가했다. dependency-policy
generator로 notice lock digest를 갱신했으며 수동 inventory 편집은 하지 않았다.

## PR-Boundary Review Findings

PR 생성 직전 전체 diff를 직접 리뷰하면서 다음 문제를 발견해 수정했다.

1. **resumed command 재실행**: PaneCanvas가 `initialCommand` 대신 영속 `startCommand`를
   TermPane에 전달하고 있었다. 기존 tmux/zellij session 재연결 때 server/start command가 다시
   실행될 수 있어 필드를 수정하고 fresh/resumed fixture를 추가했다.
2. **반복 credential marker 우회**: 첫 `--token=$TOKEN`만 검사하면 뒤의
   `--token=literal-value`가 누락됐다. 모든 occurrence를 검사하고 Authorization 공백 변형과
   private-key header 변형도 backend/frontend에서 동일하게 거부했다.
3. **danger flag 순서 우회**: `rm -rf`만 사실상 가정해 `rm -fr`을 놓쳤다. force 여부와 무관하게
   모든 recursive rm option token을 재확인하고 일반 hyphen filename은 오탐하지 않는 fixture를
   추가했다.
4. **tmux option scope 오류**: `mouse`는 session option인데 `set-option -w`로 강제돼 실행 오류가
   날 수 있었다. official tmux option scope에 맞춰 devbox session target으로 수정하고 전체 argv
   equality test로 고정했다.
5. **profile 전환 경합**: 전환 중 context/layout/palette action이 새/이전 identity 사이에 낄 수
   있었다. synchronous loading ref와 domain-wide busy guard로 진입점을 막았다.
6. **partial restore selection**: 요청 active pane 시작이 실패했을 때 실패 pane의 distro가 selector에
   남을 수 있었다. 실제 fallback active session에 대응하는 definition만 선택하게 수정했다.

## Test Coverage

Rust tests cover:

- store round-trip, unsupported/corrupt fail-closed와 upsert order
- duplicate/missing/orphan pane refs와 active-tab membership
- unsafe cwd, multiline/control command, repeated literal credential와 private-key variants
- stable bounded mux name, native/tmux/zellij exact argv와 no-shell/no-global-option policy
- exact probe argv, optional mux absence, zellij running/exited fixture
- broadcast 2~32 unique target와 input bound
- 기존 PTY UTF-8 chunk, lifecycle, resize/session command 회귀

Frontend tests cover:

- last-layout round-trip와 corrupt/version/orphan rejection
- runtime session ID 제거와 stable pane-key persistence
- POSIX/Windows path, command credential/private-key validation
- cold/hot profile applink와 stable identity/layout restore
- partial profile start failure 시 성공 pane/active identity 보존
- fresh/resumed pane의 one-time initial command 전달
- action palette search/arrows/Enter/Escape/backdrop
- explicit broadcast targets, multiline raw-content 비노출, danger cancel/reconfirm
- recursive rm flag variants, false-positive filename guard, bounded pending buffer
- 기존 context menu, xterm clipboard/OSC/search/link/font/resize 회귀

## Verification Results

### Frontend

```text
$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter wsl-desktop exec vitest run --passWithNoTests --maxWorkers=1
Test Files  14 passed (14)
Tests       101 passed (101)
exit 0

# PR review fixes after the full run
$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter wsl-desktop exec vitest run \
      src/App.applink.test.tsx src/lib/workspace.test.ts \
      src/lib/broadcastSafety.test.ts src/components/PaneCanvas.test.tsx \
      src/components/TermPane.test.tsx --maxWorkers=1
Test Files  5 passed (5)
Tests       38 passed (38)
exit 0

$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter wsl-desktop exec tsc --noEmit
exit 0

$ NODE_OPTIONS=--max-old-space-size=768 pnpm --filter wsl-desktop build
64 modules transformed
JS 641.44 kB / gzip 183.84 kB
CSS 15.45 kB / gzip 3.65 kB
exit 0
```

Vite의 500 kB chunk warning은 기존 xterm bundle 계열의 비차단 권고다.

### Rust

```text
$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo test -p wsl-desktop --lib -j1
42 passed; 0 failed
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo check -p wsl-desktop -j1
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo clippy -p wsl-desktop --all-targets -j1 -- -D warnings
exit 0

$ cargo fmt --package wsl-desktop -- --check
exit 0
```

### Repository policy

```text
$ python3 .github/scripts/check-dependencies.py check
dependency policy OK; notices match Cargo.lock and pnpm-lock.yaml

$ python3 .github/scripts/test-check-dependencies.py
dependency policy regression tests passed

$ python3 .github/scripts/test-build-manifest.py
build-manifest notice tests passed

$ bash .github/scripts/check-catalog.sh
exit 0

$ pnpm audit --audit-level moderate
No known vulnerabilities found

$ cargo deny --locked check
advisories ok, bans ok, licenses ok, sources ok
```

## Resource Discipline

Local validation used one feature worktree, `CARGO_BUILD_JOBS=1`, Cargo `-j1`, a Linux-native shared
target directory, Vitest `--maxWorkers=1` and Node 768 MiB heap cap. Frontend tests, build and Rust
checks ran sequentially. The full Vitest duration was dominated by `/mnt/e` jsdom environment setup;
raising worker count was intentionally avoided. No background watch/test process was left running.

## Remaining Checkpoint

- Windows W1 packaged build smoke, real WSL distro terminal restore, actual installed tmux/zellij
  attach/resume and evidence capture remain in the planned P1 checkpoint.
- WSL Desktop target version 0.4.0 is applied in Wave 9 version-bump/release preparation, not in this
  feature PR.
- Resource summary, external tool install/download, full mux session inventory/management and WebGL or
  drag-resize are outside issue #263 and were not pulled into this PR.
