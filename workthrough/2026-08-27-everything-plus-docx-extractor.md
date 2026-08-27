# Everything+ DOCX content extractor

## Overview

Issue `#302` adds bounded, offline `.docx` main-document text indexing to
Everything+. The extractor uses the repository's already pinned MIT Rust ZIP
and XML libraries, so users do not need Microsoft Office, LibreOffice, a
converter executable, a network connection, or a post-install download.

This stays a standalone PR even after adopting the repository's cohesive
multi-issue PR policy. DOCX shares the content-index lifecycle with PDF and
spreadsheets, but WordprocessingML package relationships, text semantics,
failure fixtures, version marker, and rollback are an independently
reviewable parser boundary. Legacy DOC, macro-enabled DOCM, OCR, image/style
extraction, non-main document parts, and semantic search are outside `#302`.

## User-visible contract

- A content-enabled root now admits `.docx` case-insensitively.
- Searchable text comes from `word/document.xml` `w:t` nodes. Paragraphs,
  tabs, and explicit line breaks retain searchable separators.
- Hyperlink labels remain searchable. An external hyperlink relationship may
  be validated as bounded metadata, but its URI is never opened, fetched,
  indexed, or returned.
- Field instructions are not indexed. Headers, footers, footnotes, comments,
  images, styles, embedded objects, and macros are not read or executed.
- Empty main-document text produces `no_text`; corrupt, unsupported, or
  resource-limited packages retain filename search and an empty FTS body.
- Password-protected OOXML CFB containers and encrypted ZIP entries produce
  `unsupported_encrypted`.
- The existing 20 MiB file, 2,000,000 Unicode-scalar retained text, 10-second
  cooperative deadline, sensitive-filename skip, file fingerprint, snippet
  redaction, and app-local-only persistence policies remain authoritative.

## Implementation

### Candidate dispatch and versioning

File: `apps/everything-plus/src-tauri/src/core/content.rs`

- `is_docx_ext` and `is_docx_path` accept only `.docx`; `.doc` and `.docm`
  remain non-candidates.
- `extract_file` selects `DOCX_EXTRACTOR_VERSION = "docx-v1"` before any
  size, sensitive-name, read, race, or timeout failure. A DOCX failure never
  falls back to generic text or spreadsheet auto-detection.
- `extract_docx_bytes` contains parser panics and maps failures to fixed
  status/error codes without returning a source path, XML, ZIP, or parser
  diagnostic.

### ZIP admission before XML parsing

The extractor works on the already bounded in-memory file buffer and never
extracts an archive member to disk.

1. The shared raw EOCD/ZIP64 admission verifies an unambiguous single-disk
   package and caps the declared entry count at 4,096 before
   `ZipArchive::new` can allocate central-directory metadata.
2. The actual archive is scanned again for at most 4,096 entries, encrypted
   flags, NUL/backslash/unenclosed names, case-insensitive duplicate names,
   per-entry 32 MiB, and total uncompressed 64 MiB.
3. Canonical `[Content_Types].xml`, `_rels/.rels`, and `word/document.xml`
   parts are required. `word/_rels/document.xml.rels` is optional and, when
   present, is only scanned within the same limits; no target is opened.
4. Each required part is read with its own byte cap and cooperative deadline
   checks. The reader rejects an entry that grows beyond its admitted size
   limit while streaming.

### Package and XML trust boundary

- `[Content_Types].xml` must contain exactly one ordinary WordprocessingML
  main-document override for `/word/document.xml`. A macro-enabled main type
  is rejected with the fixed `unsupported_document` code.
- `_rels/.rels` must have exactly one internal office-document relationship
  targeting `word/document.xml`. URI-like, external, traversal, control,
  percent-encoded, empty, or ambiguous package targets fail closed.
- Relationship IDs are unique and bounded. DTDs are rejected and XML is never
  allowed to fetch or resolve an external entity.
- Every scanned part is bounded by XML depth 128, 1,000,000 events, an
  8,000,000-unit aggregate of decoded text Unicode scalars plus raw attribute
  bytes, and 4,096 relationships where applicable. Counting attributes by
  byte is deliberately stricter for non-ASCII metadata than a scalar count.
- DOCX WordprocessingML has no spreadsheet-style shared-string table. The
  issue's generic shared-string/OOM requirement is therefore satisfied here
  by the main-part source-text/attribute cap plus the independent retained
  output cap, rather than by inventing a non-existent DOCX SST parser.

### Text normalization

The WordprocessingML reader is streaming and retains only `w:t` content.
`w:tab` queues a tab, `w:br`/`w:cr` queue a newline, and the end of `w:p`
queues a paragraph newline. Separators are emitted only before later retained
text, so empty structure and the end of a document do not create a false text
hit. General XML character references accept the five built-in entities and
valid decimal/hexadecimal XML 1.0 scalar references. Literal/encoded forbidden
XML controls, DTDs, and unknown references fail closed.

The accumulator stops at exactly 2,000,000 Unicode scalar values and returns
an indexed, Unicode-safe prefix with `truncated=true` and `text_limit`.
Whitespace-only output is `no_text`. Field instruction text uses a different
element and is intentionally absent from the FTS body.

## Format-specific reindex lifecycle

Files:

- `apps/everything-plus/src-tauri/src/core/db.rs`
- `apps/everything-plus/src-tauri/src/commands/indexing.rs`
- `apps/everything-plus/src-tauri/src/core/watcher.rs`
- `apps/everything-plus/src-tauri/src/lib.rs`

DOCX adds one bit to the existing compact `FormatSet`. The same bit drives
candidate matching, root-scoped cleanup, startup stale-format composition,
and completion-marker recording.

- `meta.docx_extractor_version` missing or different from `docx-v1` forces the
  first DOCX scan even when no old DOCX row exists.
- A stale DOCX `file_content` row also forces the scan after the marker is
  current.
- Only a successful all-root full or DOCX-only pass records the marker.
  Partial-root and cancelled passes do not claim completion.
- A new root or user full-index request queued during a format-only pass
  escalates the next pass to `All`.
- DOCX-only clear/reindex removes only `.docx` files below the selected root;
  plain text, PDF, XLS, XLSX, ODS, sibling-root rows, and their markers remain
  independent.
- The watcher uses the same case-insensitive candidate registry and extraction
  function as a full scan.

## Security and privacy review

- No network API, Office process, converter, script, formula, macro, embedded
  object, relationship target, image renderer, or archive filesystem write is
  reachable from this feature.
- Password-protected Office CFB input is identified by the standard
  `EncryptedPackage` stream before ZIP admission. Malformed CFB input simply
  continues to the ordinary corrupt-package path.
- ZIP metadata is bounded both before and after archive construction. XML is
  bounded before retained output, and parser panics are contained at the
  per-file boundary.
- Sensitive filenames are rejected before opening the file. Extracted content
  is stored only in the existing app-local SQLite FTS table; it is not copied
  into logs, telemetry, app-link snapshots, or another app.
- Search snippets still pass Authorization/Bearer/password/token/private-key,
  provider-token, AWS-key, and JWT redaction plus the 4,096-character output
  cap. Failure records contain no source bytes.
- The 10-second limit is cooperative rather than a hard thread preemption.
  It is checked around ZIP entry reads and each XML/text loop; the initial
  bounded 20 MiB CFB probe and individual synchronous library operations cannot
  be interrupted safely. File, archive, entry, XML, relationship, and output
  caps bound admitted work instead of claiming a hard deadline.

## Dependency review

No dependency or lockfile change is required. DOCX reuses:

- `zip = 8.6.0`, default features disabled, `deflate` enabled — MIT;
- `quick-xml = 0.41.0`, default features disabled, `encoding` enabled — MIT;
- `cfb = 0.7.3` — MIT, used only to identify standard encrypted OOXML CFB.

All are exact direct dependencies already reviewed for the spreadsheet PR and
already represented in `Cargo.lock` and generated third-party notices. They
add no runtime executable, service, network permission, or user install step.

## Deterministic fixtures

The Rust suite covers:

- case-insensitive `.docx` admission and explicit `.doc`/`.docm` exclusion;
- Korean/English text, XML entity decoding, paragraph/tab/line-break
  separators, and field-instruction exclusion;
- empty/whitespace-only, corrupt, oversized, pre-expired timeout, ZIP
  encrypted flag, and CFB `EncryptedPackage` inputs;
- external and traversal package targets, macro-enabled content type,
  case-duplicate canonical part, DTD, and excessive XML nesting;
- declared ZIP entry-count and entry-size bombs before parser work;
- an actually inflated high-compression main XML part that exceeds the source
  text budget, in addition to forged central-directory metadata;
- exact Unicode output truncation and sensitive-filename/snippet privacy;
- root- and format-scoped database cleanup, first-install/stale marker
  behavior, successful/cancelled completion markers, watcher candidate
  parity, and a real temporary-root DOCX-only reindex that replaces the DOCX
  hit while preserving text/XLSX hits and a corrupt filename row.

## Verification

Local checks used a Linux-native Cargo target, `CARGO_INCREMENTAL=0`, at most
two Rust jobs, and at most two concurrently built frontend workspaces. The
final reviewed branch head passed:

```text
cargo test -p everything-plus docx
11 passed; 0 failed

cargo test -p everything-plus docx_reindex_replaces
1 passed; 0 failed

cargo fmt --all -- --check
passed

cargo check --locked -p everything-plus --all-targets -j2
passed

cargo test --locked -p everything-plus --all-targets -j2
96 passed; 0 failed

cargo clippy --locked -p everything-plus --all-targets -j2 -- -D warnings
passed

cargo check --locked --workspace --all-targets -j2
passed

cargo test --locked --workspace --all-targets -j2
passed

cargo clippy --locked --workspace --all-targets -j2 -- -D warnings
passed

pnpm --filter everything-plus test
16 passed; 0 failed

pnpm --filter everything-plus build
passed

pnpm --workspace-concurrency 2 -r build
passed for all frontend workspace projects

pnpm --workspace-concurrency 1 --filter '!code-pad' -r test
passed for context-menu and all 12 non-Code-Pad apps

pnpm --filter code-pad exec vitest run --passWithNoTests --maxWorkers=2
14 files, 118 tests passed

python3 .github/scripts/check-dependencies.py check
passed; generated notices match the lockfiles and policy

python3 .github/scripts/test-check-dependencies.py
passed

python3 .github/scripts/test-build-manifest.py
passed

.github/scripts/check-catalog.sh
passed

pnpm audit --audit-level moderate
no known vulnerabilities

cargo deny --locked check
advisories, bans, licenses, and sources passed; configured duplicate-version
warnings were informational
```

The first concurrent frontend workspace run completed every production build,
then Code Pad's Vitest pool reported a worker-start timeout while all 115 tests
that had executed were green. It was running beside the full Rust compilation,
so Code Pad was rerun alone with two workers and passed all 118 current tests.
All other frontend tests then passed sequentially, confirming a resource-runner
failure rather than a product regression.

Remaining external gates are GitHub Actions' six required jobs and the Windows
W2 packaged checkpoint. Windows-only evidence must verify a real Korean DOCX,
empty/corrupt/encrypted fixtures, watcher mutation, cancel/restart, filename
fallback, and absence of raw content/path in logs/screens. This WSL host has
Windows PowerShell and Node forwarding but no Windows pnpm/Rust/Tauri toolchain,
so installing a large host toolchain was not silently added to this feature.
As with the spreadsheet extractor PR, packaged W2 remains a release checkpoint;
the PR does not merge until all required CI jobs are green.
