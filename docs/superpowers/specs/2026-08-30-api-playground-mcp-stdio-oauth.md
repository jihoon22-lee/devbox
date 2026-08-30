# API Playground MCP stdio + OAuth security contract

**Date:** 2026-08-30  
**Milestone:** v0.6.0  
**Issue:** [#485](https://github.com/jihoon22-lee/devbox/issues/485)  
**PR boundary:** Protocol Lab PR 2 — stdio transport and HTTP OAuth

## 1. Outcome

Protocol Lab adds two deliberately separate capabilities:

1. A native MCP stdio client that starts one reviewed executable without a
   shell, speaks bounded newline-delimited JSON-RPC, and owns the complete
   process tree until disconnect, cancellation, timeout, or failure.
2. An OAuth 2.1 authorization grant for MCP HTTP profiles that discovers and
   validates the protected resource and authorization server, uses an external
   browser with PKCE S256, and persists only DPAPI-sealed tokens.

OAuth is never applied to stdio. The MCP authorization specification defines
authorization for HTTP transports; stdio credentials come only from explicit
environment-reference bindings. The renderer never receives an executable
path chosen by the native dialog, resolved environment plaintext, an access or
refresh token, a DPAPI blob, or a callback authorization code.

## 2. Authoritative protocol baseline

- MCP transports and transport security:
  [2026-07-28 transports](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports)
- MCP newline-delimited stdio framing and lifecycle:
  [2026-07-28 stdio](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/stdio)
- MCP HTTP authorization:
  [2026-07-28 authorization](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization)
- Authorization-server discovery:
  [MCP discovery](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization/authorization-server-discovery)
- MCP client registration options:
  [MCP client registration](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization/client-registration)
- MCP authorization threats:
  [security considerations](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization/security-considerations)
- OAuth protected-resource metadata: [RFC 9728](https://www.rfc-editor.org/rfc/rfc9728)
- Authorization-server metadata: [RFC 8414](https://www.rfc-editor.org/rfc/rfc8414)
- PKCE: [RFC 7636](https://www.rfc-editor.org/rfc/rfc7636)
- Authorization response issuer: [RFC 9207](https://www.rfc-editor.org/rfc/rfc9207)
- Resource indicators: [RFC 8707](https://www.rfc-editor.org/rfc/rfc8707)
- Native app browser and loopback redirect: [RFC 8252](https://www.rfc-editor.org/rfc/rfc8252)
- Token revocation: [RFC 7009](https://www.rfc-editor.org/rfc/rfc7009)

The already shipped HTTP implementation retains its modern-first
`server/discover` negotiation and bounded legacy initialize fallback. Stdio
uses the same protocol builders and projections but owns an independent
transport lifecycle.

## 3. Non-goals for this PR

- OAuth device authorization, client credentials, password grant, embedded
  webviews, or accepting a client secret from the renderer.
- Client ID Metadata Documents or Dynamic Client Registration. v0.6.0 accepts
  a user-supplied, pre-registered public client ID. A hosted HTTPS CIMD cannot
  be truthfully provided by a local-only desktop app, and DCR materially
  expands the credential lifecycle.
- OAuth for stdio, WSL-hosted stdio, shell command strings, terminal emulation,
  or an interactive stdin console.
- Persisting stdio process profiles, stdio environment values, protocol
  timelines, raw stderr, OAuth callback data, or OAuth server error text.
- Background token refresh. Refresh occurs only at the next explicit connect
  or request boundary and only for the exact stored grant binding.
- gRPC, custom CA, TLS profiles, and mTLS. Those remain Protocol Lab PR 3.

## 4. Threat model and invariants

### 4.1 Untrusted inputs

Treat executable output, stderr, MCP JSON, OAuth metadata, HTTP headers,
redirect callback parameters, token responses, environment rows, and all
renderer-supplied IDs as untrusted.

### 4.2 Fixed invariants

- No shell is started. The executable and every argv value are passed as
  distinct native process arguments.
- Native selections are backend-owned, opaque, single-purpose, short-lived,
  and bounded. IPC exposes only a selection ID and display label.
- A selected executable must be an existing regular file. A selected cwd must
  be an existing directory. Both are canonicalized and revalidated immediately
  before spawn; symlinks/reparse aliases may not change the approved identity.
- Child environment begins with `env_clear()`. Only a fixed runtime allowlist
  plus explicit source-name-to-child-name bindings is restored.
- Secrets are resolved only at the spawn or HTTP request boundary into
  zeroizing backend memory. They never enter argv, timeline, persisted profile,
  diagnostic text, or IPC.
- One stdio connection owns one process tree. A request cancellation or timeout
  invalidates and terminates that connection so a late response cannot be
  mistaken for a later request.
- OAuth endpoints never follow redirects. Tokens are sent only in an
  `Authorization: Bearer` header to the exact validated resource origin, never
  in query parameters.
- Every grant is bound to exact normalized issuer, resource, and client ID.
  Refresh, injection, and revocation fail closed on a binding mismatch.
- Stable error codes cross IPC. Raw OS, network, executable, metadata, header,
  token, callback, and server error strings do not.

## 5. Native selection boundary

`McpNativeSelectionState` stores at most 32 selections for at most ten minutes.
The commands are:

```text
pick_mcp_stdio_executable() -> Option<McpNativeSelection>
pick_mcp_stdio_cwd()        -> Option<McpNativeSelection>
```

`McpNativeSelection` contains:

```text
selectionId: 128-bit lowercase hex
kind: executable | directory
label: final file/directory name, control-free and capped at 256 bytes
expiresAtMs: display-only deadline
```

Selection consumption does not permanently reveal or persist the path. A
connection snapshot retains the canonical backend path for its lifetime, but a
new connection after expiry requires a new selection. Executable and cwd IDs
are not interchangeable. Unknown, expired, duplicated, malformed, changed, or
wrong-kind selections fail with a stable code.

## 6. Stdio profile and environment

Renderer input:

```text
McpStdioProfile {
  executableSelectionId,
  cwdSelectionId?,
  era: auto | modern | legacy,
  args: string[],
  environment: { childName, sourceName }[],
  timeoutMs
}
```

Bounds:

- 64 argv values; 8 KiB per value; 64 KiB total.
- 64 environment bindings; 256 bytes per name; 256 KiB resolved total.
- Environment names use the portable `[A-Za-z_][A-Za-z0-9_]*` subset.
- Duplicate child names or source names, missing source variables, controls,
  NUL, empty values, and secrets referenced by argv are rejected.
- Timeout is 100 ms to 120 seconds.

The inherited allowlist is intentionally narrow:

```text
Windows: PATH, PATHEXT, SYSTEMROOT, WINDIR, COMSPEC, TEMP, TMP
Unix:    PATH, HOME, TMPDIR, LANG, LC_ALL, LC_CTYPE
```

Explicit bindings overwrite allowlisted values only when their child name is
different from protected runtime names. The resolved value may be empty only
when the source environment explicitly contains an empty non-secret value.

WSL stdio is excluded from v0.6.0. A Windows Job Object can own `wsl.exe`, but
cannot prove ownership of a Linux daemonized descendant after the launcher
returns. The UI says “native executable” and does not imply WSL process-tree
cleanup.

## 7. Stdio process ownership

### 7.1 Spawn

The backend uses `tokio::process::Command` with piped stdin/stdout/stderr,
`kill_on_drop(true)`, exact argv, and no console window.

- Windows: create with `CREATE_SUSPENDED | CREATE_NO_WINDOW`, create a
  kill-on-close Job Object, assign the root, verify exactly one active process,
  find exactly one primary thread, then resume once. Any failure kills and
  reaps the still-suspended root before returning.
- Unix test/runtime boundary: place the child in a private process group before
  spawn. Disconnect sends TERM, waits briefly, then KILLs the group and reaps
  the root. A malicious descendant that calls `setsid()` remains an explicitly
  documented OS authority limitation.

Closing or dropping the connection retains the Job/group authority until the
root is reaped and the owned tree is observed empty. The app's window exit path
also drops every connection through the same owner.

### 7.2 Framing

- Exactly one UTF-8 JSON-RPC message per line.
- A line may end in LF or CRLF; the terminator is removed before JSON parsing.
- Embedded newline in one outbound JSON document is forbidden.
- Maximum line and parsed JSON size: 4 MiB.
- Empty stdout lines, non-UTF-8, non-JSON stdout, oversized lines, duplicate
  response IDs, or an unexpected response ID invalidate the connection.
- stderr is never parsed as protocol. It is drained concurrently into a
  64 KiB/256-line zeroizing ring, with controls removed and known resolved
  values redacted. IPC receives only a bounded safe count/summary, not raw
  stderr.

The Code Pad LSP `Content-Length` transport is not reused for framing. Only its
no-shell/process-tree ownership patterns are applicable.

### 7.3 Negotiation

- `modern`: send modern `server/discover`; do not fall back.
- `legacy`: perform legacy `initialize`, version selection, and `initialized`.
- `auto`: start a fresh process and try modern. Only an unrecognized-method
  response or the modern discovery compatibility timeout permits fallback.
  The modern process is terminated and reaped before a fresh legacy process is
  spawned. Network/spawn/framing/malformed/credential failures never fall back.

The stdio adapter reuses the existing `core/mcp.rs` request builders,
capability gates, list/schema validation, safe result projection, pagination
budgets, and modern/legacy contracts.

### 7.4 Request, cancel, timeout, disconnect

- At most 8 live stdio connections.
- One active request per stdio connection. The UI already exposes a single
  active operation; serialization avoids response ownership ambiguity.
- The response reader is listening before stdin is written.
- A user cancellation sends `notifications/cancelled` best-effort, then closes
  stdin and terminates/reaps the entire connection.
- A timeout follows the same invalidating cleanup path.
- Explicit disconnect closes stdin, waits a bounded grace period for normal
  exit, then terminates/reaps the full tree.
- EOF before the matching response is a stable transport failure. A normal
  root exit still triggers descendant cleanup.

## 8. OAuth grant model

### 8.1 Profile

`McpHttpProfile` gains optional `oauthGrantId`. A profile cannot provide both
an OAuth grant and an enabled custom `Authorization` header. The endpoint URL
continues to be the OAuth resource used for the MCP request.

The renderer-visible grant projection contains only:

```text
grantId, issuer, resource, clientId, scopes[], expiresAtMs?, status
```

No token, callback code, verifier, state, DPAPI envelope, discovery body, raw
header, or storage path is serializable to IPC.

### 8.2 Authorization request

```text
authorize_mcp_http(requestId, endpoint, issuer?, clientId, scopes[])
cancel_mcp_oauth(requestId)
list_mcp_oauth_grants()
revoke_mcp_oauth_grant(grantId, removeLocalOnRemoteFailure)
```

Input bounds include an 8 KiB endpoint/issuer/client ID, 32 scopes, 256 bytes
per scope, and one active authorization flow. Client IDs are public identifiers
and may be HTTPS URLs; they are not treated as secrets. Client secrets are not
accepted.

### 8.3 Discovery and binding

1. Validate the MCP resource endpoint and HTTPS policy. Loopback HTTP is
   permitted for local fixtures; non-loopback HTTP is rejected.
2. Probe the resource without credentials. Parse a bounded
   `WWW-Authenticate` challenge and RFC 9728 protected-resource metadata.
   Try the path-specific well-known location before the origin-root fallback.
3. Require the metadata `resource` to equal the normalized requested resource.
4. If multiple authorization servers are advertised, require the user-selected
   issuer to match one exact entry. Never guess by origin similarity.
5. Perform RFC 8414 authorization-server discovery, using the MCP-defined
   OAuth metadata then OIDC discovery order. Require exact issuer equality.
6. Require HTTPS authorization and token endpoints except loopback fixtures.
   Require `S256` PKCE support and public-client token authentication `none`.
7. Reject metadata redirects, cross-origin substitutions, duplicate fields,
   oversized bodies, controls, credentials in URLs, fragments, and unsupported
   response or grant types.

### 8.4 Browser and callback

The backend binds an ephemeral loopback port before constructing the
authorization URL. It generates a cryptographically random state and PKCE
verifier, derives the S256 challenge, adds `resource` to the authorization
request, and opens the system browser through the existing opener plugin.

The callback listener:

- accepts only loopback peers and one bounded HTTP request;
- requires the exact callback path and GET method;
- rejects duplicate `code`, `state`, `iss`, or `error` parameters;
- validates exact state before any exchange;
- validates `iss` when supplied and requires it when advertised by metadata;
- returns a fixed local success/failure page without reflecting parameters;
- discards and zeroizes the authorization code immediately after exchange.

No embedded webview is used. Cancellation closes the listener and zeroizes the
state/verifier/code without changing stored grants.

### 8.5 Token exchange, refresh, and injection

The token request uses an exact form POST with authorization code, redirect
URI, client ID, PKCE verifier, and the same `resource`. Redirects are disabled.
The response is bounded before JSON decoding. Bearer is the only accepted token
type for v0.6.0.

Access and refresh tokens are held in `Zeroizing<String>`. Expiry uses a
backend monotonic safety margin for the live session and a wall-clock timestamp
only for persistence/display. At connect/request time:

- a non-expired exact-binding access token is injected as a derived Bearer
  header;
- an expired grant with a refresh token is refreshed once with the exact issuer,
  resource, and client binding;
- refresh rotation is persisted atomically before the old refresh token is
  discarded;
- a failed or malformed refresh never falls back to a different grant and
  returns a stable reauthorization-required code.

The injected token is added to the existing MCP redactor before any response,
timeline, header projection, error mapping, or IPC projection is constructed.
OAuth never overrides a user-defined Authorization header silently.

### 8.6 Persistence and revocation

Store at most 32 grants in a versioned JSON file under the app-local data
directory. The app directory and file are revalidated against symlink/reparse
redirection. Writes use `devbox_filesystem::atomic_write`.

Each access and refresh token is independently wrapped with the existing
versioned `devbox_secrets` envelope and DPAPI sealer. The file stores base64
ciphertext plus non-secret binding metadata. Non-Windows builds return a stable
unsupported-storage error and are used only for pure logic tests.

Revocation posts the selected token to the exact discovered revocation endpoint
when present. The UI distinguishes remote revocation success, remote failure,
and deliberate local-only removal. A network failure does not silently erase
the only refresh token unless the user explicitly chose local removal.

## 9. Stable IPC errors

Stdio:

```text
mcp_stdio_selection_invalid
mcp_stdio_profile_invalid
mcp_stdio_environment_invalid
mcp_stdio_spawn_failed
mcp_stdio_transport_failed
mcp_stdio_protocol_invalid
mcp_stdio_message_too_large
mcp_stdio_request_timeout
mcp_stdio_request_cancelled
mcp_stdio_connection_stale
mcp_stdio_cleanup_failed
```

OAuth:

```text
mcp_oauth_required
mcp_oauth_request_invalid
mcp_oauth_discovery_failed
mcp_oauth_resource_mismatch
mcp_oauth_issuer_mismatch
mcp_oauth_pkce_required
mcp_oauth_client_unsupported
mcp_oauth_callback_failed
mcp_oauth_token_failed
mcp_oauth_storage_failed
mcp_oauth_reauthorization_required
mcp_oauth_cancelled
mcp_oauth_revoke_failed
```

Errors never include an input, URL, path, header, process output, token, code,
state, verifier, client ID, scope, server description, or OS diagnostic.

## 10. UI contract

- Protocol Lab has an HTTP/stdio transport switch. Changing transport while
  connected requires disconnect and clears transport-specific volatile state.
- Stdio uses native “Choose executable” and optional “Choose cwd” buttons,
  structured argv rows, structured environment bindings, and an explicit
  native-process warning. Raw path text fields do not exist.
- The existing tools/resources/prompts explorer, pagination, schema form,
  cancellation, and bounded timeline are reused after connection.
- HTTP profiles show “Authorize”, stored grant selection/status, and “Revoke”.
  OAuth controls are disabled for stdio.
- The current “memory-only” wording is split: protocol request/result timelines
  remain memory-only; OAuth tokens are encrypted with Windows DPAPI and persist
  until revoked or removed.
- Browser preview remains `native_required` for stdio and OAuth actions.

## 11. Test matrix

### Pure/backend

- Profile, argv, environment-name, bounds, duplicate, and selection-expiry
  validation.
- LF/CRLF framing, split reads, malformed/non-UTF-8/oversized stdout, wrong or
  duplicate IDs, bounded stderr, and redaction.
- Modern, legacy, and eligible/ineligible auto-fallback fixtures.
- Cancellation, timeout, EOF, normal exit, tree cleanup, and stale connection.
- Windows Job Object assignment/cleanup compile tests and native fixture tests.
- RFC 9728/8414 URL construction and bounded metadata parsing.
- Exact issuer/resource/client binding; PKCE S256, state, callback, `iss`, and
  duplicate-parameter rejection.
- Token bounds/type/expiry, exact Bearer injection, refresh rotation, atomic
  store round-trip, malformed DPAPI blob, grant cap, revoke outcomes, and no
  secret IPC serialization.

### Frontend

- HTTP/stdio switching and cleanup.
- Native selection cancellation/expiry/error states.
- Structured argv/environment editing and validation.
- Stdio connection/invoke/cancel/disconnect using the shared explorer.
- OAuth authorize/cancel/grant selection/expiry/revoke states.
- Memory-only versus DPAPI persistence disclosure.
- No raw paths, token fields, or callback values in rendered state.

### Release acceptance

- Packaged Windows native stdio fixture with spaces and Unicode in executable
  and cwd; modern/legacy negotiation; tools/resources/prompts; cancel/timeout;
  Task Manager confirmation that no child remains.
- OAuth loopback fixture with the real system browser, exact discovery/binding,
  refresh rotation, restart persistence, revoke, cancelled callback, and
  negative redirect/issuer/resource/PKCE cases.
- Official MCP Inspector fixture comparison for shared protocol behavior.

## 12. Implementation order

1. Add pure stdio profile/framing validation and process-tree owner.
2. Add native selection state and stdio lifecycle commands.
3. Adapt existing MCP negotiation and explorer operations to stdio.
4. Add pure OAuth metadata/PKCE/binding/store model.
5. Add browser callback, token exchange/refresh/revoke, and HTTP injection.
6. Wire the frontend transport and OAuth controls.
7. Run focused tests, full workspace DoD, Windows CI, and the release acceptance
   matrix. Version changes remain centralized in release issue #493.
