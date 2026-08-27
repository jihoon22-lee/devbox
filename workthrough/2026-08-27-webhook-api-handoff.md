# Webhook Lab → API Playground `api-request/v1` Handoff

## Overview

Issue #315 adds the first app-to-app one-time handoff on top of the protocol-v2
`crates/applink` store. Webhook Lab publishes only a backend-owned masked history
or fixture projection; API Playground claims it, shows an explicit preview, and
applies it only after the user confirms. The implementation is intentionally
bounded and does not use clipboard or temporary-file fallback.

## Context

The preceding #284 applink core and #314 Webhook Lab masked-fixture storage are
on `main`. Before this work, Webhook Lab's conversion action was disabled and API
Playground had no handoff receiver. The main risks were accidentally moving raw
Webhook credentials through a renderer/argv path, changing the request editor
before confirmation, and leaving a claimed payload stuck after cancel or a
consumer crash.

## Changes Made

### Producer: Webhook Lab

- Added `core/handoff.rs` with the strict `api-request/v1` payload shape and
  masked fixture → request conversion.
- Reads history/fixture data by opaque ID from backend-owned state; it never
  accepts URL, body, header, or filesystem path data from the frontend.
- Preserves origin-form targets, rewrites masked credential markers to exact
  environment references such as `${WEBHOOK_SECRET}`, and rejects invalid
  fixtures before publication.
- Discovers the installed API Playground capability through `crates/launch`,
  writes a 10-minute/10 MiB bounded envelope with producer and consumer IDs,
  and launches with only `kind` plus the opaque 128-bit ID in argv.
- Added history context-menu and stored-fixture actions, fixed safe errors, and
  explicit no-clipboard behavior for missing target or launch failure.

### Receiver: API Playground

- Added single-instance cold/hot AppLink delivery using a process-local pending
  slot and listener-first renderer setup.
- Added strict receiver validation for method, origin/absolute URL, headers,
  body JSON depth/node/string limits, credential references, and unsafe paths.
- Claims into native process memory; the renderer receives a preview without the
  claim token or raw secret. `적용` performs ack/delete and updates the request
  editor; `취소` performs restore.
- Maps expiry, wrong target/kind, duplicate claim, lease, storage, and corrupt
  payload failures to fixed messages that do not echo paths or payload content.

### Shared contract, catalog, and versions

- Tightened the applink raw-assignment scanner so an exact reference inside JSON
  punctuation (for example `"${WEBHOOK_SECRET}"}`) remains valid while raw
  credential assignments remain rejected.
- Kept Webhook Lab at `0.1.0`; its `0.2.0` target remains deferred to the last
  Webhook Lab feature PR or release preparation. API Playground already reached
  `0.4.0` on main before this integration, and that existing version is preserved.
- Raised `apps/catalog.json` to revision 7 and declared the API Playground
  receiver and Webhook Lab producer capability.

### Tests and documentation

- Added producer payload and shared-store claim/ack/delete coverage, receiver
  bounded/privacy/unsafe-input coverage, and API Playground UI fixtures for
  listener ordering, cold/hot delivery, preview-before-apply, cancel/restore,
  expiry, and no-clipboard behavior.
- Updated app READMEs, architecture, roadmap, Webhook Lab design, interop
  design, v0.5.0 plan, catalog/Manager expectations, and this workthrough.

### Follow-up hardening review

- Bounded every receiver entry point to the exact 128-bit lowercase hexadecimal
  handoff ID before touching process state or the native store; a poisoned
  state lock now attempts to restore a successfully claimed item instead of
  stranding it until lease expiry.
- Validated the raw absolute URL path before URL normalization, rejecting
  encoded dot-segments, a leading double separator, backslashes, and controls. The
  receiver now fails closed for credential-like field names, assignment-shaped
  raw secrets (including whitespace around `=`/`:`), JWT-shaped values, and
  shared `X-Client-Token` cases while preserving exact `${NAME}` references.
- Added an unmount race guard that restores a late native claim, cleanup
  restoration for an active preview, and a modal focus/keyboard boundary:
  initial cancel-button focus, Escape-to-cancel, Tab trapping, and restoration
  of the previously focused element.
- Added regression fixtures for bounded IDs, encoded unsafe paths, raw
  credential forms, late-claim restoration, and modal focus behavior.

## Code Examples

### Producer payload and one-time store

```rust
let payload = build_api_request_payload(&fixture)?;
let descriptor = store.create(CreateHandoff {
    kind: "api-request/v1".into(),
    source_app: "webhook-lab".into(),
    target_app: Some("api-playground".into()),
    payload: serde_json::to_value(payload)?,
}, now_ms)?;
launch_open("api-playground", &OpenRequest {
    target: descriptor.into(),
    from: Some("webhook-lab".into()),
})?;
```

### Preview apply/cancel boundary

```typescript
const preview = await claimApiRequest(handoffId);
// The editor remains unchanged while this modal is visible.
await ackApiRequest(preview.handoffId);     // Apply: consume/delete.
await restoreApiRequest(preview.handoffId); // Cancel: make pending again.
```

## Verification Results

All Rust commands use `CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-issue315`,
`CARGO_INCREMENTAL=0`, and `-j2`. Frontend tests use Vitest `--maxWorkers=2`.

### Focused verification

```text
cargo check -p webhook-lab -p api-playground -j2   PASS
cargo test -p webhook-lab -p api-playground -j2    PASS (86 tests)
cargo test -j2                                    PASS (workspace tests)
pnpm --dir apps/webhook-lab test -- --maxWorkers=2 PASS (51 tests)
pnpm --dir apps/api-playground test -- --maxWorkers=2 PASS (129 tests)
pnpm --dir apps/api-playground build               PASS
pnpm --dir apps/webhook-lab build                  PASS
pnpm build                                         PASS (17 workspace projects)
git diff --check                                   PASS
```

The follow-up focused rerun after the hardening changes also passed:

```text
cargo fmt --all -- --check                         PASS
cargo test -p applink -p webhook-lab -p api-playground -j2 PASS (144 tests)
cargo check -p applink -p webhook-lab -p api-playground -j2 PASS
cargo clippy -p applink -p webhook-lab -p api-playground --all-targets -- -D warnings PASS
pnpm test -- --maxWorkers=2 (api-playground)       PASS (131 tests)
pnpm test -- --maxWorkers=2 (webhook-lab)          PASS (51 tests)
pnpm build (api-playground)                        PASS
pnpm build (webhook-lab)                           PASS
```

### Release-gate notes

- Full workspace `cargo check`/`cargo test`, root `pnpm build`, and formatter/
  diff checks completed before the single conventional commit.
- Windows packaged launch and the installed-target cold/hot smoke test remain
  parent review/release-gate work; this draft does not push, open a PR, merge,
  or remove the dedicated worktree.

## Next Steps

- Parent agent performs core review/rebase and Windows packaged W2 verification.
- Keep Life Log → Knowledge and Webhook replay/sequence (#362) as separate
  features; do not broaden this handoff PR with those producers/consumers.
