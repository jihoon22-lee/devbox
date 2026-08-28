# Run Manager History Filters and Native Task Import

## Overview

Issues #357 and #358 are implemented as one Run Manager history/import candidate.
Run history now has a bounded, parameterized status/kind/target/date/duration query,
and the import dialog can preview local `package.json` scripts and `Cargo.toml`
targets before saving disabled drafts. The implementation stays native-first and
offline: it reads the contents of only the two explicitly selected manifest files,
uses bounded metadata for Cargo's fixed standard layout, and never invokes npm,
Cargo, a shell, a dotenv loader, a network client, or an imported command.

## Context

- #357 requires combined run history filters for jobs and services without exposing
  secrets, raw log data, or unsafe paths.
- #358 requires local package/Cargo task preview/import with environment and working
  directory confirmation, while explicitly excluding command auto-run and remote,
  Kubernetes, or DAG import.
- The candidate was created from `origin/main` at
  `2968660d085e0ce67c6104e7845bed493039a6f8` in the dedicated worktree
  `/mnt/e/projects/devbox-worktrees/run-manager-history-import` on branch
  `feat/run-manager/history-import`.
- Existing Run Manager SQLite schema v2 already contains the required job/run
  fields, so the feature uses the existing idempotent migration without adding a
  column or rewriting user data.

## Changes Made

### 1. Bounded run history filtering

Files: `apps/run-manager/src-tauri/src/core/models.rs`,
`apps/run-manager/src-tauri/src/storage.rs`,
`apps/run-manager/src-tauri/src/commands.rs`,
`apps/run-manager/src/types.ts`, `apps/run-manager/src/api.ts`,
`apps/run-manager/src/components/RunHistory.tsx`, `apps/run-manager/src/App.tsx`,
and `apps/run-manager/src/App.css`.

- Added the `RunHistoryFilter` wire contract: optional job ID, `job`/`service`
  kind, status, half-open epoch-millisecond date range, duration range, and a
  limit capped at 500.
- Storage joins `runs` to `jobs` and applies every selected filter in one
  parameterized query. Active duration uses the supplied query time; queued rows
  do not match a duration filter. Returned rows remain the existing redacted
  `RunView` DTO.
- The UI can select all targets, job-only, service-only, status, date, and
  minimum/maximum duration. Service rows cannot accidentally trigger job-only
  run/stop/rerun actions.
- Added model, storage, and fixture coverage for invalid ranges and a combined
  job/service/status/date/duration query.

### 2. Native offline package/Cargo importer

Files: `apps/run-manager/src-tauri/src/core/imports.rs`,
`apps/run-manager/src-tauri/src/core/mod.rs`,
`apps/run-manager/src-tauri/src/commands.rs`,
`apps/run-manager/src-tauri/src/lib.rs`,
`apps/run-manager/src-tauri/Cargo.toml`, and `Cargo.lock`.

- Reads only immediate `package.json` and `Cargo.toml` contents below a
  canonical, absolute, non-symlink project root. Cargo's standard target layout
  is inspected with fixed-depth, bounded metadata only. Reads are bounded to
  512 KiB per manifest and 128 total items; root, command, name,
  environment-key, operation-ID, and selection-ID limits are enforced before
  storage.
- Converts scripts into stable `npm run -- <name>` commands and Cargo targets
  into restricted `cargo run/test/bench` command forms. Target names are checked
  against a narrow safe character set, malformed Cargo target shapes fail closed,
  and no parser path reaches a process adapter. Cargo auto targets are emitted
  only after their bounded metadata path is present; a library-only package never
  receives a phantom bare `cargo run`. An invalid non-boolean flag fails closed
  while explicit `[[bin]]` targets remain importable.
- Script bodies are not returned, persisted, or put into errors. Only bounded
  environment key names are shown; environment values and `.env` files are never
  read. Canonical cwd is displayed for confirmation and all imported jobs start
  disabled with a fixed review schedule that cannot trigger until explicit enable.
- Uses filesystem identity plus an opaque source revision to reject replaced or
  edited source between preview and apply. Current `(kind, name, cwd)` conflicts
  are marked in preview and rechecked during apply.
- Project apply validates all rows, checks cancellation at every bounded row
  boundary and immediately before commit, and commits one SQLite transaction.
  Duplicate source items, races, and cancellation before commit are skipped or
  rolled back atomically.

### 3. Preview lifecycle and persistence safety

Files: `apps/run-manager/src/components/ImportDialog.tsx`,
`apps/run-manager/src/api.ts`, `apps/run-manager/src-tauri/src/core/imports.rs`,
`apps/run-manager/src-tauri/src/storage.rs`.

- Added a process-local operation registry with safe IDs, duplicate/busy limits,
  cooperative cancellation, a five-second native budget, and no retained path,
  command, source bytes, or environment values.
- Import UI discards stale/late preview responses by generation, cancels the exact
  project operation on user cancel or client timeout, and always requires the
  preview's revision when applying. Definition JSON parsing/saving is bounded
  but intentionally non-cancellable in the UI; Escape therefore does not claim
  to cancel that already-running operation. Project cancellation before commit
  rolls back the entire batch.
- Existing definition JSON import now also uses a bounded fixed revision and
  always creates disabled jobs/services with environment ciphertext cleared;
  unknown secret-shaped fields are not deserialized into the stored DTO.

### 4. Remediation audit for #357/#358

Files additionally hardened: `apps/run-manager/src-tauri/src/core/imports.rs`,
`apps/run-manager/src-tauri/src/storage.rs`,
`apps/run-manager/src-tauri/src/commands.rs`,
`apps/run-manager/src/components/ImportDialog.tsx`,
`apps/run-manager/src/components/RunHistory.tsx`,
`apps/run-manager/src/App.tsx`, `apps/run-manager/README.md`, and
`docs/superpowers/specs/2026-08-12-run-manager-design.md`.

- Closed the project-root TOCTOU window by binding the selected spelling to its
  no-follow directory identity during canonicalization, then re-checking that
  identity after each source read and immediately before the plan is returned.
  Non-UTF-8 roots now fail closed instead of being converted through lossy
  display text. Each source read now also binds the initially inspected path to
  the opened file handle, verifies the handle before and after the bounded read,
  then verifies the current path identity/fingerprint. A same-sized replacement
  cannot be read and restored between path-only checks without invalidating the
  preview.
- Replaced the 64-bit revision with a 256-bit SHA-256 digest rendered as a
  fixed 64-character hexadecimal value. The digest includes source labels,
  exact byte lengths/content, presence markers, and an opaque root identity;
  it never includes an absolute path. Apply rejects malformed or stale
  revisions before saving.
- Made project-import conflict persistence use the same normalized
  `SafeProjectPath` identity as preview (including Windows case and separator
  aliases), and keep the check inside the `BEGIN IMMEDIATE` transaction. Added
  a kind-aware query so a concurrent definition cannot invalidate the preview
  into a duplicate save. Preview and definition-ID conflict checks now query
  only the bounded set of candidate names/IDs instead of materializing the
  entire definitions table.
- Made selected definition JSON import a single atomic database transaction for
  jobs and services. All IDs, fields, disabled/non-auto-start invariants, and
  protected-environment boundaries are validated before locking SQLite; ID
  conflicts are skipped inside the transaction and any validation/SQL failure
  rolls back the whole batch. Service instance rows are created in the same
  transaction.
- Made project-import cancellation part of the storage transaction boundary:
  the command checks the exact operation before each insert and immediately
  before commit, and a cancellation error drops the transaction so rows from a
  partially processed batch cannot remain. The UI disables/labels the
  non-cancellable definition-import action as processing instead of implying
  that Escape can undo a committed save.
- Added dialog keyboard safety: initial focus, Tab/Shift+Tab focus trapping,
  Escape cancellation/close behavior, `aria-busy`, tab/tabpanel relationships,
  preview discard generation invalidation, exact operation cancellation on
  unmount, and stale apply error suppression. App restores focus to the import
  trigger after close or successful import. The combined job/service definition
  list is memoized so ordinary App refreshes do not retrigger history queries.
  Run History retains existing target-selection behavior while the bounded
  combined filter remains the default backend contract.
- Added fixtures proving root-bound revisions, command/body credential values
  from package scripts are not copied, normalized Windows cwd conflicts,
  disabled/idempotent definition batches, and all-or-nothing validation.
- Tightened history filter validation so negative timestamps and zero or
  over-500 explicit limits fail at the native boundary instead of being silently
  clamped into a different query.

## Code Examples

### Parameterized history contract

```rust
// apps/run-manager/src-tauri/src/storage.rs
pub fn list_run_history(
    &self,
    filter: &RunHistoryFilter,
    now: i64,
) -> Result<Vec<Run>, StorageError> {
    filter.validate()?;
    // One bounded query; status/kind/date/duration are parameters.
    // Logs and environment ciphertext are not selected.
}
```

### No-execution project import

```rust
// apps/run-manager/src-tauri/src/core/imports.rs
let command = format!("npm run -- {name}");
let environment_keys = referenced_environment_keys(body);
// ProjectImportItem stores `command` and these key names only. The source body
// is intentionally not assigned to the preview DTO or persistence input.
```

### Stale-safe apply

```rust
// apps/run-manager/src-tauri/src/commands.rs
let plan = verify_preview_revision_with_control(path, &source_root, &revision, control)?;
database.create_import_jobs_at_with_cancel(imported_inputs, current_epoch_millis(), || {
    control.check().map_err(|error| StorageError::Validation(error.to_string()))
})?;
```

## Verification Results

Focused checks were run in the dedicated worktree with the native Cargo target
cache and a single test process. The full workspace gate and Windows packaged
smoke remain the parent PR's responsibility.

```text
rustfmt --edition 2021 <run-manager Rust files>    pass (exit 0)
git diff --check              pass (exit 0)
source ~/.cargo/env && CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-run-manager \
  cargo test -p run-manager --lib -j1                         pass (186 tests at baseline)
source ~/.cargo/env && CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-run-manager \
  cargo check -p run-manager --lib -j1                        pass (exit 0)
source ~/.cargo/env && cargo clippy -p run-manager --all-targets -- -D warnings
                                                               pass (exit 0)
pnpm --dir apps/run-manager test -- --maxWorkers=2
                              pass (6 files / 39 tests)
pnpm --dir apps/run-manager build                            pass (exit 0)
python3 .github/scripts/check-dependencies.py check          pass (exit 0)
CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-run-manager \
  cargo test -p run-manager core::imports::tests::cargo_import_respects_disabled_automatic_binary_discovery
                              pass (1 focused regression)
```

The Rust unit/fixture tests cover parser bounds, unsafe symlinks, root/file
identity and source revision changes, operation duplicate/cancel/timeout state,
history filter combinations, normalized cwd conflicts, and atomic duplicate/
invalid definition/project import batches. All 186 baseline Run Manager library
tests passed; the first re-audit inventory was 189 after the autobins and
selection fixtures, and the metadata-only Cargo follow-up brings the final
inventory to 198 passing tests as recorded below. Frontend dependencies were restored from the local offline
pnpm store;
the complete Vitest suite and production TypeScript/Vite build passed. Windows
W3 packaged smoke is pending.

## Next Steps / Known Risks

- Rebase onto the latest merged `main`, rerun the workspace gates, and require
  every pull-request CI job plus the Windows W3 packaged smoke before merging.
- Confirm Windows canonical-path display and WSL/Windows cwd behavior on real
  drives, UNC roots, and reparse-point fixtures.
- Cancellation is cooperative: an operation whose SQLite transaction has already
  committed is not rolled back. Project apply checks cancellation before each
  insert and immediately before commit, so cancellation observed earlier drops
  the transaction and leaves no partial batch. Definition JSON import remains a
  bounded non-cancellable operation once saving starts.
- A source directory can still be replaced immediately after the final identity
  check; complete elimination would require holding an OS directory handle and
  opening files relative to that handle. Opened source files are now bound to
  their handles and current path fingerprints, while the repeated root checks
  make a changed directory fail on the next bounded boundary in normal races.
- Definition and project apply are atomic at the SQLite boundary, but an
  already committed transaction is intentionally not undone by a later UI
  cancellation acknowledgement.

## Final Re-audit (2026-08-28)

The final #357/#358 review was performed again on the candidate before
rebasing, pushing, or opening a PR. GitHub issue #358 explicitly scopes
the native importer to `package.json` scripts and Cargo targets; VS Code
`tasks.json` parsing remains the separate §13.2 follow-up and is documented as such
instead of silently expanding this PR boundary.

### Remediation changes

- `apps/run-manager/src-tauri/src/core/imports.rs` now follows Cargo's explicit
  `[[bin]]` name derivation when `name` is omitted: a bounded, relative, non-
  traversal `path` supplies the filename stem, while path-less entries retain the
  package name. The path is metadata only and is never opened or executed. A
  regression fixture covers both inference and traversal rejection.
- `apps/run-manager/src-tauri/src/commands.rs` now treats selection IDs as an
  allow-list: definition apply and project apply reject IDs not present in the
  validated preview plan. This closes a fail-open no-op/forged-selection path and
  preserves the existing count, revision, transaction, conflict, and disabled-draft
  invariants.
- `apps/run-manager/src/components/RunHistory.tsx` guards history/active refresh
  results and run actions with the mounted and generation checks, prevents a pending
  refresh loop from continuing after unmount, and exposes stream selection through
  `aria-pressed`.
- `apps/run-manager/src/components/ImportDialog.tsx` guards preview/apply state
  updates after unmount (including StrictMode-safe mounted setup), while retaining
  exact operation cancellation. All local action buttons now declare
  `type="button"`; `packages/diff-view/src/index.tsx` applies the same form-safe
  default to shared change-set actions.
- README/spec/workthrough text now records Cargo `autobins` behavior, selection
  allow-listing, the VS Code follow-up boundary, and the final stale/a11y behavior.

### Re-audit conclusions

- History filters remain native-boundary validated: negative timestamps, inverted
  or zero-length ranges, negative/out-of-range durations, zero limits, and limits
  over 500 fail closed. The legacy positional `list_runs` command builds the same
  filter and maps validation failures to the fixed
  `run-history-invalid-filter` code; it cannot bypass the structured query.
- Import still reads only immediate `package.json` and `Cargo.toml`, with 512 KiB
  per-file, 128-item, operation-ID, command/name, environment-key, and 5-second
  cooperative bounds. npm/Cargo/shell/network/.env are not invoked or read; script
  bodies and environment values are not persisted. Generated command names remain
  allow-listed and imported definitions are disabled drafts.
- Root and source-file identity checks, opened-handle fingerprints, opaque SHA-256
  revision comparison, normalized Windows cwd conflict checks, transaction-local
  duplicate checks, and pre-commit cancellation/rollback remain in force. The
  committed-transaction cancellation and final check-to-use filesystem race remain
  documented residual OS boundaries.

## Re-audit Verification

The remediation was verified with the dedicated target cache
`/home/jihoon/.cache/targets/devbox-run-manager` and `-j1`; no new large target
directory was created. The existing full-suite/build passes above belong to the
pre-remediation candidate; the final re-audit intentionally ran only changed-
boundary checks because the parent worktree was under resource pressure.

```text
git diff --check                                      pass (exit 0)
cargo fmt --all -- --check                            pass (exit 0)
source ~/.cargo/env && CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-run-manager \
  cargo test -p run-manager --lib cargo_import_derives_explicit_bin_name_from_relative_path -j1
                                                       pass (1; 188 filtered; 189 total)
source ~/.cargo/env && CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-run-manager \
  cargo test -p run-manager --lib import_selection_ids_must_belong_to_the_preview_plan -j1
                                                       pass (1; 188 filtered; 189 total)
source ~/.cargo/env && CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-run-manager \
  cargo test -p run-manager --lib -- --list                     pass (189 tests)
pnpm --dir apps/run-manager exec tsc --noEmit              pass (exit 0)
pnpm --dir apps/run-manager test -- --maxWorkers=1 --no-file-parallelism
                                                       interrupted: /mnt/e 9p I/O stall
                                                       before a result; not counted as a pass
```

## Metadata-only Cargo auto-target follow-up

### Overview

The final P1 audit found that parsing `Cargo.toml` alone cannot identify Cargo's
standard-layout targets. The importer previously created a phantom bare
`cargo run` when no explicit `[[bin]]` existed, while omitting automatic
`src/main.rs`, `src/bin`, examples, tests, and benchmarks. This follow-up keeps
the no-Cargo/no-shell boundary but permits a fixed-depth, bounded, metadata-only
layout probe. Cargo source contents are still never read.

### Changes made

- `apps/run-manager/src-tauri/src/core/imports.rs`
  - Split Cargo manifest parsing from layout discovery and item merging.
  - Apply Cargo edition defaults and all five `auto*` switches.
  - Discover `src/lib.rs`, `src/main.rs`, direct `.rs` files and one-level
    `main.rs` target directories under `src/bin`, `examples`, `tests`, and
    `benches`.
  - Validate explicit target paths without opening source contents. Missing
    explicit targets, root escapes, symlinks, and reparse points fail closed.
  - Merge explicit and automatic targets by kind, name, and safe relative path.
    An explicit target is authoritative for the same standard-layout path;
    conflicting same-kind names or duplicate explicit paths fail instead of
    silently selecting one.
  - Exclude non-binary examples and targets with `required-features` from
    execution-task items. Every binary command now uses `cargo run --bin <name>`;
    bare `cargo run` is never generated.
  - Bound directory entries and layout items, sort the snapshot deterministically,
    repeat cancellation/root/link/identity checks, and include target metadata
    in the opaque preview revision so layout changes become stale at apply.
  - Added regression fixtures for standard layout discovery with intentionally
    invalid source bytes, edition/auto-flag behavior, explicit/automatic merge,
    required-feature/non-binary exclusion, missing explicit files, layout stale
    detection, and symlink rejection.
- `apps/run-manager/README.md` and
  `docs/superpowers/specs/2026-08-12-run-manager-design.md`
  - Clarified that only the two manifest contents are read; Cargo layout
    metadata is the sole additional read.
  - Documented the fixed-depth boundary, auto flags, edition behavior, no bare
    `cargo run`, no workspace-member traversal, and layout-aware stale revision.

### Verification status

The parent review ran the new boundary tests in the existing dedicated cache
with one build job. The first compile exposed two unconstrained `BTreeMap`
value types; after adding the exact entry/reference types, the first focused
run exposed an explicit non-binary example path being rediscovered as an
automatic executable example. The merge now treats an explicit target as
authoritative for the same kind/path and rejects duplicate explicit paths.

```text
cargo fmt --all -- --check                                  pass
cargo test -p run-manager core::imports --lib -j1           pass (22 tests)
cargo test -p filesystem -j1                                pass (17 tests)
cargo test -p run-manager --lib -j1                         pass (198 tests)
```

After rebasing onto the Code Pad merge, the shared Diff View conflict retained
its newer `cancelDisabled` contract together with the form-safe button and
checkbox label. Dependency notices were regenerated from the merged lockfile.
The first post-rebase check also found a test-only revision helper in the
production dead-code surface; it is now compiled only for tests.

```text
cargo test -p run-manager -j1                               pass (198 tests)
cargo check -p run-manager -j1                              pass
cargo clippy -p run-manager --all-targets -j1 -- -D warnings pass
pnpm --dir apps/run-manager test -- --maxWorkers=2           pass (6 files / 39 tests)
pnpm --dir apps/run-manager build                            pass
python3 .github/scripts/check-dependencies.py check          pass
git diff --check                                             pass
```

Push, pull-request CI, and the Windows W3 packaged smoke remain. Windows
reparse-point behavior remains part of that packaged acceptance boundary.

### Design boundary

The public `parse_cargo_targets(bytes)` helper remains a pure manifest-only
parser for callers that do not have a project root. Project preview uses the
root-aware metadata path so automatic discovery cannot be faked from manifest
bytes. Virtual workspaces remain unsupported because resolving members would
require reading additional manifests or invoking Cargo.

## Pull-request CI remediation

The first pull-request run exposed two platform/load-sensitive defects that
were not visible in the focused Linux gates:

- The Windows compile check rejected the unstable
  `std::os::windows::fs::MetadataExt` volume/file-index methods. The importer
  now carries the existing cross-platform `FilesystemIdentity` value in Cargo
  layout fingerprints instead of reaching through platform metadata. The
  shared filesystem crate also exposes `open_filesystem_object`, which returns
  a no-follow handle and the identity captured from that same handle. Manifest
  reads retain that exact handle through the bounded read and compare it with
  the current path before accepting the snapshot, avoiding a reopen gap.
- Under the concurrent CI frontend load, the Launcher confirmation dialog was
  observable before its passive focus effect ran. Its initial-cancel focus and
  close-time focus restoration now run as a layout effect, so the modal focus
  contract is established in the same committed render.

The local all-frontend reproduction also timed out while starting an unrelated
API Playground Vitest worker after all 29 of its started files and 211 tests
passed. This was treated as resource contention rather than a product result;
the complete Run Manager frontend suite was rerun in isolation.

```text
cargo test -p filesystem -j1                              pass (18 tests)
cargo test -p run-manager -j1                             pass (198 tests)
cargo check -p run-manager -j1                            pass
cargo clippy -p run-manager --all-targets -j1 -- -D warnings pass
pnpm --dir apps/run-manager test -- --maxWorkers=2         pass (6 files / 39 tests)
pnpm --dir apps/run-manager build                          pass
pnpm test                                                  inconclusive: unrelated
                                                            API Playground worker
                                                            startup timeout
cargo check -p run-manager --target x86_64-pc-windows-gnu -j1
                                                           unavailable locally:
                                                           MinGW C compiler absent;
                                                           GitHub Windows CI is gate
git diff --check                                           pass
```
