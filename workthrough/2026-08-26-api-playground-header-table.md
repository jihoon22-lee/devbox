# API Playground Request Header Table

## Overview

Issue #268의 P1-09 다섯 번째 범위로 API Playground의 요청 header 편집을 전용 table로 교체했다.
같은 이름의 header 여러 행, 행별 enabled 상태와 현재 environment의 봉인된 secret reference를
History·Collection·전송·cURL 경계 전체에서 일관되게 처리한다.

```text
Header table rows (ordered, max 100)
          │
          ├─ enabled=true ─► backend-only reference resolve
          │                         │
          │                         ├─ reqwest duplicate append in row order
          │                         └─ masked / confirmed one-shot cURL
          │
          └─ enabled=false ─► persist row + mask literal
                              skip unseal / redaction seed / send / cURL
```

새 dependency, sidecar, network service와 localStorage key/schema version은 추가하지 않았다. 기존
History/Collection v2에서 `enabled`가 없는 header는 true로 올리는 호환 정규화를 사용한다.
Windows packaged UI smoke는 W1 P1 묶음 checkpoint에 남겼다.

## Context and Constraints

- acceptance는 duplicate header와 enabled 상태 보존, secret reference 허용이다.
- HTTP header name은 case-insensitive지만 같은 이름의 여러 value는 의미가 있으므로 object/map으로
  축약하지 않고 원래 array 순서를 유지해야 한다.
- disabled는 삭제가 아니다. 다시 켤 수 있도록 History·Collection에는 남지만 어떤 전송·cURL에도
  포함되지 않아야 한다.
- secret picker는 현재 선택된 environment의 봉인된 변수 이름만 볼 수 있다. DPAPI plaintext나
  sealed blob은 component prop과 frontend resolve 경로에 들어오지 않는다.
- P1-02의 persistence sanitizer, backend-only resolve, redirect stripping과 one-shot revealed cURL
  경계를 약화하면 안 된다.
- Cookie editor, multipart와 response header/cookie tab은 각각 별도 기능 PR이다.
- API Playground 0.4.0 version bump는 Wave 9 release preparation에서 별도로 수행한다.

## Data Contract

### Frontend

```ts
interface RequestHeader extends KeyValue {
  enabled?: boolean;
}
```

optional은 이전 v2 JSON과 TypeScript fixture를 읽기 위한 wire compatibility다. public helper는
`undefined !== false` 규칙으로 기존 행을 활성으로 해석하고, 편집·저장 경계에서는 항상 boolean을
명시한다.

| Operation | Contract |
|---|---|
| add | empty key/value, enabled true |
| update | exact index만 변경, 전체 행을 boolean enabled로 정규화 |
| duplicate | source 바로 다음에 key/value/enabled clone |
| delete | exact index 제거 |
| duplicate count | trim + lowercase name, 빈 name 제외; 표시 전용 |
| secret picker | valid `[A-Za-z0-9_.-]+` 이름만 code-point 정렬·dedupe |
| maximum | first 100 rows; add/duplicate disabled at limit |

### Rust wire

`RequestTemplate.headers`와 backend-only `ResolvedRequest.headers`는 `Vec<RequestHeader>`다.
`#[serde(default = "default_header_enabled")]`로 field가 없는 이전 request를 enabled=true로
deserialize한다. resolve와 reference collection은 모두 처음 100행만 처리한다.

response headers와 query params는 enabled가 없는 기존 `KeyValue`를 계속 사용한다. 따라서 request
header 기능이 response model이나 cookie/multipart 범위로 번지지 않는다.

## Changes Made

### 1. Bounded immutable header operations

Files:

- `apps/api-playground/src/types.ts`
- `apps/api-playground/src/lib/headers.ts`
- `apps/api-playground/src/lib/headers.test.ts`

추가·수정·복제·삭제는 입력 array/object를 변경하지 않고 새 배열을 반환한다. case-insensitive
duplicate count는 UI summary에만 쓰며 header key casing과 row order를 정규화하지 않는다.

secret reference helper는 변수 이름만 받아 `${NAME}`을 만든다. invalid/empty 이름은 거부하며
picker용 목록은 host locale에 의존하지 않는 code-point 비교로 고정한다. 이 API는 secret value를
인자로 받을 필요가 없다.

### 2. Dedicated header table UI

Files:

- `apps/api-playground/src/HeaderTable.tsx`
- `apps/api-playground/src/HeaderTable.test.tsx`
- `apps/api-playground/src/App.tsx`
- `apps/api-playground/src/App.css`

Params는 기존 `KeyValueEditor`를 계속 사용하고 Headers tab만 전용 component로 바꿨다. 각 행은
enabled checkbox, name/value input, secret reference select, duplicate/delete action을 제공한다.
summary는 active/total과 duplicate-name group count를 표시한다.

picker에는 `currentEnv.variables.filter(secret)`의 이름만 전달한다. 선택하면 행 value 전체를
`${NAME}`으로 바꾸고, `Bearer ` 같은 접두사는 사용자가 value input에서 명시한다. 이 교체 동작과
plaintext를 표시·저장하지 않는다는 점을 table 아래 notice에 항상 표시한다.

긴 value와 좁은 창에서 열이 찌그러지지 않도록 table을 최소 790px grid와 horizontal scroll로
만들었다. disabled 행은 toggle을 제외한 내용의 opacity만 낮춰 상태와 복구 action을 모두 유지한다.

### 3. History and Collection compatibility

Files:

- `apps/api-playground/src/lib/persistence.ts`
- `apps/api-playground/src/lib/persistence.test.ts`
- `apps/api-playground/src/lib/collections.ts`
- `apps/api-playground/src/lib/collections.test.ts`
- `apps/api-playground/src/lib/contextMenu.ts` (기존 spread clone 계약 재사용)

History/Collection parser는 header key/value와 optional boolean enabled의 wire type을 검증한다.
read-back 시 `normalizePersistedRequest()`를 적용하므로 기존 v2의 enabled 누락은 true가 되고,
100행을 넘는 입력은 경계에서 잘린다. params도 실제 key/value string pair인지 함께 검증하도록
기존 느슨한 array check를 보강했다.

`sanitizeRequestForPersistence()`는 header를 정규화한 뒤 sensitive literal/reference sanitization을
수행하고 enabled를 결과에 붙인다. disabled 행도 저장되므로 raw Authorization/Cookie 같은 literal을
그대로 남기지 않는다. `${NAME}` reference는 기존 정책대로 보존한다. Collection save/read-back과
History parse fixture에서 duplicate order, true/false와 reference가 byte structure 수준으로
유지되는지 검증했다.

### 4. Environment and browser preview

Files:

- `apps/api-playground/src/lib/environments.ts`
- `apps/api-playground/src/lib/environments.test.ts`
- `apps/api-playground/src/api.ts`

frontend non-secret preview 치환은 header object를 spread해 기존 value reference 치환 중에도
enabled를 유지한다. 원본 request와 enabled는 변경하지 않는 fixture를 추가했다.

browser fallback은 `Record<string, string>` 대입으로 duplicate를 덮어쓰던 구현을 `Headers.append`로
교체하고 disabled 행을 건너뛴다. auth는 append하고 JSON Content-Type은 기존 동작처럼 set한다.
Fetch/User Agent가 같은 header를 comma-join하거나 제한할 수 있으므로 README는 exact duplicate wire
계약을 packaged Rust backend 기준으로 명시한다.

browser response redactor의 direct-secret seed도 enabled sensitive header만 수집한다. 전송하지 않은
disabled literal이 응답을 과도하게 마스킹하는 일을 막는다.

### 5. Native send, resolve and cURL

File:

- `apps/api-playground/src-tauri/src/commands/request.rs`

Rust reference scanner는 enabled request header만 보고 disabled reference의 DPAPI envelope를 열지
않는다. resolve 결과는 enabled를 유지하고 처음 100행만 보존한다. `execute_request()`는 enabled,
non-empty, redirect policy를 통과한 각 행을 `RequestBuilder.header()`에 Vec 순서대로 추가한다.

redactor는 enabled sensitive header만 seed로 수집한다. masked frontend cURL은 sanitized request에서
disabled를 거르고, backend revealed cURL도 resolved request에서 같은 조건을 사용한다. 따라서
disabled 행은 confirm 뒤 원문 cURL에도 나타나지 않는다.

localhost TCP fixture는 실제 reqwest 요청 원문에서 다음을 검증한다.

```text
x-trace: one
x-trace: two
# x-skip is absent
```

별도 fixture는 enabled `${TOKEN}`만 unseal하고 disabled `${BROKEN}`의 invalid base64 envelope를
무시하며, resolved Vec와 cURL에서 duplicate 순서/disabled 제외를 확인한다.

### 6. Documentation synchronization

Files:

- `apps/api-playground/README.md`
- `docs/product-opportunities.md`
- `docs/roadmap.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`

README와 상세 계획에 persistence migration, native/browser duplicate 차이, enabled secret boundary,
100-row limit과 explicit non-scope를 기록했다. roadmap은 #267/PR #399 merge 뒤 #268 범위로
이동했고 product opportunity inventory도 구현 상태로 갱신했다.

새 dependency와 lockfile 변화는 없다. main production JS 230,799바이트/gzip 72,244바이트에서
234,696바이트/gzip 73,337바이트로 3,897/1,093바이트 증가했다. CSS는 9,334/2,319바이트에서
10,351/2,529바이트로 1,017/210바이트 증가했다.

## PR-Boundary Review Findings

전체 diff와 acceptance를 직접 검토하며 다음을 확인·보강했다.

1. **map collapse 제거**: native request는 이미 Vec였지만 browser fallback은 object assignment로
   마지막 duplicate만 남겼다. `Headers.append`로 바꾸고 native localhost wire를 별도 검증했다.
2. **legacy enabled default**: frontend parser와 Rust serde 양쪽에서 enabled 누락을 true로 처리해
   기존 History/Collection과 inbound request가 조용히 disabled가 되거나 폐기되지 않게 했다.
3. **disabled는 저장되지만 비실행**: persistence에는 boolean과 masked/reference value를 남기고,
   reference scan·unseal·redactor·send·두 cURL 경로에서는 모두 제외했다.
4. **secret picker 최소 권한**: component는 현재 environment의 secret 이름 배열만 받고 sealed value나
   다른 environment 이름을 받지 않는다. invalid reference name도 option에서 제거한다.
5. **redirect 보안 유지**: header.enabled check를 기존 sensitive/header/body redirect filter 앞에
   추가했을 뿐 cross-origin Authorization/Cookie/body 억제 순서를 변경하지 않았다.
6. **bounded UI/backend 일치**: UI add/duplicate, persistence normalize, Rust reference/resolve를 모두
   100행으로 맞춰 crafted IPC가 frontend 제한만 우회하지 못하게 했다.
7. **wire scope 분리**: response `KeyValue`와 Params editor/model에는 enabled를 추가하지 않아 #269
   Cookie와 이후 response tab을 선행 구현하지 않았다.
8. **failure reflection**: 새 UI는 secret value와 backend 오류를 직접 다루지 않는다. 기존 App의
   generic send/persistence/cURL 오류 경계가 그대로 유지된다.

## Test Coverage

Frontend tests cover:

- legacy enabled default and explicit false normalization
- immutable add/update/duplicate/remove with row order
- case-insensitive duplicate group count
- deterministic valid secret-name options and `${NAME}` output
- 100-row add/duplicate bound
- HeaderTable enabled toggle, summary, clone/delete and no-secret add
- picker name-only contract and persistent no-unseal notice
- History v2 legacy normalization and duplicate/disabled/reference parse
- Collection save/read-back duplicate/enabled/reference round-trip
- persistence sensitive masking with enabled retention
- environment header value substitution with enabled retention
- masked cURL duplicate order and disabled omission
- all existing persistence, redaction and context-menu regressions

Rust tests cover:

- serde default enabled=true
- enabled-only reference collection and corrupt disabled envelope skip
- resolved duplicate order and revealed cURL disabled omission
- live reqwest duplicate header wire order and disabled absence
- all existing secret redaction, persistence, redirect and generic-error regressions

## Verification Results

### Frontend

```text
$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter api-playground exec vitest run --environment=node --maxWorkers=1 \
      src/lib/headers.test.ts
Test Files  1 passed (1)
Tests       4 passed (4)
exit 0

$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter api-playground exec vitest run --maxWorkers=1 \
      src/HeaderTable.test.tsx src/lib/persistence.test.ts \
      src/lib/collections.test.ts src/lib/environments.test.ts src/App.test.ts
Test Files  5 passed (5)
Tests       64 passed (64)
exit 0

$ NODE_OPTIONS=--max-old-space-size=768 \
    pnpm --filter api-playground test -- --maxWorkers=1
Test Files  8 passed (8)
Tests       78 passed (78)
exit 0

$ NODE_OPTIONS=--max-old-space-size=768 pnpm --filter api-playground build
45 modules transformed
JS 234.70 kB / gzip 73.57 kB
CSS 10.35 kB / gzip 2.52 kB
exit 0
```

### Rust acceptance

```text
$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo test -p api-playground --jobs 1
19 passed; 0 failed
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo check -p api-playground --jobs 1
exit 0

$ CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox \
    cargo clippy -p api-playground --all-targets --jobs 1 -- -D warnings
exit 0

$ cargo fmt --manifest-path apps/api-playground/src-tauri/Cargo.toml -- --check
exit 0
```

### Repository hygiene

```text
$ bash .github/scripts/check-catalog.sh
exit 0

$ python3 .github/scripts/check-dependencies.py check
dependency policy OK; notices match Cargo.lock and pnpm-lock.yaml

$ git diff --check
exit 0
```

Remote CI still validates dependency policy, catalog, scoped frontend and Linux/Windows Rust gates before
merge.

## Resource Discipline

한 feature worktree에서만 작업하고 Vitest `--maxWorkers=1`, Node 768MiB heap, Cargo job 1과
Linux-native shared target을 사용했다. frontend와 Rust/build를 겹치지 않고 순차 실행했다. 전체
78-test frontend 회귀는 252.87초였고 assertion은 789ms, jsdom environment setup은 205.25초였다.
검증 중 available memory는 약 9.2GiB, free swap은 5.9GiB였으며 background devbox watch/test/build
process는 남기지 않는다.

## Remaining Checkpoint

- Windows packaged WebView2에서 horizontal table scroll, duplicate inputs, enabled toggle, secret-name
  picker, History/Collection reload와 masked/revealed cURL을 확인하는 W1 evidence는 P1 묶음
  checkpoint에서 수행한다. evidence에는 secret 원문을 남기지 않는다.
- API Playground 목표 version 0.4.0은 Wave 9 version-bump/release preparation에서 적용한다.
- request Cookie editor는 다음 독립 P1-09 issue #269에 남긴다.
- multipart, response header/cookie tab, row drag reorder, bulk import와 browser exact duplicate wire는
  이 PR에 포함하지 않았다.
