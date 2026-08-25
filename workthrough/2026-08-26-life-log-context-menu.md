# Life Log Date Context Menu

## Overview

Issue #261의 P1-06-LL 범위로 Life Log의 선택 날짜와 주·월 daily chart 날짜에
`@devbox/context-menu`를 적용했다. pointer 우클릭, `Shift+F10`, Menu 키는 같은
app-owned action 경로를 사용한다. chart date에서 메뉴를 열면 exact `YYYY-MM-DD`를
먼저 Life Log의 선택 날짜로 동기화하고, 닫힌 뒤에는 원래 date input 또는
chart button으로 focus를 복원한다.

exact menu topology는 다음과 같다.

```text
날짜 복사
Markdown 내보내기 (#305 전까지 disabled)
JSON 내보내기 (#305 전까지 disabled)
```

#261은 date menu와 selection/focus 동기화만 소유한다. date-range Markdown/JSON/CSV
export와 deterministic source metadata/native save는 #305(P2-10), local daily/weekly digest는 #306,
Knowledge draft handoff는 #307/#353의 단일 기능 PR 경계에 남겼다.

## Context

- Life Log의 날짜 선택 UI는 toolbar의 native date input과 week/month 요약의 daily
  chart로 나뉩어 있었다.
- daily chart column은 단순 `<div>`라 pointer 표시 외의 날짜 선택, keyboard focus,
  accessible name을 제공하지 않았다.
- UX 설계는 날짜 복사와 Markdown/JSON export를 같은 메뉴에 요구하지만 issue
  #261은 export를 명시적 비범위로 두고 #305가 date-range format, source metadata,
  privacy, native save를 하나의 계약으로 소유한다.
- 현재 React의 `day`/`range` 상태를 임시 JSON으로 직렬화하면 session/Git/source
  포함 기준과 masking이 없어 #305의 privacy 경계를 우회하게 된다.
- clipboard 실패 원문은 환경/path/permission 정보를 포함할 수 있으므로 UI에
  그대로 반향하지 않아야 한다.

## Changes Made

### 1. App-owned date menu contract

Files:

- `apps/life-log/src/lib/contextMenu.ts`
- `apps/life-log/src/lib/contextMenu.test.ts`

공용 package는 viewport placement, keyboard navigation, focus trap/restore, outside close,
disabled 표현만 소유한다. Life Log는 exact item ID/label, clipboard action, export
availability를 소유한다.

`buildDateContextMenu` 계약 테스트는 세 항목의 순서와 #305 export 항목의 상시
disabled 상태를 고정한다. 날짜 복사는 clipboard 작업 중에만 disabled다.

### 2. Strict local date parsing

Files:

- `apps/life-log/src/lib/contextMenu.ts`
- `apps/life-log/src/App.tsx`
- `apps/life-log/src/lib/contextMenu.test.ts`

`parseDateKey` 는 exact four-digit `YYYY-MM-DD`를 분해해 local midnight `Date`로 변환한다.
`setFullYear` 후 year/month/day를 다시 비교해 leap day를 포함한 실재 존재 날짜만
받고, overflow·잘못된 padding·arbitrary 문자열은 거부한다.

toolbar date input의 change 경로도 같은 parser를 쓰므로 context menu와 일반
날짜 선택이 같은 local calendar 계약을 공유한다. UTC 문자열 파싱으로 타임존에서
하루가 바뀌는 경로를 만들지 않았다.

### 3. Target-first date selection

Files:

- `apps/life-log/src/App.tsx`
- `apps/life-log/src/App.contextMenu.test.tsx`

toolbar input과 daily chart button은 `data-date` 하나를 context target의 단일 진실 소스로
사용한다. `onBeforeOpen`은 이 값을 엄격히 파싱한 뒤 `contextDate`와 Life Log
선택 `date`를 먼저 동기화한다. 메뉴 action은 이전 toolbar selection이 아니라
우클릭/키보드로 연 exact target date를 사용한다.
잘못된 `data-date`는 context snapshot을 비우고 복사 action을 disabled로 만든다. 복사
실행 직전에도 날짜를 다시 검증해 stale/invalid target을 이전 선택으로 retarget하지
않는다.

native date input은 선택 날짜를 포함한 한국어 `aria-label`을 가진다. chart column은
native `<button type="button">`으로 바꾸어 tab focus·Enter/Space click을 무료로 얻고,
exact date accessible name과 `aria-current="date"`로 선택 상태를 표시한다. 기존 bar
height/label 시각 구성은 그대로 유지한다.

### 4. Clipboard boundary and safe feedback

Files:

- `apps/life-log/src/App.tsx`
- `apps/life-log/src/App.contextMenu.test.tsx`

날짜 복사는 검증된 `YYYY-MM-DD` 10자만 `navigator.clipboard.writeText`에 전달한다.
activity/session/title/path/source payload를 읽지 않고 storage·backend·network를 변경하지 않는다.

성공 시 복사한 날짜를 한국어 notice로 표시한다. Clipboard API 미지원·권한
거부·write 실패는 backend/browser 원문을 반향하지 않고 고정된 `날짜를
클립보드에 복사하지 못했습니다.`만 표시한다.

### 5. Shared focus restoration

Files:

- `apps/life-log/src/App.tsx`
- `apps/life-log/src/App.contextMenu.test.tsx`

date input과 chart button은 모두 실제 DOM focus target이므로 공용 package의
`restoreFocusTo`를 그대로 사용한다. pointer/키보드로 메뉴를 열고 action을 실행한
뒤 원래 exact 날짜 요소로 focus가 돌아오는 지 통합 테스트한다.

### 6. Explicit export non-scope

Markdown/JSON 항목은 exact menu에 존재하지만 `onDateContextSelect`는 날짜 복사 ID만
dispatch한다. disabled 항목에는 파일 대화상자, frontend JSON stringify, backend command,
clipboard fallback, 임시 파일 동작이 없다.

#305에서 date-range Markdown/JSON/CSV schema, source/rule metadata, masking, atomic native save,
cancel/failure cleanup을 함께 구현한 뒤에만 해당 항목을 활성화한다.

### 7. Documentation and dependency boundary

Files:

- `apps/life-log/README.md`
- `docs/architecture.md`
- `docs/superpowers/specs/2026-08-15-ux-improvements-design.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`
- `THIRD_PARTY_NOTICES.md`
- `workthrough/2026-08-26-life-log-context-menu.md`

새 의존성은 기존 private workspace package `@devbox/context-menu` 하나뿐이다. native plugin,
Tauri capability, sidecar, network dependency, 외부 export tool을 추가하지 않았다.

## Test Coverage

Frontend unit/integration tests cover:

- exact three-item topology과 #305 export disabled 계약
- copy busy state와 export 상시 disabled 불변식
- valid leap day·invalid overflow/padding/arbitrary value 날짜 파싱
- toolbar selected date의 pointer right-click과 exact clipboard payload
- chart date의 `Shift+F10` target-first selection과 `aria-current="date"`
- invalid/stale `data-date`가 이전 valid context로 retarget되지 않고 copy를 disabled로 유지
- date input/ chart button으로 menu close 후 focus restore
- Menu key clipboard action
- clipboard reject 시 fixed safe message과 raw path/credential-like 텍스트 DOM 비노출
- 기존 week/month/date range와 Knowledge Data Source rendering 회귀

## Verification Results

PR 직전 단일 worker와 Linux-native Cargo target cache로 확인했다.

### Frontend tests and type check

```text
$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter life-log exec vitest run --maxWorkers=1
Test Files  3 passed (3)
Tests       18 passed (18)
exit 0

# 최종 검토에서 invalid/stale target 회귀를 추가한 뒤
$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter life-log exec vitest run \
      src/App.contextMenu.test.tsx src/lib/contextMenu.test.ts --maxWorkers=1
Test Files  2 passed (2)
Tests       7 passed (7)
exit 0

$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter life-log exec tsc --noEmit
exit 0
```

최종 현재 suite는 3개 파일·19개 테스트이며, 추가된 1개와 해당 메뉴 파일의
기존 6개를 최종 targeted run에서 다시 통과시켰다. TypeScript `--noEmit`도 그 뒤
다시 통과했다.

### Frontend build

```text
$ NODE_OPTIONS=--max-old-space-size=768 pnpm --filter life-log build
vite production build passed (40 modules)
exit 0
```

### Rust tests and compile gates

```text
$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo test -p life-log -j1
47 passed; 0 failed
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo check -p life-log -j1
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo clippy -p life-log -j1 -- -D warnings
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

1. **date context는 `YYYY-MM-DD`만 실어 나른다.** activity 집계·세션·path·source 원문을
   clipboard나 menu state로 복제하지 않는다.
2. **chart 날짜를 exact target으로 삼는다.** 메뉴를 열기 전 Life Log selection을
   먼저 동기화해 이전 toolbar 날짜에 action이 잘못 적용되지 않는다.
3. **daily column을 native button으로 사용한다.** 수제 keyboard emulation 대신 표준
   tab/Enter/Space semantics와 visible focus를 얻는다.
4. **export를 가짜로 구현하지 않는다.** 항목은 계획 topology에 남기되 #305의
   format/source/privacy/save 계약이 완성될 때까지 disabled다.
5. **clipboard failure는 fixed safe message다.** browser/OS raw error를 로그·DOM·notice에
   반향하지 않는다.
6. **date menu는 read-only다.** 날짜 선택은 조회 범위만 바꾸고 DB, snapshot,
   settings, file, network external state를 변경하지 않는다.

## Follow-up Work

- #262~#263: WSL Desktop terminal clipboard 기본기와 native profile/workspace를 각 기능 PR로
  구현한다.
- #305(P2-10): date-range Markdown/JSON/CSV export, source metadata, masking, native save를
  구현하고 메뉴 export 항목을 활성화한다.
- #306: session/Git/Run Manager/Knowledge source의 deterministic daily/weekly digest를 구현한다.
- #307/#353: versioned Knowledge draft handoff와 preview/save 상태를 별도로 구현한다.
- W1 checkpoint에서 packaged WebView2 native date input, daily chart pointer/keyboard trigger,
  focus restore, clipboard permission 성공/거부, viewport placement를 확인한다.
