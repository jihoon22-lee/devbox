# API Playground GraphQL request support (#294)

## Overview

Implemented the v0.5.0 P2-05 GraphQL request flow in the API Playground. GraphQL
query, variables, and operation name stay separate from the existing REST body,
while the existing native `reqwest` transport, auth/header/cookie/environment
resolution, response vault, History, and Collection boundaries are reused.
The feature remains offline-capable at the application boundary: it adds no
CLI, sidecar, schema service, network service, or newly locked package. The
already locked/runtime MIT `tokio` package is declared directly only for
interruptible native connect/body waits.

## Context

The app already supported bounded REST requests, but GraphQL users had to
manually assemble a JSON body, encode GET parameters, and distinguish HTTP
failures from a GraphQL `errors` envelope. The implementation had to preserve
the existing secret and persistence rules while avoiding raw generated bodies,
GET URLs, resolved environment values, and response error extensions in saved
or displayed metadata.

## Changes Made

### 1. Bounded native GraphQL contract

- Added `apps/api-playground/src-tauri/src/core/graphql.rs` and `core/mod.rs`.
- Added a bounded document lexer/structural validator with operation counting,
  duplicate/missing operation-name checks, introspection rejection, and
  subscription rejection. Schema validation remains the server's concern.
- Added strict JSON-object variables parsing and deterministic JSON body
  serialization. Query/variables/body, token/node/depth/string, operation,
  response data, error, and error-path limits are enforced before or during
  projection.
- Added JSON nesting preflight so deeply nested hostile input is rejected
  before recursive JSON parsing.
- Added a safe GraphQL response projection containing only `data` and bounded
  `message`, `locations`, and `path`; unknown error fields such as `extensions`
  are omitted. Malformed JSON and oversized content produce fixed envelope
  states.

### 2. Native HTTP transport and cancellation

- Extended `RequestTemplate`, `ResolvedRequest`, and `ApiResponse` with GraphQL
  fields and registered a process-local `RequestCancellation` Tauri state plus
  `cancel_request` command.
- POST uses canonical JSON and transport-owned `Content-Type`; GET uses URL
  encoding for existing params and GraphQL query/variables/operationName. URL,
  endpoint userinfo, control characters, credential-shaped query keys, derived
  headers, fragment, timeout, request headers, and response body are bounded/fail-closed.
- Preserved existing auth, cookie, redirect, response-header vault, and raw
  copy boundaries. GraphQL redirect targets are revalidated and cross-origin
  sensitive forwarding remains blocked. Bounded caller request IDs route early
  or delayed Cancel IPC only to the request that emitted it; monotonic native
  tokens make a new request supersede an older one and interrupt both
  connect/header wait and bounded response-body read. Browser cancellation uses
  `AbortController`.
- Added request/response redaction for GraphQL credential-shaped literals and
  variable keys, known token patterns, generated body, redirect URL metadata,
  and server response fields. Persistence stores only allowlisted GraphQL
  fields, masks query literals except exact whole-value environment references,
  and never stores the generated POST body or GET URL.

### 3. Frontend editor, browser parity, and lifecycle safety

- Added `apps/api-playground/src/GraphqlEditor.tsx` and GraphQL styling in
  `App.css` for labelled query, variables, and operation-name controls.
- Added the same document, variable, endpoint, URL, header, response, and
  nesting limits in `src/lib/graphql.ts`; browser GET/POST uses the same pure
  builder/projection and bounded streamed response reader.
- Added explicit Send/Cancel behavior, abort wiring, mounted/sequence guards,
  stale response suppression, fixed error allowlisting, busy/double-action
  protection, and inline accessible alerts in `App.tsx`.
- Kept browser preview honest about CORS and forbidden headers. GraphQL browser
  redirects use manual mode so credentials are not silently forwarded.
- Added HTTP status versus GraphQL envelope/data/errors presentation in
  `ResponseViewer.tsx`, with bounded display fields and keyboard-accessible
  response tabs.

### 4. Persistence, types, and fixtures

- Extended `types.ts`, Collection shape validation/cloning, and History/
  Collection persistence sanitization for the separate GraphQL shape.
- Added pure frontend fixtures in `src/lib/graphql.test.ts`, persistence
  redaction coverage in `src/lib/persistence.test.ts`, response summary
  coverage in `ResponseViewer.test.tsx`, and native unit/loopback fixtures in
  `src-tauri/src/core/graphql.rs` and `commands/request.rs`.
- Fixtures cover canonical POST, encoded GET, params/header bounds, endpoint
  userinfo and credential-query rejection, invalid/ambiguous operations,
  variable and response limits, malformed/error projection, query literal and
  variable redaction, cancellation, and HTTP/GraphQL error distinction.

### 5. Project documentation

- Updated `apps/api-playground/README.md` with the wire contract, limits,
  privacy/persistence/cancellation behavior, browser caveats, and explicit
  exclusions (persisted queries, introspection explorer/schema cache,
  subscriptions, code generation, replay/telemetry, and external clients).
- Updated `docs/architecture.md`, `docs/roadmap.md`, and
  `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md` to reflect
  the implementation boundary and remaining release-gate work.

### 6. Root review hardening

- Fixed operation selection so a valid `operationName` selects one named
  operation from a multi-operation document, while mixed anonymous/named
  documents remain rejected.
- Replaced the check-only boolean cancellation flag with request-ID-routed,
  monotonic request tokens and interruptible native waits. A rapid new Send
  cannot clear an old Cancel and resurrect the stale request, and a delayed old
  Cancel cannot terminate the new request.
- Dropped stale GraphQL editor state as soon as the request switches to another
  body kind. Native resolution no longer unseals environment references from an
  inactive GraphQL draft, and both frontend/backend persistence omit the field.
- Rejected endpoint fragments in both native and browser validation so POST/GET
  URL assembly cannot diverge around a client-only fragment.
- Ran the full frontend suite and build, then fixed the context-menu API mock
  and the shared GraphQL-derived-header helper that focused fixtures had not
  compiled.
- Added direct `tokio 1.53.1` (`macros`, `time`; MIT) use. It was already in the
  locked/runtime graph, adds no package, external executable, network behavior,
  or online requirement. Regenerated the notice digest and re-ran dependency
  policy checks.

## Code Examples

### Canonical POST body

```rust
// apps/api-playground/src-tauri/src/core/graphql.rs
// operationName is included only when explicitly selected; variables are a
// strict JSON object and the serialized body is bounded before transport.
{"operationName":"Viewer","query":"query Viewer { viewer { id } }","variables":{}}
```

### Safe persistence shape

```typescript
// apps/api-playground/src/lib/persistence.ts
// Generated transport state is discarded; only the editable GraphQL fields
// are retained and query literals are masked before History/Collection save.
{
  body_kind: "graphql",
  body: "",
  graphql: { query: maskedQuery, variables: sanitizedVariables, operation_name }
}
```

### HTTP and GraphQL status distinction

```tsx
// apps/api-playground/src/ResponseViewer.tsx
<span>HTTP {response.status >= 400 ? "error" : "success"}</span>
<span>GraphQL envelope: {graphql.envelope}</span>
```

## Verification Results

### Completed

```text
base after rebase: b3fe815 feat(devbox-manager): add custom install root (#426)
cargo fmt --all -- --check: passed
CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-api-playground-graphql-review \
  cargo test -p api-playground -j3: 46 passed, 0 failed
CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-api-playground-graphql-review \
  cargo check -p api-playground -j3: passed
CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-api-playground-graphql-review \
  cargo clippy -p api-playground --all-targets -j3 -- -D warnings: passed
vitest (GraphQL + persistence fixtures): 2 files, 25 tests passed
vitest (ResponseViewer fixture): 1 file, 5 tests passed
pnpm --filter api-playground test: 14 files, 124 tests passed
pnpm --filter api-playground build: passed
pnpm build: passed (17 workspace projects)
cargo test --workspace -j3: passed
cargo check --workspace -j3: passed
cargo clippy --workspace --all-targets -j3 -- -D warnings: passed
python3 .github/scripts/check-dependencies.py check: passed
git diff --check: passed
```

`#308` custom install root merge `b3fe815` 위로 최종 rebase하면서 두 기능이 함께 수정한
`Cargo.lock`, architecture/roadmap/native-first 계획을 모두 보존하고, 유일한 content conflict인
`THIRD_PARTY_NOTICES.md`는 결합된 lockfile에서 다시 생성했다. rebase 직후 다음 focused gate를
동일하게 재실행했다.

부모 PR의 Windows launch fixture와 legacy fallback canonicalization 보정이 #426으로 merge된 뒤
최신 main `b3fe815` 위로 다시 rebase했다. GraphQL diff와 겹치는 파일이 없어 content conflict 없이
완료됐으며, 아래 focused gate로 최종 부모 tree와의 결합을 다시 확인한다.

```text
cargo fmt --all -- --check: passed
cargo test -p api-playground -j2: 46 passed, 0 failed
cargo check -p api-playground -j2: passed
cargo clippy -p api-playground --all-targets -j2 -- -D warnings: passed
pnpm --dir apps/api-playground test: 14 files, 124 tests passed
pnpm --dir apps/api-playground build: passed
python3 .github/scripts/check-dependencies.py check: passed
git diff --check: passed
```

최종 PR gate는 disk 사용을 제한하기 위해 `CARGO_INCREMENTAL=0`과 단일 전용 target을 사용했고,
merge 또는 재검증 종료 직후 해당 재생성 가능 cache를 정리한다.

### Pending / environment-limited

- Windows packaged W2 verification and the complete workspace CI gate remain
  release/PR checks owned by the parent workflow.

## Next Steps

- Offline `pnpm install --frozen-lockfile` populated only ignored test links;
  they are removed before handoff and no generated dependency tree is committed.
- Windows review should verify packaged redirects, native cancellation timing,
  browser CORS behavior, and no-raw-value persistence/redaction guarantees.
