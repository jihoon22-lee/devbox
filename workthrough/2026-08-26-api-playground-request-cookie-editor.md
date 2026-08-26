# API Playground Request Cookie Editor

## Overview

Issue #269의 P1-09 여섯 번째 범위로 API Playground에 request `Cookie` header 전용 편집기를
추가했다. 이 기능은 브라우저의 domain cookie jar나 response `Set-Cookie` 관리 기능이 아니다.
현재 요청에 포함할 cookie name/value를 구조화된 행으로 편집하고, 저장·cURL·전송·redirect
경계에서 민감값을 일관되게 처리한다.

```text
Cookie rows (ordered, max 100)
        │
        ├─ enabled + valid ─► backend-only reference resolve
        │                             │
        │                             ├─ one `Cookie` header: a=1; b=2
        │                             ├─ response/error redaction seed
        │                             └─ cross-origin redirect에서 제거
        │
        ├─ disabled ─────────► persist shape, mask literal, skip unseal/send/cURL
        │
        └─ invalid/conflict ─► frontend 표시 + backend fail-closed
```

새 dependency, sidecar, network service, localStorage key 또는 storage schema version은 추가하지
않았다. 기존 History/Collection v2에서 `cookies`가 없는 request는 빈 배열로 정규화한다.
Windows packaged UI smoke는 계획대로 W1 P1 묶음 checkpoint에 남겼다.

## Scope and Non-Scope

### Included

- request Cookies 전용 tab
- ordered name/value rows와 enabled 상태
- 기본 password 표시와 명시적 보기/숨김
- 현재 environment의 봉인된 secret 이름 reference picker
- immutable add/update/duplicate/remove
- 최대 100행과 Cookie name/value 문자 검증
- raw `Cookie` header와 구조화 Cookie의 충돌 차단
- History·Collection masking, round-trip과 deep clone
- backend-only reference resolve와 단일 Cookie header 조립
- masked cURL과 확인 후 backend one-shot revealed cURL
- response/error redaction seed와 cross-origin redirect 제거
- legacy v2 compatibility

### Excluded

- response `Set-Cookie` viewer
- browser/domain cookie jar
- domain, path, expiry, SameSite, Secure, HttpOnly 속성 관리
- multipart/form-data
- cookie import/export

위 제외 항목은 #269의 명시적 비범위이며 response header/cookie와 multipart의 독립 PR 경계를
유지한다.

## Data Contract

Frontend request template에 다음 wire-compatible type을 추가했다.

```ts
interface RequestCookie {
  name: string;
  value: string;
  enabled?: boolean;
}
```

`enabled`가 없는 행은 활성으로 읽고 저장·편집 경계에서는 명시적 boolean으로 정규화한다.
`RequestTemplate.cookies`는 내부에서 항상 배열이지만 기존 v2 JSON parser는 field 누락을 허용한
뒤 빈 배열로 올린다.

Rust에는 같은 shape의 `RequestCookie`와 `#[serde(default)] RequestTemplate.cookies`를 추가했다.
backend-only `ResolvedRequest`도 별도 cookie vector를 가지므로 sealed secret의 plaintext는
frontend request state나 저장 JSON으로 되돌아오지 않는다.

## Editor Behavior

`CookieEditor`는 각 행에 다음 control을 제공한다.

- enabled checkbox
- Cookie name input
- 기본 `type=password` value input
- 명시적 보기/숨김 button
- secret reference select
- duplicate/delete button
- 행별 validation 오류

picker에는 현재 선택한 environment의 `secret: true` 변수 이름 중 reference 문법에 맞는 값만
정렬·중복 제거해 전달한다. 선택 결과는 value 전체를 `${NAME}`으로 교체하며 sealed blob이나
plaintext는 component prop으로 전달하지 않는다.

보기 상태는 request 전환 시 component key를 바꿔 초기화한다. 행을 disable하거나 secret
reference로 교체하면 해당 행을 숨기고, add/duplicate/delete로 index 의미가 바뀔 때는 모든 보기
상태를 초기화한다. 따라서 이전 행에서 선택한 reveal 상태가 새 request나 이동한 행에 적용되지
않는다.

## Validation and Ambiguity Boundary

Cookie name은 RFC token에 사용할 수 있는 ASCII 문자만 허용한다. value는 cookie-octet 범위를
따라 공백, 세미콜론, 따옴표, 역슬래시, comma와 제어 문자를 거부한다. 완전히 빈 행과 disabled
행은 전송 대상이 아니므로 validation에서 제외한다.

frontend는 첫 오류의 행 번호를 전역으로 표시하고 Cookies tab에도 행별 오류를 표시한다. 오류가
있는 동안 Send와 cURL을 비활성화한다. Rust는 Tauri command 진입 직후 원본을 먼저 검사하고,
secret resolve 뒤에도 다시 검사한다. resolve 결과가 cookie 문법을 깨뜨리는 경우도 전송하지
않는다.

구조화 Cookie와 Headers tab의 활성 raw `Cookie` header가 동시에 있으면 두 입력을 임의로 합치지
않는다. frontend, browser preview와 Rust backend가 모두 같은 충돌을 거부한다. disabled raw
header와 내용이 없는 cookie 행은 충돌이 아니다.

## Persistence and Masking

History와 Collection sanitizer는 모든 cookie row를 최대 100개로 정규화한다. 직접 입력된 value는
enabled 여부와 관계없이 `[REDACTED]`로 교체하고 `requiresSecretReview`를 설정한다. 저장된
disabled 행을 다시 켤 수는 있지만 원문은 남아 있지 않다.

값 전체가 단일 `${NAME}` 또는 `{{NAME}}` reference일 때만 reference를 보존한다.
`prefix-${NAME}`처럼 literal과 reference가 섞이면 직접 literal의 성격을 증명할 수 없으므로 전체를
마스킹한다. 빈 value는 유효한 `name=` Cookie 표현을 위해 그대로 둔다.

frontend의 구조화 sanitizer에 더해 Rust persistence sanitizer도 JSON의 `cookies[].value`를
인식한다. command가 직접 호출되더라도 direct/mixed value를 마스킹하며, 잘못된 non-string value는
마스킹된 string으로 바꿔 schema parser가 안전하게 거부하도록 한다.

History·Collection context menu 복제는 cookies array를 깊은 복사하며 이미 마스킹된 request만
사용한다. 기존 v2 request에 cookies field가 없으면 read-back에서 `[]`로 복구한다.

## Native Send and Redirect Behavior

Rust는 활성 cookie value의 reference만 검색한다. disabled cookie의 reference는 environment secret
unseal 대상이 아니므로 손상된 envelope도 열지 않는다. resolve 후 유효한 활성 행은 원래 순서대로
`name=value`를 만들고 `; `로 연결해 `Cookie` header 하나로 전송한다.

cookie value는 request redactor의 exact secret seed에 포함된다. 서버가 response body나 오류에
값을 되돌려도 `[REDACTED]`로 바뀐다. redirect가 cross-origin이면 기존 Authorization/API-key/body
억제와 함께 구조화 Cookie header도 다음 요청에 추가하지 않는다.

기본 cURL은 frontend persistence sanitizer를 거친 cookie rows로 만들기 때문에 name과 단일
reference만 보존하고 직접 값은 마스킹한다. 사용자가 경고를 확인한 one-shot 원문 cURL만 backend가
resolved cookie header를 생성하며 결과는 저장하지 않는다.

브라우저 preview도 non-secret cookie를 같은 방식으로 조립하지만 Fetch 구현은 `Cookie`를 forbidden
request header로 제한할 수 있다. 따라서 실제 wire header, 순서와 redirect 제거의 기준은 packaged
native 경로다.

## Files Changed

### Frontend model and logic

- `apps/api-playground/src/types.ts`
- `apps/api-playground/src/lib/cookies.ts`
- `apps/api-playground/src/lib/cookies.test.ts`
- `apps/api-playground/src/lib/environments.ts`
- `apps/api-playground/src/lib/environments.test.ts`

### UI and cURL/browser path

- `apps/api-playground/src/CookieEditor.tsx`
- `apps/api-playground/src/CookieEditor.test.tsx`
- `apps/api-playground/src/App.tsx`
- `apps/api-playground/src/App.css`
- `apps/api-playground/src/App.test.ts`
- `apps/api-playground/src/api.ts`

### Persistence and clone paths

- `apps/api-playground/src/lib/persistence.ts`
- `apps/api-playground/src/lib/persistence.test.ts`
- `apps/api-playground/src/lib/collections.ts`
- `apps/api-playground/src/lib/collections.test.ts`
- `apps/api-playground/src/lib/contextMenu.ts`
- `apps/api-playground/src/lib/contextMenu.test.ts`
- `apps/api-playground/src/App.contextMenu.test.tsx`

### Native backend and documentation

- `apps/api-playground/src-tauri/src/commands/request.rs`
- `apps/api-playground/README.md`
- `docs/product-opportunities.md`
- `docs/roadmap.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`

## Verification

Local verification used one frontend worker and one Cargo build job at a time.

- TypeScript: `pnpm --filter api-playground exec tsc --noEmit` — passed
- focused cookie logic/UI: 2 files, 11 tests — passed
- persistence/cURL/environment regression selection: 7 files, 79 tests — passed
- full API Playground frontend: 10 files, 93 tests — passed
- production build: 47 modules — passed
- Rust format: `cargo fmt ... -- --check` — passed
- Rust unit/live HTTP: 22 tests — passed
- Rust check: `cargo check -p api-playground --jobs 1` — passed
- Rust clippy: `cargo clippy -p api-playground --all-targets --jobs 1 -- -D warnings` — passed
- catalog consistency — passed
- dependency policy/notices and regression scripts — passed
- `git diff --check` — passed

Native fixtures cover:

- old RequestTemplate without cookies and old cookie without enabled
- active secret resolve and disabled corrupt envelope isolation
- invalid name/value, raw header conflict and 100-row limit
- one exact ordered Cookie wire header and disabled omission
- response body redaction seeded by a direct cookie value
- cross-origin redirect Cookie removal
- backend persistence sanitization of direct, exact-reference and mixed-reference values

## Bundle Impact

Compared with main at #268:

| Asset | Main | Feature | Delta |
|---|---:|---:|---:|
| JS | 234,696 B | 241,019 B | +6,323 B |
| JS gzip | 73,337 B | 75,080 B | +1,743 B |
| CSS | 10,351 B | 11,300 B | +949 B |
| CSS gzip | 2,529 B | 2,655 B | +126 B |

No dependency or lockfile change accounts for this delta.

## Deferred Evidence

WSL cannot validate WebView2 packaged behavior. The W1 P1 checkpoint must still capture:

- Cookie tab visual layout and horizontal overflow
- default masked value and explicit reveal reset
- masked cURL clipboard content
- confirmed one-shot revealed cURL content
- native request wire Cookie header
- invalid/conflicting configuration disabled state and message
- cross-origin redirect fixture in packaged runtime

Response `Set-Cookie` presentation remains the separate response header/cookie feature and is not
claimed by this work.
