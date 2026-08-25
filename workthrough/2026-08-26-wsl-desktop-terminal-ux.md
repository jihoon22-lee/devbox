# WSL Desktop Clipboard and Terminal UX

## Overview

Issue #262의 P1-07 범위로 WSL Desktop에 외부 터미널을 실행하지 않아도 동작하는
clipboard·검색·OSC·링크·글꼴 기본기를 추가했다. 각 xterm 인스턴스가 selection, 현재 cwd,
검색 UI와 paste를 직접 소유하고 App은 exact-pane handle registry, 자동 탭 제목과 영속 설정만
조정한다. 기존 xterm 인스턴스를 설정 변경 때 재생성하지 않으므로 PTY attachment와 10,000줄
scrollback을 보존한다.

주요 사용자 경로는 다음과 같다.

```text
drag selection -> 120ms 동안 안정된 selection 자동 복사 (기본 on)
Ctrl+Shift+C -> exact pane selection 명시적 복사
Ctrl+Shift+V / middle click -> clipboard read -> multiline 확인 -> term.paste()
Ctrl+Shift+F / context menu -> exact pane scrollback search
OSC 0/2 -> pane title -> 활성 auto-title tab
OSC 7 -> validated current cwd -> exact pane cwd 복사
OSC 8 / plain URL -> HTTP(S) allowlist -> credential 거부 -> host 확인 -> 기본 브라우저
Ctrl + / - / 0 -> 영속 font size 변경 -> xterm 유지 -> fit/resize retry
```

native workspace/profile, layout persistence, action palette와 optional tmux/zellij adapter는
#263(P1-08)에 남겼다. WebGL renderer와 drag pane resize도 #262 acceptance에 포함하지 않았다.

## Context

- #260은 WSL pane/tab context menu topology와 split/close를 먼저 구현했지만 selection,
  clipboard permission, search addon, OSC 7이 없어서 copy/paste/search/cwd action을 disabled로
  남겼다.
- `Pane.cwd`는 시작 경로일 뿐 현재 shell cwd가 아니므로 menu capability로 사용할 수 없었다.
- `Ctrl+C`를 selection 상태에 따라 복사로 바꾸면 readline/TUI의 SIGINT가 불안정해진다.
- clipboard read는 WebView user activation/permission 실패가 가능하며 raw 오류에는 host/path
  정보가 들어갈 수 있다.
- multiline paste는 bracketed paste를 지원하지 않는 shell/TUI에서 명령을 즉시 실행할 수 있다.
- xterm option prop을 mount effect 의존성에 넣으면 font/setting 변경 때 terminal이 재생성돼
  scrollback을 잃는다.
- output link는 terminal content가 만든 외부 상태 변경이므로 scheme validation과 사용자
  confirmation 없이 opener로 넘길 수 없다.

## Changes Made

### 1. Pure terminal input and metadata boundaries

Files:

- `apps/wsl-desktop/src/lib/terminalUx.ts`
- `apps/wsl-desktop/src/lib/terminalUx.test.ts`

`matchTerminalKey`는 terminal-local shortcut만 분류한다. `Ctrl+Shift+C/V/F`와
`Ctrl++/-/0`을 가로채되 bare `Ctrl+C`는 언제나 null을 반환해 PTY의 SIGINT 경로에 남긴다.
AltGr 계열의 Ctrl+Alt와 Meta 조합도 가로채지 않는다.

OSC 7 parser는 최대 4,096자의 `file:` URL만 받고 credential, port, query, fragment,
control character, decode 실패와 비절대 경로를 거부한다. OSC 0/2 title은 control character와
줄바꿈을 제거하고 120자로 제한한다. URL은 최대 2,048자, HTTP(S)만 허용하며 embedded
username/password를 거부한다.

font size는 9~24px 정수로 clamp한다. clipboard paste는 1,000,000자, search query는 512자로
상한을 고정해 plugin/addon에 비정상적으로 큰 입력을 넘기지 않는다.

### 2. Clipboard channel and failure isolation

Files:

- `apps/wsl-desktop/src/api.ts`
- `apps/wsl-desktop/src/components/TermPane.tsx`
- `apps/wsl-desktop/src-tauri/Cargo.toml`
- `apps/wsl-desktop/src-tauri/capabilities/default.json`
- `apps/wsl-desktop/src-tauri/src/lib.rs`

selection write는 기존 앱 선례와 동일한 `navigator.clipboard.writeText`를 사용한다. selection
change마다 부분 문자열을 반복 복사하지 않도록 120ms 동안 값이 유지된 마지막 selection만
복사하고 동일 selection 재이벤트는 무시한다. 설정을 끄면 명시적 `Ctrl+Shift+C`와 menu copy만
남는다.

clipboard read는 공식 Tauri plugin의 `readText`만 허용한다. capability는
`clipboard-manager:allow-read-text` 하나이며 image/write/clear permission은 주지 않았다.
browser preview에서는 `navigator.clipboard.readText`로 같은 계약을 유지한다.

paste는 `writeSession`이 아니라 `term.paste()`를 사용해 xterm bracketed-paste 처리를 거친다.
개행이 있으면 clipboard 원문 대신 줄 수와 실행 가능성만 confirmation에 표시한다. 사용자가
취소하면 PTY input을 전혀 쓰지 않고 terminal focus만 복원한다. read/write/paste 실패는 각각
고정된 한국어 메시지만 App에 전달하며 raw exception을 DOM이나 console에 반향하지 않는다.

right click은 #260 context menu로 유지하고 middle-button `auxclick`만 paste로 연결했다.

### 3. Exact-pane handle registry and menu capabilities

Files:

- `apps/wsl-desktop/src/App.tsx`
- `apps/wsl-desktop/src/lib/contextMenu.ts`
- `apps/wsl-desktop/src/components/PaneCanvas.tsx`
- `apps/wsl-desktop/src/components/TermPane.tsx`

각 TermPane은 session ID에 `TerminalPaneHandle`을 등록한다. handle은 현재 xterm을 직접 조회하는
`getCapabilities`, `copySelection`, `pasteClipboard`, `openSearch`, `copyCwd`만 노출한다.
selection change마다 App state를 갱신하지 않으며 menu `onBeforeOpen` 순간 exact pane의
`hasSelection`과 validated OSC 7 cwd 존재 여부를 snapshot한다.

copy와 cwd copy는 해당 capability가 있을 때만 활성화되고, paste/search는 pane action이 busy가
아니면 활성화된다. dispatch도 context snapshot의 session ID로 handle을 다시 찾으므로 이전
active pane으로 action이 잘못 전달되지 않는다. menu close 후 #260 focus registry가 실제
xterm으로 focus를 복원한다.

### 4. Per-pane scrollback search

Files:

- `apps/wsl-desktop/src/components/TermPane.tsx`
- `apps/wsl-desktop/src/App.css`

official `@xterm/addon-search`를 각 xterm에 한 번 load한다. `Ctrl+Shift+F`와 context action은 pane
header 아래의 search bar를 열고 input에 focus한다. 입력 변화는 bounded incremental search,
Enter/다음 버튼은 next, Shift+Enter/이전 버튼은 previous를 수행한다. addon result event를
`current/total`로 표시하고 Escape/닫기에서 decoration과 query를 지운 뒤 terminal로 focus를
돌린다.

검색 UI state 변경은 mount effect dependency가 아니므로 xterm과 scrollback이 유지된다.

### 5. OSC title/cwd and safe links

Files:

- `apps/wsl-desktop/src/components/TermPane.tsx`
- `apps/wsl-desktop/src/types.ts`
- `apps/wsl-desktop/src/App.tsx`

`term.onTitleChange`로 정규화한 OSC 0/2 title을 Pane에 저장한다. 사용자가 이름을 정하지 않은
탭은 현재 활성 pane title을 따르고, context menu rename은 `customTitle`을 기록해 이후 OSC가
사용자 이름을 덮어쓰지 못하게 한다. 같은 title/cwd가 반복되면 기존 state object를 반환해
불필요한 App rerender를 막는다.

OSC 7 handler는 순수 parser를 통과한 current cwd만 pane과 handle에 저장한다. 시작 cwd가
있더라도 유효한 OSC 7을 받기 전에는 `cwd 복사`를 활성화하지 않는다.

OSC 8은 xterm core `linkHandler`, plain output URL은 official `addon-web-links`를 사용하되 둘 다
같은 validation/confirmation 함수로 수렴한다. unsafe scheme과 credential URL은 opener에
도달하지 않는다. valid HTTP(S)는 전체 output을 prompt에 노출하지 않고 hostname만 확인한 뒤
Tauri opener 또는 browser preview의 noopener 새 탭으로 연다.

### 6. Persistent copy/font settings without remount

Files:

- `apps/wsl-desktop/src/lib/storage.ts`
- `apps/wsl-desktop/src/App.tsx`
- `apps/wsl-desktop/src/App.css`

selection auto-copy는 저장 값이 없으면 on이고 사용자가 끈 값도 보존한다. font size는 13px
기본값과 9~24px validation을 거쳐 localStorage에 저장한다. 툴바의 A-/px/A+ controls와
terminal shortcut은 같은 updater를 쓴다.

font prop 변화는 기존 `Terminal.options.fontSize`만 갱신한 뒤 현재 fit/resize 함수를 rAF에서
호출한다. hidden pane은 기존 4행×20열 floor 아래 resize를 보내지 않고 다시 활성화될 때 fit한다.
resize는 기존 ack-after-commit sequence guard를 그대로 사용하므로 실패한 같은 dimensions를
다음 fit에서 재시도한다.

### 7. Dependency, notice and documentation updates

Files:

- `apps/wsl-desktop/package.json`
- `pnpm-lock.yaml`
- `Cargo.lock`
- `THIRD_PARTY_NOTICES.md`
- `docs/dependency-policy.md`
- `apps/wsl-desktop/README.md`
- `docs/roadmap.md`
- `docs/superpowers/specs/2026-08-17-wsl-desktop-terminal-design.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`

`THIRD_PARTY_NOTICES.md`는 직접 편집하지 않고 dependency-policy generator로 재생성했다.
inventory는 Rust 662개, frontend runtime 154개, 133,921 bytes이며 두 lockfile digest와
일치한다.

## Runtime Dependency Review

| Field | Evidence |
|---|---|
| Purpose | WSL pane의 native clipboard read, scrollback search와 plain URL range detection |
| Alternatives | clipboard 직접 WebView permission 처리만으로는 native 실패 경계와 일관되지 않음. search/link buffer parser 자체 구현은 xterm Unicode/wrap/range lifecycle을 복제하고 유지보수 위험이 큼. 외부 terminal 연결은 offline embedded workflow를 대체해 제품 목적과 맞지 않음 |
| Source | official Tauri plugins workspace와 official xterm.js monorepo/npm registry |
| Pin | `tauri-plugin-clipboard-manager` 2.3.2, `@xterm/addon-search` 0.16.0, `@xterm/addon-web-links` 0.12.0이 Cargo/pnpm lock integrity로 고정됨 |
| License | clipboard plugin MIT OR Apache-2.0, xterm addons MIT. 기존 allowlist에 포함되고 generated notices에 exact source/digest를 기록 |
| Size | addon unpacked size 838,673/45,573 bytes. 같은 Node/Vite build에서 JS 580.64→623.61 kB(+42.97 kB), gzip 164.78→177.98 kB(+13.20 kB). CSS는 11.69→12.74 kB. Windows installer/runtime memory delta는 W1에서 기록 |
| Security | clipboard capability read-text only. paste 1,000,000자, search 512자, URL 2,048자, OSC cwd 4,096자, title 120자 상한. link HTTP(S) only, credential reject, host confirmation. raw plugin/opener errors 비노출 |
| Offline | 설치 뒤 OS clipboard/xterm in-process addon만 사용하며 search, paste, OSC, URL validation에 network/sidecar/download가 없음. 사용자 확인 뒤 실제 link를 여는 동작만 외부 browser/network 상태를 사용할 수 있음 |
| Maintenance | Tauri/xterm release와 advisory를 lock update PR에서 감시. xterm major마다 addon compatibility와 bundle delta를 재측정. rollback은 addon load/UI와 WSL direct clipboard dependency/capability를 제거해 기존 PTY path로 복귀 가능 |

## Test Coverage

Frontend unit/integration tests cover:

- `Ctrl+Shift+C/V/F`, font shortcuts와 bare Ctrl+C pass-through
- font clamp, copy-on-select/font storage 기본값·왕복·손상 값 fallback
- OSC 7 file URL decode와 scheme/credential/query/control rejection
- OSC title control/newline stripping과 length bound
- HTTP(S) link normalization과 unsafe scheme/credential rejection
- CRLF/LF multiline detection과 line count
- 120ms settled selection auto-copy, duplicate partial selection 억제
- clipboard write reject fixed message와 raw path/credential 비노출
- explicit selection copy, multiline confirmation, `term.paste`, oversized paste rejection
- middle paste와 exact-pane capability/dispatch
- search next/previous/result count/Escape lifecycle와 512자 bound
- OSC title/cwd metadata dispatch, automatic tab title와 manual rename precedence
- OSC 8/plain link shared allowlist/confirmation/opener path
- font option update 시 terminal instance 비재마운트
- context menu capability/busy topology와 exact pane action
- 기존 ConPTY build number, scrollback-preserving PaneCanvas, resize floor/reject retry,
  pending timer cancellation, hidden-pane activation, applink, Docker/parser와 app shortcut 회귀

## Verification Results

### Frontend

```text
$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter wsl-desktop exec vitest run --maxWorkers=1
Test Files  11 passed (11)
Tests       82 passed (82)
exit 0

# 최종 입력 상한 보강 뒤
$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter wsl-desktop exec vitest run \
      src/components/TermPane.test.tsx --maxWorkers=1
Test Files  1 passed (1)
Tests       11 passed (11)
exit 0

$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter wsl-desktop exec tsc --noEmit
exit 0

$ NODE_OPTIONS=--max-old-space-size=768 pnpm --filter wsl-desktop build
60 modules transformed
JS 623.61 kB / gzip 177.98 kB
exit 0
```

Vite의 500 kB chunk warning은 기존 xterm bundle에서도 발생한 비차단 권고다. main baseline은
54 modules, JS 580.64 kB/gzip 164.78 kB였고 이번 기능 delta는 위 dependency review에 기록했다.

### Rust

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

### Dependency and repository policy

```text
$ pnpm audit --audit-level moderate
No known vulnerabilities found

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

1. **Ctrl+C는 항상 SIGINT다.** selection 자동 복사와 명시적 Ctrl+Shift+C를 별도로 제공해
   shell input 의미를 selection state에 종속시키지 않는다.
2. **paste는 xterm이 소유한다.** clipboard read 뒤 `term.paste()`를 사용하고 multiline과
   크기 상한을 PTY write 전에 확인한다.
3. **App에 selection을 복제하지 않는다.** menu open 순간 exact xterm handle에서 capability를
   읽어 output/selection의 빈번한 React state 전파를 피한다.
4. **현재 cwd는 OSC 7만 신뢰한다.** 시작 cwd가 있더라도 shell이 이동한 뒤 stale할 수 있으므로
   메뉴는 valid OSC 7 전까지 cwd copy를 비활성화한다.
5. **링크 감지와 실행을 분리한다.** xterm addon은 range detection만 하고 scheme, credential,
   confirmation, opener는 app-owned boundary가 담당한다.
6. **설정 변화로 terminal을 재마운트하지 않는다.** font/search/copy toggle state는 refs와
   option update로 연결해 scrollback과 PTY attachment를 보존한다.
7. **수동 탭 이름이 우선한다.** 자동 탭 제목은 활성 pane의 OSC title을 따르지만 사용자 rename
   이후에는 background shell sequence가 이를 덮어쓰지 않는다.

## Follow-up Work

- #263(P1-08): native layout/session/profile/workspace/action palette와 optional mux adapter.
- W1 checkpoint(나머지 P1 merge 후): packaged WebView2 clipboard read 성공/권한 실패,
  middle/multiline paste, Ctrl+C SIGINT, OSC 0/2·7·8, search, long-line wrap, hidden resize,
  font persistence와 installer/runtime memory delta를 Windows에서 기록한다.
- WebGL renderer와 drag pane resizing은 #262 acceptance 밖이므로 후속 성능/UX candidate에서
  fallback과 accessibility 계약을 별도로 설계한다.
