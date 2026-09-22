# API Studio control admission — #574 stage 4

- Branch `fix/devbox-api-studio/control-admission`, base `1882c249` after #577 merged.
- Scope: R5, unused stderr retention, HTTP timeout semantics. Native registry names
  Normal/Control explicitly; all HTTP/MCP HTTP/MCP stdio/OAuth/gRPC/SSE/WebSocket
  cancel/stop/disconnect/close methods have eight bounded control slots separate
  from 64 normal operations. Webhook stop/quit and migration cancel are included.
  Existing component/route/session/argument authorization, replay and lifetime
  reservation remain mandatory. Arbitrary cancel prefixes or wrong owners gain nothing.
- MCP stdio had no stderr consumer. Remove its unused chunk-redacted ring and drain
  into a 4 KiB zeroizing buffer cleared after every read; no stderr is published.
  Retain stdout redaction and process-tree cleanup. Replace ring-only tests with a
  backpressure regression using split secret bytes and output beyond pipe capacity.
- Document execution timeout: multipart body construction, redirects and response body share
  a monotonic budget; template/environment/secret resolution, request validation and multipart
  path/metadata preflight precede it. Protocol deadlines differ.
- Regression implementation complete: explicit command-class/owner/quota/replay tests;
  hosted native fixture fills 64 ordinary dispatcher slots using real pending OpenAPI
  HTTP requests, checks exact overload, then cancels each protocol and checks request,
  socket/HTTP2 stream or owned child retirement before releasing the load. MCP stdio
  obtains its executable through the actual native picker; no renderer path-ID injection
  or test command is added. OAuth is cancelled during discovery before browser launch.
  All fixture servers/processes run only inside the disposable hosted runner.
- Added fixture paths to native workflow triggers. Pending full local validation,
  hosted CI/native execution (including the new fixture), PR and merge. No detailed
  validation ran during individual feature implementation.
- Final local `pnpm verify:all` PASS: 1,925 frontend and 2,701 Rust tests,
  builds/typechecks/check/Clippy/fmt. Separate native workflow metadata/contract
  checks and changed JS syntax checks PASS under the shared resource supervisor.
  Hosted protocol saturation and native picker execution remain pending gates.
- CI `35706862423` PASS. Native `35706862392` passed its build/install/restore/
  packaged shell/Knowledge checks, but the new API fixture crashed on an unhandled
  socket `ECONNRESET` during cancellation (Node error event; no protocol-level verdict).
  Handle expected reset/broken-pipe retirement, retain unexpected socket faults as
  failures, and persist per-control progress. Add a real loopback TCP-reset regression
  to the native workflow. No production code changed after the full local/CI PASS.
- Focused fixture recheck PASS: real TCP reset no longer crashes the observer,
  unexpected socket errors remain failures, and workflow/syntax contracts pass.
  Each native control also requires its resource still alive after saturation and
  before dispatch, preventing an earlier failure from being counted as cancellation.
  Preserve unchanged production/frontend/Rust PASS results; final hosted gates pending.

- Final source cross-check clarified that multipart file canonicalization/metadata preflight
  precedes the deadline, while body construction is inside it. Documentation diff only; no
  production or fixture behavior changed. Supersede the in-progress gate before its costly
  product build so final acceptance uses the corrected documentation head.
