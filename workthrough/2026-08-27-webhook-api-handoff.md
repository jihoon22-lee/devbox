# Webhook Lab → API Playground `api-request/v1` Handoff

## Overview

Issue #315 is delivered in the same cohesive app-handoff PR as Life Log →
Knowledge #307. The two independently versioned flows exercise the same
protocol-v2 `crates/applink` claim/preview/ack/restore contract without sharing
business payloads. Webhook Lab publishes only a backend-owned masked history
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
- Renews the 60-second preview lease every 30 seconds without extending the
  10-minute envelope TTL, and restores a late or abandoned native claim when
  the renderer disappears.
- Accepts only the exact `webhook-lab` producer → `api-playground` target route;
  an otherwise valid envelope from another producer is restored and rejected.
- Maps expiry, wrong target/kind, duplicate claim, lease, storage, and corrupt
  payload failures to fixed messages that do not echo paths or payload content.

### Shared contract, catalog, and versions

- Tightened the applink raw-assignment scanner so an exact reference inside JSON
  punctuation (for example `"${WEBHOOK_SECRET}"}`) remains valid while raw
  credential assignments remain rejected.
- Kept Webhook Lab at `0.1.0`; its `0.2.0` target remains deferred to the last
  Webhook Lab feature PR or release preparation. API Playground moves from
  `0.3.2` to its planned `0.4.0` app version in this final scoped API feature.
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
  credential forms, route identity, lease renewal, late-claim restoration, and
  modal focus behavior. A fixture whose entire request path was redacted is
  rejected instead of publishing an unusable request.

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

### Grouped PR release-gate notes

- Focused `cargo check`/`cargo test`, four-app frontend tests/builds, policy,
  catalog, formatter, and diff checks are recorded at the grouped PR checkpoint.
- Windows packaged launch and installed-target cold/hot smoke remain CI/W2
  evidence. The grouped PR closes #307 and #315 together while retaining
  separate acceptance coverage for each producer/consumer pair.

### Final grouped #307 + #315 checkpoint

The shared app-handoff PR was revalidated after the final lifecycle, modal,
and response-to-input fixes. This is the authoritative Linux/WSL checkpoint
for the grouped change:

```text
cargo test -j2 -p applink -p api-playground -p webhook-lab \
  -p life-log -p knowledge-base -p catalog -p devbox-manager
                                                       PASS (477 tests)
cargo check --all-targets (same packages)              PASS
cargo clippy --all-targets -- -D warnings               PASS
cargo fmt --all -- --check                              PASS
pnpm test (api-playground, maxWorkers=1)                PASS (183 tests)
pnpm test (webhook-lab, maxWorkers=2)                   PASS (51 tests)
pnpm test (life-log, maxWorkers=1)                      PASS (47 tests)
pnpm test (knowledge-base, maxWorkers=2)                PASS (74 tests)
pnpm build (four affected apps, workspace concurrency 1) PASS
bash .github/scripts/check-catalog.sh                   PASS
python3 .github/scripts/check-dependencies.py check     PASS
dependency/build-manifest regression scripts           PASS
git diff --check                                       PASS
```

The remaining W2 evidence is limited to Windows packaged cold/hot launch,
receiver focus, and installed-target capability discovery. No browser-preview
path publishes a handoff or launches another process.

## Next Steps

- Keep Webhook replay/sequence (#362) and persistent handoff history/status
  (#353) as follow-up features; this PR contains only the two user-approved
  one-time app handoff flows (#307 and #315).
