# API Playground Protocol Lab — MCP stdio + HTTP OAuth (PR2)

## Overview

PR2 extends API Playground's Protocol Lab with two native capabilities that remain
separate at the transport and credential boundaries. MCP stdio starts one reviewed
executable without a shell, owns its process tree, and exchanges bounded
newline-delimited JSON-RPC. HTTP profiles can use an OAuth 2.1 authorization-code
grant with PKCE S256, exact protected-resource/issuer/client binding, external-browser
authorization, refresh, and explicit revoke.

The renderer receives only bounded projections and stable error codes. Stdio paths,
resolved environment values, process output, OAuth tokens, callback parameters,
DPAPI envelopes, and storage paths remain native-only. Protocol results, cursors, and
timelines remain memory-only; OAuth grant metadata is the only new renderer-visible
persistence projection. This worktree does not claim packaged Windows acceptance.

## Context and scope

The work follows issue [#485](https://github.com/jihoon22-lee/devbox/issues/485) and
the contract in
[`docs/superpowers/specs/2026-08-30-api-playground-mcp-stdio-oauth.md`](../docs/superpowers/specs/2026-08-30-api-playground-mcp-stdio-oauth.md).
It builds on Protocol Lab PR1's modern `2026-07-28` `server/discover`, legacy
`2025-11-25` initialize/session flow, capability gates, schema form, pagination
budgets, redaction, and bounded timeline.

The deliberate boundaries are:

- OAuth is available only for HTTP profiles. Stdio credentials come from explicit
  Environment source-to-child bindings and are resolved only at spawn time.
- Stdio accepts native executable/cwd selections, structured argv, and structured
  environment bindings. It does not accept a shell command string, terminal input,
  WSL-hosted stdio, or raw path text from the renderer.
- OAuth uses a user-supplied public client ID, authorization-code grant, PKCE S256,
  and a system browser. Device authorization, client credentials, password grants,
  embedded webviews, client secrets, DCR/CIMD, and background refresh are excluded.
- Stdio profiles, environment values, stderr, protocol timelines, callback data, and
  raw OAuth server errors are not persisted. gRPC, custom CA/TLS profiles, mTLS, and
  legacy SSE resumption remain later Protocol Lab boundaries.

## Implementation decisions

### 1. Native stdio selection and profile validation

`commands/mcp_stdio.rs` keeps native dialog selections behind expiring opaque IDs.
The renderer sees only a lowercase 128-bit hex ID, kind, safe basename label, and
display-only expiry. At most 32 selections live for ten minutes. Before every spawn,
the backend revalidates canonical identity and rejects missing, expired, changed,
wrong-kind, symlink, or reparse-redirected selections.

The profile is structured as executable selection, optional cwd selection, era
preference, argv rows, environment bindings, and timeout. Bounds are enforced before
any process starts:

| Boundary | Limit |
|---|---:|
| argv values | 64 |
| one argv value | 8 KiB |
| total argv bytes | 64 KiB |
| environment bindings | 64 |
| one environment name | 256 bytes |
| resolved child environment | 256 KiB |
| timeout | 100 ms–120 s |
| live stdio connections | 8 |
| active request per connection | 1 |

The child starts with `env_clear()`. Only the platform runtime allowlist is restored;
explicit bindings map an Environment source name to a distinct child name. Secret
sources are unsealed into zeroizing backend memory and seeded into the redactor, but
never enter argv, timeline, diagnostics, or IPC. Protected runtime names, duplicate
names, missing sources, controls/NUL, empty secret values, and secret references in
argv fail closed.

### 2. Process-tree ownership and stdio framing

The native process uses piped stdin/stdout/stderr, `kill_on_drop(true)`, exact argv,
and no console window. Windows starts a suspended child and assigns it to a
kill-on-close Job Object; the Unix test/runtime boundary uses a private process group.
The connection retains tree authority until the root is reaped and descendants are
gone. Cancellation sends the legacy cancelled notification best-effort, then closes
and terminates the whole connection. Timeout, EOF, framing/protocol failure, and
disconnect use the same invalidating cleanup path, so a late response cannot be
reused by a later request.

The transport accepts exactly one UTF-8 JSON-RPC message per LF or CRLF line. Empty
lines, embedded outbound newlines, non-UTF-8/non-JSON stdout, duplicate or unexpected
response IDs, and incomplete/oversized lines are rejected. An outbound request follows
the shared 1 MiB MCP request bound; each line and parsed response is capped at 4 MiB,
and one exchange is capped at 4 MiB or 1,000 messages. Stderr is never parsed as
protocol: controls are removed, known resolved values are redacted, and only a
zeroizing 64 KiB/256-line ring is drained and then cleared. Raw stderr does not cross
IPC.

`modern` sends `server/discover`; `legacy` performs `initialize` and
`notifications/initialized`. `auto` falls back only for the defined compatibility
signals (unrecognized method or a modern discovery compatibility timeout/version
response), after terminating and reaping the modern process before starting a fresh
legacy process. Spawn, network, framing, malformed-message, and credential failures
do not trigger fallback. The existing MCP builders, capability gates, list/schema
validation, result projection, pagination budgets, and timeline are reused.

### 3. OAuth discovery, browser callback, and exact binding

`core/oauth.rs` is a pure validation layer for secure URL normalization, RFC 9728
protected-resource metadata, RFC 8414/OIDC authorization-server discovery, exact
issuer/resource matching, bearer challenges, PKCE material, callback parsing, and
Bearer-only token responses. Duplicate JSON fields, unsupported token types, unsafe
URLs, controls, fragments, credentials, metadata redirects, and malformed bounded
responses produce stable errors.

`commands/mcp_oauth.rs` owns the native flow. It performs one unauthenticated resource
probe, follows the path-specific protected-resource well-known location before the
origin-root fallback, selects only an exact advertised issuer, and requires the
authorization server to advertise authorization code, public-client `none`, and
PKCE `S256`. Non-loopback HTTP is rejected; loopback HTTP is reserved for local
fixtures. OAuth clients never follow redirects.

The backend binds an ephemeral `127.0.0.1` listener before opening the system browser.
It sends `resource`, a cryptographic state value, and the S256 challenge in the
authorization request. The callback accepts one loopback HTTP/1.1 GET at
`/oauth/callback`, validates state before exchanging the code, checks `iss` when
advertised, rejects duplicate parameters and reflected values, and returns a fixed
success/failure page. State, verifier, code, and callback buffers are zeroized.

Token exchange is an exact form POST with the code, redirect URI, public client ID,
verifier, and the same resource. Only `Bearer` is accepted. Access and refresh tokens
are held in `Zeroizing<String>` values, added to the existing MCP redactor before
projection, and injected only as an `Authorization: Bearer` header for the exact
validated resource. A grant can refresh only at an explicit connect/request boundary,
with the exact issuer/resource/client binding; refresh rotation is persisted
atomically before the old refresh token is discarded. A failed refresh returns a
reauthorization error and never selects another grant.

### 4. Windows-only grant persistence and revocation

The grant store is a versioned JSON document at the app-local data directory's
`oauth/mcp-grants.json`. It accepts at most 32 grants and 1 MiB. Each access and
refresh token is independently wrapped by the existing versioned `crates/secrets`
DPAPI envelope; the file retains only ciphertext and non-secret binding metadata.
Parent/file identities are checked against symlink/reparse redirection and writes use
`devbox_filesystem::atomic_write`.

Non-Windows builds return the stable `mcp_oauth_storage_failed` boundary and are used
for pure logic tests. This is intentional: WSL cannot prove Windows DPAPI behavior.
The renderer projection contains only `grantId`, issuer, resource, public client ID,
scopes, expiry, and `active`/`expired` status. It never serializes a token, callback
code, verifier, state, DPAPI blob, discovery body, or storage path.

Revoke posts the selected refresh/access token to the exact discovered revocation
endpoint when one exists. The UI distinguishes remote revocation from local removal;
a remote failure keeps the grant unless the user explicitly chooses local removal.
An OAuth grant cannot coexist with an enabled custom `Authorization` header, avoiding
silent credential precedence changes.

### 5. Renderer transport switch and IPC defense in depth

`ProtocolLab.tsx` adds an HTTP/stdio switch. Switching while connected first
disconnects and clears transport-specific volatile state. Stdio exposes only native
“Choose executable”/“Choose cwd” controls, structured argv/environment rows, a native
process warning, and the shared explorer. HTTP exposes public client ID, optional
issuer, scopes, authorize/cancel, stored-grant selection, refresh, revoke, and the
remote-failure local-removal fallback. OAuth controls are disabled for stdio.

The native command layer enforces profile and transport bounds, while `mcpApi.ts`
revalidates every returned native projection: opaque IDs, selection kind and label,
grant metadata, timeline order, result bounds, cursor sentinel, and stable error-code
membership. Browser calls return `native_required`; they do not start MCP network or
process actions. The backend and renderer therefore share a fail-closed projection
boundary even if one side receives malformed data.

## Changes made

### Rust transport and command layer

- `apps/api-playground/src-tauri/src/commands/mcp_stdio.rs` retains the native
  selection/profile/process-tree/framing implementation and maps bounded result
  failures to the stdio stable error contract.
- `apps/api-playground/src-tauri/src/commands/mcp_oauth.rs` adds OAuth flow state,
  discovery orchestration, system-browser callback handling, token refresh/injection,
  DPAPI-backed store load/save, and remote/local revoke outcomes.
- `apps/api-playground/src-tauri/src/core/oauth.rs` adds pure URL, metadata, PKCE,
  callback, token, duplicate-key, scope, and bound validation.
- `apps/api-playground/src-tauri/src/commands/mcp.rs` adds optional OAuth grant IDs to
  HTTP profiles, exact Bearer injection, redactor seeding, refresh at connect/request
  boundaries, and the custom-Authorization conflict check.
- `apps/api-playground/src-tauri/src/commands/request.rs` lets the existing redactor
  add a newly injected OAuth token without exposing it to the renderer.
- `apps/api-playground/src-tauri/src/commands/mod.rs`,
  `apps/api-playground/src-tauri/src/core/mod.rs`, and
  `apps/api-playground/src-tauri/src/lib.rs` register the OAuth module, state, and
  Tauri commands.

### Frontend and contracts

- `apps/api-playground/src/types.ts` defines the HTTP/stdio profile, native selection,
  OAuth grant projection/status, and revoke result types.
- `apps/api-playground/src/mcpApi.ts` adds native stdio picker/connect/invoke/cancel/
  disconnect wrappers and OAuth authorize/cancel/list/revoke wrappers with projection
  validation and stable error normalization.
- `apps/api-playground/src/ProtocolLab.tsx` implements the transport switch, structured
  stdio editor, native-only warning, OAuth controls, grant status, cancellation, and
  local-removal fallback.
- `apps/api-playground/src/App.css` gives the stdio and OAuth editors responsive,
  token-based layouts while preserving the existing focus-visible boundary.
- `apps/api-playground/src/ProtocolLab.test.tsx` covers switching/cleanup, native
  selection labels and picker cancellation, structured profile submission, stdio
  explorer/cancel/disconnect, OAuth authorize/cancel, grant refresh/revoke, and the
  DPAPI persistence disclosure.
- `apps/api-playground/src/mcpApi.test.ts` covers native-only routing, malformed
  projections, stable errors, stdio payloads, OAuth payloads, grant bounds, and
  secret/path exclusion.

### Dependency and documentation record

- `apps/api-playground/src-tauri/Cargo.toml` adds `sha2` and `url`, enables `reqwest`
  form and Tokio networking features, and `Cargo.lock` records the resulting graph.
- `apps/api-playground/README.md` documents the two transport boundaries, security
  limits, stable errors, Windows-only storage, source evidence, and pending packaged
  acceptance.
- `workthrough/2026-08-30-api-playground-mcp-stdio-oauth.md` records this implementation
  and validation boundary.

## Stable error contract

The native commands and `mcpApi.ts` expose stable codes only. Raw OS/network/process
output, executable paths, undocumented input/server-error URLs, headers, server text, token values,
callback values, and storage paths are not reflected in errors or renderer state. The bounded grant
projection deliberately includes its public issuer and resource URL.

```text
mcp_stdio_selection_invalid       mcp_stdio_profile_invalid
mcp_stdio_environment_invalid     mcp_stdio_spawn_failed
mcp_stdio_transport_failed        mcp_stdio_protocol_invalid
mcp_stdio_message_too_large       mcp_stdio_request_timeout
mcp_stdio_request_cancelled       mcp_stdio_connection_stale
mcp_stdio_cleanup_failed          mcp_stdio_connection_limit
mcp_stdio_request_limit

mcp_oauth_required                mcp_oauth_request_invalid
mcp_oauth_discovery_failed        mcp_oauth_resource_mismatch
mcp_oauth_issuer_mismatch         mcp_oauth_pkce_required
mcp_oauth_client_unsupported      mcp_oauth_callback_failed
mcp_oauth_token_failed            mcp_oauth_storage_failed
mcp_oauth_reauthorization_required mcp_oauth_cancelled
mcp_oauth_revoke_failed
```

## Code examples

### Stdio profile remains structured and native-owned

```rust
// apps/api-playground/src-tauri/src/commands/mcp_stdio.rs
pub struct McpStdioProfile {
    executable_selection_id: String,
    cwd_selection_id: Option<String>,
    era: EraPreference,
    args: Vec<String>,
    environment: Vec<McpStdioEnvironmentBinding>,
    timeout_ms: u64,
}
```

The renderer sends selection IDs and binding names. The backend resolves the
canonical executable/cwd and secret environment values only immediately before
spawning the owned process.

### OAuth injection extends the existing redaction boundary

```rust
// apps/api-playground/src-tauri/src/commands/mcp.rs
if let Some(grant_id) = prepared.oauth_grant_id.clone() {
    let bearer = oauth.bearer_for(app, &grant_id, &endpoint, &mut cancellation).await?;
    prepared.apply_oauth_bearer(bearer);
}
```

`apply_oauth_bearer` adds the token to the redactor before any MCP response,
timeline, error, header projection, or IPC value is constructed.

### Renderer-visible OAuth projection excludes token material

```ts
// apps/api-playground/src/types.ts
export interface McpOAuthGrantProjection {
  grantId: string;
  issuer: string;
  resource: string;
  clientId: string;
  scopes: string[];
  expiresAtMs: number | null;
  status: "active" | "expired";
}
```

## Verification results

The source/unit evidence recorded for this worktree is:

```text
$ cargo test -p api-playground
test result: ok. 160 passed; 0 failed

$ cargo check -p api-playground
Finished successfully

$ cargo clippy -p api-playground --all-targets --all-features -- -D warnings
Finished successfully with warnings denied

$ pnpm --filter api-playground test
Test Files: 33 passed (33)   Tests: 244 passed (244)

$ pnpm --filter api-playground build
tsc and the Vite production build completed successfully
```

The Rust test suite therefore records **160 passed**, strict Rust Clippy with
warnings denied **passed**, the frontend records **33 files / 244 tests passed**,
and the API Playground production frontend build **passed**.

These are app/source evidence. WSL cannot exercise Windows DPAPI sealing, native
picker and Job Object process ownership, system-browser loopback authorization, or a
packaged Windows `.exe`. Consequently this worktree makes no Windows packaged
acceptance claim: the Windows stdio fixture (including cancel/timeout and no-child
cleanup), OAuth browser/discovery/restart/refresh/revoke matrix, negative redirect/
issuer/resource/PKCE cases, and MCP Inspector comparison remain pending.

## Follow-up boundaries

- Run the packaged Windows acceptance matrix on a real Windows runner after the source
  and unit evidence is integrated.
- Keep OAuth persistence and stdio process ownership Windows-specific; WSL remains a
  pure-logic/source-test environment for these boundaries.
- Protocol Lab PR3 remains the separate gRPC and TLS/mTLS workstream.

## Changed-file inventory

- `Cargo.lock`
- `apps/api-playground/src-tauri/Cargo.toml`
- `apps/api-playground/src-tauri/src/commands/mcp.rs`
- `apps/api-playground/src-tauri/src/commands/mcp_oauth.rs`
- `apps/api-playground/src-tauri/src/commands/mcp_stdio.rs`
- `apps/api-playground/src-tauri/src/commands/mod.rs`
- `apps/api-playground/src-tauri/src/commands/request.rs`
- `apps/api-playground/src-tauri/src/core/mod.rs`
- `apps/api-playground/src-tauri/src/core/oauth.rs`
- `apps/api-playground/src-tauri/src/lib.rs`
- `apps/api-playground/src/ProtocolLab.test.tsx`
- `apps/api-playground/src/ProtocolLab.tsx`
- `apps/api-playground/src/App.css`
- `apps/api-playground/src/mcpApi.test.ts`
- `apps/api-playground/src/mcpApi.ts`
- `apps/api-playground/src/types.ts`
- `apps/api-playground/README.md`
- `workthrough/2026-08-30-api-playground-mcp-stdio-oauth.md`
