# API Playground Protocol Lab — MCP/gRPC design

**Date:** 2026-08-30

**Status:** v0.6.0 implementation contract

**Tracking:** #485

## 1. Goal

API Playground 안에 Protocol Lab 패널을 추가한다. 별도 데스크톱 앱을 만들지 않고 기존
environment secret, bounded transport, cancellation, native/browser 경계를 재사용한다.

Protocol Lab은 세 개의 독립적인 리뷰·rollback 경계로 구현한다.

1. MCP contract + Streamable HTTP
2. MCP stdio + OAuth
3. gRPC + TLS/mTLS

이 문서의 즉시 구현 범위는 1번이다. 2번의 child process와 credential callback, 3번의 proto
compiler/reflection과 private key는 서로 다른 보안 경계이므로 첫 PR에 섞지 않는다.

## 2. Authoritative protocol baseline

현재 공식 MCP specification은 `2026-07-28`이다. 이 revision은 기존 `2025-11-25`와 wire
lifecycle이 호환되지 않는다.

- modern `2026-07-28`: stateless, `server/discover`, request별 version/capability/client metadata,
  HTTP session/GET stream 없음
- legacy `2025-11-25`: `initialize`/`notifications/initialized`, optional HTTP session ID,
  독립 GET stream과 explicit cancelled notification 가능

The PR1 client uses the modern Streamable HTTP POST flow and the legacy initialize/session POST
flow. Backward-compatible legacy SSE resumption and the legacy GET listener are not implemented
in PR1, even though the legacy baseline defines those transport behaviors.

Protocol Lab은 dual-era client다. 기본 `auto` 모드는 modern을 먼저 확인하고 공식 fallback
규칙으로만 legacy를 선택한다. 사용자는 진단을 위해 `modern` 또는 `legacy`를 강제할 수 있다.
era는 self-reported server name이나 HTTP banner로 추정하지 않는다.

Primary references:

- https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning
- https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http
- https://modelcontextprotocol.io/specification/2026-07-28/server/discover
- https://modelcontextprotocol.io/specification/2026-07-28/schema
- https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle
- https://modelcontextprotocol.io/specification/2025-11-25/basic/transports
- https://github.com/modelcontextprotocol/inspector

## 3. User flow

### 3.1 Connection

Protocol Lab의 Streamable HTTP form은 다음만 받는다.

- endpoint: absolute `http`/`https` URL
- era: `auto`, `modern`, `legacy`
- timeout: 100 ms–120 s
- enabled custom headers: 기존 API Playground header row와 `${NAME}`/`{{NAME}}` reference
- current Environment: secret은 기존 DPAPI envelope를 backend에서만 해제

`Connect`는 저장이나 tool 실행이 아니다. capability/version을 확인하고 연결 상태만 만든다.
modern은 `server/discover`, legacy는 initialize와 initialized notification까지만 수행한다.

성공 화면은 다음 safe metadata를 보여 준다.

- selected era와 negotiated protocol version
- server name/version의 bounded self-reported display value
- tools/resources/prompts capability 유무
- legacy session 사용 여부만 표시하고 session value는 표시하지 않음

### 3.2 Explore

capability가 있는 section만 활성화한다.

- Tools: `tools/list`, schema-driven argument editor, explicit `tools/call`
- Resources: `resources/list`, explicit `resources/read`
- Prompts: `prompts/list`, argument editor, explicit `prompts/get`

list는 한 page씩 읽는다. `nextCursor`가 있으면 `다음 페이지`를 사용자가 눌러 진행한다.
기존 page를 replace하지 않고 connection-local bounded list에 append하며 duplicate identity는
diagnostic error다. backend는 직전 응답의 exact cursor만 다음 요청에 허용하고, 결과를 반영하는
connection-state lock 안에서 이를 다시 atomically revalidate해 concurrent pagination race를
막는다. 이미 사용한 cursor 또는 cycle도 거부한다. raw cursor는 다음 요청을 위한 memory
state/typed field에만 있고 result와 timeline에는 `[PRESENT]`로 표시한다. 자동 무한 pagination은
하지 않는다.

tool call, resource read, prompt get은 항상 별도 버튼이다. list/import/connect만으로 실행하지
않는다. tool description과 annotation은 untrusted display text이며 동작을 바꾸는 authority가
아니다.

### 3.3 Timeline

각 operation은 bounded in-memory timeline을 갖는다.

- outgoing request: sequence, time offset, method, request ID, safe name/URI만 표시
- incoming notification: method와 bounded masked payload
- incoming response/error: matching ID, status/error code, bounded masked payload

성공한 protocol exchange만 timeline으로 반환한다. 취소·timeout·transport 종료는 원문 event를
만들지 않고 고정 error code로 표시한다. Auto fallback 성공 시 최종 선택 era와 실제 legacy
exchange를 표시하며, 실패한 modern HTTP body나 header는 timeline에 반향하지 않는다.

Authorization/custom header/session ID, endpoint query credential, tool/prompt arguments,
pagination cursor, raw HTTP header, OS/parser error는 timeline에 원문으로 넣지 않는다. 응답의
value뿐 아니라 object key, server metadata, capability, actionable list definition에 반사된 known
credential도 IPC 전에 제거한다. Timeline은 localStorage, History, Collection, log, integration
snapshot, telemetry에 저장하지 않는다.

## 4. Architecture

```text
ProtocolLab.tsx
  ├─ pure schema/list/timeline state (bounded, memory-only)
  └─ api.ts typed IPC
       ├─ connect/disconnect
       └─ invoke/cancel
            ↓
commands/mcp.rs
  ├─ one bounded connection registry
  ├─ existing environment reference + DPAPI resolution
  ├─ modern/legacy negotiation
  ├─ request ownership/cancellation
  └─ reqwest JSON/SSE transport
            ↓
core/mcp.rs
  ├─ JSON-RPC envelope validation
  ├─ era-specific request construction
  ├─ tool/resource/prompt projection
  ├─ JSON/schema bounds and redaction
  └─ pure fixture tests
```

The frontend never receives resolved environment secrets, custom header plaintext marked sensitive,
legacy session ID, raw response headers, redirect locations, or native error strings.

## 5. Native state and ownership

The backend owns a maximum of eight connection records, but the initial UI opens one. A record has:

- opaque 128-bit connection ID
- validated endpoint and resolved headers in process memory
- redactor containing request/environment secret values
- selected era and protocol version
- safe server capability projection
- optional legacy session ID in a zeroizing value
- exact next-cursor/used-cursor set과 callable tool/prompt definition
- frontend가 생성하고 backend가 재검증하는 bounded request ID
- request ID에 묶인 active cancellation sender, connection당 최대 네 개

Connection IDs and request IDs are exact bounded tokens. Pending connect attempts also consume the
eight-slot limit, so concurrent IPC cannot bypass it. Connection and active-request reservations
are RAII guards and are released on success, failure, cancellation, timeout, or drop/unwind. A
request for another connection or an old generation cannot cancel or mutate the current one. The
exact pagination cursor is revalidated atomically when the bounded list result is committed, so a
concurrent request cannot reuse or skip the cursor through a check-then-update race. Disconnect
first cancels owned requests, then attempts legacy session DELETE, then drops all in-memory data.
Modern disconnect only cancels/drops; there is no protocol session to terminate.

## 6. Version and era selection

### 6.1 Auto modern probe

Send `server/discover` as a modern request with:

- body `_meta.io.modelcontextprotocol/protocolVersion = 2026-07-28`
- clientInfo `{ name: "devbox-api-playground", version: app version }`
- bounded clientCapabilities object
- `MCP-Protocol-Version`, `Mcp-Method`, `Accept`, `Content-Type` derived headers

A valid DiscoverResult selects modern. A recognized `UnsupportedProtocolVersion` error is trusted
only when its `data.requested` exactly matches the version sent by this client and its bounded
`data.supported` list contains a locally implemented version. Protocol Lab currently implements one
version per era, so it does not send an unknown or merely advertised revision. In `auto` mode only,
an internally consistent response that excludes `2026-07-28` but explicitly advertises implemented
`2025-11-25` selects the legacy initialize flow as version negotiation. A response that also claims
the requested modern version is supported is inconsistent and fails closed.

### 6.2 Legacy fallback

Heuristic fallback is allowed only when the auto probe produces the official non-modern boundary:
an unrecognized legacy-shaped response/error or an eligible 400/404/405 without a recognized modern
JSON-RPC error. The explicit advertised-version selection in §6.1 is a separate negotiation path.
Network failure, TLS failure, timeout, malformed oversized input, credential failure, and recognized
modern validation errors do not trigger heuristic fallback. A malformed envelope that still contains
a recognizable modern error code is not trusted for negotiation, but remains sufficient evidence to
suppress a heuristic downgrade.

Legacy connection sends `initialize` using `2025-11-25`, validates response ID/version/capabilities,
captures an optional safe ASCII session header, then sends `notifications/initialized`. If the
handshake fails after a session was assigned, the client best-effort sends legacy session DELETE
before returning the fixed error. All later POSTs include negotiated `MCP-Protocol-Version` and the
session header when assigned. After initialization, a legacy response may omit the originally
assigned header, but it may not change that value; if initialization assigned no session, a later
response may not introduce one. Modern responses may not contain a session header at all.

Explicit `modern` or `legacy` mode never silently changes era.

## 7. Streamable HTTP contract

- endpoint must be absolute HTTP(S), have a host, and contain no userinfo, fragment, control or
  credential-shaped query key
- request redirects are disabled; a redirect is a safe fixed error, not an automatic credential hop
- derived MCP headers cannot be overridden by custom headers
- POST `Accept` always lists JSON and SSE; body is one JSON-RPC message
- modern requests mirror `method` in `Mcp-Method`; call/read/get also mirror name/URI in `Mcp-Name`
- non-ASCII or unsafe Mcp-Name values use the specification base64 sentinel encoding
- tool `x-mcp-header` is validated only on statically reachable primitive properties; invalid
  annotations exclude that tool and produce a bounded warning
- response content type must be JSON or SSE; decoded payload is bounded before deserialization
- JSON success response ID must exactly match; under the official modern schema, a protocol error
  may omit `id`, but an idless error is accepted only for a recognized modern error code (including
  recognized method-not-found evidence). Unrelated response/request envelopes fail closed
- modern discover/list/read responses require their official non-negative `ttlMs` and
  `cacheScope`; call/read/get content shapes are checked before IPC
- SSE comments are ignored; notifications may precede exactly one final matching response
- modern cancellation drops the request-scoped response stream
- legacy cancellation also sends `notifications/cancelled` for the exact request ID when possible;
  이 best-effort notification은 2초 안에 끝낸다
- a modern response `MCP-Session-Id` is rejected; after legacy initialization, a changed or newly
  appearing response session is rejected. A session-bound legacy HTTP 404 invalidates the local
  connection and returns `mcp_connection_stale`; the side-effecting request is never auto-replayed
- modern HTTP JSON-RPC errors map to stable UI codes: `-32022` to `mcp_version_unsupported`,
  `-32021`/`-32601` to `mcp_capability_unavailable`, `-32020` to `mcp_message_invalid`, and
  other recognized response errors to `mcp_server_error`

The client does not implement backward-compatible legacy SSE resumption or the legacy GET listener
in this release. It may identify that a target appears legacy/deprecated and show an actionable
diagnostic.

## 8. JSON, schema, and projection limits

| Boundary | Limit |
|---|---:|
| endpoint | 8 KiB |
| custom header rows / total | 100 / 128 KiB |
| single header name/value | 256 B / 64 KiB |
| derived parameter header rows / value / total | 100 / 64 KiB / 128 KiB |
| Environment input total | 1 MiB |
| request JSON | 1 MiB |
| response JSON/SSE decoded | 4 MiB |
| JSON depth / nodes | 64 / 20,000 |
| string/key | 256 KiB / 4 KiB |
| timeline events / bytes | 1,000 / 4 MiB |
| list pages / retained items / retained bytes per kind | 100 / 10,000 / 16 MiB |
| name/URI/cursor | 1 KiB / 8 KiB / 4 KiB |
| tool schema depth/nodes/properties | 32 / 10,000 / 2,000 |
| active connections / requests per connection | 8 / 4 |

Schema form supports object properties with string, integer, number, boolean, enum, nested object,
and bounded arrays. Required fields are marked. A root `$schema` string is supported as metadata and
remains visible in the callable projection; nested `$schema` is view-only and therefore disables the
callable form. `$ref`, composition, conditional, and unknown valid JSON Schema 2020-12 keywords
remain viewable in a read-only JSON fallback; they are never guessed. Known-secret redaction preserves
legitimate `password`/`token` property names in callable tool schemas, while reflected credential
strings or keys are rejected before IPC.
Before call, the complete argument object is checked against the supported projection. Unsupported
schema does not make the tool executable through a misleading partial form. Prompt names and their
required/optional string arguments are likewise captured only from validated list pages and checked
again at the native boundary before `prompts/get`.

## 9. Error contract

Native errors exposed to UI are stable codes only:

```text
mcp_invalid_profile
mcp_secret_unavailable
mcp_connection_limit
mcp_connect_timeout
mcp_transport_failed
mcp_redirect_blocked
mcp_response_type_invalid
mcp_request_too_large
mcp_response_too_large
mcp_message_invalid
mcp_version_unsupported
mcp_capability_unavailable
mcp_request_limit
mcp_request_timeout
mcp_request_cancelled
mcp_cursor_invalid
mcp_schema_unsupported
mcp_connection_stale
mcp_server_error
```

The UI maps these to Korean text and may display the code. No endpoint, header, session, payload,
certificate, parser input, filesystem path, or raw OS/network error is concatenated into an error.

## 10. Persistence and privacy

- No MCP response, timeline, tool argument, resource, prompt, session, cursor, or server instruction is
  written to existing REST History/Collection automatically.
- Phase 1 does not persist connection profiles. Header reference names may be copied manually, but
  resolved values are never returned.
- Clipboard and export actions are absent in phase 1. A later export must define a separate sanitized
  document contract before implementation.
- Browser preview performs no MCP network request and clearly reports `native_required`.
- No telemetry or cloud service is introduced.

## 11. Verification

Pure Rust fixtures cover:

- official modern discover/list/call/read/get shapes and required headers
- legacy initialize/initialized/session sequence
- recognized modern/advertised legacy negotiation versus eligible heuristic fallback
- JSON and SSE response, notification ordering, mismatched ID, missing final response
- pagination cursor sequence/cycle, duplicate item, item/page/count/byte bounds
- header injection, unsafe URL/query, derived header override, reflected-secret redaction
- x-mcp-header validation and base64 sentinel encoding
- cancellation/timeout and stale request ownership

Native loopback fixtures expose the same Streamable HTTP endpoint shape used by the official MCP
Inspector examples and compare observed requests. Frontend tests cover connection state, capability
gating, one-page-at-a-time pagination, explicit call/read/get, schema fallback, timeline bounds,
cancellation, and native-required browser behavior. Keyboard focus visibility is implemented in the
panel styles and remains part of packaged-app acceptance rather than the automated test claim.

Focused MCP Rust tests — **33 passed** across pure protocol/schema and
command/loopback/state/transport coverage. Full API Playground Rust tests — **133 passed**. API
Playground frontend tests — **33 files / 231 tests passed**. `cargo check`, strict Rust Clippy with
warnings denied, and the API Playground production build passed. These are app/source checks only;
no full workspace CI or Windows packaged acceptance result is claimed here.

## 12. Follow-up boundaries

### PR 2 — stdio and OAuth

- structured executable/argv/cwd/environment references; no shell
- owned process tree, bounded newline JSON-RPC stdout, bounded stderr ring
- modern stdio discover probe and legacy fallback
- OAuth protected-resource/auth-server discovery, PKCE, issuer/resource validation
- localhost/HTTPS callback, DPAPI token storage keyed by issuer/resource/client, explicit revoke

### PR 3 — gRPC and TLS/mTLS

- detailed contract: `2026-08-30-api-playground-grpc-tls.md`
- local `.proto` import and reflection with explicit source provenance
- unary, server/client/bidirectional streaming with bounded messages and cancel
- TLS server-name/CA profile and DPAPI-backed client credential references
- secret-safe history/export designed as a separate versioned contract
