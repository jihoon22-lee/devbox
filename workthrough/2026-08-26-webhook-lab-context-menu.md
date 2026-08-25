# Webhook Lab History and Rule Context Menus

## Overview

Issue #257의 P1-06-WH 범위로 Webhook Lab의 수신 history와 response rule 행에
`@devbox/context-menu`를 적용했다. pointer right-click, `Shift+F10`, Menu key는 같은 app-owned
action 경로를 사용하고, 메뉴를 열기 전에 정확한 행을 선택하며 닫힌 뒤 원래 행으로 focus를 복구한다.

History 메뉴:

```text
마스킹 복사
원본 복사 (별도 확인)
헤더 복사
API Playground로 변환 (P2 #315 전까지 disabled)
────────
삭제 (danger + 확인)
```

Rule 메뉴:

```text
편집
복제
예시 curl 복사 (P1 #283 전까지 disabled)
────────
삭제 (danger + 확인)
```

기존 history는 수신 시 민감 헤더를 바로 버렸기 때문에 “원본 복사”를 구현할 수 없었다. 이번 변경은
raw credential을 일반 DTO에 추가하지 않는다. 마스킹 record와 raw header를 한 bounded in-memory
entry로 묶고 raw 부분에는 Serialize/Debug를 구현하지 않았다. 일반 조회·마스킹 복사·헤더 복사는
마스킹 snapshot만 사용하며, 사용자가 별도 경고를 확인한 뒤 정확한 opaque history ID로 요청한
일회성 copy command만 raw header를 결합한다.

fixture, captured request replay, response sequence, `api-request/v1` handoff, example curl 생성은 각
후속 issue의 범위이며 이 PR에서 선구현하지 않는다.

## Context

- history/rule 행에는 pointer·keyboard context menu가 없었고 파괴 action은 inline button 또는 전체
  history 비우기에 흩어져 있었다.
- rule inline 삭제와 history 전체 비우기는 확인 없이 실행됐다.
- 기존 `History::push`는 Authorization·Cookie·API key 값을 마스킹한 record만 저장해 재현을 위한
  명시적 raw copy가 구조적으로 불가능했다.
- raw 값을 일반 `RequestRecord`에 넣으면 `list_history`, frontend state, test fixture와 향후 persistence/
  snapshot 경계에 우발적으로 노출될 수 있다.
- history는 200건, body는 256K자로 제한됐지만 raw header를 새로 장기 보관하려면 요청별 header 총량도
  반드시 제한해야 한다.
- 기존 새 rule 저장은 UUID를 map key에만 쓰고 `ResponseRule.id`는 빈 문자열로 남겨, list 이후 편집·삭제
  대상 ID가 실제 key와 일치하지 않았다. rule 복제는 이 identity 결함을 먼저 고쳐야 했다.
- 설계의 “API Playground로 변환”과 “예시 curl 복사”는 각각 #315와 #283으로 독립된 기능 단위 PR이다.
  임시 clipboard/file channel을 만들거나 incomplete curl을 중복 구현하면 계획 경계와 secret 계약이
  깨진다.

## Changes Made

### 1. Target-first pointer and keyboard menus

Files:

- `apps/webhook-lab/package.json`
- `pnpm-lock.yaml`
- `apps/webhook-lab/src/App.tsx`
- `apps/webhook-lab/src/App.css`
- `apps/webhook-lab/src/lib/contextMenus.ts`

history/rule row는 focusable하고 `data-history-id` 또는 `data-rule-id`에 opaque ID만 둔다. 공용 hook의
`onBeforeOpen`은 현재 collection에서 ID를 다시 찾은 뒤 selection과 context snapshot을 함께 갱신한다.
refresh/delete로 target이 사라지면 열린 메뉴를 닫고 stale selection을 제거한다.

공용 package는 viewport placement, 바깥 클릭·Esc·scroll close, 위/아래/Enter navigation, disabled skip,
focus restore를 담당한다. Webhook Lab은 exact items, busy/danger state와 action dispatch를 소유한다.

별도 issue인 API handoff와 example curl 항목은 명세상의 위치와 label을 유지하지만 fail-closed로
비활성화했다. #315는 `api-request/v1` producer/consumer와 no-clipboard handoff를, #283은 current bind와
safe shell quoting을 구현한 뒤 각각 활성화한다.

### 2. Bounded raw retention boundary

Files:

- `apps/webhook-lab/src-tauri/src/core/history.rs`
- `apps/webhook-lab/src-tauri/src/commands.rs`
- `apps/webhook-lab/src-tauri/src/lib.rs`
- `apps/webhook-lab/src/api.ts`

`HistoryEntry`는 다음 두 부분을 process memory에만 보관한다.

```text
masked: RequestRecord        # Serialize 가능, 일반 list/copy DTO
raw_headers: Vec<(String,String)>  # Serialize/Debug 없음, explicit raw copy 전용
```

raw body를 별도로 복제하지 않는다. 기존 body snapshot은 일반 history와 같은 256K자 경계를 사용한다.
header는 마스킹과 raw entry를 만들기 전에 요청당 100개, 이름+값 총 64K자로 제한하므로 두 snapshot이
동일한 bounded input을 공유한다. history 200건을 넘으면 가장 오래된 masked/raw entry를 함께 제거한다.
clear는 entries만 비우고 process-local monotonic ID는 유지해 이미 열린 메뉴의 ID가 이후 새 요청에
재사용되지 않게 한다.

command 경계는 네 개로 분리했다.

- `copy_masked_history(id)`: 마스킹된 전체 RequestRecord JSON
- `copy_history_headers(id)`: 마스킹된 `name: value` header lines
- `copy_raw_history(id)`: 별도 확인 뒤에만 호출하는 일회성 raw-header RequestRecord JSON
- `delete_history(id)`: exact ID만 제거하고 stale/missing ID는 고정 오류

일반 `list_history`는 언제나 `list_masked()`만 반환한다. raw copy 반환값은 clipboard write 한 번에만
사용하고 state, persistence, log, snapshot에 저장하지 않는다. clipboard 실패와 missing ID 오류도 raw
value를 반향하지 않는 고정 메시지로 처리한다.

### 3. Confirmation and clipboard behavior

Files:

- `apps/webhook-lab/src/App.tsx`
- `apps/webhook-lab/src/api.ts`

마스킹 복사는 기본 action이고 별도 확인이 없다. 원본 복사는 Authorization·Cookie·API key가 포함될 수
있다는 경고를 먼저 표시하며 cancel이면 raw command 자체를 호출하지 않는다. 헤더 복사는 raw 확인이
없는 항목이므로 backend의 masked header snapshot만 사용한다.

history 개별 삭제, rule 삭제, history 전체 비우기는 context menu와 기존 inline button 모두 확인을
거친다. cancel이면 IPC와 local collection을 변경하지 않는다. 모든 row mutation은 exact context object의
opaque ID를 전달하고 성공 뒤 backend list를 다시 조회한다.

### 4. Stable rule identity, edit, and duplicate

Files:

- `apps/webhook-lab/src-tauri/src/core/rules.rs`
- `apps/webhook-lab/src-tauri/src/commands.rs`
- `apps/webhook-lab/src/App.tsx`
- `apps/webhook-lab/src/api.ts`

`rules::upsert`는 새 rule이면 UUID를 `rule.id`에 먼저 기록한 뒤 같은 값으로 map에 저장한다. 기존 rule은
동일 ID로 교체하고 command는 실제 저장 ID를 반환한다. missing rule 삭제는 성공처럼 숨기지 않고 고정
오류를 반환한다.

편집은 exact rule을 editor draft로 옮기고 저장 button을 “규칙 저장”으로 바꾼다. 복제는 기존 rule의
필드를 복사하되 빈 ID로 backend에 보내 새 identity를 부여한다.

### 5. Documentation and dependency boundary

Files:

- `apps/webhook-lab/README.md`
- `docs/architecture.md`
- `workthrough/2026-08-26-webhook-lab-context-menu.md`

README와 architecture에 exact menu, raw retention/clipboard 경계, header/body/history 상한, ID 재사용 방지,
후속 action의 disabled 계약을 기록했다. 새 의존성은 기존 internal workspace package
`@devbox/context-menu` 하나뿐이며 native plugin, network service, Tauri capability와 registry license
surface는 추가하지 않았다.

## Test Coverage

Frontend tests cover:

- history/rule 기존 렌더링과 masked badge 회귀
- pointer로 연 exact history/rule target 우선 선택
- 두 메뉴의 exact label/order와 danger/disabled 상태
- masked full copy와 masked header copy의 exact history ID
- raw copy cancel 시 backend 미호출, confirm 시 exact ID와 clipboard write
- raw copy 실패 시 backend 원문을 UI error에 반향하지 않음
- Shift+F10/Menu key와 close 뒤 row focus restore
- history 개별 삭제와 전체 clear의 cancel/confirm
- rule exact edit, 새 ID duplicate와 danger delete
- 분리된 API handoff/example curl action의 fail-closed disabled 상태
- LAN 공개 경고 회귀

Rust tests cover:

- Authorization·Cookie·API key case-insensitive masking
- 200건 history ring과 256K body truncation
- raw/masked snapshot의 100개·총 64K header 상한
- 일반 list, masked copy, header copy에 raw credential 부재
- explicit raw copy에만 원본 header 존재
- exact history removal과 clear 뒤 stale ID 비재사용
- rule method/path exact/wildcard matching 회귀
- 새 rule ID와 map key 일치, 기존 ID upsert 보존

## Verification Results

PR 직전 최종 검증 결과로 갱신한다.

### Frontend tests

```text
$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter webhook-lab test -- --maxWorkers=1
Test Files  2 passed (2)
Tests      12 passed (12)
exit 0
```

### Rust tests

```text
$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo test -p webhook-lab -j1
8 passed; 0 failed
exit 0
```

### Frontend build

```text
$ NODE_OPTIONS=--max-old-space-size=768 pnpm --filter webhook-lab build
vite production build passed (40 modules)
exit 0
```

## Key Decisions

1. **raw credential은 일반 history record의 optional field로 두지 않는다.** 타입과 serialization 경계에서
   분리해 평상시 조회가 raw를 실수로 반환할 수 없게 한다.
2. **raw copy 확인 전에 backend를 호출하지 않는다.** frontend에 raw 문자열을 미리 가져와 숨기지 않는다.
3. **raw와 masked snapshot은 같은 bounded input을 공유한다.** raw 기능 추가가 history의 장기 메모리
   상한을 우회하지 못하게 한다.
4. **clear 뒤 ID를 재사용하지 않는다.** 열린 context menu의 stale action이 새 request에 retarget되는
   것을 막는다.
5. **rule key와 public ID는 하나의 identity다.** 복제·편집·삭제가 list에 노출된 ID로 정확히 동작한다.
6. **분리된 후속 기능은 메뉴 위치만 예약하고 disabled로 둔다.** 임시 clipboard handoff나 불완전한 curl을
   만들지 않고 #315/#283의 secret·quoting acceptance를 보존한다.

## Follow-up Work

- #282: method/path wildcard/status/delay 의미를 editor에 항상 표시한다.
- #283: current bind, method/path, headers/body와 safe shell quoting을 반영한 example curl을 구현하고 rule
  menu 항목을 활성화한다.
- #314: captured request를 마스킹된 fixture와 response rule 초안으로 저장한다.
- #315: `api-request/v1` handoff로 API Playground 변환을 구현하고 history menu 항목을 활성화한다.
- #362/#363: replay와 response sequence/reset은 P3 독립 PR로 유지한다.
- W1 checkpoint에서 packaged WebView2의 pointer/Shift+F10/Menu key, focus restore, clipboard, raw-copy
  confirmation과 history/rule delete evidence를 수집한다. evidence에는 raw credential을 기록하지 않는다.
