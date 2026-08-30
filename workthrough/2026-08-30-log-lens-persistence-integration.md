# Log Lens persistence and Webhook Lab integration (W08 PR2)

**Date:** 2026-08-30

**Branch:** `feat/log-lens/saved-views-reconnect`

**Issue:** [#489](https://github.com/jihoon22-lee/devbox/issues/489)

## Outcome

W08 PR2 connects Webhook Lab's bounded capture inspection flow to Log Lens and
adds reusable Log Lens views without making either app a raw-log archive.
Log Lens targets version 0.2.0 and Webhook Lab targets version 0.3.0. Catalog
revision 17 declares the `webhook-log/v1` producer/consumer capability.

This is an integration contract and implementation record, not a release
record. Windows packaged acceptance for installed capability discovery,
cold/hot launch, saved-view persistence/reconnect, and the Webhook Lab → Log
Lens path remains pending.

## Implementation

### Strict app-local saved views

Log Lens stores reusable configuration in its own
`app_local_data_dir()/saved-views.json`. The schema is intentionally small:

```json
{
  "schemaVersion": 1,
  "revision": 3,
  "views": [
    {
      "name": "Errors",
      "sources": [{ "kind": "run", "sourceId": "run-manager:run-1:stdout" }],
      "filter": { "text": "error", "regex": false }
    }
  ]
}
```

Only source descriptors and filter settings are persisted. Records, cursors,
bookmarks, handoff envelopes, body previews, and raw log bytes have no field
in the document. Schema v1 accepts at most 20 uniquely named views within a
bounded document size. Every source and filter value is validated again at
the native command boundary, including sensitive-value checks.

Save and delete commands receive the caller's expected `revision`. A mismatch
is a compare-and-swap conflict and leaves the document unchanged. Valid writes
use atomic replacement and reject links/reparse points for the app-local
directory and file. A corrupt, oversized, unknown-field, unsafe, or linked
store is preserved and returns a fixed error; startup and mutation never
silently repair it by overwriting the user's file.

WSL file descriptors and `WebhookCapture` sources are deliberately rejected
from persistence. A saved view may describe reusable local files/directories,
WSL journals, Run source identities, and container source settings, subject to
the normal source validation. The Webhook capture remains a one-time source.

Loading a view changes only the source/filter configuration. Log Lens clears
the current in-memory records, cursors, bookmarks, and selection, stops follow
refresh, marks the source disconnected, and does not read immediately. The
user must choose the Korean `source 재연결` action, which starts a fresh
bounded read. This prevents a saved configuration from silently opening a
source after selection.

### Webhook Lab → Log Lens handoff

Webhook Lab publishes a separate `handoff:webhook-log/v1` capability rather
than extending the Run/WSL `log-source/v1` identity contract. The producer
reads only a capture-time-masked history record or masked fixture and builds a
strict display projection:

```json
{
  "schemaVersion": 1,
  "method": "POST",
  "target": "/hooks?[REDACTED]",
  "receivedAtMs": 0,
  "headerNames": ["Authorization", "Content-Type"],
  "bodyPreview": "[REDACTED]",
  "redacted": true,
  "truncated": false
}
```

The target is a sanitized HTTP origin-form request target, never a filesystem
path. Header values are never represented; only bounded, de-duplicated header
names are retained. The complete bounded input body is checked for sensitive
content before taking a maximum 4 KiB redacted preview. The payload has no raw
body, header value, command, environment, credential, raw log, or archive
field. Log Lens's handoff preview modal shows source identity and summary only;
it never shows the body preview.

The producer verifies the installed Log Lens capability, publishes a 10-minute
one-time envelope through `crates/applink`, and passes only the handoff kind
and 128-bit opaque ID in AppLink argv. Log Lens validates protocol, target,
producer, source family, schema, bounds, and the response shape before it
offers the source. Preview claims remain pending until the user explicitly
chooses read-only source addition; cancel or validation failure restores the
claim. If target launch fails, Webhook Lab rechecks the immutable descriptor
and removes only the exact pending entry it just created. There is no clipboard,
temporary-file, network, or raw-archive fallback.

The resulting source is an ephemeral, read-only in-memory capture. It can be
read as a bounded Log Lens record but cannot be placed in a saved view. The
wire `displayName` is the stable English string `Webhook capture`; Korean
translation is limited to UI labels, notices, reconnect controls, and modal
copy.

### Capability and compatibility updates

- `apps/catalog.json` revision 17 declares Webhook Lab as a
  `webhook-log/v1` producer and Log Lens as its consumer/action target.
- Log Lens accepts both `log-source/v1` and `webhook-log/v1`; the existing Run
  and WSL source families remain strict and unchanged.
- Webhook Lab preserves its existing `api-request/v1` handoff separately from
  the sanitized Log Lens projection.
- The shared AppLink store remains the one-time transport. No durable
  integration snapshot or raw log archive is added.

## Safety

The feature preserves the following boundaries:

- No raw header values, raw request body, filesystem path, command,
  environment value, credential, or log archive crosses the Webhook handoff.
- Sensitive target/query/body content is redacted before truncation or
  publication; display-only redaction is not converted into a secret
  reference for Log Lens.
- Saved views contain reusable source/filter configuration only. WSL file
  paths and ephemeral Webhook captures cannot become durable view state.
- Saved-view reads and writes are bounded, strict, atomic, and no-link. A
  corrupt store is preserved for recovery instead of being replaced.
- Revision CAS prevents stale UI state from overwriting another saved-view
  mutation.
- Handoff claim/lease/ack/restore keeps the exact opaque ID. Launch failure
  cleanup is exact and fixed-error only; no sensitive storage details are
  surfaced to the renderer.
- The canonical wire source names remain English for compatibility even while
  the W08 UI is Korean.

## Test coverage and evidence

The branch contains targeted coverage for the contract, including:

- `apps/log-lens/src-tauri/src/core/saved_views.rs`: schema and configuration-
  only round trip, revision conflicts, 20-view limit, sensitive-value
  rejection, WSL/Webhook non-persistence, corrupt/unknown preservation, and
  linked-store rejection.
- `crates/applink/src/webhook_log.rs`: header-name-only projection, redaction
  before the 4 KiB preview, strict unknown/raw/path rejection, and target/body
  bounds.
- Log Lens native and frontend handoff tests: webhook source-family checks,
  preview body non-disclosure, explicit accept/cancel, claim recovery, and
  fixed error handling.
- Log Lens saved-view/API/App tests: strict response parsing, CAS request
  shape, load-to-disconnected behavior, explicit reconnect, and source
  exclusion.
- Webhook Lab API/App/context-menu tests and catalog tests: producer routing,
  opaque-only launch, browser guard, exact capability declaration, and
  catalog revision 17.

The completed local verification was:

- `pnpm --dir apps/log-lens test -- --run`: 5 files, 32 tests passed;
- `pnpm --dir apps/webhook-lab test -- --run`: 6 files, 82 tests passed;
- `cargo test -p applink -p log-lens -p webhook-lab -p catalog -j2`:
  AppLink 87, Log Lens 60, Webhook Lab 86, and catalog 11 tests passed;
- `pnpm --workspace-concurrency=2 -r build`: every workspace package/app with
  a build script passed after the strict frontend parser type narrowing was
  corrected;
- `cargo test -j2`: the complete workspace passed, with the existing physical
  WSL-interoperability-only test remaining ignored;
- `cargo check -j1` and `cargo fmt --all -- --check`: the complete workspace
  passed;
- `CARGO_BUILD_JOBS=1 cargo clippy -p applink -p log-lens -p webhook-lab -p
  catalog --all-targets -- -D warnings`: all changed Rust targets passed; and
- `bash .github/scripts/check-catalog.sh`: catalog, packaged-smoke, downloaded-
  release verifier, and installer-acceptance configuration checks passed; and
- regenerated `THIRD_PARTY_NOTICES.md`, then passed dependency policy,
  dependency-policy regression, build-manifest notice, and release-input tests.

The Windows packaged smoke configuration now pins Webhook Lab 0.3.0 and Log
Lens 0.2.0 and checks the new Korean Log Lens UI markers. These static checks
validate the release contract configuration, not physical Windows execution.

## Pending Windows acceptance

Physical Windows validation is still required for:

- installed catalog discovery at revision 17 and Webhook Lab 0.3.0 → Log Lens
  0.2.0 cold/hot AppLink launch;
- exact pending-envelope cleanup when Log Lens is absent or launch fails;
- saved-view atomic persistence, CAS conflict, corrupt-file preservation, and
  no-link behavior in the packaged app-local directory;
- load-view disconnected state and explicit `source 재연결` behavior;
- Korean UI rendering while wire `displayName` remains English; and
- confirmation that no raw body, header value, path, command, environment,
  credential, or archive is exposed or persisted.

Until those checks are complete, W08 PR2 is not a release claim.
