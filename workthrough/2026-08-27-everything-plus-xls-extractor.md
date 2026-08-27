# Everything+ legacy XLS text extractor

## Overview

Issue `#299` adds the XLS-only follow-up to the Everything+ content index from
`#297` and `#298`. The implementation uses the MIT-licensed, pure-Rust
`calamine::Xls` reader offline for legacy `.xls` worksheet values, preserves
the existing content-status contract, and reindexes only XLS rows when the
independent `xls-v1` extractor changes.

The work is rebased on the merged PDF extractor commit
`c6327f690deb083ea498bcbdd92628e6fcbe7800` in the dedicated worktree
`/mnt/e/projects/devbox-worktrees/everything-issue299` on
`feat/everything-plus/xls-extractor`. Issue #299 does not make XLSX/ODS enter
the legacy BIFF parser; those format acceptances remain isolated. The final PR
will nevertheless close #299, #300, and #301 together because the three
spreadsheet extractors share one user-facing content-index flow and common
reindex/resource policy. DOCX, formula recalculation,
VBA/macro execution or preservation, OCR, styles/images, semantic search, and
external tools remain outside this issue.

## Context

The content index has explicit allowlists and shared 20 MiB file,
2,000,000-Unicode-scalar text, and 10-second candidate boundaries. Issue
`#299` requires legacy binary Excel text extraction without widening that
privacy boundary. A workbook can be malformed, password protected, or sparse
with hostile BIFF dimensions, so failure must leave an empty FTS body while
retaining a fixed status and the filename hit.

## Changes Made

### 1. Bounded, offline XLS extraction

File: `apps/everything-plus/src-tauri/src/core/content.rs`

- Added case-insensitive `.xls` candidate detection and a dedicated
  `XLS_EXTRACTOR_VERSION` (`xls-v1`). `.xlsx` and `.ods` do not dispatch here.
- Added `calamine::Xls` extraction of worksheet cell values only. It does not
  evaluate formulas, open VBA projects, render images/styles, or follow
  external resources. Cached cell values are treated as parser output; no
  formula recalculation is performed.
- Added fixed `unsupported_encrypted` handling for encrypted workbooks,
  `resource_limit` for structural/allocation bounds, and `extract_error` for
  malformed/unsupported/parser failures.
  `catch_unwind` contains parser panics so one workbook cannot terminate the
  indexing worker, and no parser detail or source path is persisted/logged.
- Added a fail-closed pure-Rust `cfb`/BIFF preflight for the already-read
  Workbook stream. It rejects malformed containers and record ranges before
  calamine entry, detects FilePass before structural resource classification,
  validates non-inverted Dimensions and sparse coordinates, and bounds sheets,
  records, metadata, formulas, logical ranges, estimated parser memory, unique
  SST text, and `LabelSst` clone amplification. A post-parse range check remains
  as a second guard.
- Added bounded cell-to-text accumulation with tab/newline separators,
  Unicode-safe `text_limit` truncation, and timeout checks. Empty workbooks
  produce `no_text`; all failure records contain an empty content body.
- Added a source-reviewable test fixture at
  `apps/everything-plus/src-tauri/fixtures/biff5_write.xls.b64` with
  attribution and test-only usage documented in `fixtures/README.md`.

Key contract:

```rust
pub const XLS_EXTRACTOR_VERSION: &str = "xls-v1";
pub const XLS_MAX_SHEETS: usize = 256;
pub const XLS_MAX_CELLS: usize = 4_000_000;
pub const XLS_MAX_RECORDS: usize = 1_000_000;
pub const XLS_MAX_SHARED_STRINGS: usize = 200_000;
pub const XLS_MAX_SHARED_STRING_CHARS: usize = 8_000_000;
pub const XLS_MAX_EXPANDED_STRING_CHARS: usize = 16_000_000;
pub const XLS_MAX_FORMULAS: usize = 100_000;
pub const XLS_MAX_ESTIMATED_MEMORY_BYTES: usize = 256 * 1024 * 1024;

pub fn extract_xls_bytes(bytes: &[u8], started: Instant) -> ContentRecord {
    // file/time/preflight bounds, calamine Xls parsing, fixed statuses,
    // worksheet values, and the shared Unicode text limit
}
```

### 2. Format-specific persistence and reindex

Files: `apps/everything-plus/src-tauri/src/core/db.rs`,
`apps/everything-plus/src-tauri/src/commands/indexing.rs`,
`apps/everything-plus/src-tauri/src/lib.rs`,
`apps/everything-plus/src-tauri/src/core/watcher.rs`

- Added `clear_xls`, `xls_reindex_required`, and the independent persistent
  `meta.xls_extractor_version` completion marker. The marker is required even
  when no XLS row exists, so first rollout and clear-then-cancel both retry.
  Successful full/XLS scans record it; partial or cancelled scans do not.
- Added `IndexFilter::Xls` and `IndexFilter::PdfAndXls`, including startup
  dispatch when either or both format versions are stale. Full scans and the
  watcher use the same candidate predicate and extractor dispatch. A queued
  root/full request escalates every format-only worker to `All`.
- Added DB and temporary-root integration tests proving that XLS replacement
  removes the old cell hit, adds the new cell hit, preserves ordinary text,
  and leaves a corrupt workbook's filename/status visible.

The reindex boundary is intentionally narrow:

```rust
match filter {
    IndexFilter::Xls => {
        for root in &roots {
            clear_xls(&conn, &root.path)?;
        }
    }
    IndexFilter::PdfAndXls => {
        // clear_pdf and clear_xls; ordinary text rows remain intact
    }
    _ => {}
}
```

### 3. Dependency, documentation, and notices

Files: `apps/everything-plus/src-tauri/Cargo.toml`, `Cargo.lock`,
`THIRD_PARTY_NOTICES.md`, `apps/everything-plus/README.md`,
`docs/roadmap.md`, `docs/architecture.md`,
`docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`

- Added exact `calamine = 0.36.1` and `cfb = 0.7.3` dependencies. Both are
  pure Rust and pinned in `Cargo.lock`; the generated notice inventory records
  the full transitive dependency graph and digests.
- Updated README, architecture, roadmap, and the P2-08 spec status with the
  XLS boundary, statuses, resource limits, privacy behavior, fixture policy,
  and independent reindex contract.
- No network, converter process, telemetry, raw credential, secret payload,
  unsafe path, or external state mutation was introduced.

#### Dependency review

- **Purpose:** `calamine::Xls` supplies the bounded, offline legacy BIFF value
  parser; `cfb` performs an independent fail-closed container/Workbook-stream
  admission check before calamine allocates parser structures.
- **Alternatives:** invoking Excel or LibreOffice was rejected because it adds
  installation, process, license-distribution, and offline availability
  requirements. A new in-repository BIFF value parser would duplicate a large
  mature format implementation; using calamine behind a local structural
  preflight keeps this issue limited to text extraction.
- **Source and pin:** crates.io/GitHub upstreams
  `tafia/calamine` and `mdsteele/rust-cfb`, pinned exactly to `calamine 0.36.1`
  (`Cargo.lock` checksum
  `5fa68281b1a76b54a62156474adb06bb380a67e07dd60656e3217152b42183f3`)
  and `cfb 0.7.3` (checksum
  `d38f2da7a0a2c4ccf0065be06397cc26a81f4e528be095826eee9d4adbb8c60f`).
- **License and advisory:** both direct dependencies are MIT. The complete
  locked graph passes `deny.toml`/dependency-policy checks; no accepted
  advisory exception was added.
- **Install size:** neither dependency adds a runtime asset or download; code
  is statically linked. Registry archives are 150 KiB and 62 KiB respectively
  (824 KiB and 520 KiB unpacked source, not shipped). Final Windows bundle
  delta remains part of W2 packaging evidence.
- **Offline/update:** all parsing runs on local bytes with no network or
  external executable. The Everything+ maintainer owns updates; a version bump
  requires the same adversarial fixtures, dependency policy/notices refresh,
  and an `XLS_EXTRACTOR_VERSION` review.

## Verification Results

This is the XLS checkpoint before integrating the XLSX/ODS commits. Commands
were run from `/mnt/e/projects/devbox-worktrees/everything-issue299`.
Rust commands use `CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-issue299`,
`CARGO_INCREMENTAL=0`, and `-j2`; the recursive frontend build uses
`--workspace-concurrency 2`.

```text
cargo fmt --all -- --check
passed

cargo test --manifest-path apps/everything-plus/src-tauri/Cargo.toml -j2
70 passed; 0 failed

cargo check --locked --workspace --all-targets -j2
passed

cargo clippy --locked --workspace --all-targets -j2 -- -D warnings
passed

cargo test --locked --workspace --all-targets -j2
passed

pnpm --workspace-concurrency 2 build
all 17 runnable workspace builds completed successfully

pnpm --filter everything-plus test
16 passed; 0 failed

python3 .github/scripts/check-dependencies.py check
dependency policy OK; notices match Cargo.lock and pnpm-lock.yaml

python3 .github/scripts/test-check-dependencies.py
dependency policy regression tests passed

bash .github/scripts/check-catalog.sh
passed

cargo deny check
advisories, bans, licenses, and sources passed; existing duplicate warnings only

git diff --check
passed
```

The Rust suite covers normal cell extraction, `.xlsx`/`.ods` exclusion,
corrupt/password/oversize/timeout inputs, Unicode text limits, sparse BIFF
dimension preflight, format-specific DB cleanup/stale detection, and
integration reindex isolation. A recursive frontend test attempt was stopped
after the PR boundary changed to include #300/#301; the complete frontend and
Rust workspace gates will run once on the final integrated branch rather than
three times. Windows W2 packaged smoke was not run because this WSL environment
has no Windows `cargo`, `rustc`, or `pnpm` on `PATH`.

## Next Steps

- Integrate #300 XLSX and #301 ODS behind a generic format set without allowing
  either format to bypass the XLS CFB/BIFF admission boundary.
- Run the final integrated workspace gates and Windows W2 packaged smoke with
  real `.xls`, `.xlsx`, and `.ods` files, watcher updates, cancellation, and
  evidence that logs/screens contain no raw credential or unsafe path/error
  details.
- Open one PR with separate `Closes #299`, `Closes #300`, and `Closes #301`
  acceptance/verification sections, then merge only after every CI job passes.
