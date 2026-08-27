# API Playground SSE Streaming Workthrough

- Date: 2026-08-27
- Issue: #295 `feat(api-playground): SSE streaming`
- Branch: `feat/api-playground/sse-streaming`
- Initial base: `2c20eb55109253aca411e7cb0449bd3b4590158e` (`origin/main` at start)
- Final parent: `64234e1cb969409de95e0ce64c67eda8d736e519` (`origin/main` after rebase,
  including OpenAPI #432 and the Code Pad race fix #433)
- Target: API Playground / v0.5.0 P2-06
- Status: implementation, direct pre-PR review and Linux Rust/frontend validation complete;
  GitHub Actions and Windows W2 evidence remain release gates

## Outcome

API Playground now has a bounded native-first SSE stream path alongside the existing REST request
surface. A user can run a GET or POST request with the existing auth, environment, ordered header,
Cookie, parameter and body draft, observe events incrementally, pause rendering, stop the network
task, and optionally enable a capped reconnect policy. The browser preview uses the same protocol
parser and masking rules while clearly retaining browser CORS, forbidden-header and redirect
limitations.

The implementation keeps request resolution and secret release in the existing Rust boundary. The
native task exposes only an opaque session ID and a fixed, masked event DTO. No URL, local path,
request credential, raw chunk, redirect detail, parser error or network stderr crosses into the
frontend, log, history or telemetry. Stream events are retained only in bounded process memory;
the only outward action is an explicit copy of the masked visible range.

## Scope

### Included

- GET and POST SSE requests using the current RequestTemplate/editor
- existing environment reference, Basic/Bearer/API-key auth, ordered headers, Cookies, params and
  JSON/form/raw/multipart text body handling
- native Rust task and Tauri event bridge with one active stream and opaque session IDs
- browser Fetch preview with CORS/forbidden-header/redirect limitations
- incremental UTF-8 parser in Rust and TypeScript
- BOM, CR/LF/CRLF, comments, `event`, multiline `data`, `id`, empty id, `retry` and EOF flush
- fixed malformed-input behavior for invalid UTF-8, NUL, retry grammar and all parser bounds
- connect, idle and total timeout bounds
- opt-in reconnect with bounded retry delay and attempt count
- pause-rendering versus stop-network semantics
- 20 MiB decoded stream and 10,000 event/20 MiB oldest-first retention bounds
- visible 1,000-row UI cap and eviction count
- event metadata/status, fixed errors, aria live log, keyboard/native input behavior and focus-safe
  controls
- explicit masked event clipboard copy without automatic history/export
- parser, buffer, command, native loopback and event-bridge fixtures
- API Playground README, roadmap, native-first plan and this workthrough

### Excluded

- WebSocket, Socket.IO, STOMP or GraphQL subscriptions
- arbitrary reconnect configuration or automatic reconnect (reconnect is opt-in and capped)
- `Last-Event-ID` replay or user-controlled event ID forwarding
- server-side event persistence, automatic History/Collection storage, telemetry or raw export
- remote URL import, SSE service discovery and external sidecar/runtime downloads
- native certificate-verification bypass
- browser file multipart and browser response-cookie access

## Existing Boundary and Design Decisions

The existing REST flow already owned RequestTemplate resolution, DPAPI unseal, multipart file
preparation and response/body redaction. Reusing those helpers keeps SSE from creating a second
secret or request semantics. The small visibility changes in `commands/request.rs` expose only
`pub(crate)` helpers to the sibling SSE command; ResolvedRequest is still backend-only and is never
serialized.

```text
RequestTemplate + environment
        |
        | validate raw rows, method, body, URL and options
        v
Rust resolve_template + secret unseal (memory only)
        |
        | validate resolved values, cookie/multipart/files, build Redactor
        v
opaque native task / browser resolved Fetch request
        |
        v
bounded SSE parser -> redacted event DTO -> generation/session UI guard
```

The native command owns a single active `SseState`. `reserve` happens after request validation and
before spawn; `attach` records the abortable task handle; `finish` releases the slot only for the
matching session; `cancel` removes and aborts only the matching task. A failed attach cleans the
reservation. Stop is idempotent for a stale but syntactically valid session ID and rejects IDs that
could carry a URL/path or arbitrary input.

## Protocol Parser Contract

`apps/api-playground/src-tauri/src/core/sse.rs` and `apps/api-playground/src/lib/sse.ts` are
dependency-free parser modules. Both preserve incomplete UTF-8 across arbitrary input chunks and
apply the same field and memory limits. Both treat the first UTF-8 BOM as transport metadata,
support CR, LF and CRLF, ignore comment lines and unknown fields after applying line/field bounds,
and dispatch only when a blank line closes an event. The final unterminated line and event are
flushed at EOF.

`data` lines append a newline and remove only the final separator on dispatch. `event` uses the
last value in an event block and otherwise defaults to `message`. An empty `id` clears the stored
last-event ID and is not emitted as a credential-bearing value. `retry` accepts decimal digits only
and is capped; a `retry` field without an event still updates reconnect state. Neither parser
forwards `Last-Event-ID`.

| Value | Bound/behavior |
|---|---|
| decoded stream | 20 MiB, hard failure rather than truncation |
| line and field | 64 KiB UTF-8 bytes |
| event name | 256 UTF-8 bytes |
| event data | 1 MiB UTF-8 bytes per event |
| event ID | 256 UTF-8 bytes; NUL rejected |
| retry | decimal 0–60,000 ms |
| retained history | 10,000 events or 20 MiB, oldest-first eviction |
| browser visible rows | most recent 1,000; retention continues while paused |

Malformed UTF-8 is never replaced with U+FFFD. Native `ParseError` and browser `SseParseError`
contain only fixed messages/codes; raw line, field, source URL, path and parser-runtime text are
not included.

## Native Transport and Redirect Policy

`commands/sse.rs` builds a reqwest client with the requested connect timeout and disabled automatic
redirects. It validates environment count/key/value bounds, URL scheme/host/userinfo/fragment,
combined URL size, header/value sizes, auth kind/header syntax, body kind, body size, multipart
bounds and file path size before starting the task. `Accept` is always `text/event-stream`; a
user-provided Accept or Last-Event-ID header is
not reused. A successful response must be 2xx with media type `text/event-stream`.

Native redirect handling is explicit and capped at 10 redirects per session. Each location is
validated for scheme, host, userinfo, fragment and combined URL size. A cross-origin redirect
removes sensitive headers/auth and the request body; a destination with credential-shaped URL
content is rejected before following. POST-to-GET redirect rules follow the existing request
contract. Transport, status, content-type, timeout and redirect failures become fixed stream
messages only.

The task applies idle timeout to each response chunk and one total deadline across redirects,
reconnects and parsing. A cancelled task does not emit a terminal raw error. Native events are
redacted by the existing Redactor, then checked again against event bounds before emitting:

```text
{ sessionId, kind, event?, data?, id?, retryMs?, sequence, dropped, attempt?, message? }
```

The DTO contains no URL, headers, body, path, response status text, raw chunk or error source.

## Browser Preview

Browser preview resolves only non-secret environment values. It validates the same method, URL,
header/Cookie/parameter/body/auth and multipart bounds, uses `redirect: "error"`, applies local
connect/idle/total timeout handling and streams `Response.body` through the TypeScript parser.
Browser Cookie behavior remains subject to the Fetch forbidden-header implementation; file parts
and per-part Content-Type are rejected with a fixed desktop-only message. Browser network failures,
abort, parser failures and response-type failures are converted to fixed messages.

Browser reconnect is disabled by default and capped at five attempts when enabled. Server retry is
clamped to 250 ms–60 s, `retry` fields without dispatched events still update the next delay, and
no Last-Event-ID is ever sent. A shared abort signal stops both fetch and reconnect sleep. The
browser event path applies request-secret, sensitive-key and known-token masking before retention
and callback delivery; JSON-shaped event data is sanitized by key as well as exact secret value.

## UI, Async and Accessibility

`App.tsx` adds an SSE control row with Start/Stop, explicit reconnect opt-in and bounded timeout
inputs. Start and Send are mutually exclusive while a stream is active. A generation ref and
terminal-generation guard reject events or handles from a stopped, replaced, stale or unmounted
stream. The native listener is installed before invoke and queues only a small bounded pre-session
window; events for another session are discarded. Stop aborts the current handle and invalidates
the generation before awaiting it.

The viewer displays masked event name, id, retry and data as React text nodes in a `role="log"`
with polite live updates. Pause disables only rendering; the bounded history and eviction count
continue to advance. `Copy masked events` is disabled while copying or with no events and catches
clipboard failures with a fixed message. Labels, status/live regions, `aria-label`s, disabled
states, native inputs and normal keyboard/IME behavior are retained; no key handler intercepts
composition or text editing.

## Privacy and Persistence Review

- Secret environment variables are unsealed only in native memory for the request task.
- Direct sensitive header/Cookie/parameter/auth/body/query values seed the existing Redactor.
- Browser additionally masks sensitive JSON keys and common token prefixes.
- Session IDs are generated locally and accepted only in a bounded opaque format.
- Native and browser callbacks carry fixed fields only; no raw transport stderr/error is reflected.
- Events are not added to REST History or Collection and are never written to localStorage.
- Viewer copy is explicit and masked; there is no automatic clipboard, file save or export.
- Reconnect never imports an event ID or credential/control value into a new request.
- Request and redirect bounds prevent oversized URLs, rows, fields, body and file path values from
  being sent through this feature.

## File Changes

### Native

- `apps/api-playground/src-tauri/src/core/mod.rs`
  - register pure SSE parser module
- `apps/api-playground/src-tauri/src/core/sse.rs`
  - incremental parser, fixed errors, bounds, event buffer and unit fixtures
- `apps/api-playground/src-tauri/src/commands/sse.rs`
  - options/DTO/state, native transport, redirect/reconnect/cancel and command fixtures
- `apps/api-playground/src-tauri/src/commands/request.rs`
  - `pub(crate)` sharing of resolver, body, validation, redaction and URL helpers only
- `apps/api-playground/src-tauri/src/commands/mod.rs`, `src/lib.rs`
  - command registration and managed state
### Frontend

- `apps/api-playground/src/lib/sse.ts`
  - browser parser/buffer parity
- `apps/api-playground/src/lib/sse.test.ts`
  - parser and bound/eviction fixtures
- `apps/api-playground/src/SseEventViewer.test.tsx`
  - safe text rendering, pause state, explicit masked copy and clipboard failure fixture
- `apps/api-playground/src/api.ts`
  - native event bridge and browser streaming transport/redaction
- `apps/api-playground/src/types.ts`
  - options and safe update DTO
- `apps/api-playground/src/SseEventViewer.tsx`, `App.tsx`, `App.css`
  - controls, status, bounded viewer, pause/copy and lifecycle guards
- `apps/api-playground/src/App.contextMenu.test.tsx`
  - API mock compatibility for the new exported stream API
- `apps/api-playground/src/App.sse.test.tsx`
  - terminal listener release and active-stream unmount cleanup
- `apps/api-playground/src/api.sse.test.ts`
  - browser preflight rejection for GET multipart, file parts and per-part Content-Type

### Documentation

- `apps/api-playground/README.md`
- `docs/roadmap.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`
- this workthrough

## Fixtures and Review Vectors

Native and TypeScript fixtures cover:

- BOM split across chunks and incomplete UTF-8 code point split across chunks
- CR, LF and CRLF, comments, unknown fields and EOF without a final newline
- multiline data and event-name reset, empty id clearing, retry metadata and retry-only chunks
- malformed retry, NUL event/id, invalid UTF-8, line/field/data/name/id/stream overflow
- count and byte history eviction with cumulative dropped count
- unsafe URL userinfo/scheme, opaque session IDs, timeout/reconnect defaults, early task-finish
  reservation handling and chunk fixture
- native loopback HTTP chunked response with split UTF-8, SSE media type, fixed Accept and ignored
  Last-Event-ID request header
- request-secret redaction fixture proving event text and JSON-shaped data do not echo auth values
- fixed error behavior without source-value reflection
- a single CR separating two data lines without prematurely dispatching the event, in both parsers
- GET multipart content rejection in the raw template and resolved native request boundaries
- browser file multipart and per-part Content-Type rejection before any background fetch starts
- terminal native/browser stream updates releasing the listener handle, plus active-stream unmount

## Direct Pre-PR Review Corrections

The final review rebased the feature on the OpenAPI/GraphQL request surface and checked that REST
cancellation, GraphQL resolution/redaction, OpenAPI import controls and SSE remain independently
available. It found and corrected four boundary/lifecycle defects before the PR:

1. Both incremental parsers treated a lone CR as two line breaks when the following byte was not
   LF. The pending-CR branch now only consumes an optional LF because the CR already finalized the
   line. Rust and TypeScript parity fixtures lock the multiline behavior.
2. A terminal `closed` or `error` update cleared the React handle without invoking its idempotent
   stop/unlisten closure. Terminal updates now capture and stop the handle before discarding it;
   the component fixture also proves active-stream cleanup on unmount.
3. Browser multipart file parts were rejected only inside the asynchronously running fetch task,
   while text-part Content-Type was silently discarded. Both unsupported browser-only capabilities
   are now rejected synchronously before a stream handle/background fetch is created.
4. GET body validation inspected only the ordinary body string and could miss enabled multipart
   content. Template, resolved-native and browser validation now all treat populated multipart
   parts as a body and reject them for GET.

The review also reduced sibling-command visibility again for request helpers that SSE does not use;
only the resolver, request/body/header/redirect helpers and redactor required by SSE remain
`pub(crate)`.

The existing App context-menu fixture now mocks `startSseStream` so its persistence and focus tests
continue to exercise the unchanged REST/Collection behavior. The dedicated viewer fixture verifies
safe React text rendering, pause semantics, explicit masked copy and non-reflective clipboard
failure; the pure parser fixture remains runnable without React/JSDOM.

## Validation

Checks completed in the final rebased worktree:

```text
pnpm install --offline --frozen-lockfile
  passed: linked the existing package cache without a network download
source ~/.cargo/env
CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-issue295 CARGO_INCREMENTAL=0 \
cargo test -p api-playground -j2
  passed: 69 tests (including native loopback, parser parity, redaction, GET multipart and
  early-task lifecycle fixtures)
CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-issue295 CARGO_INCREMENTAL=0 \
cargo check -p api-playground --all-targets -j2
  passed
CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-issue295 CARGO_INCREMENTAL=0 \
cargo clippy -p api-playground --all-targets -j2 -- -D warnings
  passed
cargo fmt --all -- --check
  passed
pnpm --filter api-playground exec vitest run src/lib/sse.test.ts src/api.sse.test.ts \
  src/App.sse.test.tsx src/SseEventViewer.test.tsx --maxWorkers=2
  passed: 4 files, 14 focused SSE tests
pnpm --filter api-playground test -- --maxWorkers=2
  passed: 20 files, 160 tests
pnpm --filter api-playground build
  passed: TypeScript and Vite production build
python3 .github/scripts/check-dependencies.py check
python3 .github/scripts/test-check-dependencies.py
python3 .github/scripts/test-build-manifest.py
  passed: dependency policy, notices and manifest regression checks
bash .github/scripts/check-catalog.sh
  passed
git diff --check
  passed
```

Windows W2 remains required for packaged loopback GET/POST streaming, native/browser behavior,
cancel/reconnect, response content type, redaction and no-persistence evidence, as well as narrow
viewport, keyboard/IME and focus return behavior. Full workspace gates and GitHub Actions remain
the PR checkpoint; the feature-level Rust/frontend gates above are complete.

## Remaining Risks

- Browser Fetch cannot guarantee Cookie header transmission and remains CORS constrained.
- Native loopback, real cancellation and packaged Tauri event timing still need Windows W2 smoke
  evidence.
- The stream intentionally does not replay Last-Event-ID after reconnect, so servers requiring
  resumable delivery must be handled manually outside this feature.
