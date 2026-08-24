# 공용 Context Menu Primitive

## Overview

Issue #248의 P1-06-C 범위로 기존 13개 앱과 신규 앱이 공통으로 사용할
`@devbox/context-menu` React package를 추가했다. package는 root/submenu 위치 계산,
pointer·keyboard open, keyboard navigation, focus 수명주기, separator와 disabled/danger 표현만
소유한다. 메뉴 항목, catalog 상태, clipboard 내용, destructive confirmation과 실제 action은
각 소비 앱이 소유한다.

이 PR은 공용 primitive만 구현한다. Port Manager부터 Life Log까지 기존 13개 앱 적용은
#249~#261의 기능 단위 PR로 분리하며, 앱 고유 메뉴 항목이나 “다른 앱으로 열기” catalog 생성
로직을 미리 넣지 않는다.

## Context

- 기존 13개 앱에는 공용 HTML context menu가 없고 browser/WebView 기본 동작 또는 무반응만
  존재한다.
- 설계 §2.5/P1-06과 UX 설계 §1.0~1.1은 13개 소비자가 이미 확정됐으므로 첫 앱 내부에 임시
  구현하지 않고 공용 package를 먼저 만들도록 정한다.
- native menu plugin은 앱마다 Tauri 권한과 dependency가 늘고 CodeMirror/terminal처럼 앱별
  DOM selection과 결합해야 하는 영역을 다루기 어렵다. 기존 React/CSS만 사용하는 HTML portal
  primitive가 현재 범위에 맞다.
- text input과 CodeMirror 소비자는 IME와 cut/copy/paste/undo를 잃지 않아야 한다. primitive는
  정확한 context-menu open key만 소비하고 다른 keyboard event를 통과시켜야 한다.
- 파괴적 action은 danger 표현과 앱별 확인이 모두 필요하지만, 확인 대상과 결과는 app domain
  state이므로 공용 component가 추측해서는 안 된다.

## Public Contract

### Item definitions

File: `packages/context-menu/src/types.ts`

```ts
type ContextMenuEntry =
  | {
      type: "item";
      id: string;
      label: string;
      shortcut?: string;
      disabled?: boolean;
      danger?: boolean;
    }
  | {
      type: "submenu";
      id: string;
      label: string;
      disabled?: boolean;
      danger?: boolean;
      items: readonly ContextMenuEntry[];
    }
  | { type: "separator"; id?: string };
```

동일 menu level의 interactive ID는 소비 앱이 고유하게 만든다. primitive는 selected ID만
`onSelect`로 돌려주며 label, raw path, secret, clipboard payload를 저장하거나 log하지 않는다.

### Controlled component and trigger hook

Files:

- `packages/context-menu/src/ContextMenu.tsx`
- `packages/context-menu/src/useContextMenu.ts`
- `packages/context-menu/src/index.ts`

`ContextMenu`는 `open`, nullable viewport `anchor`, app-owned `items`, `onSelect`, `onClose`와
optional focus target을 받는 controlled component다. React portal로 `document.body`에 렌더해
panel overflow clipping을 피한다.

`useContextMenu`는 다음 open trigger만 같은 controller state로 모은다.

- pointer `contextmenu`의 `clientX/clientY`
- Shift+F10
- Menu/ContextMenu key
- 앱이 직접 위치를 제공하는 `openAt`

`onBeforeOpen(reason, target)`은 우클릭한 row 선택 같은 app-owned 상태를 실제 menu open 전에
동기화하기 위한 hook이다. selection logic 자체는 package에 넣지 않는다.

## Changes Made

### 1. Viewport-safe pure positioning

File: `packages/context-menu/src/position.ts`

- root menu는 anchor의 right/down 배치를 우선한다.
- menu가 viewport right/bottom safe margin을 넘으면 anchor 기준 left/up으로 뒤집는다.
- 뒤집은 결과도 8px margin 안으로 clamp한다.
- submenu는 parent item 오른쪽을 우선하고 공간이 없으면 왼쪽으로 뒤집는다.
- 양쪽 모두 완전히 맞지 않으면 더 넓은 쪽을 선택한 뒤 clamp한다.
- tall submenu는 parent top을 우선하고 bottom overflow만큼 위로 clamp한다.
- NaN, 음수 size와 viewport보다 큰 menu도 유한한 safe position을 반환한다.

계산 함수는 DOM과 분리돼 각 앱 없이 deterministic fixture로 검증된다. component는 실제
`getBoundingClientRect`와 `window.innerWidth/innerHeight`를 넣고 resize 때 다시 계산한다.

### 2. Keyboard navigation and focus lifecycle

File: `packages/context-menu/src/ContextMenu.tsx`

- ArrowDown/Up은 separator와 disabled item을 건너뛰고 순환한다.
- Home/End는 첫/마지막 enabled item으로 이동한다.
- Enter/Space는 action을 실행하거나 submenu를 연다.
- ArrowRight는 focused submenu를 열고 첫 enabled child로 이동한다.
- ArrowLeft는 submenu를 닫고 parent item으로 돌아간다.
- Escape는 전체 menu를 닫는다.
- Tab/Shift+Tab은 현재 menu level의 enabled item 사이를 순환해 focus가 WebView 뒤로 빠지지 않게
  한다.
- pointer hover도 같은 item에 focus를 옮겨 이후 Enter가 hover item과 다른 action을 실행하지
  않게 한다. disabled hover는 focus/tabIndex를 바꾸지 않는다.
- submenu는 단순 keyboard focus만으로 자동 열리지 않는다. pointer hover, click, Enter 또는
  ArrowRight에서만 열린다.
- close 시 focus가 아직 menu 안/body에 있을 때만 원래 trigger로 복원한다. 앱이 dialog 같은
  별도 target으로 focus를 옮겼다면 그 focus를 덮어쓰지 않는다.
- item 배열이 열린 동안 재생성돼도 root focus를 매 render마다 첫 항목으로 되돌리지 않는다.
  active item이 제거/disabled된 경우에만 다음 유효 item을 선택한다.

### 3. Close and event boundaries

File: `packages/context-menu/src/ContextMenu.tsx`

- document capture `pointerdown`이 root portal 밖에서 발생하면 닫는다.
- Escape에서 닫는다.
- document/window scroll에서 닫되, max-height로 bounded된 menu 자체 scroll은 유지한다.
- window resize는 menu를 닫지 않고 새 viewport에 맞춰 다시 배치한다.
- menu 내부 `contextmenu`는 browser 기본 menu가 겹치지 않게 막는다.

### 4. IME and editor-safe trigger behavior

File: `packages/context-menu/src/useContextMenu.ts`

- `nativeEvent.isComposing` 또는 keyCode 229인 composition event에서는 Shift+F10도 소비하지
  않는다.
- Ctrl+C/X/V/Z와 다른 keyboard event는 `preventDefault`하지 않는다.
- hook은 input value, selection range, CodeMirror state를 읽거나 수정하지 않는다.
- CodeMirror DOM handler 연결과 editor action은 Knowledge/Code Pad 적용 PR이 소유한다.

### 5. Accessible and theme-compatible rendering

Files:

- `packages/context-menu/src/ContextMenu.tsx`
- `packages/context-menu/src/styles.css`

- `role=menu/menuitem/separator`, `aria-haspopup`, `aria-expanded`, `aria-disabled`를 제공한다.
- shortcut 텍스트가 accessible name에 합쳐지지 않도록 menuitem `aria-label`을 실제 label로
  고정했다.
- active/focus, disabled, danger, separator, shortcut, submenu arrow를 고유 class로 표현한다.
- 공용 design token을 우선 사용하되 CSS fallback을 둬 package 단독 fixture도 렌더된다.
- content는 ellipsis, viewport width/height는 bounded되어 과도한 label/item 수가 화면 밖으로
  확장되지 않는다.

### 6. Workspace integration and documentation

Files:

- `packages/context-menu/package.json`
- `packages/context-menu/tsconfig.json`
- `packages/context-menu/vitest.config.ts`
- `packages/context-menu/README.md`
- `pnpm-lock.yaml`
- `THIRD_PARTY_NOTICES.md`
- `AGENTS.md`
- `CONVENTIONS.md`
- `docs/architecture.md`

pnpm workspace의 `packages/*` glob이 새 package를 자동 포함한다. package는 기존 workspace의
React/ReactDOM/TypeScript만 재사용하고 새 third-party library를 추가하지 않는다. lockfile에는
새 importer 1개만 생겼으며 dependency notice generator로 pnpm lock digest를 동기화했다.

저장소 사실과 architecture에는 네 번째 공용 package가 구현됐고 13개 앱 적용은 후속 기능
PR이라는 상태를 기록했다. convention에는 package와 앱의 책임 경계를 추가했다.

## Security and Failure Boundaries

- primitive는 item ID 외의 app payload를 받거나 보존하지 않는다.
- raw credential, path, clipboard text, catalog/install state를 읽지 않는다.
- DOM에 app label은 React text node로 렌더되며 HTML injection API를 사용하지 않는다.
- disabled item은 mouse click, keyboard selection과 focus 순환에서 실행되지 않는다.
- `onSelect`가 동기적으로 throw해도 `finally`에서 menu close를 요청한다.
- danger는 표현만 한다. confirmation 없는 destructive action을 primitive가 실행하지 않으며,
  후속 앱 PR acceptance가 app-owned confirmation을 필수로 검증한다.
- SSR/document 부재에서는 render하지 않고, event listener는 open 수명에만 등록·정리한다.

## Verification Results

### Package strict build

```text
$ NODE_OPTIONS=--max-old-space-size=1024 \
    pnpm --filter @devbox/context-menu build
> tsc --noEmit
exit 0
```

### Unit and interaction fixtures

```text
$ NODE_OPTIONS=--max-old-space-size=1024 \
    pnpm --filter @devbox/context-menu exec vitest run \
      --passWithNoTests --maxWorkers=1
Test Files  2 passed (2)
Tests       16 passed (16)
```

fixture 범위:

- root right/down과 left/up flip, oversized/NaN clamp
- submenu right/left flip, vertical clamp, 양쪽 부족 시 공간 비교
- pointer coordinate open과 app `onBeforeOpen` 순서
- Shift+F10, Menu key와 close focus restore
- Ctrl+C selection/IME composition event 비간섭
- Arrow/Home/End/Enter/Space/Escape/Tab 순환 기반
- disabled/separator skip과 disabled pointer non-activation
- danger/shortcut/ARIA 표현
- pointer hover와 keyboard active item 일치
- submenu ArrowRight/Left, disabled child skip, Enter selection
- outside pointer, underlying scroll close와 internal menu scroll 유지

첫 interaction run은 shortcut이 menuitem accessible name에 합쳐져 `CopyCtrl+C`로 노출되는 문제를
발견했다. `aria-label`을 app label로 고정한 뒤 같은 fixture가 `Copy`를 안정적으로 찾고 전체
keyboard test가 통과했다.

### Dependency policy

```text
$ pnpm install --lockfile-only --offline
downloaded 0

$ python3 .github/scripts/check-dependencies.py check
dependency policy OK; notices match Cargo.lock and pnpm-lock.yaml

$ python3 .github/scripts/test-check-dependencies.py
dependency policy regression tests passed

$ python3 .github/scripts/test-build-manifest.py
build-manifest notice tests passed
```

worktree에 연결한 root/package `node_modules` symlink는 각 command의 trap에서 제거했다. Vitest은
1 worker, Node 1 GiB heap 상한으로만 실행했고 종료 후 관련 process가 남지 않았음을 확인했다.
전체 frontend workspace build/typecheck/test는 package 변경으로 PR의 GitHub Actions가 권위 있는
gate가 된다.

## Explicit Non-scope and Follow-up

- #249~#261: 기존 13개 앱별 item/action/selection/confirmation 적용
- catalog 기반 “다른 앱으로 열기” submenu 생성
- clipboard read/write와 raw/masked copy policy
- CodeMirror `EditorView.domEventHandlers`와 terminal selection 연결
- destructive action confirmation, retry와 result UI
- 신규 Devbox Launcher/Log Lens item definitions
- app version bump와 Windows packaged W1 evidence

Windows W1 checkpoint에서는 실제 WebView2의 right-click, Shift+F10/Menu key, focus restore,
viewport/submenu flip, IME와 각 app destructive confirmation 화면·로그 evidence를 남긴다.
