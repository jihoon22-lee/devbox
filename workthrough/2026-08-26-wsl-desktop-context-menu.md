# WSL Desktop Pane and Tab Context Menu

## Overview

Issue #260의 P1-06-WSL 범위로 WSL Desktop의 터미널 팬과 탭에
`@devbox/context-menu`를 적용했다. pointer 우클릭, `Shift+F10`, Menu 키는 같은
app-owned action 경로를 사용하고, 메뉴를 열기 전 exact pane/tab ID를 활성화한다.
팬 메뉴가 닫힌 뒤에는 wrapper DOM을 넘어 실제 xterm 인스턴스로 focus를 복원한다.

팬 메뉴 topology는 다음과 같다.

```text
복사 (P1-07 전까지 disabled)
붙여넣기 (P1-07 전까지 disabled)
검색 (P1-07 전까지 disabled)
세로 분할
가로 분할
cwd 복사 (P1-07 전까지 disabled)
팬 닫기 (danger, confirmation)
```

탭 메뉴 topology는 다음과 같다.

```text
닫기 (danger, confirmation)
다른 탭 닫기 (danger, confirmation)
이름 변경
레이아웃 전환 ▸
  격자
  세로 분할
  가로 분할
```

터미널 selection, clipboard read/write, search addon, OSC 7 cwd는 #262(P1-07),
pane key/session ID 분리와 profile/layout 저장은 #263(P1-08)의 단일 기능 PR 경계에
남겼다.

## Context

- WSL Desktop에는 기존 pane close button, tab close button, `Ctrl+Shift+W`, layout toolbar가
  있었지만 pointer/keyboard context menu가 없었다.
- 기존 close 경로는 확인 없이 backend session을 닫고 UI state를 즉시 제거했다.
- xterm은 팬 wrapper 안에 자체 focusable surface를 소유하므로 공용 메뉴의 DOM focus
  restore만으로는 terminal input을 재개한다고 보장할 수 없다.
- xterm 내부 handler가 bubble을 멈출 수 있어 pane pointer menu는 capture phase에서
  받아야 한다.
- `Pane.cwd`는 세션을 시작한 경로일 뿐 현재 shell cwd가 아니다. 이 값을
  `cwd 복사`로 표시하면 잘못된 경로를 유출한다.
- 탭의 여러 session을 동시에 닫을 때 backend 완료 이벤트, 부분 실패,
  새 session 생성/팬 이동이 겹칠 수 있다. 최신 상태를 보지 않고 탭 전체를
  지우면 새 팬이 고아 상태로 남는다.

## Changes Made

### 1. App-owned exact menu contracts

Files:

- `apps/wsl-desktop/src/lib/contextMenu.ts`
- `apps/wsl-desktop/src/lib/contextMenu.test.ts`

공용 package는 배치, keyboard navigation, focus trap/restore, submenu, danger/disabled 표현만
소유한다. WSL Desktop은 exact item ID/label, action availability와 dispatch를 소유한다.

팬의 copy/paste/search/copy-cwd는 topology에서 삭제하지 않았다. 대신 #262가
xterm selection snapshot, clipboard permission, multiline paste confirm, search lifecycle, OSC 7을
구현할 때까지 disabled로 두어 외형만 있는 가짜 action을 막았다.

### 2. Target-first pane and tab triggers

Files:

- `apps/wsl-desktop/src/App.tsx`
- `apps/wsl-desktop/src/components/PaneCanvas.tsx`
- `apps/wsl-desktop/src/components/TermPane.tsx`
- `apps/wsl-desktop/src/components/TabBar.tsx`

팬 root에 stable backend session ID, 탭 pill에 stable tab ID를 `data-*` attribute로 놓았다.
`onBeforeOpen`은 현재 `panes`/`tabs`에서 ID를 다시 찾고 소유 탭·활성 팬을 먼저
동기화한다. target이 사라지면 열린 메뉴와 stale context snapshot을 닫는다.

탭 pill은 `tabIndex=0`, 한국어 `aria-label`, `aria-current`를 가진다. 팬 root는
programmatic focus target이지만 기존 xterm keyboard surface를 대체하지 않도록 `tabIndex=-1`을
사용한다. xterm이 bubble event를 멈춰도 우클릭을 받도록 pointer trigger는
`onContextMenuCapture`에 연결했다.

### 3. Native xterm focus restoration

Files:

- `apps/wsl-desktop/src/App.tsx`
- `apps/wsl-desktop/src/components/TermPane.tsx`
- `apps/wsl-desktop/src/components/TermPane.test.tsx`

`App` 소유 registry가 session ID와 `term.focus()` handle을 연결한다. 공용 menu를 닫은
뒤 zero-delay task에서 exact pane handle을 호출해 wrapper만 포커스되는 문제를 막았다.

focus registry effect는 xterm mount effect와 분리했다. React callback identity가 바뀌어도
`Terminal` 인스턴스가 재생성되지 않아 스크롤백과 PTY attachment를 보존한다.

### 4. Split and layout actions

Files:

- `apps/wsl-desktop/src/App.tsx`
- `apps/wsl-desktop/src/App.contextMenu.test.tsx`

팬 split은 우클릭한 팬의 소유 탭, distro, 초기 cwd를 exact input으로 사용한다.
세션 시작이 성공한 후에만 owner tab을 `cols` 또는 `rows`로 전환하며, 실패 시
일부 layout state를 남기지 않는다. 탭 layout submenu는 context target tab ID를 사용하여
이전 active selection에 잘못 적용되지 않는다.

### 5. Unified danger confirmation and close reconciliation

Files:

- `apps/wsl-desktop/src/App.tsx`
- `apps/wsl-desktop/src/components/PaneCanvas.tsx`
- `apps/wsl-desktop/src/components/TabBar.tsx`
- `apps/wsl-desktop/src/App.contextMenu.test.tsx`

팬 닫기, 탭 닫기, 다른 탭 닫기는 session/pane 개수와 실행 중 작업 종료
가능성을 알리고 승인 전에 backend를 호출하지 않는다. 기존 pane/tab close button과
`Ctrl+Shift+W`도 같은 request handler를 사용해 확인 우회 경로를 없앰다.

탭 batch close는 각 session의 settled result를 ID와 결합한다. 성공한 session ID만
최신 모든 tab pane list와 pane pool에서 제거하고, 실패한 session과 닫기 중
새로 생긴 session은 유지한다. backend close event가 먼저 UI를 갱신해도 동일 변환은
멱등적이다. 활성 팬이 닫혀도 존재하는 탭/팬으로 fallback을 재계산한다.

### 6. Fixed safe errors and action busy boundary

Backend의 raw error는 Windows/WSL path, command 문자열, credential-like 텍스트를 포함할
수 있다. context split, pane close, tab close 실패는 각각 고정된 한국어 오류만 화면에
표시한다. 변경 중은 메뉴 mutation과 기존 close/new-tab/layout button을 비활성화해
중복 action을 줄였다.

### 7. Documentation and dependency boundary

Files:

- `apps/wsl-desktop/README.md`
- `docs/architecture.md`
- `docs/superpowers/specs/2026-08-17-wsl-desktop-terminal-design.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`
- `THIRD_PARTY_NOTICES.md`
- `workthrough/2026-08-26-wsl-desktop-context-menu.md`

새 의존성은 이미 검증된 private workspace package `@devbox/context-menu` 하나뿐이다. native
plugin, Tauri capability, sidecar, network dependency, 외부 terminal/tool은 추가하지 않았다.

## Test Coverage

Frontend unit/integration tests cover:

- pane/tab exact item topology, submenu, danger/disabled state
- pointer target-first selection과 `Shift+F10`/Menu key
- context menu close 후 exact xterm focus handle 복원
- split의 exact distro/cwd/owner tab 인자와 성공 후 layout 전환
- close confirmation 취소 전 backend/storage 불변
- 탭 이름 줄바꿈 정규화·80자 상한·빈 이름 거부
- 다른 탭 닫기의 exact session 대상
- batch close 부분 실패 시 성공 session만 제거하고 실패 pane 보존
- split/close backend raw path·credential-like 오류의 DOM 비노출
- focus registry callback 변경 시 xterm 재마운트 방지
- 기존 ConPTY build number, resize floor/retry, hidden pane, applink, shortcut 회귀

## Verification Results

PR 직전에 단일 worker와 Linux-native Cargo target cache로 확인했다.

### Frontend tests

```text
$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter wsl-desktop exec vitest run --maxWorkers=1
Test Files  10 passed (10)
Tests       66 passed (66)
exit 0

# 최종 검토에서 exact inactive-pane target과 단일 tab close 회귀를 추가한 뒤
$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter wsl-desktop exec vitest run src/App.contextMenu.test.tsx --maxWorkers=1
Test Files  1 passed (1)
Tests       8 passed (8)
exit 0
```

최종 현재 suite는 10개 파일·67개 테스트이며, 추가된 1개와 해당 파일의
기존 7개를 최종 targeted run에서 다시 통과시켰다. TypeScript `--noEmit`도 그 뒤
다시 통과했다.

### Frontend build

```text
$ NODE_OPTIONS=--max-old-space-size=768 pnpm --filter wsl-desktop build
vite production build passed (54 modules)
exit 0
```

Vite는 xterm이 포함된 580.64 kB 단일 JS chunk에 대한 500 kB 권고를 표시했지만
build는 성공했다. 이 비차단 번들 구성은 본 PR에서 새로 추가한 외부 의존성이
아니며 별도 performance 최적화 범위다.

### Rust tests and compile gates

```text
$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo test -p wsl-desktop -j1
29 passed; 0 failed
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo check -p wsl-desktop -j1
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo clippy -p wsl-desktop -j1 -- -D warnings
exit 0

$ cargo fmt --all --check
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

$ git diff --check
exit 0
```

## Key Decisions

1. **우클릭은 메뉴다.** 붙여넣기는 #262에서 가운데 버튼·`Ctrl+Shift+V`와 함께
   구현하고 right-click paste로 되돌리지 않는다.
2. **현재 cwd를 위조하지 않는다.** 시작 cwd는 split 인자로 재사용하지만 복사 표시는
   OSC 7이 연결될 때까지 disabled다.
3. **danger 확인은 모든 진입점에서 공유한다.** context menu만 안전하고 기존
   close button/단축키는 즉시 닫는 우회를 허용하지 않는다.
4. **batch close는 탭이 아니라 settled session을 reconciliation 단위로 쓴다.**
   성공한 session만 제거해 부분 실패와 동시 상태 변경을 보존한다.
5. **terminal focus handle을 별도 registry로 둔다.** DOM focus restore 후 xterm input을
   복원하되 registry 변경으로 Terminal을 재생성하지 않는다.
6. **#262/#263을 당겨오지 않는다.** clipboard, search, OSC, persisted profile/layout은
   각 기능 PR의 전체 권한·persistence 계약과 함께 구현한다.

## Follow-up Work

- #261: Life Log date context menu로 기존 13개 앱의 P1-06 적용을 끝낸다.
- #262(P1-07): selection auto-copy, `Ctrl+Shift+C/V`, middle paste, search, OSC 7/8,
  title/wrap/scrollback/font, resize safety를 연결해 disabled pane action을 활성화한다.
- #263(P1-08): pane key/session ID 분리, tab/pane/layout/profile persistence, action palette,
  opt-in tmux/zellij attach, broadcast safety를 별도 PR로 구현한다.
- W1 checkpoint에서 packaged WebView2/ConPTY로 pointer capture, keyboard menu, submenu viewport,
  confirm, split, xterm focus restore, backend close event 경합을 확인한다.
