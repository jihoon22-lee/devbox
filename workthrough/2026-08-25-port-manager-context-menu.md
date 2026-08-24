# Port Manager Context Menu

## Overview

Issue #249의 P1-06-PM 범위로 Port Manager의 포트/프로세스 행에
`@devbox/context-menu`를 적용했다. 우클릭, Shift+F10, Menu key로 동일한 앱 고유 메뉴를 열고,
대상 행 선택을 먼저 동기화한 뒤 port/PID/localhost URL/실행 파일 경로 관련 작업과 확인이
필수인 Kill을 수행한다.

이 작업은 Port Manager context menu만 소유한다. refresh/diff/favorite/provenance와 full command
line, process start time 기반 identity-safe kill은 각각 후속 #264 및 P2 #285 경계로 유지한다.

## Context

- 공용 렌더링·포커스·키보드·viewport primitive는 선행 #248에서 구현됐지만 Port Manager는
  아직 행 우클릭이나 선택 상태가 없었다.
- 기존 Kill 버튼은 확인 없이 곧바로 프로세스를 종료했다. 설계는 메뉴를 통한 Kill뿐 아니라
  파괴적 action 자체에 danger 표현과 사용자 확인을 요구한다.
- `list_ports`는 process name까지만 반환하지만 기존 `get_process_info` command가 PID별 실행
  파일 경로를 제공한다. P2의 전체 process identity DTO를 포트 목록에 미리 넣지 않고 메뉴를
  열 때만 이 경로를 지연 조회한다.
- 탐색기 표시 command가 임의의 프론트엔드 경로를 받으면 WebView 입력이 opener 권한으로
  확대된다. 따라서 프론트는 PID만 보내고 백엔드가 실행 시점의 executable을 다시 해석한다.

## Changes Made

### 1. App-owned menu, row selection, and action routing

Files:

- `apps/port-manager/src/App.tsx`
- `apps/port-manager/src/App.css`

각 포트 행은 focus 가능한 context-menu trigger가 됐고, pointer/keyboard open 직전에
`data-port-row-key`로 실제 `PortRow`를 다시 찾아 선택 상태와 메뉴 대상을 함께 갱신한다.
선택된 행은 hover와 구분되는 accent 배경을 사용하며 keyboard focus outline을 제공한다.

```tsx
<tr
  data-port-row-key={rowKey}
  tabIndex={0}
  aria-selected={selected}
  className={selected ? "selected" : undefined}
  {...contextMenu.triggerProps}
>
```

메뉴 항목은 UX 설계 §1.2의 Port Manager 행을 그대로 구현한다.

- Copy port
- Copy PID
- Copy localhost URL
- Open localhost — 유효한 port의 `LISTENING` 행에서만 활성
- Copy process path — PID 조회가 성공하고 executable이 있을 때만 활성
- Show in Explorer — 같은 조건에서 활성
- Kill process — PID가 있을 때만 활성, danger 스타일과 확인 필수

port/PID/path가 없는 항목은 제거하지 않고 disabled로 표시해 menu topology와 keyboard 순서를
안정적으로 유지한다. Clipboard 쓰기는 기존 저장소 정책대로 `navigator.clipboard.writeText`를
사용하며 새 clipboard plugin이나 capability를 추가하지 않았다.

### 2. Lazy process path resolution with stale-result protection

Files:

- `apps/port-manager/src/App.tsx`
- `apps/port-manager/src/api.ts`
- `apps/port-manager/src/types.ts`

메뉴를 열 때만 기존 `get_process_info` IPC를 호출한다. 요청 sequence ref와 row key를 함께
검사하므로 첫 행의 느린 응답이 나중에 연 두 번째 행의 path action을 활성화하지 못한다.
프로세스가 이미 종료됐거나 executable이 없으면 passive lookup 오류를 경로 미사용 상태로
격리하고 Copy path/Show in Explorer를 disabled로 유지한다.

브라우저 preview에는 mock port에 대응하는 deterministic `ProcessInfo`를 반환하되, UI는 경로를
자동 표시하거나 기록하지 않고 명시적 Copy action에서만 clipboard로 전달한다.

### 3. PID-only Explorer reveal boundary

Files:

- `apps/port-manager/src-tauri/src/commands/ports.rs`
- `apps/port-manager/src-tauri/src/lib.rs`

새 `reveal_process(pid)` command는 `sysinfo`로 PID를 새로 조회하고 OS가 보고한 executable만
`tauri-plugin-opener`의 `reveal_item_in_dir`에 전달한다.

```rust
let executable = process
    .exe()
    .ok_or_else(|| format!("PID {pid} 프로세스의 실행 파일 경로를 확인할 수 없습니다."))?;

app.opener().reveal_item_in_dir(executable)
```

이 계약에는 path 인자가 없어 임의 WebView 문자열로 다른 경로를 열 수 없다. PID가 사라졌거나
경로를 얻을 수 없거나 opener가 실패하면 복구 가능한 오류를 반환하고 앱의 기존 error banner에
표시한다.

### 4. Shared confirmation and recoverable failures

File: `apps/port-manager/src/App.tsx`

기존 Kill 버튼과 context-menu Kill이 같은 `onKill` 경로를 사용한다. PID와 가능한 process name을
확인 문구에 포함하고 사용자가 거절하면 IPC, busy state, refresh가 모두 실행되지 않는다. 승인한
경우에만 Kill을 요청하며 성공 후 목록을 갱신한다.

Copy/Open/Reveal은 공통 `runAction` 경계를 사용해 거절된 Clipboard promise, browser opener 오류,
PID reveal 오류를 unhandled rejection으로 남기지 않고 banner에 표시한다. localhost open은
외부 local address를 재사용하지 않고 항상 `http://localhost:<port>`를 만들며 LISTENING 행으로
제한한다.

### 5. Workspace dependency and generated notices

Files:

- `apps/port-manager/package.json`
- `pnpm-lock.yaml`
- `THIRD_PARTY_NOTICES.md`

Port Manager에 `@devbox/context-menu: workspace:*`를 추가했다. 새 third-party dependency나 Tauri
permission은 없다. offline lockfile update 뒤 dependency generator로 notices의 pnpm lock digest를
동기화했다.

### 6. App interaction fixtures

File rename and expansion:

- `apps/port-manager/src/App.test.ts` → `apps/port-manager/src/App.test.tsx`

기존 `matches` 6개 회귀 fixture를 보존하고 JSX interaction fixture 7개를 추가했다.

- stable row key와 localhost URL 생성
- 우클릭 행 selection 선동기화와 정확한 7개 menu item
- Shift+F10 open, 값 복사, trigger focus 복원
- 선택 PID의 지연 path 조회, exact path copy, PID-only Explorer reveal
- process lookup 실패 시 path action 비활성화
- Kill 거절 시 무동작, 승인 시 kill과 refresh
- ESTABLISHED 행 localhost open 차단과 opener 실패 banner

공용 primitive의 Menu key, Arrow/Home/End/Enter/Escape, viewport flip, submenu, IME 회귀 fixture는
선행 package 테스트가 계속 담당하고 이 앱 테스트는 app-owned selection/action/confirmation에
집중한다.

## Verification Results

### Frontend interaction tests

```text
$ NODE_OPTIONS=--max-old-space-size=1024 \
    pnpm --filter port-manager test -- --maxWorkers=1
Test Files  1 passed (1)
Tests      13 passed (13)
exit 0
```

첫 실행은 JSX로 확장된 fixture가 `.ts` 확장자에 남아 transform 단계에서 실패했다. 파일을
`App.test.tsx`로 명확히 변경한 뒤 동일 command가 통과했다.

### Frontend production build

```text
$ NODE_OPTIONS=--max-old-space-size=1024 pnpm --filter port-manager build
tsc
vite v7.3.6 building client environment for production...
39 modules transformed
dist/assets/index-BpiUq50M.js  207.69 kB | gzip: 65.56 kB
exit 0
```

첫 typecheck가 closure 안의 nullable PID narrowing을 지적했다. 조건문에서 검증된 PID를 local
constant로 캡처해 async callback에 전달한 뒤 production build가 통과했다.

### Rust formatting, tests, and compile check

```text
$ cargo fmt --package port-manager -- --check
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo test -p port-manager -j1
test result: ok. 0 failed
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo check -p port-manager --all-targets -j1
Finished dev profile
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo clippy -p port-manager --all-targets -j1 -- -D warnings
Finished dev profile
exit 0
```

Rust command는 Windows-only process UX를 직접 실행하지 않지만 Linux/WSL target에서 새 command,
`OpenerExt`, sysinfo path type과 Tauri registration이 모두 compile됨을 확인한다. 실제 Explorer,
WebView2 clipboard와 packaged focus 동작은 계획된 Windows W1 checkpoint evidence 범위다.

### Dependency artifacts

```text
$ NODE_OPTIONS=--max-old-space-size=768 pnpm install --lockfile-only --offline
downloaded 0

$ python3 .github/scripts/check-dependencies.py generate
generated THIRD_PARTY_NOTICES.md

$ python3 .github/scripts/check-dependencies.py check
dependency policy OK; notices match Cargo.lock and pnpm-lock.yaml

$ python3 .github/scripts/test-check-dependencies.py
dependency policy regression tests passed

$ python3 .github/scripts/test-build-manifest.py
build-manifest notice tests passed

$ bash .github/scripts/check-catalog.sh
exit 0
```

전체 workspace matrix와 Windows compile은 GitHub Actions를 권위 있는 gate로 사용한다.

## Security and Failure Boundaries

- 메뉴 label과 clipboard payload는 Port Manager state에서만 생성하며 raw credential을 다루지
  않는다.
- process path는 메뉴 open 때 해당 PID에 한해 조회하고 UI, persistence, log에 자동 노출하지
  않는다.
- Explorer command는 프론트 제공 path 대신 backend-resolved executable만 연다.
- non-listening local endpoint는 open action이 disabled이며 handler도 같은 guard를 재검사한다.
- disabled item은 공용 primitive가 pointer/keyboard activation을 모두 차단한다.
- Kill은 버튼과 메뉴 모두 confirmation 전에는 IPC를 보내지 않는다.
- PID 재사용까지 막는 start-time/executable identity check와 full command 표시 기능은 P2 #285가
  소유하며 이 PR에서 불완전하게 선구현하지 않는다.

## Follow-up

- #250~#261: 나머지 기존 앱의 앱 고유 context menu
- #264: Port Manager refresh/diff/favorite/provenance
- #285: full command/executable/start time과 identity-safe kill
- Windows W1: packaged build에서 mouse/Menu key/Shift+F10, focus restore, clipboard, Explorer,
  confirmation과 오류 화면 evidence
