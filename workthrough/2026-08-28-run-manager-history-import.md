# Run Manager History Filters and Native Task Import

## Overview

Issues #357 and #358 are implemented as one Run Manager history/import candidate.
Run history now has a bounded, parameterized status/kind/target/date/duration query,
and the import dialog can preview local `package.json` scripts and `Cargo.toml`
targets before saving disabled drafts. The implementation stays native-first and
offline: it reads only the two explicitly selected source files and never invokes
npm, Cargo, a shell, a dotenv loader, a network client, or an imported command.

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

- Reads only immediate `package.json` and `Cargo.toml` files below a canonical,
  absolute, non-symlink project root. Reads are bounded to 512 KiB per file and
  128 total items; root, command, name, environment-key, operation-ID, and
  selection-ID limits are enforced before storage.
- Converts scripts into stable `npm run -- <name>` commands and Cargo targets
  into restricted `cargo run/test/bench` command forms. Target names are checked
  against a narrow safe character set, malformed Cargo target shapes fail closed,
  and no parser path reaches a process adapter. A package with `autobins = false`
  no longer receives a phantom default `cargo run`; an invalid non-boolean flag
  fails closed while explicit `[[bin]]` targets remain importable.
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
  cargo test -p run-manager --lib -j1                         pass (186 tests)
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
invalid definition/project import batches. All 186 Run Manager library tests
passed. Frontend dependencies were restored from the local offline pnpm store;
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
