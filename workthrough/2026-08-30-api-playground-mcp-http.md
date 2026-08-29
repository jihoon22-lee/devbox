# API Playground Protocol Lab — MCP Streamable HTTP (PR1)

## Overview

PR1 adds a native-only Protocol Lab workspace to API Playground for inspecting MCP
Streamable HTTP servers. It implements the modern `2026-07-28` `server/discover`
contract, the legacy `2025-11-25` initialize/session sequence, and a bounded
`auto` negotiation path that falls back only at the defined legacy boundary. The
legacy backward-compatible SSE resumption and GET listener are intentionally not
implemented in PR1.
Tools, resources, prompts, pagination, schema-driven tool arguments, cancellation,
and a redacted in-memory timeline are exposed through typed Tauri IPC. Stdio/OAuth
and gRPC/TLS remain separate follow-up boundaries.

## Context and scope

The design is tracked by issue #485 and the implementation contract in
[`docs/superpowers/specs/2026-08-30-api-playground-protocol-lab.md`](../docs/superpowers/specs/2026-08-30-api-playground-protocol-lab.md).
The immediate user flow is connect, inspect bounded capability metadata, explicitly
list one page at a time, and explicitly call/read/get an item. Connecting or listing
never executes a tool. Browser preview renders the panel but is deliberately unable
to make an MCP network request; the native Tauri backend owns transport, secret
resolution, session state, and cancellation.

The implementation does not claim a complete MCP client, a persistent MCP profile,
or a packaged Windows acceptance result. Legacy SSE resumption/GET listening, stdio
processes, OAuth callbacks/tokens, proto compilation/reflection, and TLS/mTLS
credential profiles are outside PR1.

## Changes made

### Protocol and transport core

- Added `apps/api-playground/src-tauri/src/core/mcp.rs`, a pure validation and
  projection layer with no network, process, filesystem, secret, or Tauri state.
- Modern requests carry per-request `_meta` for protocol version, client info, and
  bounded client capabilities, plus derived `MCP-Protocol-Version` and `Mcp-Method`
  headers. `tools/call`, `resources/read`, and `prompts/get` also derive a bounded
  `Mcp-Name`; unsafe/non-ASCII names use the specification's base64 sentinel form.
- Legacy connection uses `initialize` with `2025-11-25`, validates the response,
  sends `notifications/initialized`, retains only a bounded optional session ID, and
  uses a best-effort `notifications/cancelled` on request cancellation. A failed
  handshake best-effort deletes any assigned legacy session; after initialization a
  response session must not change, and a session cannot newly appear when initialize
  assigned none. Modern response session headers are rejected. Disconnect attempts
  legacy session `DELETE`; modern disconnect only cancels and drops local state.
- JSON-RPC envelopes require matching request IDs for success responses, `jsonrpc: 2.0`,
  exactly one final response, valid notification ordering, and the modern
  `resultType: complete` marker. Per the official modern schema, a protocol error may
  omit `id`, but only recognized error codes (including recognized method-not-found
  evidence) are accepted idless. JSON and SSE responses are bounded before
  projection/deserialization.
- Modern discover/list/read results also require the official `ttlMs`/`cacheScope`
  fields, while call/read/get responses validate their required content structure.
  A malformed envelope carrying recognizable modern error evidence cannot trigger
  heuristic legacy fallback.
- List results validate identity (`name`, `uri`, or `uriTemplate`), cursors, prompt
  arguments, tool schemas, and duplicate items. A connection retains at most 100
  pages, 10,000 items, and 16 MiB per list kind; updates are atomic so an overflow or
  duplicate does not partially mutate the connection.
- Pagination accepts only the exact cursor issued by the preceding page, atomically
  revalidates that cursor while committing the bounded list result to connection state
  to prevent concurrent races, rejects reused/cyclic cursors, and keeps the raw value
  only in the dedicated in-memory transport field. Result and timeline projections show
  `[PRESENT]`.
- Valid modern `x-mcp-header` annotations stay callable and are mirrored from the
  exact statically reachable primitive field. An invalid annotation excludes only
  that tool and emits a count-only warning; it does not fail unrelated tools or log
  an untrusted name/schema. The native boundary stores and validates only the same
  supported callable schema subset as the form, so direct IPC cannot bypass the
  view-only fallback. Root `$schema` metadata is supported; nested `$schema` remains
  view-only. Known-secret redaction preserves legitimate `password`/`token` property
  names in callable tool schemas, while reflected credential strings/keys are rejected.

### Native command boundary and ownership

- Added `apps/api-playground/src-tauri/src/commands/mcp.rs` and registered
  `connect_mcp_http`, `invoke_mcp_http`, `cancel_mcp_http`, and
  `disconnect_mcp_http` in `src-tauri/src/lib.rs`.
- The process-local registry accepts at most eight connections and four active
  requests per connection; pending connects reserve the same eight slots. Connection
  and request reservations use RAII guards, so success, failure, cancellation,
  timeout, and drop/unwind paths release their slots. Connection IDs are 128-bit
  OS-CSPRNG tokens, request IDs are bounded opaque tokens, and cancellation is scoped
  to the exact connection and request generation. Disconnect cancels owned requests
  before dropping the snapshot.
- `reqwest` follows no redirects. Endpoints must be absolute HTTP(S) URLs without
  userinfo, fragments, control characters, or credential-shaped query keys. Enabled
  custom headers are bounded to 100 rows/128 KiB and cannot override transport,
  protocol, session, or derived MCP headers. Timeout input is limited to 100 ms–120 s.
- Existing Environment references are resolved through the backend's sealer path.
  Unsealed values, resolved custom-header plaintext marked sensitive, session IDs,
  raw response headers, redirect locations, and native transport errors never cross
  the IPC boundary. The renderer receives stable error codes only.
- Known credentials reflected by a server in metadata, capability values/object
  keys, cursors, or actionable list definitions are redacted or excluded before IPC.
- Modern HTTP JSON-RPC errors map to stable renderer codes: `-32022` to
  `mcp_version_unsupported`, `-32021`/`-32601` to `mcp_capability_unavailable`,
  `-32020` to `mcp_message_invalid`, and other recognized response errors to
  `mcp_server_error`. A legacy session-bound 404 invalidates local connection state
  and never auto-replays the side-effecting request.

### Frontend Protocol Lab

- `apps/api-playground/src/ProtocolLab.tsx` adds the connect profile (endpoint, era,
  timeout, and existing Environment-aware header table), capability-gated Tools,
  Resources, and Prompts sections, explicit list/read/call/get actions, cancellation,
  and a memory-only result/timeline view.
- `apps/api-playground/src/McpSchemaEditor.tsx` and `src/lib/mcp.ts` implement the
  supported form subset: bounded object properties, string/integer/number/boolean,
  enum, nested objects, and bounded arrays. Root `$schema` metadata is supported,
  while nested `$schema`, `$ref`, composition, conditional, and unknown keywords fall
  back to read-only JSON and disable execution; arguments are validated again before a
  call.
- Pagination is user-driven: the next page is fetched only after the user presses
  the button, and list identity duplicates are rejected across retained pages.
  Tool descriptions and other server-provided text remain untrusted display data.
- Callable prompt definitions are retained from validated prompt-list pages and
  required/unknown argument names are rechecked natively before `prompts/get`, like
  the existing native tool-schema check.
- `src/mcpApi.ts` validates every IPC result, timeline sequence, server projection,
  cursor, and stable error code on the renderer side. `native_required` is returned
  for browser preview and unknown native messages map to a fixed transport code.

### Documentation and dependency record

- Updated `apps/api-playground/README.md` with the Protocol Lab flow, limits, and
  privacy boundary.
- Added the protocol contract and primary references to
  `docs/superpowers/specs/2026-08-30-api-playground-protocol-lab.md`.
- Added direct Cargo edges for `bytes` and `getrandom` in
  `apps/api-playground/src-tauri/Cargo.toml` and the workspace lock graph. The
  current locked versions are `bytes 1.12.1` and `getrandom 0.4.3`.
  `bytes` makes the bounded `reqwest` response-stream item type explicit; `getrandom`
  fills the 128-bit connection ID directly from the OS CSPRNG. This avoids adding a
  higher-level `rand`/UUID API or an external executable at this boundary while keeping ownership and
  failure handling local to the command boundary. The dependency-policy entry and
  generated third-party notices retain the source, license, and checksum evidence.

## Security and privacy decisions

The feature treats the MCP server and all returned descriptions, schemas, payloads,
notifications, and result data as untrusted. The main protections are:

- request JSON is capped at 1 MiB, decoded response JSON/SSE at 4 MiB, JSON depth at
  64, and JSON nodes at 20,000; display projections are bounded separately;
- timeline entries are capped at 1,000 events/4 MiB and never enter localStorage,
  History, Collection, logs, integration snapshots, or telemetry;
- outgoing tool/prompt arguments are shown as `[REDACTED]`, pagination cursors as
  `[PRESENT]`, and URI query/userinfo/data payloads are masked before timeline use;
  legitimate `password`/`token` property names in callable schemas remain usable, but
  reflected credential strings and keys are rejected;
- custom headers cannot replace derived headers, session IDs are held in zeroizing
  storage, redirect credential hops are blocked, and cancellation/timeout/transport
  failures expose fixed codes rather than endpoint, path, header, payload, or OS text.
  Modern HTTP JSON-RPC errors are normalized to stable codes; modern response session
  headers are rejected, and legacy post-initialize session changes/new appearance are
  rejected;
- connection profiles, results, cursors, and sessions have no persistence/export
  path; clipboard export and automatic MCP persistence are intentionally absent.

## Code examples

### Modern request metadata and derived headers

```rust
// apps/api-playground/src-tauri/src/core/mcp.rs
let request = build_modern_request(id, "server/discover", json!({}))?;
// Adds _meta.io.modelcontextprotocol/protocolVersion, clientInfo, and
// clientCapabilities; the command layer adds the derived MCP headers.
```

### Scoped cancellation and legacy cleanup

```rust
// apps/api-playground/src-tauri/src/commands/mcp.rs
let (connection, mut cancellation) = state.begin_request(&connection_id, &request_id)?;
let exchange = execute_message(&connection.profile, &headers, &request,
    connection.session_id.as_ref().map(|id| id.as_str()), &mut cancellation).await;
// A legacy cancellation sends notifications/cancelled for the same request ID
// within a bounded two-second best-effort window. The request RAII guard releases
// the active-request reservation on every return path.
```

### Unsupported schema remains view-only

```ts
// apps/api-playground/src/lib/mcp.ts
const analysis = analyzeMcpToolSchema(inputSchema);
// analysis.mode === "json" for $ref/composition/unknown keywords; the UI
// displays bounded JSON and disables the tool call.
```

## Official MCP primary references

These links are the primary references already listed in the protocol spec's
`Primary references` section:

- [MCP versioning — 2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning)
- [MCP Streamable HTTP transport — 2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http)
- [MCP `server/discover` — 2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28/server/discover)
- [MCP lifecycle — 2025-11-25](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle)
- [MCP transports — 2025-11-25](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)
- [Official MCP Inspector](https://github.com/modelcontextprotocol/inspector)

## Verification results

The following focused evidence was already observed for this PR1 worktree:

- Focused Rust MCP tests — **33 passed** across pure protocol/schema and
  command/loopback/state/transport coverage. This is app-focused evidence, not a
  workspace-wide Rust result.
- Full API Playground Rust tests — **133 passed** with no ignored or failed tests.
- API Playground frontend tests — **33 files / 231 tests passed**.
- API Playground `cargo check` — **passed**.
- API Playground strict Rust Clippy with warnings denied — **passed**.
- API Playground production build — **passed** (`tsc` followed by the Vite
  production build). Vite reports the existing dynamic-import diagnostic and a
  **534.92 kB** main-chunk size warning; both are non-failing follow-up optimization
  signals rather than hidden acceptance claims.
- Dependency policy/notices, release catalog, manifest/release-input regression
  scripts, `pnpm audit --audit-level moderate`, and `cargo deny --locked check` —
  **passed**. `cargo deny` reports only the repository's allowed duplicate-version
  warnings; advisories, bans, licenses, and sources are all `ok`.
- `git diff --check -- apps/api-playground/README.md
  docs/superpowers/specs/2026-08-30-api-playground-protocol-lab.md
  workthrough/2026-08-30-api-playground-mcp-http.md` — **passed** after updating these
  documents.

No full workspace build or CI result, Windows packaged run, or Windows MCP Inspector
comparison is claimed here. Those acceptance checks remain outside this focused
source/unit evidence.

## Follow-up boundaries

### PR2 — MCP stdio and OAuth

- Structured executable/argv/cwd/environment references with no shell execution.
- Owned process tree, bounded newline-delimited JSON-RPC stdout, and a bounded stderr
  ring; modern discover and legacy fallback over stdio.
- OAuth protected-resource/auth-server discovery, PKCE, issuer/resource validation,
  localhost/HTTPS callback, DPAPI token storage keyed by issuer/resource/client, and
  explicit revoke.

### PR3 — gRPC and TLS/mTLS

- Local `.proto` import and reflection with explicit source provenance.
- Unary, server/client/bidirectional streaming with bounded messages and cancellation.
- TLS server-name/CA profiles and DPAPI-backed client credential references.
- A separate versioned secret-safe history/export contract before any persistence.

## Changed-file inventory

- `Cargo.lock`
- `THIRD_PARTY_NOTICES.md`
- `apps/api-playground/README.md`
- `apps/api-playground/src-tauri/Cargo.toml`
- `apps/api-playground/src-tauri/src/commands/mod.rs`
- `apps/api-playground/src-tauri/src/commands/mcp.rs`
- `apps/api-playground/src-tauri/src/core/mod.rs`
- `apps/api-playground/src-tauri/src/core/mcp.rs`
- `apps/api-playground/src-tauri/src/lib.rs`
- `apps/api-playground/src/App.css`
- `apps/api-playground/src/App.tsx`
- `apps/api-playground/src/api.ts`
- `apps/api-playground/src/types.ts`
- `apps/api-playground/src/McpSchemaEditor.tsx`
- `apps/api-playground/src/ProtocolLab.test.tsx`
- `apps/api-playground/src/ProtocolLab.tsx`
- `apps/api-playground/src/lib/mcp.test.ts`
- `apps/api-playground/src/lib/mcp.ts`
- `apps/api-playground/src/mcpApi.test.ts`
- `apps/api-playground/src/mcpApi.ts`
- `docs/dependency-policy.md`
- `docs/superpowers/specs/2026-08-30-api-playground-protocol-lab.md`
- `workthrough/2026-08-30-api-playground-mcp-http.md`
