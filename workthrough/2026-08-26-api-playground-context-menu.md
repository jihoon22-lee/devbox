# API Playground History and Collection Context Menu

## Overview

Issue #259의 P1-06-API 범위로 API Playground의 History·Collection v2 항목에
`@devbox/context-menu`를 적용했다. pointer right-click, `Shift+F10`, Menu key는 같은
app-owned action 경로를 사용하고 메뉴를 열기 전에 정확한 항목을 선택한다. 닫힌 뒤에는 원래
History button 또는 Collection row로 focus가 돌아간다.

두 대상의 정확한 menu topology는 다음과 같다.

```text
복제
이름 변경
삭제 (danger, confirmation)
curl 복사 (항상 마스킹)
```

모든 action은 현재 request editor나 해제된 environment secret이 아니라 v0.4.2에서 확립한
`PersistedHistoryRequest`만 입력으로 사용한다. Collection import/export, 새 handoff protocol,
OpenAPI·GraphQL·SSE·WebSocket은 이번 기능 경계에 포함하지 않는다.

## Context

- History·Collection row에는 click-to-load와 Collection inline delete만 있었고 pointer/keyboard
  context menu가 없었다.
- 기존 Collection inline delete는 확인 없이 즉시 v2 storage를 바꿨다.
- 설계는 History에도 이름 변경을 요구하지만 기존 History v2에는 표시 이름 필드가 없었다.
- raw request와 unsealed secret은 Rust 전송 경계 안에서만 존재해야 하며 context action이 이를
  다시 frontend/storage/clipboard로 끌어오면 v0.4.2 보안 핫픽스를 회귀시킨다.
- 저장소는 backend sanitizer와 read-back 확인이 성공한 뒤에만 React state에 반영되어야 한다.
- 복제 구현이 안전한 persisted request를 `RequestTemplate`로 되돌렸다가 다시 sanitize하면
  `requiresSecretReview`와 마스킹 provenance를 잃을 수 있다.

## Changes Made

### 1. Target-first History and Collection menus

Files:

- `apps/api-playground/package.json`
- `pnpm-lock.yaml`
- `apps/api-playground/src/App.tsx`
- `apps/api-playground/src/App.css`
- `apps/api-playground/src/lib/contextMenu.ts`

History button과 Collection row는 stable v2 ID를 `data-*` attribute에 두고 keyboard focus가
가능하다. 공용 hook의 `onBeforeOpen`은 현재 store에서 exact ID를 다시 찾은 뒤 selection과 context
snapshot을 동기화한다. 저장 결과에서 항목이 사라지면 열린 메뉴와 stale selection을 닫는다.

공용 package는 viewport placement, keyboard navigation, focus trap/restore, outside click·Esc·scroll
close와 danger/disabled 표현만 담당한다. API Playground는 exact menu item, persistence readiness,
busy 상태와 action dispatch를 소유한다.

### 2. Backward-compatible History display name

Files:

- `apps/api-playground/src/types.ts`
- `apps/api-playground/src/lib/persistence.ts`
- `apps/api-playground/src-tauri/src/commands/request.rs`

`HistoryItem.name?: string`을 v2에 선택적으로 추가했다. 기존 이름 없는 v2 item은 URL을 계속 표시해
migration이나 version bump 없이 읽힌다. parser는 name이 없거나 string인 경우만 허용하며 다른 wire
type은 해당 item을 fail-closed로 제외한다.

이름은 줄바꿈을 공백으로 정규화하고 trim한 뒤 120자로 제한한다. 전체 History store는 이름을 포함해
backend JSON sanitizer를 다시 통과한다. environment secret이 표시 이름에 들어가도 Rust sanitizer가
`[REDACTED]`로 바꾸고 원문이 serialized output에 남지 않는 테스트를 추가했다.

### 3. Mask-preserving duplicate and rename mutations

Files:

- `apps/api-playground/src/lib/contextMenu.ts`
- `apps/api-playground/src/lib/collections.ts`

History·Collection 복제는 저장된 `PersistedHistoryRequest`의 headers, params, auth를 깊은 복사한다.
`toRequestTemplate`이나 현재 editor state를 거치지 않으므로 raw credential을 읽지 않고 기존
`requiresSecretReview`도 보존한다. 새 stable ID와 timestamp를 부여하고 이름에 `복사본`을 붙인 뒤
전체 v2 store를 다시 sanitize/read-back 한다.

이름 변경과 삭제도 exact stable ID만 바꾼다. 존재하지 않는 ID는 store를 변경하지 않는다. History는
50개 상한을 그대로 유지한다.

### 4. Confirmed deletion

Files:

- `apps/api-playground/src/App.tsx`

History·Collection menu의 삭제는 danger 스타일이고 되돌릴 수 없다는 확인을 거친다. 취소 시 sanitizer,
storage write와 React state 변경이 발생하지 않는다. 기존 Collection inline `✕` 버튼도 같은 확인된
삭제 경로를 사용하도록 통합해 우회 경로를 제거했다.

실패 시 backend/storage 상세나 item 원문을 반향하지 않고 대상별 고정 메시지만 표시한다.

### 5. Persisted-only masked cURL copy

Files:

- `apps/api-playground/src/App.tsx`
- 기존 `apps/api-playground/src/lib/persistence.ts`

context menu의 cURL 복사는 context item의 persisted request를 `buildCurl`에 전달한다. 이 builder는
Authorization, Cookie, API key, auth preset, URL/body의 sensitive field와 known token pattern을 다시
마스킹하고 environment reference만 보존한다. context action은 `buildRevealedCurl`을 호출하지 않으며
raw-copy 확인 경로도 제공하지 않는다.

clipboard 실패에는 고정 오류만 표시하고 rejected value나 exception detail을 DOM에 남기지 않는다.
기존 toolbar의 명시적 확인 후 일회성 원문 cURL 기능은 별도 경계로 유지한다.

### 6. Documentation and dependency boundary

Files:

- `apps/api-playground/README.md`
- `docs/architecture.md`
- `THIRD_PARTY_NOTICES.md`
- `workthrough/2026-08-26-api-playground-context-menu.md`

새 의존성은 기존 private workspace package `@devbox/context-menu` 하나뿐이다. native plugin, Tauri
capability, sidecar, network dependency와 외부 service를 추가하지 않았다. lockfile 변경 뒤 repository
dependency notice generator로 provenance hash를 갱신했다.

## Test Coverage

Frontend unit/integration tests cover:

- 설계의 exact four-item menu와 delete danger 표시
- persistence 미준비/진행 중 action disable
- pointer right-click target-first selection
- `Shift+F10`, Menu key와 close 뒤 focus restore
- History·Collection exact ID 복제·이름 변경·삭제
- History optional name의 기존 v2 하위 호환과 invalid type 거부
- request nested value의 깊은 복사와 `requiresSecretReview` 보존
- raw URL/header/body credential의 storage·clipboard·DOM 비노출
- cURL의 Authorization/body masking
- delete confirmation 취소 시 storage 불변
- sanitizer/clipboard failure의 fixed safe message
- 기존 persistence migration, environment, cURL, API helper 회귀

Rust tests cover:

- History display name의 정상 wire shape 보존
- 표시 이름 속 environment secret redaction
- `requiresSecretReview` boolean metadata 보존과 다른 타입 fail-closed redaction
- History·Collection persisted wire shape sanitizer 회귀
- request/response/redirect redaction과 cross-origin credential stripping

## Verification Results

PR 직전 단일 worker와 공용 Linux-native Cargo target cache로 확인했다.

### Frontend tests

```text
$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter api-playground exec vitest run --maxWorkers=1
Test Files  6 passed (6)
Tests      67 passed (67)
exit 0
```

### Rust tests

```text
$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo test -p api-playground -j1
17 passed; 0 failed
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo check -p api-playground -j1
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo clippy -p api-playground -j1 -- -D warnings
exit 0

$ cargo fmt --all --check
exit 0
```

### Frontend build

```text
$ NODE_OPTIONS=--max-old-space-size=768 pnpm --filter api-playground build
vite production build passed (43 modules)
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

1. **context action의 유일한 request 입력은 persisted v2다.** 현재 editor와 environment secret을
   읽지 않는다.
2. **복제는 재구성하지 않고 깊은 복사한다.** 마스킹 값과 review provenance를 그대로 보존한 뒤
   backend sanitizer를 다시 통과한다.
3. **History 이름은 optional v2 extension이다.** 기존 store migration/version bump를 만들지 않고
   invalid wire type만 제외한다.
4. **cURL 복사는 항상 masking이다.** 원문 cURL은 기존의 별도 confirm+backend one-shot 경로에만 남긴다.
5. **inline delete도 같은 확인 경로를 사용한다.** context menu만 안전하고 기존 button은 즉시 삭제하는
   우회 상태를 허용하지 않는다.
6. **collection I/O와 protocol을 끌어오지 않는다.** 메뉴 action은 현재 local v2 CRUD 경계만 사용한다.

## Follow-up Work

- P2-04~P2-07: OpenAPI import, GraphQL, SSE, WebSocket을 각각 독립 기능 PR로 구현한다.
- P2 handoff receive: `api-request/v1`의 target/TTL/size/one-time 검증 뒤에만 외부 앱 request를 받는다.
- P3-07: Collection import/export, History search/filter, binary response preview/save를 독립 PR로 구현한다.
- W1 checkpoint에서 packaged WebView2 pointer/Shift+F10/Menu key, focus restore, clipboard, rename/delete와
  v2 storage masking evidence를 수집한다. secret 원문은 evidence에 남기지 않는다.
