# API Playground WebSocket Request

## Overview

Issue #296 adds a native-first WebSocket request workflow to `api-playground`.
The panel reuses the current request's URL, parameters, headers, cookies, auth, and
environment resolution, then exposes explicit Connect, Send, Ping, Close, Disconnect,
and binary-save actions. Native desktop transport uses `tokio-tungstenite` with rustls
native roots; browser preview uses the standard WebSocket API with its capability
limits clearly shown.

The implementation is intentionally limited to the WebSocket scope in P2-07. It
coexists with the already merged SSE (#295) and OpenAPI import (#293) paths without
changing their behavior, and does not add Socket.IO, STOMP, or GraphQL subscriptions.
No external relay or hosted service is required.

## Context

Before this change, API Playground handled HTTP, multipart/cookies, OpenAPI, GraphQL,
and SSE requests but had no interactive WebSocket transport or bounded message log. A native
implementation was needed for custom handshake headers, auth, cookies, and `ws`/`wss`
TLS verification without browser CORS limitations. The webview must not receive
resolved credentials, raw filesystem paths, raw errors, or unbounded frame data.

## Changes Made

### Native transport and bounded core

- `apps/api-playground/src-tauri/src/core/websocket.rs`
  - Added pure WebSocket message kinds/directions, payload validation, RFC close code
    and reason checks, UTF-8-safe preview truncation, and oldest-first retention.
  - Enforced 4 MiB message/frame, 125-byte ping/pong, 123-byte UTF-8 close-reason,
    10,000-message, and 20 MiB buffer limits.
- `apps/api-playground/src-tauri/src/commands/websocket.rs`
  - Added the native session state machine and bounded command queue.
  - Uses opaque numeric session IDs (`ws-<number>`) and fixed state/message event DTOs.
  - Resolves request templates through the existing backend environment/secrets path,
    validates `ws`/`wss` endpoints, rejects userinfo/fragments/credential-shaped query
    keys, validates header/cookie/parameter/auth/timeout bounds, and ignores transport-
    derived handshake headers.
  - Supports text, binary, ping, pong, close, peer-ping auto-pong, connection states,
    and explicit native binary save through the Tauri file picker. File bytes are
    written outside the async executor with the shared atomic sibling-replace helper.
  - Keeps raw payloads in a bounded process-local buffer only. Text, close reasons,
    binary UTF-8/hex previews, and errors pass through redaction/fixed-message gates.
  - Added loopback handshake/text/binary, path/session, bounds, header, and masking tests.
- `apps/api-playground/src-tauri/src/commands/request.rs`
  - Exposed only crate-local request resolution/redaction helpers needed by the sibling
    WebSocket command and added binary secret detection.
- `apps/api-playground/src-tauri/src/core/mod.rs`,
  `apps/api-playground/src-tauri/src/commands/mod.rs`,
  `apps/api-playground/src-tauri/src/lib.rs`
  - Registered the core module, commands, managed session state, and Tauri invoke
    handlers.
- `apps/api-playground/src-tauri/Cargo.toml`, `Cargo.lock`
  - Added `tokio-tungstenite 0.30.0`, `futures-util 0.3.31`, and Tokio sync support.
  - TLS uses `rustls-tls-native-roots`; no certificate-verification bypass is exposed.
- `apps/api-playground/src-tauri/capabilities/default.json`
  - Granted only the dialog save permission required for explicit binary export.

### Frontend API, projection, and lifecycle

- `apps/api-playground/src/lib/websocket.ts`
  - Added endpoint/query/header/auth/close validation, UTF-8/hex/base64 conversion,
    secret masking (including sensitive JSON fields and known token patterns), preview
    projection, and a shared 10,000-message/20 MiB oldest-first buffer.
  - Browser raw binary entries are removed by exact message ID when retention evicts
    them, preventing stale in-memory save handles.
- `apps/api-playground/src/api.ts`
  - Added native event parsing with an allowlist, byte/format-bounded preview fields,
    and a bounded pre-session event queue.
  - Added native/browser WebSocket handles with explicit send/ping/close/save/stop
    operations. Browser preview rejects secret environments, custom headers, cookies,
    auth, and direct ping/pong because the browser API cannot safely provide them.
  - Browser binary reads stop after close/stop, close reasons are UTF-8 bounded, and a
    request-timeout timer closes sockets that remain in `CONNECTING`.
- `apps/api-playground/src/App.tsx`
  - Added generation and sequence guards so stale sessions and out-of-order events
    cannot overwrite the current UI.
  - Added explicit lifecycle cleanup, bounded UI retention, safe WebSocket error display,
    and handlers for all panel actions without persisting frames to History/Collection.
    Terminal events clean listeners/network even when they arrive before handle setup,
    while retaining the bounded save handle until the next connection or unmount.
- `apps/api-playground/src/types.ts`
  - Added WebSocket connection, message, input, and fixed event envelope types.
- `apps/api-playground/src/WebSocketPanel.tsx`, `apps/api-playground/src/App.css`
  - Added the accessible connection/status controls, payload encoding controls, live
    message log, bounded-retention indicator, preview-truncation indicator, and explicit
    binary save buttons. Rendered payloads remain normal React text nodes (no HTML
    injection path); status and log use polite live regions.
- `apps/api-playground/src/lib/websocket.test.ts`,
  `apps/api-playground/src/WebSocketPanel.test.tsx`,
  `apps/api-playground/src/api.websocket.test.ts`,
  `apps/api-playground/src/api.websocket.native.test.ts`,
  `apps/api-playground/src/App.websocket.test.tsx`
  - Added endpoint, encoding, close, bounds, masking, eviction, control-state,
    accessibility, save-action, text-escaping, browser timeout, native event boundary,
    early-terminal cleanup, and post-terminal binary-save coverage.

### Documentation and dependency inventory

- `apps/api-playground/README.md`
  - Documented the WebSocket wire contract, native TLS/header/auth behavior, browser
    limitations, redaction/path/error boundary, retention limits, and Windows smoke.
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`
  - Recorded the P2-07 #296 implementation draft and explicit exclusions.
- `docs/roadmap.md`
  - Added the #296 draft status without folding in adjacent SSE/OpenAPI work.
- `THIRD_PARTY_NOTICES.md`
  - Regenerated the locked dependency inventory and Cargo.lock digest.

## Dependency, security, and design decisions

| Required field | Evidence |
| --- | --- |
| Purpose | `tokio-tungstenite 0.30.0` provides native `ws`/`wss` handshake and frame I/O so desktop requests can use custom headers/auth/cookies without browser CORS. |
| Alternatives | A browser-only client cannot attach custom handshake headers and has CORS/runtime limits; an external relay or sidecar would violate the native-first/offline boundary. |
| Source | Official repository and registry source: `https://github.com/snapview/tokio-tungstenite` / crates.io; transitive `tungstenite` is from `https://github.com/snapview/tungstenite-rs`. |
| Pin | Manifest pins `tokio-tungstenite = 0.30.0` and lockfile resolves `0.30.0`; `futures-util` is pinned to `0.3.31`. |
| License | `tokio-tungstenite` is MIT; `tungstenite` is MIT OR Apache-2.0. `cargo deny --locked check` and regenerated `THIRD_PARTY_NOTICES.md` cover the complete locked graph. |
| Size | Native target cache measured `5.8G` after test/check/clippy; frontend production output contains an `index` bundle of 469.59 kB JS and 22.34 kB CSS before gzip. The target value is a build-cache measurement, not installer size. |
| Security | Rustls native roots keep certificate verification enabled. URL/userinfo/fragment/control/credential-query, header/cookie/auth, frame/message, close, preview, path, command-queue, and retention limits fail closed; event errors are fixed allowlisted strings and raw secrets/paths never cross the webview. |
| Offline | The native transport is bundled in the app and has no relay or sidecar. Browser preview is optional and uses the standard WebSocket API; custom headers/auth/cookies and direct ping/pong are disabled there. |
| Maintenance | Track tokio-tungstenite/tungstenite advisories through the existing cargo-deny and notices gates. Rollback removes the WebSocket command/module, panel/API helpers, dependency entries, capability, and docs while leaving HTTP/GraphQL isolated. |

## Code Examples

### Fixed native event projection

```rust
// apps/api-playground/src-tauri/src/commands/websocket.rs
pub struct WebSocketUpdate {
    pub session_id: String,
    pub kind: &'static str,
    pub sequence: u64,
    pub dropped: usize,
    // Only redacted, bounded text/hex fields follow.
}
```

### Stale-event and byte-bounded UI handling

```tsx
// apps/api-playground/src/App.tsx
if (update.sequence <= webSocketSequenceRef.current) return;
webSocketSequenceRef.current = update.sequence;
webSocketBufferRef.current.push(message);
setWebSocketMessages([...webSocketBufferRef.current.messages]);
```

## Verification Results

All commands were run from the dedicated worktree on
`feat/api-playground/websocket-request` with the requested native target directory,
`CARGO_INCREMENTAL=0`, and cargo build jobs limited to 2 where applicable.

### Rust app checks

```text
cargo test --manifest-path apps/api-playground/src-tauri/Cargo.toml --lib
82 passed; 0 failed

cargo check --manifest-path apps/api-playground/src-tauri/Cargo.toml --lib
Finished `dev` profile

cargo clippy --manifest-path apps/api-playground/src-tauri/Cargo.toml --lib -- -D warnings
Finished `dev` profile

cargo fmt --all -- --check
exit code: 0
```

### Frontend app checks

```text
pnpm --filter api-playground exec tsc --noEmit
exit code: 0

pnpm --filter api-playground test
25 test files passed; 175 tests passed

pnpm --filter api-playground build
vite production build completed successfully
```

### Dependency and repository gates

```text
pnpm install --frozen-lockfile                         passed
pnpm audit --audit-level moderate                      No known vulnerabilities found
python3 .github/scripts/check-dependencies.py check    dependency policy OK
python3 .github/scripts/test-check-dependencies.py     regression tests passed
cargo deny --locked check                              advisories/bans/licenses/sources OK
python3 .github/scripts/test-build-manifest.py         notice tests passed
bash .github/scripts/check-catalog.sh                  passed
git diff --check                                       passed
```

The app-only native target occupied approximately `5.8G` after test/check/clippy. This is a
diagnostic cache measurement, not a packaged application size.

## Remaining Windows smoke

The following require the Windows packaged desktop environment and remain release-gate
checks: `ws://` and `wss://` loopback handshake with native-root TLS verification,
custom header/auth/cookie handshake behavior, text/binary/ping/close lifecycle, binary
file-picker save, unsafe endpoint/path/generic-error behavior, reconnect/stale-session
handling, and keyboard/screen-reader inspection of the native UI. These are deliberately
not replaced by a browser or external-service test.
