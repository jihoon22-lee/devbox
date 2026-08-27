# Everything+ spreadsheet content extractors

## Overview

Issues `#299`, `#300`, and `#301` complete one user-visible Everything+
spreadsheet content-index feature for legacy XLS, XLSX, and ODS. The PR keeps
three independent extractor versions and parser threat models, while sharing
candidate dispatch, indexing state, completion markers, privacy rules, and
final verification.

The implementation is based on merged PDF extractor commit
`c6327f690deb083ea498bcbdd92628e6fcbe7800` in the dedicated worktree
`/mnt/e/projects/devbox-worktrees/everything-issue299`. It runs entirely on
local bytes using exact, MIT-licensed Rust dependencies. It does not require
Excel, LibreOffice, a converter process, network access, or a post-install
download.

## Why these issues share one PR

The three issues are variants of the same Everything+ action: enable content
indexing on a root and search spreadsheet cell values. They share all of the
following review and rollback boundaries:

- one explicit content-candidate registry and extension dispatch;
- the same 20 MiB file, 2,000,000-character output, 10-second cooperative
  processing, fingerprint/race, sensitive-file, snippet-redaction, and empty
  failure-body contracts;
- one compact format-set lifecycle for startup migration, clear, reindex,
  cancellation, queued full restart, and completion markers;
- one `calamine 0.36.1` value normalization policy and dependency review;
- one final workspace/CI/Windows spreadsheet acceptance pass.

Grouping does not merge the actual parsers. XLS retains CFB/BIFF admission,
XLSX retains OOXML ZIP/relationship/XML admission, and ODS retains
OpenDocument repeat/dense-range admission. Each format has its own version,
fixed errors, focused fixtures, and can be reindexed independently. DOCX,
OCR, semantic search, formula recalculation, macro execution, image/style
preservation, and external-tool integration remain outside this PR.

## Implementation

### 1. Common spreadsheet dispatch and output contract

File: `apps/everything-plus/src-tauri/src/core/content.rs`

- Added case-insensitive, disjoint `.xls`, `.xlsx`, and `.ods` candidates.
- Preserved independent `xls-v1`, `xlsx-v1`, and `ods-v1` versions.
- All three extract worksheet cell values only. Formula source is not
  evaluated or indexed; a cached value supplied by the parser is handled like
  any other cell value.
- The output accumulator inserts tabs between retained cells and newlines
  between retained rows/sheets, truncates on a Unicode scalar boundary, and
  never stores parser errors or paths.
- Password/encryption, malformed structure, resource bounds, unsupported
  encoding, no text, timeout, file-size failure, and changed-during-read remain
  deterministic metadata states. Failure rows keep an empty FTS body so
  filename search still works.
- The full scan and watcher continue to call the same `extract_file` dispatch,
  including opened-file and path fingerprint comparison before committing.

### 2. XLS checkpoint retained

The detailed XLS CFB/BIFF work is recorded in
`workthrough/2026-08-27-everything-plus-xls-extractor.md`. Its core limits are
256 sheets, 4,000,000 cells, 1,000,000 BIFF records, 200,000 shared strings,
8,000,000 unique shared-string characters, 16,000,000 expanded clone
characters, 100,000 formulas, and 256 MiB estimated parser memory. The
preflight detects `FilePass`, invalid record bounds, inverted/sparse
dimensions, bad sheet offsets, SST continuation errors, and `LabelSst` clone
amplification before `calamine::Xls` constructs ranges.

### 3. XLSX streaming extractor

Files: `apps/everything-plus/src-tauri/src/core/content.rs`,
`apps/everything-plus/src-tauri/Cargo.toml`

- Uses `calamine::Xlsx::worksheet_cells_reader` instead of
  `worksheet_range`. An untrusted dimension therefore does not cause this
  integration to request a dense worksheet range.
- Detects password-protected OOXML stored as a CFB `EncryptedPackage` before
  ZIP admission and maps it to `unsupported_encrypted`.
- Applies both dimension-derived logical-cell and actual visited-cell bounds.
  Row and column coordinates are checked again while streaming.
- Contains parser panic with `catch_unwind`; a single malformed workbook
  cannot terminate the indexing worker.

#### OOXML admission before calamine

The preflight is intentionally two-stage:

1. A raw EOCD/ZIP64 envelope reader checks single-disk structure and the
   declared entry count before `ZipArchive::new` can allocate central-directory
   metadata. The limit is 4,096 entries.
2. `ZipArchive` then checks the actual entry count, encryption bit, NUL or
   unenclosed paths, case-normalized duplicate names, per-entry 32 MiB, and
   total uncompressed 64 MiB.

It requires the canonical `_rels/.rels`, `xl/workbook.xml`, and
`xl/_rels/workbook.xml.rels` parts. The package root must contain exactly one
internal office-document relationship targeting `xl/workbook.xml`.
Relationship IDs are unique and bounded; external `TargetMode`, URI-like or
traversal targets, control characters, DTDs, and ambiguous canonical parts are
rejected before calamine.

Bounded XML scans cover workbook, relationship, styles, shared-string, and
worksheet parts. The contract is 256 sheets, 1,048,576 rows, 16,384 columns,
4,000,000 logical/visited cells, XML depth 128, 1,000,000 events per scanned
part, 1,000,000 shared strings, and 8,000,000 shared-string characters.
`sharedStrings.xml`'s `uniqueCount` is checked before calamine can use it as a
reserve hint.

### 4. ODS dense-range admission

Files: `apps/everything-plus/src-tauri/src/core/content.rs`,
`apps/everything-plus/src-tauri/Cargo.toml`

ODS uses the same raw ZIP-envelope and actual-archive stages as XLSX, with
case-sensitive canonical `mimetype`, `META-INF/manifest.xml`, and
`content.xml`. The exact ODS MIME payload is required. Encrypted ZIP entries
and manifest `encryption-data` are mapped to `unsupported_encrypted`; DTDs and
malformed XML fail closed.

Unlike XLSX, `calamine::Ods` materializes worksheet ranges. The preflight
therefore mirrors the allocations that can happen in `calamine 0.36.1`:

- it expands `table:number-rows-repeated` and
  `table:number-columns-repeated` into sheet/row/column/logical-cell totals;
- it counts attribute values, string paragraphs, CDATA/entity content,
  repeated `text:s` spaces, and formula strings that a repeated non-empty cell
  can clone;
- it limits source text to 8,000,000 characters and repeated value/formula
  expansion to 16,000,000 characters;
- it accounts for the moment when calamine retains an old row-oriented vector
  while constructing a new dense vector. The estimate charges two `Data` and
  two `String` slots per logical cell, plus expanded UTF-8 content at a
  conservative four bytes per scalar, and rejects an estimate over 256 MiB.

This specifically closes the case where a small `content.xml` repeats one
long non-empty cell thousands of times. The logical-cell count alone was not
sufficient because every repeated `Data::String` and formula is cloned.

### 5. Generic format-specific reindex lifecycle

Files: `apps/everything-plus/src-tauri/src/core/db.rs`,
`apps/everything-plus/src-tauri/src/commands/indexing.rs`,
`apps/everything-plus/src-tauri/src/lib.rs`

The previous combination-specific enum has been replaced with a bounded
`FormatSet` containing PDF, XLS, XLSX, and ODS bits. The same set drives:

- candidate matching;
- root-scoped row cleanup;
- startup composition of every stale/missing format;
- version-marker recording after completion.

Each spreadsheet owns a `meta.<format>_extractor_version` marker. A missing
marker forces the first rollout scan even when no old row exists; a mismatched
row forces the relevant format again. Only a successful all-roots full or
format-only pass records markers. A partial-root pass, cancellation before or
after the last batch, or error does not claim completion. A root/full request
queued while any format-only worker runs cancels the current pass and promotes
the next pass to `All`, taking a fresh root snapshot.

Tests exercise XLSX-only followed by ODS-only reindex with real temporary
files. Each pass replaces only the selected format hit and preserves ordinary
text and the other spreadsheet hit. Database tests independently verify root
scoping, first-install markers, stale rows, and current rows.

## Security and privacy review

- No archive member is extracted to disk and no relationship target is
  opened. All parsing operates on the already bounded file buffer.
- No formula, macro, VBA project, embedded object, image, style, external
  relationship, DTD, or remote resource is executed or followed.
- Parser panics are contained and raw `io`, ZIP, XML, calamine, path, and
  credential details are not persisted or returned to the UI.
- Sensitive filenames are rejected before read. Extracted text remains only in
  app-local SQLite; UI snippets pass the existing credential/provider-token/
  AWS/JWT redaction and 4,096-character cap.
- The 10-second deadline is cooperative. It is checked throughout ZIP/XML/
  BIFF and cell loops and after parser calls, but Rust cannot safely interrupt
  an in-progress synchronous calamine function. Pre-parser archive,
  structure, dimension, clone, and memory bounds limit admitted work instead
  of claiming a hard preemptive timeout.
- ZIP64 is accepted only with an in-bounds locator/minimum end record and
  single-disk entry counts. Multi-disk packages are not valid spreadsheet
  documents for this feature and fail closed.

## Dependency review

Direct exact dependencies are:

- `calamine = 0.36.1` — MIT, pure-Rust XLS/XLSX/ODS cell-value readers;
- `cfb = 0.7.3` — MIT, legacy XLS and encrypted-OOXML CFB inspection;
- `quick-xml = 0.41.0` with default features disabled and `encoding` enabled —
  MIT, bounded streaming XML preflight;
- `zip = 8.6.0` with default features disabled and `deflate` enabled — MIT,
  in-memory OOXML/ODS ZIP metadata and bounded entry reads.

These crates add no runtime assets, executable, service, network permission,
or user install step. Exact manifest pins and `Cargo.lock` make updates
reviewable. A dependency/parser update requires advisory/license/notices
regeneration, the adversarial fixture suite, and a review of the affected
extractor version.

## Verification

Current integrated evidence uses Linux-native Cargo targets,
`CARGO_INCREMENTAL=0`, and at most two Rust jobs:

```text
cargo check --offline -p everything-plus -j2
passed

cargo test --offline -p everything-plus --lib -j2
85 passed; 0 failed

cargo clippy --offline -p everything-plus --all-targets -j2 -- -D warnings
passed

cargo check --locked --workspace --all-targets -j2
passed

cargo clippy --locked --workspace --all-targets -j2 -- -D warnings
passed

cargo test --locked --workspace --all-targets -j2
passed

cargo fmt --all -- --check
passed
```

The 85-test library pass includes normal XLS/XLSX/ODS values; corrupt,
oversize, timeout, password/encryption, sparse coordinate, external
relationship, duplicate path, declared entry bomb, XML depth, shared-string,
row/column/cell, and repeated non-empty value limits; Unicode-safe truncation;
fixed extractor versions at file boundaries; independent DB markers; and
temporary-root reindex isolation.

The remaining repository gates also passed locally:

- `pnpm --workspace-concurrency 2 build` for all runnable frontends;
- `pnpm --workspace-concurrency 2 test`, including 16 Everything+ frontend
  tests and every other workspace frontend suite;
- catalog consistency, dependency-policy and build-manifest regression tests;
- generated third-party notices exactly matching both lockfiles;
- `pnpm audit --audit-level moderate` with no known vulnerabilities;
- `cargo deny --locked check` with advisories, bans, licenses, and sources all
  accepted (the repository's existing duplicate-version warnings remain
  informational);
- `git diff --check`.

Only the following external evidence remains pending:

- GitHub Actions' six required jobs on the final PR head;
- Windows W2 packaged smoke with real XLS/XLSX/ODS files, watcher mutation,
  cancel/restart, encrypted/corrupt inputs, and log/screen privacy evidence.

The PR must not merge until CI is green. Windows-only packaged acceptance is a
release gate because this WSL environment cannot launch or package Tauri's
Windows application.
