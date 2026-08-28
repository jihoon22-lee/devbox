# Devbox Manager Data Inspector and redacted support bundle

## Overview

Issues #354 and #355 were implemented as one cohesive Devbox Manager diagnostics
candidate. The Manager can discover catalog-owned SQLite databases, inspect a
bounded sanitized preview, and export JSON/CSV only after an explicit preview.
It can also preview and explicitly export a bounded support bundle containing
app/catalog/schema/log metadata and diagnosis state without raw databases,
raw logs, paths, credentials, or authorization data.

The candidate is intentionally left dirty for review. No commit, push, PR, or
rebase was performed. A focused security/remediation pass was then applied in
the same worktree; it preserves the candidate changes and closes the known
#354/#355 review blockers before parent-agent integration.

## Context

- #354 requires catalog-derived SQLite discovery, read-only/query-only access,
  authorizer write blocking, the two-second/1,000-row limits, JSON/CSV export,
  privacy protection, and reproducible failure-path fixtures.
- #355 requires app/catalog/schema/log metadata, path/user/secret/auth
  redaction, and explicit exclusion of full DBs, raw logs, credentials,
  Authorization/Cookie, and arbitrary uploads.
- The work was based on `origin/main` at `2968660d085e0ce67c6104e7845bed493039a6f8`.
- The initial candidate was prepared conservatively while shared resources
  were constrained. The remediation pass was explicitly authorized to run
  focused checks with Rust parallelism `-j1`; it did not run a full workspace
  gate, Windows W3 packaged smoke, or any destructive cleanup.

## Changes Made

### 1. Catalog-derived SQLite Data Inspector

Files:

- `apps/devbox-manager/src-tauri/src/core/data_inspector.rs`
- `apps/devbox-manager/src-tauri/Cargo.toml`
- `Cargo.lock`
- `apps/devbox-manager/src-tauri/src/core/mod.rs`

The core resolves only `data_local_dir/<catalog identifier>/data.db`; the UI
never supplies a path. It rejects parent traversal, symlink/reparse
components, non-regular files, paths outside the canonical data root, and
oversized databases. Missing, unsafe, unreadable, and available states are
returned separately without returning the path.

Database connections use `SQLITE_OPEN_READ_ONLY`, URI/no-mutex/nofollow flags,
`PRAGMA query_only=ON`, per-connection SQLite limits, and a SQLite authorizer.
The authorizer allows reads, selects, recursive CTEs, and non-dangerous scalar
functions, while denying writes, transactions, pragma/attach/detach,
virtual-table creation, and file or extension-loading functions. Both the
request preflight and authorizer reject `pragma_*` table-valued functions and
raw `sqlite_schema.sql` reads. A 512 MiB database cap plus limits for SQL
length, cell length, columns, expression depth, compound selects, VDBE ops,
function arguments, variables, LIKE patterns, attached databases, and worker
threads contain parser/query allocation pressure.

Before opening, the path and `-wal`/`-shm`/`-journal` sidecars are checked for
regular non-link components. On Unix, an `O_NOFOLLOW|O_CLOEXEC` descriptor is
opened and SQLite resolves `/proc/self/fd/N` (or `/dev/fd/N`) with
`immutable=1`, so a parent-path replacement cannot redirect the connection and
SQLite cannot race into a sidecar. The main path is rechecked after open. The
immutable policy intentionally reads the last checkpointed image rather than
merging a live WAL; the UI can refresh and will receive the normal stale
revision behavior when the main DB changes.

Schema metadata is restricted to bounded table/view names and row counts, with
an integrity check and schema version. Query requests are limited to
SELECT/WITH/EXPLAIN, 16 KiB SQL, 128-byte opaque IDs, no semicolon/comments,
and catalog app IDs. Progress callbacks enforce a two-second deadline and
cooperate with cancellation. Results are limited to 64 columns, 1,000 rows,
64 KiB cells, and 1 MiB serialized result bytes.

Opaque size/mtime/identity revisions are checked before and after a query and
again before export. Stored preview results are already sanitized, retained in
a bounded token map, and are atomically removed when an export claims them;
stale and failed claims therefore cannot be replayed.

### 2. Privacy-safe value and export contract

Sensitive column names and free-form values are masked. Column-origin metadata
is read from the prepared SQLite statement, so `token AS harmless_label`
still masks the source and `token || suffix` is masked as an expression with a
stable `column_N` label. Credential/header keys, common token prefixes,
bearer/JWT values, path/email/username values, blobs, non-finite numbers, and
oversized text never cross the command boundary as raw values. JSON and CSV
exports contain only the sanitized preview, revision, bounded rows, and
`redactionVersion: "v1"`; they do not contain SQL text or a path. CSV headers
and string cells beginning (after whitespace) with `=`, `+`, `-`, or `@` are
apostrophe-escaped to prevent spreadsheet formula execution; numeric JSON
values retain their numeric representation.

Representative implementation contract:

```rust
let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
    | OpenFlags::SQLITE_OPEN_URI
    | OpenFlags::SQLITE_OPEN_NO_MUTEX;
let conn = Connection::open_with_flags(path, flags)?;
conn.pragma_update(None, "query_only", true)?;
install_read_only_authorizer(&conn);
install_progress_handler(&conn, QueryBudget::new(QUERY_TIMEOUT), Some(cancel));
```

### 3. Redacted support bundle

File: `apps/devbox-manager/src-tauri/src/core/support_bundle.rs`

The bundle includes only bounded diagnosis, catalog/app metadata, sanitized
database schema/status metadata, and direct log file counts/byte totals. It
does not read log contents or database bytes. Log inspection is capped at 128
files, 512 directory entries, and 4 MiB per app, and rejects link/reparse
roots and entries. Database metadata is sanitized recursively (including
table/view names and warnings), not only at the catalog layer. The serialized
bundle is capped at 512 KiB.

The bundle documents its redaction contract and explicit omitted sections:
raw database bytes/query text, raw log lines, filesystem paths, environment
values, credentials, and authorization headers. Preview state is represented
by an expiring five-minute token. The exact serialized bytes and a source
revision (catalog revision, every catalog DB state/fingerprint, and bounded
log metadata) are retained. Export claims/removes the token before any I/O,
so only one concurrent export can succeed; it rechecks the source revision and
returns those exact bytes rather than rebuilding diagnosis/install data and
silently changing what the user reviewed.

### 4. Native command and lifecycle boundaries

File: `apps/devbox-manager/src-tauri/src/commands/diagnostics.rs`

Added bounded state for active query/bundle cancellation and preview tokens,
fixed public error messages, generated opaque IDs, one-time export removal,
five-minute bundle expiry, and catalog/database revision revalidation. Native
commands accept app IDs, operation IDs, or preview IDs only; they never accept
arbitrary filesystem paths and never create an output file. The browser layer
receives content only after an explicit export action.

`commands/doctor.rs` now exposes a reusable read-only diagnosis collector and
uses `data_local_dir`/`data_dir_path` without creating the Manager data
directory. Every fixed version probe closes stdin/stderr, caps stdout at 64
KiB and the first line at 256 characters, applies a two-second deadline, and
terminates its Unix process group or Windows Job Object on timeout/overflow.
`lib.rs` registers the diagnostics state and all seven commands.

### 5. Browser/native UI and test drafts

Files:

- `apps/devbox-manager/src/api.ts`
- `apps/devbox-manager/src/types.ts`
- `apps/devbox-manager/src/App.tsx`
- `apps/devbox-manager/src/App.css`
- `apps/devbox-manager/src/App.test.tsx`

The environment-diagnosis tab now has Data Inspector database cards, schema
summary, bounded SQL preview, cancellation, sanitized result table, JSON/CSV
export, and failure-safe states. The support section displays included and
omitted sections, expiry/one-time status, cancellation, stale messaging, and
an explicit confirmation export button. Browser mocks are deliberately
bounded/sanitized screen-flow fixtures and cannot claim native filesystem
success.

Tests cover read-only/export sequencing, no path display, cancellation,
support omission boundaries, exact preview/export bytes, stale export
consumption/UI clearing, concurrent one-time claim, write/attach/pragma and `pragma_*`
rejection, unchanged DB bytes, alias/expression/source-origin masking,
username/email/secret/path/auth masking, row truncation, SQLite allocation
limits, symlink log/database/sidecar paths, a non-ignored bounded recursive
query timeout, CSV formula escaping, and bounded diagnosis output/process-tree
cleanup.

### 6. Remediation audit and hardening

The following review findings were addressed in the candidate before handoff:

1. SQL aliases and computed expressions could make a sensitive source look
   harmless. Prepared-statement column origins now drive masking; all
   origin-less expressions are masked and their raw labels are not echoed.
2. SQLite `pragma_*` table functions and allocation-heavy functions were
   reachable through the SELECT grammar. Request preflight and the authorizer
   reject the pragma family, while SQLite connection limits and dangerous
   function denies bound memory/bytecode pressure.
3. Database path and WAL sidecar checks were vulnerable to a check/open race.
   Sidecars are checked as regular non-link files and ignored by immutable
   opens; Unix opens the already-checked inode through an owned descriptor and
   rechecks the main path after SQLite opens.
4. Query/support one-time previews could be cloned by concurrent export, and
   support export could rebuild a different document than the preview. Both
   preview maps now claim/remove atomically; support stores exact bytes and a
   source revision and rejects stale state before export.
   The browser UI clears a claimed result on success or failure so it cannot
   offer a retry against a token the native boundary has already consumed.
5. Diagnosis subprocesses had no hard output/tree lifetime guarantee, and the
   expensive timeout test was ignored. The bounded runner now enforces the
   timeout/output limits and kills process groups/jobs; the timeout test runs
   in the focused suite.
6. Username redaction and CSV spreadsheet formula handling were strengthened;
   common username/user-id/login/email column forms are sensitive and CSV
   formula-like string/header cells receive an apostrophe guard.
7. Result-only masking still allowed sensitive columns to influence predicates
   and ordering, and quoted JSON diagnostic keys could leave value fragments.
   The SQLite authorizer now substitutes sensitive source reads with `NULL`
   before evaluation, while the text redactor consumes complete quoted
   key/value fields (including escaped quotes). Regression fixtures compare
   correct and incorrect secret guesses and cover JSON-shaped identity fields.
8. Adding a global live notice made successful removal render the same message
   in two `role=status` regions. The existing detailed removal result remains
   the single live region, while diagnostic exports use the global notice.

### 7. Documentation

File: `apps/devbox-manager/README.md`

Documented #354/#355 behavior, bounded limits, explicit preview/export,
omitted support sections, and the browser/native boundary.

## Verification Results

```text
CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-manager-354 \
cargo test -p devbox-manager --lib -j1
PASS — 110 passed, 0 failed, 0 ignored

CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-manager-354 \
  cargo clippy -p devbox-manager --all-targets -j1 -- -D warnings
PASS

cargo fmt --all -- --check
PASS

git diff --check
PASS
```

The focused Rust test includes the data inspector, support bundle, diagnostics
command, and doctor tests. Dependencies were restored from the local pnpm
store without network downloads, after which `pnpm --filter devbox-manager
test` passed 26/26 tests and `pnpm --filter devbox-manager build` passed the
TypeScript and Vite production build. The first frontend run exposed the
duplicate live-status announcement described above; the corrected run is the
one recorded as passing.

A package-only Windows GNU `cargo check` was also attempted with `-j1`. It
reached the target dependency build but stopped before compiling this package
because the environment has no `x86_64-w64-mingw32-gcc` for `aws-lc-sys`; this
is an environment/toolchain blocker, not a Rust diagnostic error. Windows
compile and W3 smoke therefore remain pending on a Windows CI/host runner.

Not run: full workspace `cargo test`, full workspace `cargo check`, Windows
W3 packaged smoke, commit, push, PR creation, and rebase. No other worktree
was touched.

## Risks and follow-up

- Windows W3 packaged smoke remains pending; the Windows path fallback uses
  SQLite `NOFOLLOW` plus pre/post metadata checks but was not executable from
  this WSL session. The Unix descriptor-anchored path hardening is covered by
  Linux compilation/tests.
- Revisions intentionally use bounded file length/mtime metadata rather than
  hashing multi-gigabyte files; Unix file identity (device/inode) and Windows
  volume/file identity are included when available, but a same-identity
  in-place write that restores the same size/mtime remains a residual stale
  detection risk. Immutable mode reads the last checkpointed DB image and
  intentionally does not merge a live WAL; users should refresh after a
  checkpoint/source change.
- Parent-directory replacement is materially reduced by opening an owned Unix
  descriptor before SQLite resolution. Windows cannot use the same proc-fd
  mechanism here; its defense-in-depth is nofollow plus lexical/canonical
  checks and pre/post identity validation, so an OS-level attacker with a
  concurrent reparse race remains a platform-specific residual risk.
- The design opportunity text mentions a database backup, while #354's issue
  acceptance asks for JSON/CSV and #355 explicitly excludes full DB bytes.
  This candidate intentionally provides no raw backup; adding one requires a
  separate privacy/product decision and an explicit destination contract.
- Diagnosis probes now have per-process deadlines and process-tree cleanup.
  Residual risk is limited to unusual Unix systems without a usable `/proc` or
  `/dev/fd`, and Windows job-assignment failures; those cases fail closed and
  return an unavailable diagnosis rather than leaving a helper running. A
  future product pass may add an aggregate doctor budget if six sequential
  probes need a shorter overall UI latency target.

## Worktree handoff

- Worktree: `/mnt/e/projects/devbox-worktrees/devbox-manager-data-inspector-support-bundle`
- Branch: `feat/devbox-manager/data-inspector-support-bundle`
- Base: `origin/main` / `2968660d085e0ce67c6104e7845bed493039a6f8`
- State: reviewed dirty candidate plus remediation; no push/PR/rebase yet;
  other worktrees were not modified.
