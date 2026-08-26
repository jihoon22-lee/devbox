# API Playground OpenAPI 3.x local/URL import

## Overview

Issue #293의 독립 기능으로 API Playground에 로컬 파일과 HTTP(S) URL 기반 OpenAPI 3.0/3.1 JSON·YAML import를
추가했다. 입력은 앱 내부의 순수 bounded parser에서 deterministic request preview로 변환되고,
사용자가 확인한 뒤에만 현재 draft에 적용하거나 새 Collection 항목으로 저장된다. 이 기능은
인터넷이 없거나 제한된 개발 환경에서도 local import가 그대로 동작하고, URL을 명시적으로 입력한
경우에만 별도의 native bounded fetch를 수행한다. Swagger UI bundle, 자동 request 전송과 secret
주입은 포함하지 않는다.

## Context and decisions

- `CONVENTIONS.md`의 native-first, offline-capable, 기능 단위 PR, raw credential/path 비노출
  규칙과 issue #293의 operation preview/merge 요구사항을 기준으로 삼았다.
- OAS `$ref`는 해석하거나 네트워크로 가져오지 않는다. operation·parameter·body·security
  scheme에 `$ref`가 있으면 그 operation만 적용 불가로 표시하고 다른 operation은 계속 preview한다.
  path item 자체의 `$ref`만 그 경로 전체에 적용하며, 한 HTTP method 내부의 `$ref`가 같은 path의
  형제 method까지 오염시키지 않도록 operation 단위로 검사한다. 문자열이 아닌 잘못된 `$ref`도
  무시하지 않고 fail-closed한다.
- 기존 Collection의 overwrite API는 추가하지 않았다. 현재 draft 적용은 한 operation,
  Collection 저장은 체크한 operation을 새 항목으로 prepend하는 기존 `addEntry` 경로를 사용한다.
- issue #293과 원 계획의 local/URL acceptance를 재검토해 URL을 후속으로 미루었던 초안 판단을
  철회했다. URL은 parser와 분리된 Rust command에서만 읽고, local file 경로에는 네트워크 호출을
  추가하지 않았다.
- 공용 `crates/`·`packages/` 추출은 필요하지 않았다. parser와 preview DTO는 API Playground의
  request template 계약에만 의존하므로 `src/lib/openapi.ts`와 `src/OpenApiImport.tsx`에 격리했다.
- YAML 1.2 parser와 strict JSON validator는 이미 workspace lock/notices에 존재하는
  `yaml@2.9.0`(ISC)·`jsonc-parser@3.3.1`을 사용한다. 새 버전 다운로드나 install은 수행하지
  않았고, app importer와 lock importer만 추가한 뒤 생성된 `THIRD_PARTY_NOTICES.md`를 갱신했다.
- URL source는 API Playground가 이미 사용하는 `reqwest`를 재사용한다. 전체 timeout을 redirect와
  streaming body까지 공유하기 위해 직접 `tokio` time feature를 추가했고, gzip/deflate/brotli/zstd
  decoded-body cap을 위해 reqwest compression features와 lock/notices를 갱신했다. URL 원문은
  native command 입력에서만 일시적으로 사용하고 DTO, preview, persistence, 오류, 로그에는 포함하지 않는다.
  응답 DTO는 raw 문서가 credential example을 포함할 수 있으므로 의도적으로 `Debug`를 구현하지 않는다.

## Changes made

### 1. Bounded parser and request normalization

File: `apps/api-playground/src/lib/openapi.ts`

- JSON은 comments/trailing comma/unsafe number를 `jsonc-parser`로 먼저 거부하고 YAML parser의
  `uniqueKeys`로 duplicate key도 거부한다. YAML은 YAML 1.2 core, merge 비활성, unknown tag
  warning 거부, alias expansion 50개 상한으로 읽는다.
- UTF-8 입력 4 MiB, graph depth 40, object/array node 50,000, string 16,384자, path 250,
  operation 1,000, server 20, parameter 2,000, security scheme 100, operation별 media type
  50, body 512 KiB, Collection 표시 이름 120자의 공통 상한을 `OPENAPI_LIMITS`로 고정했다.
- parsed graph를 null-prototype object로 다시 만들며 cycle/alias, non-finite·unsafe number,
  non-plain object, `__proto__`·`prototype`·`constructor` key와 제어문자를 fail-closed한다.
- `http`/`https` server만 허용한다. userinfo, query/fragment, dot-segment, invalid variable,
  민감 variable 이름은 사용하지 않고, server variable default만 안전하게 치환한다. 경로는
  `/`로 시작하고 query/fragment/backslash/dot-segment/잘못 닫힌 `{parameter}`를 거부한다.
- JSON visitor 단계에서도 depth/node 상한을 먼저 적용해 parser가 입력을 materialize하기 전에
  과도한 중첩·항목을 중단한다. parameter/body scalar와 object key의 제어문자는 draft에 넣지
  않으며, `${ENV}`/`{{ENV}}` 참조와 Bearer/Basic literal도 값으로 반입하지 않는다. encoded
  traversal server/path도 fail-closed한다.
- path를 code-unit 정렬하고 HTTP method를 GET, POST, PUT, PATCH, DELETE 순으로 정렬한다.
  parameter는 path/query/header/cookie 순서와 이름으로 정렬하고 header만 case-insensitive
  identity를 사용한다.
- parameter example 선택 순서는 direct example → named examples 정렬 → schema example,
  default, enum 첫 값이다. 비민감 path parameter 값만 URI component로 URL placeholder에
  넣고 민감 path는 placeholder로 남긴다. query/header/cookie는
  scalar만 문자열화하며 object/array 예제는 빈 값과 고정 warning으로 둔다.
- request body media type은 `application/json`, `+json`, form, multipart, lexical fallback
  우선순위로 고른다. 구조화 JSON body property를 key-sorted JSON으로 만들고, form/multipart는
  scalar field만 변환한다. opaque raw body와 이름 없는 scalar/array 문자열 example은 secret
  오입력을 막기 위해 생략한다.
- `authorization`, `cookie`, `token`, `secret`, `password`, `credential`, API key 등의
  parameter/property는 값을 빈 문자열로 만들고 redacted metadata만 남긴다. basic/bearer/
  api-key security는 auth kind과 header/query/cookie 위치만 만들며 `valuesInjected: false`를
  고정한다. 최종 request의 parameter/header/cookie 행은 각각 기존 editor 상한(100행)으로 다시
  자르고 초과 operation은 적용 불가로 표시한다. OAuth2/openIdConnect, AND 여러 scheme, missing
  scheme은 operation 오류다.

### 2. Native bounded URL source

File: `apps/api-playground/src-tauri/src/commands/openapi.rs`

- URL은 최대 2,048자이며 HTTP(S), host 존재, 공백 없음만 허용한다. userinfo, fragment와
  credential-shaped query key/value는 오입력과 반향 위험 때문에 요청 전에 거부하되, `format=json`
  같은 비민감 query는 사용할 수 있다.
- connect timeout 5초, 전체 timeout 15초, redirect 3회, decoded body 4 MiB를 강제한다.
  redirect는 같은 host·유효 port의 동일 scheme 또는 HTTP→HTTPS(80→443) 승격만 허용하고
  HTTPS downgrade와 다른 host/port 이동을 차단한다.
- response는 성공 status와 UTF-8만 허용한다. reqwest가 gzip/deflate/brotli/zstd를 해제한 뒤
  `Content-Length`를 먼저 검사하고 실제 decoded chunk 누적 크기도 다시 검사해 압축·누락 header
  경로가 상한을 우회하지 못하게 한다.
- content type 또는 최종 path suffix로 JSON/YAML format만 정하고 source name은
  `remote-openapi.json|yaml`로 고정한다. network/status/redirect/size/encoding 실패는 URL·status
  text·library 오류가 없는 고정 메시지만 반환한다.

### 3. Preview and explicit apply UI

Files: `apps/api-playground/src/OpenApiImport.tsx`, `apps/api-playground/src/App.tsx`,
`apps/api-playground/src/App.css`

- request bar의 `OpenAPI` 버튼은 persistence migration이 끝나고 기존 전송/저장 작업이 없을
  때만 활성화된다.
- native/browser 모두 사용할 수 있는 local file input(`.json`, `.yaml`, `.yml`)과 desktop native
  URL import form을 제공한다. `File.size`, native response, parser가 각각 4 MiB를 확인한다.
- modal은 `role=dialog`, `aria-modal`, labelled heading/description, status/alert region,
  labelled server select와 checkboxes를 사용한다. Escape는 IME composition 중에는 닫지 않고,
  busy/applying 중에는 중복·취소 동작을 막는다. Tab/Shift+Tab은 dialog 안에서 순환하고,
  닫을 때 처음 열기 동작의 focus를 복원한다.
- 파일/URL read가 공유하는 request generation, synchronous busy ref와 unmount cleanup을 사용해
  중복 submit과 늦게 도착한 결과가 현재 preview를 덮어쓰지 못하게 했다. 파일명은 basename만
  120자까지, URL source는 고정 이름만 표시한다.
- URL fetch와 Collection 저장의 중복 실행 차단은 React render 시점의 state가 아니라 동기 ref를
  기준으로 결정한다. 따라서 같은 event turn의 연속 submit과 busy 중 Escape도 차단되며,
  unmount 이후 완료된 fetch·persistence는 state나 close callback을 갱신하지 않는다.
- source summary, validated server select, operation method/path, parameter names and redaction
  markers, body example state, auth location/name metadata, 고정 warning/error를 미리 보여 준다.
  operation 오류는 row를 disabled로 만들고 나머지 row는 계속 선택할 수 있다.
- `현재 draft에 적용`은 applyable operation이 정확히 하나 체크된 경우에만 작동하며 request
  state·editor revision·선택 tab·response를 갱신한다. 이 callback은 `sendRequest`를 호출하지
  않는다.
- `새 Collection에 추가`는 체크한 applyable operation 모두를 `OpenAPI` folder의 새 항목으로
  `addEntry`/기존 sanitizer/read-back 경로에 전달한다. prepend semantics는 batch를 reverse해
  preview 순서를 Collection에도 보존하며, 표시 이름은 120자로 제한하고 기존 항목 ID를
  찾아 덮어쓰지 않는다. random UUID가 기존 ID와 충돌해도 deterministic fallback ID를 다시
  할당하며 synchronous save ref와 기존 `collSaving` 상태로 중복 저장을 막는다. 저장 실패는 fixed
  message로만 표시하고 unmount 뒤 state를 갱신하지 않는다.

### 4. Fixtures and documentation

Files: `apps/api-playground/src/lib/openapi.test.ts`, `apps/api-playground/src/OpenApiImport.test.tsx`,
`apps/api-playground/package.json`, `pnpm-lock.yaml`, `THIRD_PARTY_NOTICES.md`,
`apps/api-playground/README.md`, `docs/roadmap.md`,
`docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`,
`docs/superpowers/specs/2026-08-15-ux-improvements-design.md`

- JSON/YAML 3.0/3.1 fixture, deterministic path/method/parameter ordering, server switching,
  request body redaction, empty security values, unsupported `$ref`, unsafe path/key, cyclic
  graph, fetched source의 고정 이름, byte/depth/node/body/row limit와 basename/credential
  sanitization tests를 추가했다. generated row cap, control-character body omission과 `${ENV}`
  secret-reference 비반입도 고정했다.
- Rust fixture는 URL scheme/userinfo/credential-shaped query/fragment 거부, bounded UTF-8 JSON fetch,
  oversized Content-Length, same-host redirect/HTTPS downgrade 정책과 오류 URL 비반향을 검증한다.
- UI fixture는 local file read와 URL native result 후 explicit current-draft apply, multi-select new
  Collection callback, URL 중복 submit/stable error, IME-safe Escape, accessible dialog와 invalid
  operation disabled row를 검증한다. 추가 회귀 fixture는 busy 중 Escape 차단과 unmount 뒤 URL
  결과 폐기, 동일 path의 `$ref` 오류가 형제 method에 전파되지 않는 operation 격리를 검증한다.
- app manifest/lock importer에 이미 승인·잠긴 parser dependency를 명시하고 dependency notice를
  generator로 재생성했다.
- README에 supported metadata, local offline/URL network bounds/no-auto-send 정책과 redaction
  계약을 추가했다. roadmap와 native-first/UX spec은 원래 local/URL acceptance를 유지하면서
  native URL 경계의 정확한 timeout/redirect/size 정책으로 구체화했다.

## Code example

```typescript
const result = parseOpenApiSource({
  kind: "file",
  name: file.name,
  text,
});

if (result.ok) {
  // Only a user click reaches onApply/onAddToCollection; parsing never sends.
  setPreview(result.preview);
}
```

Remote source text enters the same parser without retaining its URL:

```typescript
const source = await fetchOpenApiSource(url);
const result = parseOpenApiSource({ kind: "url", format: source.format, text: source.text });
```

Security output is intentionally empty-valued:

```typescript
{
  kind: "bearer",
  username: "",
  password: "",
  token: "",
  api_key: "",
  api_value: "",
}
```

## Verification results

### Focused parser and UI assertions

Root review 전에 작성된 worktree를 최신 `main`의 GraphQL 변경 위로 rebase했고, PR 직전에는
Code Pad offline managed-LSP 변경까지 포함한 `main` 위로 다시 rebase했다. 충돌한 README,
roadmap/spec, generated notices는 각 기능의 계약을 모두 보존하도록 합쳤다. 그 과정에서 중복된
`tokio` dependency 선언을 하나로 정리하고 dependency notices를 다시 생성했다. 이후 frozen lock과
offline store만 사용해 worktree dependency를 연결하고 순수 parser/UI 회귀 suite를 다시 실행했다.
Assertions cover ordering, server selection, redaction, dangerous key, remote source, YAML, oversized
input, sibling-operation isolation, synchronous busy guard와 unmount discard다.

첫 PR frontend run은 선행 Code Pad fixture의 event-turn race를 드러내 별도 PR #433에서 수정했다.
그 위로 rebase한 두 번째 run은 50,000-node 경계를 검증하는 25,000-line block YAML fixture가 모든
workspace UI suite와 병렬 실행될 때 15초 제한을 넘는 문제를 드러냈다. 동일 normalized key/value
graph를 compact JSON으로 구성해 parser/normalizer의 실제 `NODE_LIMIT` 계약은 유지하면서 fixture
준비 비용을 줄였다. JSON streaming preflight도 object property key를 node로 세도록 normalization과
일치시켜, 같은 제한을 YAML document와 전체 normalized graph를 만들기 전에 조기에 거부한다.
production parser limit이나 일반 test timeout은 완화하지 않았다.

> `openapi.test.ts` + `OpenApiImport.test.tsx`: 22 passed

> `pnpm install --offline --frozen-lockfile`: passed (workspace store reused; network download 없음)

### Native URL boundary

> `cargo test -p api-playground -j2`: 50 passed (OpenAPI, 기존 request, GraphQL 회귀 포함)

> `cargo check -p api-playground --all-targets -j2`: passed

> `cargo clippy -p api-playground --all-targets -j2 -- -D warnings`: passed

> `cargo fmt --all -- --check`: passed

### Type and syntax checks

- `pnpm --filter api-playground test -- --maxWorkers=2`: 16 files, 146 tests passed.
- `pnpm --filter api-playground build`: passed; strict TypeScript project build와 Vite production
  bundle이 최신 GraphQL/OpenAPI 통합 source graph의 135 modules를 변환했다.
- `git diff --check`: passed for tracked edits and no whitespace diagnostics were emitted for the
  new source/test files.
- `python3 .github/scripts/check-dependencies.py generate`: passed.
- `python3 .github/scripts/check-dependencies.py check`: `dependency policy OK; notices match
  Cargo.lock and pnpm-lock.yaml`.
- `python3 .github/scripts/test-check-dependencies.py`: passed.
- `python3 .github/scripts/test-build-manifest.py`: passed.
- `bash .github/scripts/check-catalog.sh`: passed.

The isolated Rust target remained outside the worktree at
`/home/jihoon/.cache/targets/devbox-issue293` and used 4.9 GiB after test/check/clippy. The ignored
frontend `dist` output used 440 KiB. PR-wide Linux/Windows workspace gates remain the final merge
verification after the branch is rebased onto the latest `main`.

## Known limitations and follow-up

- URL import intentionally permits loopback/private-network hosts because API Playground is a
  developer client for local and intranet APIs. It does not accept credentials, credential-shaped query, fragment,
  custom headers or cookies; authenticated spec endpoints remain outside this feature.
- Operation/path-level server overrides are detected with a dedicated error and fail-closed because
  the existing simple request template has only one document-wide server selector; their precedence
  should be specified before extending the server UI. Full OpenAPI parameter serialization styles
  are also not represented.
- `$ref` resolution, OAuth2/OpenID flows, GraphQL/SSE/WebSocket and code generation remain separate
  features. No automatic request or secret lookup is performed by this importer.
