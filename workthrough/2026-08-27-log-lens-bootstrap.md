# Log Lens 0.1.0 Bootstrap

## Overview

Implemented and audited the #321 Log Lens native-first bootstrap on
`feat/log-lens/bootstrap-integrated`, based on `origin/main` `2968660`.
The new Tauri app is an offline, read-only viewer for selected local files and
directories, fixed WSL sources, and fixed Docker/Podman container adapters. It
parses plain text, JSONL, and logfmt input, merges records deterministically,
and retains only bounded in-memory data. The `log-source/v1` Run Manager
receiver is present as an identity-only boundary; producer claim/ack wiring is
left for the planned follow-up integration PR.

## Context

The P3-02 contract requires local/WSL/Run/container sources, best-effort
timestamp and level parsing, follow/pause/filter/bookmark/export behavior,
rotation/truncate handling, and a 100,000-line or 64 MiB bound. The safety
boundary excludes arbitrary WSL commands, network ingestion, log archives, and
execution of log content. The app therefore keeps parser logic app-local until
a second consumer justifies a `crates/logs` extraction.

## Changes Made

### 1. New Log Lens app and workspace registration

- Added `apps/log-lens` React/Vite/Tauri scaffold, package metadata, CSP, default
  capability, reusable existing icon assets, and `LogLens` bundle identity
  (`com.devbox.loglens`).
- Added `apps/log-lens/src-tauri` to the Cargo workspace and the app importer to
  `pnpm-lock.yaml` (including its `@devbox/context-menu` workspace dependency);
  `tokio` is a direct backend dependency for blocking source work outside the
  Tauri runtime thread.
- Registered the app in `apps/catalog.json` as catalog revision 9 with only the
  implemented `handoff:log-source/v1` input. No snapshot producer is declared
  until Log Lens actually publishes one; Run/WSL producer claim/ack remains a
  separate integration boundary.
- Updated catalog regression assertions and repository/app documentation in
  `README.md`, `apps/log-lens/README.md`, `docs/{architecture,development,projects,roadmap,windows-guide}.md`,
  and the native-first plan.

### 2. Bounded parser and deterministic merge

- `core/parser.rs` handles plain, JSONL, and logfmt lines with RFC3339/space
  timestamps, common level fields, quoted logfmt escapes, scalar/nested JSON
  fields, malformed-line fallback, invalid UTF-8 replacement, and UTF-8-safe
  16 KiB line truncation.
- Filters support literal/linear-regex text, source, level, time, and field
  constraints. Rust regex program limits prevent unbounded regex resources.
- `MergeBuffer` keys records by timestamp, opaque source ID, and source sequence,
  then evicts the oldest entries at the global 100,000-record/64 MiB bound.
  Multi-source commands consume each source snapshot immediately instead of
  retaining up to 16 independent 64 MiB snapshots.
- Export is deterministic, metadata-aware, UTF-8 byte bounded at 8 MiB, and
  validates command-supplied records before constructing output. Record and
  field limits also protect the public filter/export commands from oversized
  caller payloads.

### 3. Read-only source adapters and lifecycle

- `core/model.rs` validates absolute local paths, safe filename-only patterns,
  WSL distro/path values, opaque Run source IDs, container IDs, filters, saved
  views, cursors, and fixed error boundaries. Source summaries expose only a
  stable opaque ID and generic display name.
- `core/sources.rs` reads local files/directories with sorted bounded patterns,
  platform file identity, decimal text cursors, append/rotation/truncate
  detection, and chunk-level cancellation/deadline checks.
- WSL uses only fixed `wsl.exe -d … -- cat -- …` and journalctl argv. Container
  reads use only fixed `docker|podman logs --timestamps --tail 100000 …` argv.
  Adapter stdout is read through a bounded sync channel; termination drops the
  receiver before killing/joining the child so cancellation/output-limit paths
  cannot deadlock. A post-stdout `try_wait` loop preserves the 10-second
  deadline even when a child closes stdout but remains alive.
- `core/lifecycle.rs` provides opaque operation IDs, generation checks,
  single-flight cancellation, stale-result rejection, bounded operation
  tracking, and explicit cancel lookup.
- Source-list validation rejects duplicate descriptors before any member is
  opened, preventing equal opaque IDs and per-source sequence namespaces from
  collapsing into one merged row set. The UI reports the duplicate as a fixed
  validation message as well.
- Tauri read commands use `spawn_blocking`, allowing `cancel_read` to execute
  concurrently. Frontend cleanup cancels the active opaque operation and guards
  mounted/generation state before applying results.

### 4. Frontend behavior and browser fixture

- `App.tsx` provides source selection, follow/pause, merge display, text/regex/
  source/level/time/field filtering, safe message highlighting, bookmarks,
  selected export/copy, and in-memory source/filter-only saved views.
- Browser mode starts with a deterministic bounded fixture; native mode uses the
  same typed snapshot contract. Browser export follows the native timestamp,
  field ordering, quoting, control-character escaping, UTF-8 byte cap, and
  truncation semantics.
- Explicit download and clipboard actions use fixed user-facing errors. No
  localStorage, archive, arbitrary save/open path, shell command, or network
  ingest is added.
- Labels, focus outlines, semantic controls, live status/error regions,
  keyboard form submission, and composition guards are included. Packaged IME,
  focus, clipboard, and download behavior still require Windows validation.

### 5. Fixtures and tests

- Added `src-tauri/tests/fixtures/mixed.log` covering plain/JSONL/logfmt.
- Rust unit coverage covers parser formats/fallback/UTF-8 limits, deterministic
  merge, filters/export, ring eviction/backpressure, source cursor
  append/rotation/truncate, fixed adapter argv, cancellation, stale
  generations, identity-only handoff, and strict DTOs.
- Added frontend filter tests and catalog capability/revision assertions.

## Code Examples

### Fixed adapter boundary

```rust
SourceSpec::WslFile { distro, path } => AdapterPlan {
    program: "wsl.exe".to_string(),
    args: vec!["-d", distro, "--", "cat", "--", path],
    source_kind: SourceKind::WslFile,
    read_only: true,
}
```

### Bounded cancellation-aware command

```rust
let task_operations = Arc::clone(&operations);
let result = tokio::task::spawn_blocking(move || {
    let context = LoadContext::new(&operation_id, generation, &token, &task_operations);
    load_source(&source, cursor.as_ref(), sequence_start, &context)
}).await;
operations.finish(&operation_id, generation);
```

### Deterministic merge key

```rust
let key = (
    record.timestamp_millis.unwrap_or(i64::MAX),
    record.source_id.clone(),
    record.sequence,
);
```

## Verification Results

The integration worktree is
`/mnt/e/projects/devbox-worktrees/log-lens-bootstrap-integrated` on
`feat/log-lens/bootstrap-integrated`.

### Focused verification (2026-08-28)

```text
cargo test -p log-lens -j1                          PASS (36 tests)
cargo clippy -p log-lens --all-targets -D warnings PASS
pnpm --filter log-lens test                         PASS (10 tests)
pnpm --filter log-lens build                        PASS
cargo test --workspace -j1                          PASS
cargo check --workspace -j1                         PASS
pnpm test                                           PASS (all workspace frontends)
pnpm build                                          PASS (all workspace frontends)
bash .github/scripts/check-catalog.sh               PASS
check-dependencies.py check                         PASS
cargo fmt --all -- --check                          PASS
git diff --check                                    PASS
```

The full workspace and Windows CI gates are run by the PR workflow after this
focused gate. Windows W3 remains a separately tracked packaged-runtime
checkpoint because WSL/container availability and WebView clipboard/download
semantics cannot be proven by WSL unit tests.

The first full frontend run found the expected catalog-size regression in the
Manager table assertion: 15 catalog apps minus self-managed Manager plus the
table header is 15 rows. The assertion was updated and the complete frontend
suite then passed from a clean rerun.

## Next Steps and Known Limits

- Windows W3 packaged smoke remains: installed/missing WSL and Docker/Podman,
  native file identity and path behavior, packaged download/clipboard, focus,
  and IME composition.
- Run Manager and WSL Desktop producer claim/ack handoff is intentionally not
  included in this bootstrap; the receiver validates only opaque
  `log-source/v1` identity and reports the source unavailable until the planned
  producer integration PR.
- Catalog output `snapshot:log-lens/sources/v1` is intentionally not declared
  because this bootstrap has no publisher implementation. Add it only with the
  producer/publication feature and its contract tests.
- The integration worktree is retained only until its PR passes all six CI
  checks and is merged; the task then removes both final/integration worktrees
  and their local/remote branches according to `AGENTS.md`.

## Post-bootstrap audit follow-up (2026-08-28)

The dedicated worktree was reviewed again against #321's acceptance boundary.
The following bounded-I/O, adapter, parser, lifecycle, and browser-parity
improvements were applied without widening the feature scope:

- Directory reads now mark the snapshot as truncated when more than 256
  matching files exist, and pass the directory's remaining byte budget into
  each member read instead of allocating another full 64 MiB temporary buffer.
  An unreadable member is skipped as a visibly partial directory source while
  cancellation/deadline errors still abort the operation.
- Native process-tree cleanup now falls back to the direct child kill when
  `taskkill`/`kill` exits unsuccessfully, preventing a failed helper command
  from leaving a reader thread or child process behind.
- The operation registry evicts old cancelled entries even when callers reuse
  one generation, so repeated refreshes cannot grow its map beyond the fixed
  tracking limit.
- Windows device namespace paths (`\\.\\`, `//./`, `\\?\\`, `//?/`) and adapter
  boundary whitespace are rejected rather than passed to a different path or
  distro than the one validated.
- Parser normalization preserves escaped newlines/carriage returns/tabs as
  safe record content, replaces other controls, and recognizes journal-style
  `+0900` timestamps plus fractional and ISO-without-zone variants.
- Browser regex guards reject quantified alternations in addition to nested
  quantifiers, and browser merge ordering uses bytewise opaque-ID comparison to
  match Rust's deterministic native ordering rather than locale-dependent
  `localeCompare`.

These follow-up changes are covered by the focused Rust/frontend gates listed
above, including strict Clippy and dependency/catalog consistency checks.

## Latest-main integration audit (2026-08-28)

- Integrated the reviewed candidate onto `origin/main` `2968660` instead of
  reusing the stale mixed candidates. Log Lens files and registration were
  carried forward while the already-merged Devbox Launcher files and docs were
  preserved; unmerged window-state changes remained excluded.
- Preserved the latest catalog capabilities and handoff assertions for existing
  apps, incremented the catalog revision to 9, and kept Log Lens `produces`
  empty until a real snapshot publisher exists.
- Added the missing `@devbox/context-menu` importer entry so the lockfile agrees
  with `apps/log-lens/package.json`, and refreshed Cargo/pnpm lock digests in
  `THIRD_PARTY_NOTICES.md`.
- Added a duplicate-source regression test and native/UI validation. The
  source-list check runs before filesystem or adapter I/O and is shared by
  saved-view validation.
- Directory members now receive a bounded synthetic newline only when the
  previous member lacks one, preventing adjacent files from becoming a single
  record while preserving the aggregate source byte cap; a regression fixture
  covers both newline-less members.
- Directory iterator, metadata, and non-UTF-8 filename failures now mark the
  result partial instead of silently presenting an incomplete directory scan.
- Browser export now escapes tab and other control characters exactly like the
  native exporter, keeping the offline fixture contract byte-compatible for
  copied/exported records.
- Browser timestamp rendering and merge comparison now fail closed for
  JavaScript-safe values outside the ECMAScript `Date` range and compare
  numeric timestamps without precision-losing subtraction.
- Direct review additionally separated same-identity truncate/regrow from file
  rotation in `ReadStatus`, refreshed generated third-party notices from the
  merged lockfiles, and verified all focused gates before PR creation.
