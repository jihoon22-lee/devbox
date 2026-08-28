# Knowledge templates and Life Log handoff workflows

## Overview

This work implements the v0.5.0 Knowledge/Life Log workflow represented by
issues #351, #352, and #353 on top of the existing #303/#305/#306/#307
contracts. Knowledge now owns local note-template CRUD and an explicit
preview-before-create path. Life Log now explains source scope/freshness and
keeps a bounded, aggregate-only history of Knowledge draft handoffs whose
cross-process lifecycle is recorded in a metadata-only status sidecar.

The work is intentionally offline and native-first: no network, cloud sync,
LLM, external database reads, or activity-body transfer was added. All writes
remain behind the existing vault, SQLite, integration, and applink boundaries.

## Context

- #351 needed four exact template variables, CRUD, preview, and no-overwrite
  application without widening the vault path boundary.
- #352 needed every source row to explain whether it is live, range-scoped, or
  an out-of-range snapshot, with bounded freshness and stable error codes.
- #353 needed durable `pending`/`sent`/`consumed`/`expired` state and
  regeneration without retaining raw sessions, project paths, vault paths,
  claim tokens, or credentials.

## Changes Made

### 1. Knowledge note templates (#351)

- `apps/knowledge-base/src-tauri/src/core/templates.rs`
  - Adds bounded template models and deterministic substitution for exactly
    `{{title}}`, `{{date}}`, `{{time}}`, and `{{vault-relative-path}}`.
  - Rejects unknown/malformed variables, controls, traversal, non-Markdown
    targets, invalid dates/times, and oversized input/output.
  - Adds substitution, invalid-date, Windows-path, and size tests.
- `apps/knowledge-base/src-tauri/src/commands/templates.rs`
  - Adds SQLite-backed list/create/update/delete commands.
  - Adds a one-slot opaque preview approval and preview discard command.
  - Preview requires an existing, unchanged vault and existing parent; save
    revalidates identity, stages a flushed sibling, publishes with no-replace
    semantics, then updates the search index transactionally.
  - Existing targets, stale vaults, index failures, and cancellation never
    overwrite or silently reuse a preview.
- `apps/knowledge-base/src-tauri/src/core/db.rs`, `core/mod.rs`,
  `commands/mod.rs`, `commands/docs.rs`, and `src-tauri/src/lib.rs`
  - Add the `note_templates` schema, module wiring, process-local preview
    state, and Tauri command registration.
- `apps/knowledge-base/src/api.ts`, `src/components/TemplateManager.tsx`,
  `src/App.tsx`, and `src/App.css`
  - Add typed IPC/browser-preview adapters and a CRUD/editor/preview UI.
  - Browser preview applies the same path, date/time, placeholder, byte, and
    control checks before keeping an in-memory mock preview.
  - Existing Knowledge API mocks include the new exports so old app-link,
    wikilink, and editor tests do not accidentally exercise native template
    state.
- `apps/knowledge-base/README.md`
  - Documents the exact variable, preview, vault, no-overwrite, offline, and
    stale/cancel contract.

### 2. Source explanation and freshness (#352)

- `apps/life-log/src-tauri/src/core/source_explanation.rs`
  - Defines `fresh` (<=2 minutes), `stale` (<=15 minutes), `expired`,
    `unknown`, and `error` display states.
  - Defines stable scopes (`live-local`, `requested-range`,
    `latest-snapshot-out-of-range`, `unavailable`) and fixed explanations.
  - Maps snapshot failures to bounded stable error codes and tests the
    boundaries, scopes, and path-free explanations.
- `apps/life-log/src-tauri/src/commands/life.rs`
  - Extends source status with freshness state, scope, error code, and
    explanation.
  - Replaces the discoverable Life Log self-snapshot row with an explicit
    `live-local` source, while retaining isolated snapshot diagnostics for
    other producers.
- `apps/life-log/src/api.ts`, `src/App.tsx`, and `src/App.css`
  - Add typed source metadata, freshness badges, scope/explanation text, and
    stable error-code rendering to Data Sources and digest source details.
- `apps/life-log/README.md`
  - Documents source freshness/error semantics and snapshot scope rules.

### 3. Persistent Knowledge draft handoff status (#353)

- `crates/applink/src/handoff.rs` and `src/lib.rs`
  - Add a versioned `status/<opaque-id>.json` sidecar containing only kind,
    source/target app IDs, lifecycle state, update time, and envelope expiry.
  - Enforce size, ID/kind/app validation, fixed expiry (no TTL extension),
    monotonic transitions, atomic writes, and a bounded cross-process OS lock
    for concurrent producer/consumer updates.
  - Add a lifecycle/no-payload/terminal-state test.
- `apps/life-log/src-tauri/src/core/handoff.rs`, `core/draft_history.rs`, and
  `core/db.rs`
  - Add a 100-entry SQLite history containing only validated aggregate
    summary/source references plus opaque ID/timestamps and regeneration link.
  - Reuse the handoff payload validators so a corrupt DB row cannot expose a
    path or secret-shaped value; terminal status regression and invalid
    regeneration references are rejected.
- `apps/life-log/src-tauri/src/commands/handoff.rs` and `src-tauri/src/lib.rs`
  - Persist pending before launch, mark sent before dispatch to close the
    claim/launch race, revert a failed launch to pending, and reconcile sidecar
    status/expiry on history reads.
  - Return `historyId`, accept `regeneratedFrom`, and expose a bounded history
    command. Missing sidecars are never treated as consumed.
- `apps/knowledge-base/src-tauri/src/commands/handoff.rs`
  - Record pending on restore/cancel, expired on TTL failure, and consumed only
    after successful file/index/ack completion; failed save paths remain
    retryable or expire safely.
- `apps/life-log/src/api.ts`, `src/App.tsx`, `src/App.css`, and
  `src/App.contextMenu.test.tsx`
  - Add history types, refresh polling, status/source summary UI, regenerate
    action, and updated handoff mocks. Regeneration always receives a new
    handoff ID and records only `regeneratedFrom`.
- `apps/knowledge-base/README.md` and `apps/life-log/README.md`
  - Document aggregate-only retention, sidecar authority, TTL, cancel,
    stale, and regeneration behavior.

## Remediation pass after the read-only audit

The initial implementation was reviewed as a complete user-flow candidate,
not only as three isolated feature additions. The audit specifically checked
whether a template, draft, or source explanation could cross an app boundary
with a secret, path, storage handle, stale result, or misleading success
state. It also checked append/overwrite behavior, bounded input/output,
offline operation, keyboard/focus accessibility, and recovery when an async
operation or process stops between two durable mutations. The following
remediation keeps the original #351/#352/#353 acceptance list above intact and
adds the missing P1/P2 safeguards.

### P1 remediation 1 — AppLink OS lock and status-file coordination

- `crates/applink/src/handoff.rs` serializes status reads, writes, producer
  compensation, and pruning with one persistent `status/.store.lock` and the
  standard library's cross-process operating-system file lock. A process exit
  releases ownership automatically; there is no wall-clock lease that a
  second process can reclaim while a paused writer still owns the old inode.
- A single store lock deliberately avoids per-handoff lock-file accumulation
  and the Unix split-brain hazard caused by unlinking a file another process
  still has locked. Acquisition is bounded, the slot is checked for a final
  symlink/reparse point before and after open, and no payload bytes enter it.
- `read_status`, `record_status`, and bounded pruning share the same critical
  section. Pruning removes only exact bytes it just inspected, so a current
  reader/writer cannot be silently replaced or deleted during refresh.
- The status machine accepts `pending → consumed` as well as the normal
  `pending → sent → consumed` path. This closes the producer/consumer race
  where Knowledge can claim a just-published envelope before Life Log writes
  `sent`.
- Focused fixtures cover cross-owner exclusion/drop release, unsafe lock
  symlinks, metadata-only status content, direct pending-to-consumed race,
  terminal regression rejection, pre-launch `sent` compensation, and a
  producer discard that leaves an already-claimed envelope/status untouched.

### P1 remediation 2 — aggregate history transaction/CAS/bounds

- `apps/life-log/src-tauri/src/core/draft_history.rs` stores at most 100
  aggregate rows. It validates opaque IDs, fixed kind, civil dates, source
  order/shape, provenance freshness (30 days), timestamps, status/expiry,
  regenerated-from identity, and all numeric bounds before SQLite writes.
- Insert duplicate detection, immutable-ID conflict detection, row insert,
  and oldest-row pruning run inside one `BEGIN IMMEDIATE` transaction. An
  exact retry is idempotent; reusing an ID for a different summary or status
  fails without replacing the original row.
- Status changes also use an immediate transaction and a conditional
  `status + updated_ts` update. A stale caller receives a CAS conflict rather
  than overwriting a newer consumer/expiry result. Negative or overflowing
  SQLite timestamps fail closed on read.
- The Life producer compensates history insertion/status failures by removing
  the exact still-pending envelope. Launch failure rechecks the authoritative
  sidecar before changing SQLite, so a concurrent consumed/expired result is
  not regressed to pending.
- Template/history table schemas carry byte/timestamp/status constraints, and
  row decoders project SQLite BLOB byte lengths before allocating Rust
  strings. Lists query one row past their 100-row retention cap and fail
  closed if an externally modified DB exceeds it; oversized JSON/template
  corruption is covered with check constraints deliberately disabled only in
  the fixture.

### P1 remediation 3 — producer/consumer error and freshness parity

- Life and Knowledge keep the same `knowledge-draft/v1` source order,
  allowlisted scopes, semver/generated-at forms, fixed error-code set, and
  30-day freshness ceiling. `snapshot_stale` is a recognized, non-reflective
  source error rather than an ad-hoc producer string.
- `source_explanation.rs` defines deterministic fresh/stale/expired/error/
  unknown boundaries (2 minutes and 15 minutes), fixed path-free messages,
  and producer-independent browser/unavailable precedence. The
  `integration-root` failure has its own fixed explanation.
- The Life source command replaces the discoverable self-snapshot with one
  synthetic `life-log / live-local` row. Other producers remain snapshot
  provenance and are never mixed into the selected activity range. Raw
  discovery strings remain diagnostic-only; the UI-facing explanation and
  error-code path are stable.

### P1 remediation 4 — handoff transition compensation and idempotence

- Life records `pending` before any launch, records `sent` before dispatch,
  and keeps a history row only while the envelope is still owned by the
  producer. Failure at each later local step removes or reverts only the
  exact mutable state; it does not delete a claimed/terminal consumer state.
- Knowledge restores a claim on preview, cancel, stale-vault, validation,
  file, index, and retryable storage failure. Save commits the note/index
  before ack; if ack fails, it removes the newly indexed note with an
  identity-checked file cleanup and restores the claim. It reports
  `handoffStatusRecorded` separately when the final status sidecar write is
  unavailable instead of claiming a fully durable lifecycle.
- Both sides treat duplicate restore/ack/terminal status calls as safe
  idempotent retries where the underlying immutable identity still matches.
  Missing status is never interpreted as consumed, and expiration is written
  only after the envelope TTL boundary.

### P1 remediation 5 — template preview TTL/revision/unmount/atomicity

- `apps/knowledge-base/src-tauri/src/commands/templates.rs` keeps a single
  process-local preview approval with a two-minute, inclusive expiry boundary.
  The approval stores template ID, updated timestamp, name/content revision,
  target, rendered content, and vault identity; only the opaque preview ID
  crosses the save command.
- Save consumes the preview first, then stages a 0600 sibling, flushes/syncs
  it, takes the DB writer lock, verifies the template definition is unchanged,
  revalidates the vault, publishes without replacement, and updates the
  index transactionally. A definition update cannot land between the final
  revision check and publication. Stale/collision/index failures clean up
  only the identity created by this attempt.
- `TemplateManager.tsx` uses mounted/request/busy/saving refs. Late native
  preview results are discarded, unmount/close best-effort discards the
  approval, cancellation failure keeps the preview visible, and a failed
  one-shot save clears the stale card so it cannot be retried deceptively.
- Native and browser preview DTOs expose the same template revision. Browser
  mode performs the same validation and consumes approval but returns
  `saved: false` because it has no vault filesystem; it never fabricates a
  file. A browser save also rejects a changed template revision.
- Template names are trimmed before persistence and compared case-insensitively
  under the immediate writer transaction. The table's `NOCASE UNIQUE`
  constraint and list-time duplicate detection keep browser/native and
  externally modified stores from presenting ambiguous definitions.
- Native rendering expands the original template in one bounded pass. A title
  or vault-relative path containing text such as `{{date}}` remains literal
  user data and cannot be recursively interpreted as another template token;
  the output limit is checked before every append.

### P1 remediation 6 — browser/native validation and truthful success

- Browser template validation mirrors native name/body/title/path/date/time,
  placeholder, control-character, byte, and output limits. It rejects
  backslashes, absolute/traversal/drive paths, invalid calendar values,
  malformed variables, and unknown template IDs before mutating mock state.
- Native commands revalidate the configured vault and parent directory at
  preview and save, never initialize a default root or `Journal` as a hidden
  side effect, and return root-relative paths only. Existing files are never
  overwritten.
- Focused API fixtures cover controls, Windows separators, year zero,
  browser preview-only success, one-shot consumption, unknown deletion, and
  changed-definition revision rejection. Native fixtures cover TTL boundary,
  revision mismatch, and one-shot semantics.

### P1 remediation 7 — Life history stale-response protection

- `apps/life-log/src/App.tsx` increments a history request generation for every
  settings load, manual refresh, and unmount. Only a mounted response whose
  generation is still current can replace `draftHistory`; digest/load/action
  invalidation remains independent so a settings refresh cannot revive old
  digest state.
- The frontend regression fixture leaves the initial settings history request
  pending, starts a newer refresh, resolves the newer result first, and then
  verifies that the older result cannot overwrite it.

### P1 remediation 8 — explanation/self-source and privacy projection

- Source explanation selection now gives browser-preview/unavailable scope
  priority over producer-specific text and uses only fixed messages. The
  integration-root path has a dedicated safe explanation. Life Log's own
  source is represented once as `live-local` rather than as a stale external
  snapshot.
- History and handoff projections preserve only aggregate values and fixed
  source references. Body/activity rows, project/vault paths, claim tokens,
  secret values, and raw OS/parser details do not enter the status sidecar or
  history DTO. New fixtures reject token-like timezone/filter/top-app values,
  unsafe path-shaped values, invalid source freshness, and unknown error
  codes.

### P2 remediation — usability, offline, and accessibility completion

- Template manager and Life export/history surfaces have labelled dialogs,
  descriptions, `aria-modal`, `aria-busy`, live status/error regions, initial
  focus, Escape handling, Tab trapping, disabled mutation controls while
  busy, and opener-focus restoration. The focus handlers use refs so async
  state changes do not tear down a live trap.
- No network, cloud service, external database, or runtime download is used
  by this workflow. Browser mode remains a bounded preview path; native mode
  owns local filesystem/SQLite/AppLink effects behind explicit user approval.
- Race/rollback fixtures cover late preview after unmount, failed cancel,
  stale history completion, changed template revision, claim restore, ack
  rollback, sidecar lock replacement, expiry, and direct pending-to-consumed
  handoff. Existing quick-capture, wikilink, applink, snapshot, and export
  fixtures were kept in the package test suites.

### Changed-file inventory

```text
apps/knowledge-base/README.md
apps/knowledge-base/src-tauri/src/commands/docs.rs
apps/knowledge-base/src-tauri/src/commands/handoff.rs
apps/knowledge-base/src-tauri/src/commands/mod.rs
apps/knowledge-base/src-tauri/src/commands/templates.rs
apps/knowledge-base/src-tauri/src/core/db.rs
apps/knowledge-base/src-tauri/src/core/handoff.rs
apps/knowledge-base/src-tauri/src/core/mod.rs
apps/knowledge-base/src-tauri/src/core/templates.rs
apps/knowledge-base/src-tauri/src/lib.rs
apps/knowledge-base/src/App.applink.test.tsx
apps/knowledge-base/src/App.css
apps/knowledge-base/src/App.test.tsx
apps/knowledge-base/src/App.tsx
apps/knowledge-base/src/App.wikilinks.test.tsx
apps/knowledge-base/src/api.ts
apps/knowledge-base/src/api.template.test.ts
apps/knowledge-base/src/components/TemplateManager.tsx
apps/knowledge-base/src/components/TemplateManager.test.tsx
apps/life-log/README.md
apps/life-log/src-tauri/src/commands/handoff.rs
apps/life-log/src-tauri/src/commands/life.rs
apps/life-log/src-tauri/src/core/db.rs
apps/life-log/src-tauri/src/core/draft_history.rs
apps/life-log/src-tauri/src/core/handoff.rs
apps/life-log/src-tauri/src/core/mod.rs
apps/life-log/src-tauri/src/core/source_explanation.rs
apps/life-log/src-tauri/src/lib.rs
apps/life-log/src/App.contextMenu.test.tsx
apps/life-log/src/App.css
apps/life-log/src/App.tsx
apps/life-log/src/api.ts
crates/applink/src/handoff.rs
crates/applink/src/lib.rs
workthrough/2026-08-28-knowledge-lifelog-workflows.md
```

## Code Examples

### Exact template substitution

```rust
// apps/knowledge-base/src-tauri/src/core/templates.rs
while let Some(start) = rest.find("{{") {
    push_bounded(&mut output, &rest[..start])?;
    // Match one allowlisted token from the original template only.
    push_bounded(&mut output, value)?;
}
```

Only the four allowlisted tokens reach this loop; the validator rejects every
other `{{...}}` token before rendering, and substituted values are never scanned
again as template syntax.

### Metadata-only handoff status

```json
{
  "schemaVersion": 1,
  "id": "<32 lowercase hex characters>",
  "kind": "knowledge-draft/v1",
  "sourceApp": "life-log",
  "targetApp": "knowledge-base",
  "status": "sent",
  "updatedAtMs": 0,
  "expiresAtMs": 0
}
```

The actual sidecar contains no `payload`, body, path, claim token, activity
row, or credential field. The zero values above are illustrative placeholders,
not valid timestamps.

### Source freshness contract

```text
age <= 2m       fresh
2m < age <= 15m stale
age > 15m       expired
read/validation error       error + stable errorCode
```

Snapshot sources retain `latest-snapshot-out-of-range` scope and are not mixed
into the selected Life Log range totals.

### Acceptance and test draft

The following cases are the review/CI fixture draft. They are intentionally
listed even though the parent integration run owns the full Cargo, frontend,
and Windows packaged gates.

| Area | Fixture / action | Required assertion |
| --- | --- | --- |
| #351 variables | Render each of the four allowlisted variables, then render an unknown or unterminated `{{...` token | Exact substitution succeeds; unknown/malformed input fails without a file mutation |
| #351 bounds | Name 128 bytes, content 64 KiB, output 256 KiB, title 256 bytes, path 512 bytes, and one-byte-over cases | Boundary values pass; over-limit values return fixed errors before preview/save |
| #351 path/security | Absolute, `..`, `.`, doubled separator, backslash, drive-colon, non-`.md`, symlink/reparse parent, and existing target | Preview/save reject safely; existing files and ancestors are not overwritten or created |
| #351 approval | Preview, cancel/close, wrong preview ID, duplicate save, changed vault identity, and target race | Approval is one-shot; cancel/stale/duplicate paths cannot publish; competing target wins |
| #352 source rules | Live local source, requested-range Git, snapshot within/outside range, freshness at 2:00/2:00.001/15:00/15:00.001 | Scope and `fresh`/`stale`/`expired` labels are deterministic; snapshots remain provenance-only |
| #352 failure isolation | Unsupported schema, unreadable/corrupt snapshot, missing integration root, and browser preview | Stable error code/explanation is shown without raw path/payload; unrelated sources still render |
| #353 state machine | `pending → sent → consumed`, `sent → pending` launch failure, cancel, TTL expiry, and terminal regression | Sidecar/DB states agree; consumed/expired never regress; missing sidecar is never guessed consumed |
| #353 privacy/bounds | Inspect sidecar/history JSON, inject body/path/token/credential fields, 100/101 history rows, and invalid regeneration ID | Metadata/aggregate-only records pass; secret/raw fields and overflow are rejected/pruned; new ID is issued |
| #353 race/cancel | Knowledge claims while Life Log marks sent, launch failure during claim, consumer cancel, and producer restart | Monotonic sidecar lock/reconciliation converges without duplicate consumption or raw transfer |

## Verification Results

Focused validation was run serially (`-j1`/single worker) in this worktree:

- `cargo test -p applink --lib -j1`: **65 passed**.
- `cargo test -p life-log --lib -j1`: **100 passed**.
- `CARGO_TARGET_DIR=/home/jihoon/.cache/targets/knowledge-lifelog-351-353 cargo test -p knowledge-base --lib`: **122 passed** after the one-pass substitution regression was added.
- `cargo clippy -p applink -p knowledge-base -p life-log --all-targets -j1 -- -D warnings`: passed.
- `pnpm --filter knowledge-base test -- src/api.template.test.ts src/components/TemplateManager.test.tsx --maxWorkers=1 --pool=forks`: **8 passed**.
- `pnpm --filter life-log test -- src/App.contextMenu.test.tsx --maxWorkers=1 --pool=forks`: **15 passed**. Vitest's jsdom emitted its existing non-fatal `Not implemented: navigation to another Document` stderr for download anchors; no test failed and no raw fixture value was reflected.
- `pnpm --filter life-log exec tsc --noEmit`: passed.
- `pnpm --filter knowledge-base exec tsc --noEmit`: passed.
- `pnpm --dir apps/knowledge-base test -- --run`: **83 passed**.
- `pnpm --dir apps/knowledge-base build`: passed (Vite retained its existing large-chunk advisory).
- `cargo fmt --all -- --check`: passed.
- `git diff --check`: passed.

Windows-only producer compensation helpers are cfg-gated so strict Linux
Clippy does not carry dead-code warnings. Full workspace `cargo check`, root
`pnpm build`, CI, and Windows packaged W3 evidence remain parent/release gates
until this candidate is rebased onto the latest merged main. The implementation
is committed only on its local feature branch; no push or PR has been made yet.

## Next Steps

- Run the repository completion gates (`cargo test`, `cargo check`, and
  `pnpm build`) in the parent-controlled environment, followed by Windows W3
  packaged smoke and the final CI workflow.
- On Windows, exercise a real vault/template update race, locked/index-failure
  rollback, cold/hot Life→Knowledge handoff, and process termination during
  status-lock ownership. The Linux fixtures cover the same state transitions
  but cannot prove WebView2, Windows reparse-point, launcher, or file-dialog
  behavior.
- Confirm the final app data root permissions and OS-lock release after an
  abrupt process exit. A cross-filesystem crash between file publication and
  SQLite index commit remains inherently recoverable rather than a single
  physical transaction; the identity-checked cleanup and next index scan are
  the deliberate recovery boundary.
