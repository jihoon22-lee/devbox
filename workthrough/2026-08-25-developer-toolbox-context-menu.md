# Developer Toolbox Input and Output Context Menus

## Overview

Issue #250의 P1-06-DT 범위로 Developer Toolbox의 text input과 result surface에
`@devbox/context-menu`를 적용했다. 입력은 Paste·Select all·Clear, 출력은 Copy·Select all·
Save result file을 제공하며 mouse right-click, Shift+F10, Menu key, keyboard navigation과 close
후 focus 복귀는 공용 primitive를 사용한다.

모든 계산 도구와 pipeline 확장은 이 PR에 포함하지 않는다. 기존 JSON/Base64/URL/timestamp/
case/JWT/hash/UUID/regex/diff 동작은 그대로 유지하고 context menu와 필요한 clipboard read
boundary만 추가했다.

## Context

- Developer Toolbox의 입력은 controlled `<input>`/`<textarea>`, 출력은 `<pre>` 또는 diff
  `<div>`였고 browser/WebView 기본 context menu 외의 app action이 없었다.
- 입력 custom menu가 기본 browser menu를 대체해도 Ctrl+X/C/V/Z와 IME composition event를
  소비해서는 안 된다. 공용 `useContextMenu`는 context-menu open key만 처리하며 이 앱도 별도
  keydown interception을 추가하지 않는다.
- 출력 파일 저장은 결과를 다른 프로그램으로 복사해 옮기는 반복을 줄여야 하지만, 전체
  filesystem plugin이나 임의 경로 write 권한은 필요하지 않다. 명시적 action에서 plain-text
  local download만 만든다.
- Clipboard read는 WebView2의 browser permission 상태에만 의존하면 Paste가 불안정하다. 공식
  Tauri clipboard manager를 사용하되 plain-text read command 하나만 허용한다.

## Changes Made

### 1. Reusable app-owned input menu

File: `apps/developer-toolbox/src/tools/common.tsx`

`ToolTextArea`와 `ToolTextField`는 controlled value contract를 유지하면서 input별 context-menu
controller를 소유한다. 메뉴가 열리기 직전 현재 element와 selection range를 캡처한다.

```tsx
const INPUT_MENU_ITEMS = [
  { type: "item", id: "paste", label: "Paste" },
  { type: "item", id: "select-all", label: "Select all" },
  { type: "separator", id: "input-separator" },
  { type: "item", id: "clear", label: "Clear" },
];
```

- Paste는 사용자가 항목을 선택한 뒤에만 clipboard plain text를 읽는다.
- 캡처한 selection을 clipboard text로 교체하고 controlled setter를 한 번 호출한다.
- 처리 후 caret을 삽입된 text 뒤에 복원한다.
- Select all은 원래 input을 focus한 뒤 native `select()`를 사용한다.
- Clear는 controlled state를 빈 문자열로 만들고 caret을 0으로 복원한다.
- empty input에서는 Select all과 Clear가 disabled지만 Paste는 계속 사용할 수 있다.
- Clipboard 거부/사용 불가는 현재 도구 아래 recoverable `role=alert`로 표시하고 기존 입력을
  변경하지 않는다. Clipboard 내용은 오류, log, settings, snapshot에 포함하지 않는다.

### 2. Reusable result menu and local text export

File: `apps/developer-toolbox/src/tools/common.tsx`

`ToolOutput`은 `<pre>`와 diff용 `<div>` result surface를 focus 가능한 menu trigger로 만든다.

- Copy는 현재 surface가 소유한 exact result string만 clipboard에 쓴다.
- Select all은 surface를 먼저 focus한 뒤 DOM Range를 적용한다.
- Save result file은 UTF-8 `text/plain` Blob을 만들고 app-owned filename의 local download를
  시작한 뒤 object URL을 정리한다.
- empty result에서는 세 action을 모두 disabled한다.
- clipboard/download 실패는 result 내용을 바꾸지 않고 recoverable alert로 표시한다.

첫 interaction test는 Range를 적용한 다음 output에 focus하면 selection이 사라지는 순서 문제를
발견했다. focus를 먼저 이동한 뒤 Range를 추가하도록 바꿔 실제 WebView에서도 selection 수명을
안정화했다.

### 3. Every Toolbox text surface wired to the shared helpers

Files:

- `apps/developer-toolbox/src/tools/common.tsx`
- `apps/developer-toolbox/src/tools/security.tsx`
- `apps/developer-toolbox/src/tools/regex.tsx`
- `apps/developer-toolbox/src/tools/diff.tsx`

적용 대상:

- 모든 `TransformerTool`: JSON, Base64, URL, timestamp, case, JWT input/output
- Hash input/output
- UUID output
- Regex pattern field, test text, highlighted output
- Diff old/new input과 old/new result column

입력과 출력에는 도구/side를 구분하는 accessible label을 추가했다. 기존 Copy button은 빠른
단일 동작으로 유지하고 context menu는 keyboard-only 사용자가 같은 결과 action 전체에 접근할
수 있게 한다.

### 4. Clipboard read API and least-privilege Tauri registration

Files:

- `apps/developer-toolbox/src/api.ts`
- `apps/developer-toolbox/src-tauri/Cargo.toml`
- `apps/developer-toolbox/src-tauri/src/lib.rs`
- `apps/developer-toolbox/src-tauri/capabilities/default.json`

Browser preview는 표준 `navigator.clipboard.readText()`, Tauri runtime은
`@tauri-apps/plugin-clipboard-manager`의 `readText()`를 사용한다. Rust builder에는 공식 plugin을
등록했다.

Capability는 다음 한 줄만 추가했다.

```json
"clipboard-manager:allow-read-text"
```

`default`, write, image, HTML, clear permission은 허용하지 않는다. Output Copy는 이미 사용하던
WebView clipboard write 경로를 유지하므로 plugin write command 권한이 없다.

### 5. Styles and user-facing documentation

Files:

- `apps/developer-toolbox/src/App.css`
- `apps/developer-toolbox/README.md`
- `docs/architecture.md`

Output keyboard focus에 accent outline을 추가하고 context action 오류를 작은 danger text로
표시한다. App README에는 input/output menu와 clipboard privacy를 기록했고 architecture 보안
표에는 Developer Toolbox의 read-text-only permission과 explicit Paste 수명주기를 추가했다.

### 6. Dependency locks, policy evidence, and notices

Files:

- `apps/developer-toolbox/package.json`
- `Cargo.lock`
- `pnpm-lock.yaml`
- `THIRD_PARTY_NOTICES.md`
- `docs/dependency-policy.md`

`@devbox/context-menu: workspace:*`와 공식 Tauri clipboard manager v2를 추가했다. notices는
generator로만 갱신했고 정책 문서의 locked inventory count와 byte size를 동기화했다.

#### New runtime dependency review

| Field | Evidence |
|---|---|
| Purpose | WebView2/browser permission 편차 없이 explicit input Paste를 offline 제공 |
| Alternatives | browser Clipboard API 단독은 packaged WebView 권한 실패 가능. Windows/Linux clipboard custom Rust 구현은 platform code와 보안 유지비를 중복. 대형 외부 도구 연결은 input 편집 흐름을 해결하지 못함 |
| Official source | Tauri official plugins workspace and documentation: `https://v2.tauri.app/plugin/clipboard/` |
| Pin | Cargo/npm manifest major `2`; both lockfiles resolve exact `2.3.2` with registry checksum/integrity |
| License | Plugin Rust `Apache-2.0 OR MIT`, npm `MIT OR Apache-2.0`. Windows 전이 `clipboard-win 5.4.1`·`error-code 3.4.0`은 BSL-1.0: Boost 공식 조건상 machine-executable binary에는 source notice 재현 의무가 없고 상업 사용·수정·배포가 가능한 permissive license. `deny.toml`에 명시적으로 허용하고 generated notices에 exact source/digest 포함; source 배포·수정 시 copyright/전문 유지 |
| Size | npm installed package 14,675 bytes. Locked Rust union graph adds 33 packages with 4,294,949 bytes of compressed crate archives; OS별 build는 해당 target dependency만 link. Frontend JS 204,020→215,591 bytes (+11,571; gzip +3,576), CSS 4,261→6,146 (+1,885; gzip +500), notices +5,357 bytes. 이 frontend delta에는 context-menu app code/CSS 전체가 포함됨 |
| Runtime memory | sidecar, worker, daemon, network client가 없고 plugin은 app process 안에서 explicit command 때 OS clipboard를 읽음. Windows packaged RSS delta는 W1 checkpoint에서 측정 |
| Security | `allow-read-text` only; clipboard는 explicit Paste에서만 읽고 persistence/log 없음. dependency policy checks 통과, cargo-deny/pnpm audit은 PR CI gate |
| Offline | dependency와 guest JS는 installer에 compile/bundle되며 설치 뒤 download/network 없음 |
| Maintenance | Tauri official v2 plugins workspace/Dependabot을 추적. API·license·transitive graph 변경은 lock/notices/policy gate에서 재검토; permission은 소비 기능 제거 시 함께 삭제 |

Notices inventory는 Rust 629→662, frontend runtime 151→152 package가 됐고 파일은
128,169→133,526 bytes다.

## Verification Results

### Frontend interaction and regression tests

```text
$ NODE_OPTIONS=--max-old-space-size=1024 \
    pnpm --filter developer-toolbox test -- --maxWorkers=1
Test Files  2 passed (2)
Tests      36 passed (36)
exit 0
```

새 interaction fixture 7개가 다음을 검증한다.

- input exact menu topology, selection-replacing Paste와 caret restore
- Select all, Clear와 controlled value update
- Ctrl+X/C/V/Z non-prevention, composing Shift+F10 non-prevention
- clipboard read rejection 격리와 input 불변
- output exact copy, DOM selection, filename이 지정된 text download와 URL cleanup
- empty output disabled actions
- clipboard write rejection 격리와 result 불변

기존 transformer fixture 29개도 함께 통과해 JSON/Base64/JWT/URL/time/case 동작이 유지됐다.

### Frontend production build

```text
$ NODE_OPTIONS=--max-old-space-size=1024 pnpm --filter developer-toolbox build
tsc
vite v7.3.6 building client environment for production...
47 modules transformed
dist/assets/*.js  215.59 kB | gzip: 67.73 kB
exit 0
```

### Rust formatting, tests, compile, and lint

```text
$ cargo fmt --package developer-toolbox -- --check
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo test -p developer-toolbox -j1
8 passed; 0 failed
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo check -p developer-toolbox --all-targets -j1
Finished dev profile
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo clippy -p developer-toolbox --all-targets -j1 -- -D warnings
Finished dev profile
exit 0
```

### Dependency and catalog gates

```text
$ python3 .github/scripts/check-dependencies.py check
dependency policy OK; notices match Cargo.lock and pnpm-lock.yaml

$ python3 .github/scripts/test-check-dependencies.py
dependency policy regression tests passed

$ python3 .github/scripts/test-build-manifest.py
build-manifest notice tests passed

$ bash .github/scripts/check-catalog.sh
exit 0

$ NODE_OPTIONS=--max-old-space-size=768 pnpm audit --audit-level moderate
No known vulnerabilities found

$ cargo deny --locked check
advisories ok, bans ok, licenses ok, sources ok
exit 0
```

첫 `cargo deny` 실행은 plugin의 Windows 전이 의존성 `clipboard-win`과 `error-code`가 사용하는
BSL-1.0이 기존 explicit allowlist에 없어 실패했다. Boost 공식 license 원문에서 source
배포에만 copyright/license 전문 유지가 필요하고 machine-executable object code는 예외인
permissive 조건을 확인했다. 두 exact dependency 경로와 의무를 dependency policy에 기록하고
`BSL-1.0`을 `deny.toml`에 추가한 뒤 동일 gate가 통과했다.

모든 local command는 frontend 1 GiB heap, Vitest 1 worker, Cargo 1 job과 shared Linux-native target을
사용했다. 완료 뒤 developer-toolbox 관련 build/test process가 남지 않았고 RAM available 약
9.3 GiB, swap free 약 6.9 GiB를 확인했다. 전체 workspace frontend, Windows compile,
pnpm audit와 cargo-deny는 PR GitHub Actions를 권위 있는 gate로 사용한다.

## Security and Failure Boundaries

- raw JWT/credential을 포함할 수 있는 input clipboard는 Paste 선택 뒤 한 번만 읽고 current input
  state 외부로 보내지 않는다.
- clipboard contents와 output contents를 error string, log, analytics, snapshot에 포함하지 않는다.
- plugin IPC는 text read만 허용하며 image/write/clear 접근이 없다.
- output download는 현재 result로 만든 UTF-8 Blob만 사용하고 arbitrary filesystem path 또는
  background write command를 받지 않는다.
- empty output action은 disabled이며 clipboard/download failure가 계산 결과를 교체하지 않는다.
- Ctrl+X/C/V/Z와 IME composition은 app handler가 preventDefault하지 않는다.
- tool/pipeline 기능, clipboard history, background monitoring, binary/image clipboard는 비범위다.

## Follow-up

- #251~#261: 나머지 기존 앱별 context menu
- #265~#268: Developer Toolbox의 JSON↔YAML, byte codec, radix, JSON→TypeScript 기능
- WSL clipboard read/write와 terminal selection은 #262가 별도 permission/UI 경계를 소유
- Windows W1: packaged WebView2 Paste permission, IME, Menu key/Shift+F10, local download, focus restore,
  plugin RSS/installer delta evidence
