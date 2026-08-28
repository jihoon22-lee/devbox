# Webhook Lab captured replay and response sequence

## Overview

Implemented grouped GitHub issues #362 (captured request replay) and #363 (response
sequence reset) for Webhook Lab. The feature stays native-first and local-only:
replay accepts opaque history/fixture IDs, rebuilds a masked request for the active
loopback listener, and returns only the HTTP status; response sequence state is
process-local and resettable. The resilience audit also replaced the previous
third-party listener parser with a bounded native HTTP/1.x transport so malformed,
slow, or oversized clients cannot hold lifecycle shutdown indefinitely.

## Context

The existing app could store masked captured fixtures and hand them off to API
Playground, but it could not send a capture back through its local listener or
model retry/settle response behavior. The implementation keeps the existing
localhost/LAN warning, history/fixture bounds, redaction, and fixed-error
contracts. It does not add external destinations, arbitrary scripting,
distributed state, raw-secret replay, clipboard fallback, or persisted sequence
cursors. External tools remain out of this feature boundary because the goal is
to make the repeated local workflow usable offline inside Devbox.

## Changes Made

### Bounded local replay

- Added apps/webhook-lab/src-tauri/src/core/replay.rs.
- Added a small HTTP/1.1 client with explicit loopback address parsing; wildcard
  binds map to loopback and no DNS resolution is performed.
- Rebuilds requests only from validated masked fixtures, removes caller-supplied
  transport headers, creates safe Host/Content-Length/Connection headers,
  enforces the existing body/header budgets, and uses bounded connect/read
  timeouts.
- Returns only a status code and keeps response body/parser/network detail out
  of the renderer.
- Added a process-local 20 replay / 1-second limiter and redaction, target,
  header, body, and rate-limit fixtures, plus a local TCP integration fixture
  that verifies status-only response handling.
- Uses 2-second connect/write-read idle limits and a 5-second write+response wall
  clock deadline. A cancellation flag is checked before connect and during every
  bounded write/read loop.

### Bounded native listener transport

- Added `apps/webhook-lab/src-tauri/src/core/http.rs` and removed the unused
  `tiny_http` dependency from the Webhook Lab crate/lockfile.
- The listener accepts one HTTP/1.0 or HTTP/1.1 request per connection with an
  optional fixed `Content-Length`. It rejects chunked, `Expect`, and unknown
  transfer encodings instead of attempting to drain or interpret unbounded input.
- Request line, header line/count/aggregate, body, active connection, 5-second
  wall-clock, and socket idle bounds are enforced before history admission. No
  partial body enters history; errors use fixed 414/431/413/408/429 responses.
- The listener tracks cloned active sockets. `stop_server` shuts them down before
  joining the accept/worker threads, so partial bodies, blocked writes, and
  artificial response delays are interruptible.
- The 64-connection overflow path uses a short 100ms write budget for its fixed
  503 response, so a client that will not read cannot stall the accept loop on
  the normal 5-second response timeout.
- The response writer owns `Content-Length`/`Connection` framing, filters `Host`
  and all transport headers, and applies the same ASCII and aggregate bounds as
  response-rule validation. Oversized direct response calls fail before writing.

### Response sequence and reset

- Extended core/rules.rs with bounded ResponseSequenceStep data and a
  maximum of 16 additional steps.
- The existing rule response is step zero; later steps are consumed in order
  and the final step is held. ResponseSequenceState keeps the cursor in
  memory only and supports deterministic reset.
- Rule validation and collection metrics cover every step, and edits/deletes
  clear the affected cursor.
- Registered replay_history, replay_fixture, and reset_rule_sequence Tauri
  commands in src-tauri/src/lib.rs.

### Frontend/API

- Added typed replay/reset APIs and desktop-only replay behavior in
  apps/webhook-lab/src/api.ts.
- Added masked replay actions to history and fixture rows, a replay context-menu
  action, response-step editor controls, sequence count badges, and per-rule
  sequence reset actions with shared busy/focus/stale guards.
- Async start/stop, rule/fixture/history mutation, handoff, draft, replay, and
  clipboard paths now guard late results and errors with the mount state. Stale
  history context targets fail closed just like stale rules, and the unmount
  test confirms a late masked-copy result cannot write to the clipboard.
- Added browser/frontend mocks and focused tests for replay, fixed errors,
  bounded sequence editing, reset, context-menu access, and focus restoration.
- Mirrored sequence validation in src/lib/ruleValidation.ts.

### Changed file inventory

- `Cargo.lock`
- `apps/webhook-lab/README.md`
- `apps/webhook-lab/package.json`
- `apps/webhook-lab/src-tauri/Cargo.toml`
- `apps/webhook-lab/src-tauri/tauri.conf.json`
- `apps/webhook-lab/src-tauri/src/commands.rs`
- `apps/webhook-lab/src-tauri/src/core/fixtures.rs`
- `apps/webhook-lab/src-tauri/src/core/mod.rs`
- `apps/webhook-lab/src-tauri/src/core/replay.rs`
- `apps/webhook-lab/src-tauri/src/core/rules.rs`
- `apps/webhook-lab/src-tauri/src/lib.rs`
- `apps/webhook-lab/src/App.css`
- `apps/webhook-lab/src/App.test.tsx`
- `apps/webhook-lab/src/App.tsx`
- `apps/webhook-lab/src/api.ts`
- `apps/webhook-lab/src/lib/contextMenus.test.ts`
- `apps/webhook-lab/src/lib/contextMenus.ts`
- `apps/webhook-lab/src/lib/ruleValidation.test.ts`
- `apps/webhook-lab/src/lib/ruleValidation.ts`
- `docs/architecture.md`
- `docs/roadmap.md`
- `docs/superpowers/specs/2026-08-14-webhook-lab-design.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`
- `workthrough/2026-08-28-webhook-lab-replay-sequence.md`

## Resilience and security remediation audit

The grouped feature was audited as a complete request path rather than as two
isolated commands. The original implementation already bounded fixture
storage and replay, but the audit found that renderer-visible history still
could contain body/query/custom-header credentials, a prefix truncation could
cut a token at the visible boundary, transport response headers could compete
with server framing, and listener/replay lifecycle transitions were not fully
serialized. Those gaps are addressed in the same feature boundary.

### Capture and storage boundary

- `core/history.rs::History::push` now uses the fixture sanitizer for headers,
  query/path, and JSON/text body before the `RequestRecord` becomes a renderer
  DTO. Sensitive header names are matched case-insensitively while ignoring
  separators (`X-Access-Token`, `proxy_authorization`, and similar names), not
  just the three original header spellings.
- Large bodies are redacted before the history character/byte prefix is cut.
  This matters when a known token begins just before the display cap: the
  renderer receives a redacted prefix instead of the first part of a secret.
  The redaction pass now scans once and does not repeatedly allocate/replace
  every token candidate; JWT-like segment checks use iterators instead of a
  per-segment `Vec` allocation. Direct core callers that bypass listener
  admission are also capped at the fixture byte budget and receive a fixed
  marker rather than triggering an unbounded redaction scan.
- `core/fixtures.rs::read_raw` retains the parent ownership checks and adds a
  no-follow open of the final store component (`O_NOFOLLOW` on Unix and
  `FILE_FLAG_OPEN_REPARSE_POINT` on Windows). The post-open metadata check and
  bounded read remain in place, so a symlink/reparse swap between metadata and
  read fails closed without touching the target.
- Fixture/header validation rejects non-ASCII values when they would be sent
  as HTTP headers; sensitive values are normalized to `[REDACTED]` first so the
  existing display marker remains compatible with masked history. Corrupt,
  oversized, non-file, and link-backed stores still remain untouched.

### Listener, replay, and sequence concurrency

- `ServerState` now has a lifecycle mutex and owns the accept-thread
  `JoinHandle`. Start/stop transitions retire stale `AtomicBool` state, join
  the old thread before rebinding, clear stale addresses, and treat accept or
  handler panic as a stopped listener. `server_status` reports the atomic
  running value rather than treating an old `Option` as liveness.
- Listener admission rejects oversized method/target/header/body input before
  cloning or reading the retained snapshot. Declared lengths above the body
  bound fail before allocation, and chunked/unknown bodies are rejected rather
  than drained. The fixed responses are 414 (request line), 431 (headers), 413
  (body), 408 (body read timeout/failure), and 429 (request window), and no
  partial body is stored.
  Artificial response delays now poll the running flag in 50 ms slices, so a
  stop does not wait for the full 60-second rule delay before joining the
  accept thread.
- Rules and sequence cursors use the same rules→cursor lock ordering for
  matched response selection, edit, delete, and reset. Native replay calls are
  serialized by a process-local replay lock so concurrent IPC actions cannot
  reorder a response sequence. Replay holds the lifecycle lock from destination
  validation through bounded I/O, checks a stop cancellation flag, permits only app-generated loopback
  addresses, requires ASCII HTTP wire fields, strips transport headers, and
  returns status only. Because the client creates three fixed transport
  headers, it reserves three listener header slots (97 input headers) and
  validates the aggregate budget again after generation.
- Rule validation rejects `Host`, `Content-Length`, `Connection`,
  `Transfer-Encoding`, and related framing headers, plus non-ASCII values that
  native transport cannot emit. This keeps saved response metadata and actual wire
  output equivalent.
- Frontend mutation, replay, handoff, draft, and clipboard paths check
  `mountedRef` after native awaits; an unmounted view cannot receive a late
  result/error or trigger a clipboard write. Existing refresh sequence and busy
  guards remain the authority for stale data, and a focused unmount regression
  test covers the late clipboard result.

### Verification evidence

The initial implementation section above predates this audit. The commands
below were run after the remediation in this worktree; no workspace-wide build,
commit, push, or PR was performed here.

### Documentation and version

- Updated Webhook Lab README and design/spec contracts for #362/#363.
- Updated roadmap/architecture/native-first plan status, including the native
  transport, deadline, cancellation, and TOCTOU contracts.
- Aligned Webhook Lab package, Cargo, and Tauri config versions at 0.2.0.

## Code Examples

### Masked request reconstruction

    // apps/webhook-lab/src-tauri/src/core/replay.rs
    pub fn build_request(
        fixture: &CapturedFixture,
        server_address: &str,
    ) -> Result<(SocketAddr, ReplayRequest), ReplayError> {
        validate_fixture(fixture).map_err(|_| ReplayError::InvalidFixture)?;
        let destination = loopback_socket(server_address)?;
        // Input transport headers are discarded; fixed transport headers are
        // generated below. The command holds lifecycle_lock while send runs.
        ...
    }

### Deterministic sequence cursor

    // apps/webhook-lab/src-tauri/src/core/rules.rs
    pub fn next_response(&mut self, rule: &ResponseRule) -> ResponseSequenceStep {
        let cursor = self.cursors.entry(rule.id.clone()).or_insert(0);
        let index = *cursor;
        *cursor = cursor.saturating_add(1);
        rule.response_at(index)
    }

### Bounded request admission and cancellation

    // apps/webhook-lab/src-tauri/src/core/http.rs
    let request_line = read_crlf_line(&mut reader, MAX_REQUEST_LINE_BYTES, deadline)?;
    let (method, target, headers, content_length) =
        parse_head(&request_line, &mut reader, running, deadline)?;
    if !admit() {
        return Err(ParseError::RateLimited);
    }
    // The body is read only after fixed Content-Length admission, with both
    // a total deadline and a stop flag checked for every bounded chunk.

    // apps/webhook-lab/src-tauri/src/commands.rs
    state.replay_cancel.store(true, Ordering::Release);
    // stop_server sets cancellation before waiting for lifecycle_lock.

## Verification Results

- `source ~/.cargo/env && CARGO_TARGET_DIR=/home/jihoon/.cache/targets/webhook-362-363
  CARGO_BUILD_JOBS=1 cargo test -p webhook-lab --lib -- --test-threads=1` — passed,
  59 tests.
- `source ~/.cargo/env && CARGO_TARGET_DIR=/home/jihoon/.cache/targets/webhook-362-363
  CARGO_BUILD_JOBS=1 cargo check -p webhook-lab -j1` — passed.
- `source ~/.cargo/env && CARGO_TARGET_DIR=/home/jihoon/.cache/targets/webhook-362-363
  CARGO_BUILD_JOBS=1 cargo clippy -p webhook-lab --lib --tests -- -D warnings` —
  passed with `-D warnings`.
- `CARGO_TARGET_DIR=/home/jihoon/.cache/targets/webhook-362-363 cargo fmt --all
  -- --check` — passed after formatting.
- `pnpm --filter webhook-lab test -- --maxWorkers=2` — passed, 4 test files and
  60 tests (including the late-unmount clipboard regression). Dependencies are
  installed from the local pnpm store; no whole-workspace build was required.
- `pnpm --filter webhook-lab build` — passed (`tsc` and Vite production build).
- `git diff --check` — passed as the final whitespace check.

## Risks and follow-up

- The replay client intentionally reports status only, so response-body
  inspection remains the responsibility of the existing history/rule UI.
- The current editor exposes sequence status/body/delay; response headers remain
  backend-supported under the existing separate header-editor scope.
- The fixture CAS still combines raw-byte comparison with a process-local writer
  lock; a hostile independent process can race the compare and atomic replace,
  so a cross-process OS lock would be a future hardening step if the store ever
  becomes multi-process writable. Parent-directory replacement after validation
  is likewise an OS-level trust boundary not fully solved by app-owned path
  checks.
- The renderer does not expose a separate cancel button for replay. Backend stop
  cancellation is available and all replay I/O is bounded to 5 seconds, but a
  future UX pass could expose progress/cancel state if replay latency becomes
  material.
- Windows packaged replay, IPv6 bind smoke, and full workspace gates still need
  parent-agent/CI verification.

## Final bounded audit follow-up (2026-08-28)

This follow-up audited the candidate against the #362/#363 security and
concurrency boundaries without broadening the feature scope. The concrete
issues found and fixed were:

- The native HTTP reader used lossy UTF-8 conversion for request bodies. A
  bounded invalid-byte body could therefore expand in memory before history
  storage. Bodies are now strict UTF-8 and malformed input is rejected.
- Reference preservation previously checked only `${...}`/`{{...}}` delimiters.
  Sensitive JSON values now preserve only a complete bounded ASCII identifier;
  path and query-key placeholders fail closed. Oversized JSON-like history
  bodies are sanitized structurally (including escaped sensitive keys) before
  the visible prefix cap. A UTF-8-safe redactor regression is covered too.
- Handoff sensitive-query rewriting now uses the same substring-sensitive-name
  policy as fixture masking, so names such as `X-Access-Token` consistently
  become the explicit `${WEBHOOK_SECRET}` reference.
- Listener bind aliases are canonicalized before OS binding, so `localhost`
  cannot invoke hostname resolution. A monotonic listener generation rejects
  replay calls queued across stop/start or unexpected listener exit; the
  cancellation flag remains responsible for interrupting in-flight I/O.
- The main rule draft fields now join sequence controls in the busy lock. This
  prevents edits during an async save from being silently overwritten by the
  successful save refresh, with a frontend regression assertion.
- Replay status parsing accepts only the HTTP/1.0 and HTTP/1.1 versions emitted
  by the native listener.

Focused verification after these changes:

- `source ~/.cargo/env && CARGO_TARGET_DIR=/home/jihoon/.cache/targets/webhook-362-363 CARGO_BUILD_JOBS=1 cargo test -p webhook-lab --lib -- --test-threads=1` — passed, 64 tests.
- `source ~/.cargo/env && cargo fmt --manifest-path apps/webhook-lab/src-tauri/Cargo.toml -- --check` — passed.
- `pnpm --filter webhook-lab test -- --maxWorkers=2` — passed, 4 test files and 60 tests.

No commit, push, PR, rebase, or worktree cleanup was performed by this audit.
