# Developer Toolbox → API Playground text handoff

## Overview

Issue #343 adds the P3 Developer Toolbox → API Playground integration candidate.
The repository is intentionally based on `origin/main` with the existing
`api-request/v1` receiver from PR #444.  The issue's original
`toolbox-text/v1` wording is not used here: both existing Webhook Lab and the
new Toolbox producer use the canonical `api-request/v1` receiver contract.

The user-visible flow is explicit at every boundary:

1. A non-empty result surface exposes `API Playground로 보내기`.
2. Toolbox previews an editable bounded `POST /` text draft.
3. The user confirms the handoff, which creates an opaque one-time envelope
   and launches API Playground.
4. API Playground claims the envelope and previews it. `적용` only places the
   request in the editor; it never calls the request transport automatically.
5. Cancel, Escape, unmount, expiry, and failed claim paths use restore/lease
   handling already owned by the API receiver.

## Changes

### Producer: Developer Toolbox

- Added a pure `api-request/v1` payload builder with `POST`, origin-form `/`,
  `Content-Type: text/plain; charset=utf-8`, and 256,000 character / 1,024,000
  UTF-8 byte / NUL bounds.
- Added the native `create_api_request_handoff` command. It checks the catalog
  capability, writes only through `HandoffStore`, and launches API Playground
  through `crates/launch`/`crates/applink`. Launch failure revokes the exact
  still-pending envelope, so a failed delivery does not retain output until TTL.
- The native renderer string is zeroized after command completion, byte bounds
  are checked before scalar counting, and the browser preview rejects malformed
  surrogate input before IPC.
- The shared AppLink credential detector is exposed as a non-persisting text
  preflight, so obvious raw credentials are rejected before the producer clones
  text into a structured payload; the complete envelope is validated again.
- Added a reusable output action with preview/edit/confirm, fixed browser and
  native errors, and no clipboard fallback. The action keeps draft/status only
  in renderer memory and never writes input, output, secret, raw credential, or
  path values to localStorage/history/logs.
- An in-flight publish remains single-flight even if the source output changes;
  the UI does not expose the opaque handoff ID in its completion status.
- Added Toolbox dependencies on the shared applink, integration-root, and
  installed-target launch contracts.

### Receiver: API Playground

- Extended the existing `api-request/v1` route allowlist to include
  `developer-toolbox` in addition to `webhook-lab`.
- Preserved Toolbox's `text/plain` meaning when the output happens to be
  JSON-shaped, so applying the preview does not silently switch the request
  editor/transport to JSON mode.
- Kept claim tokens native-only and preserved the existing preview,
  renew, ack/delete, restore, unmount, and no-clipboard behavior.
- Gives Toolbox-originated previews a source-specific title while keeping the
  Webhook title and receiver behavior backward compatible.

### Catalog and documentation

- Raised `apps/catalog.json` to revision 10, declared Toolbox as an
  `api-request/v1` producer, and added the `send-output-to-api` action targeting
  API Playground.
- Updated the shared catalog and Devbox Manager revision assertions.
- Documented the producer/receiver contract and this workthrough.

Files changed in this candidate:

- `apps/developer-toolbox/src-tauri/src/core/handoff.rs`
- `apps/developer-toolbox/src-tauri/src/commands/handoff.rs`
- `apps/developer-toolbox/src-tauri/src/{core,commands}/mod.rs`, `lib.rs`, and
  `Cargo.toml`
- `apps/developer-toolbox/src/tools/ApiHandoffAction.tsx` and its test,
  `src/tools/common.tsx`, `src/api.ts`, `src/types.ts`, `src/App.css`, and the
  hash/HMAC output consumers
- `apps/api-playground/src-tauri/src/commands/handoff.rs`, `src/App.tsx`, and
  `src/App.applink.test.tsx`
- `apps/catalog.json`, `crates/catalog/tests/catalog.rs`,
  `apps/devbox-manager/src-tauri/src/core/catalog.rs`, and `Cargo.lock`
- `apps/developer-toolbox/README.md`, `apps/api-playground/README.md`, and
  this workthrough

## Contract examples

The native producer publishes a canonical payload through the shared store:

```rust
let payload = build_api_request_payload(output)?;
let descriptor = store.create(CreateHandoff {
    kind: "api-request/v1".into(),
    source_app: "developer-toolbox".into(),
    target_app: Some("api-playground".into()),
    payload: serde_json::to_value(payload)?,
}, created_at_ms)?;
```

The renderer can edit the body before explicitly invoking the native producer,
but it cannot supply an ID, path, target, or claim token:

```ts
const dispatch = await createApiRequestHandoff(editedOutput);
// API Playground claims and previews dispatch.handoffId; no sendRequest call.
```

## Verification

Added coverage for:

- Rust payload shape, bounds, NUL rejection, one-time ack/delete, and shared
  validator rejection before a raw credential can be persisted.
- API receiver acceptance for both Webhook Lab and Toolbox routes, including a
  text `POST /` fixture.
- Frontend preview-before-publish, edited body publication, cancel/no clipboard,
  malformed Unicode/draft fixed-error behavior, and opaque-ID-free status.
- Catalog producer/action and revision assertions.

- After integration on `main@656ba4b`, focused Rust tests passed: API Playground
  100, AppLink 60, Catalog 11, and Developer Toolbox 51 tests. These include
  exact pending-envelope revocation, credential preflight, current Collection/
  binary regressions, and both producer routes.
- API Playground's focused AppLink frontend file passed 9 tests. The final
  uninterrupted Developer Toolbox run passed 29 files/232 tests, and the handoff
  file passed 6 tests again after the TypeScript compile-only fixture correction.
- `cargo test --workspace -j1` (including doc-tests), `cargo check --workspace -j1`,
  strict Clippy for Developer Toolbox/API Playground/AppLink/Catalog all targets,
  and `cargo fmt --all -- --check` pass.
- `pnpm --workspace-concurrency=2 -r build` successfully built all 19 frontend
  projects. `git diff --check` also passes.

## Integration notes

This candidate is now integrated with #340–#342 and rebased after merged
#346–#348. Shared Toolbox command/core registration, `ToolOutput`, CSS, README,
API `App.tsx`/handoff tests, and catalog revision conflicts were resolved by
preserving both feature contracts and then re-running focused tests.

The `api-request/v1` route and one-time store remain the stable seam. No
Launcher change is included: its current static action renderer intentionally
handles only plain-text `toolbox-text/v1`; this candidate's producer action is
the Toolbox output control and its receiver is API Playground.
