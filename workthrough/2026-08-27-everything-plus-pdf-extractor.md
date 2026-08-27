# Everything+ PDF text extractor

## Overview

Issue `#298` adds the PDF-only follow-up to the Everything+ content index from
`#297`. The implementation uses the MIT-licensed `lopdf` parser offline for
text objects, keeps PDF metadata/versioning separate from the plain-text
extractor, and isolates unsupported or malformed documents without affecting
filename search.

The workthrough was rebased onto main `40b8673` on the dedicated branch
`feat/everything-plus/pdf-extractor`.
DOCX/XLS/XLSX/ODS, OCR, image/format extraction, macro execution, semantic
search, and external tools remain outside this issue.

## Context

The base content index deliberately accepted only bounded plain text and
source/Markdown files. P2-08 calls for a separate PDF format extractor with
the same privacy and resource-boundary guarantees, plus explicit handling for
image-only scans, encrypted/password documents, and corrupt input. A parser
version change must not discard or reread unrelated text/source/Markdown rows.

## Changes Made

### 1. Bounded PDF extractor

File: `apps/everything-plus/src-tauri/src/core/content.rs`

- Added PDF candidate detection and dispatch for case-insensitive `.pdf`
  paths; the existing explicit text allowlist remains unchanged.
- Added the independent `pdf-v1` extractor version and `NoText`,
  `UnsupportedEncrypted`, and `ExtractError` content statuses.
- Added `lopdf` byte loading with a 16 MiB decompressed page/object-stream
  limit plus 100,000 parsed indirect-object and 10,000-page structural bounds.
  The shared 20 MiB file, 2,000,000 Unicode scalar-character, and cooperative
  10-second candidate limits still apply. Object/page bound failures retain an
  empty body with `content_status=extract_error` and fixed
  `error_code=resource_limit`.
- Uses only PDF text objects. It does not render pages, run OCR, read images,
  follow external resources, execute document content, or expose parser/IO
  error details.
- Image-only/scanned PDFs produce `no_text`; encrypted/password PDFs produce
  `unsupported_encrypted`; malformed or extraction failures produce
  `extract_error`. Failed records contain an empty FTS body.
- Added deterministic lopdf-generated text, scan, encrypted, corrupt,
  oversize, and timeout fixtures.

Key contract:

```rust
pub const PDF_EXTRACTOR_VERSION: &str = "pdf-v1";
pub const PDF_MAX_DECOMPRESSED_BYTES: usize = 16 * 1024 * 1024;
pub const PDF_MAX_OBJECTS: usize = 100_000;
pub const PDF_MAX_PAGES: usize = 10_000;

pub fn extract_pdf_bytes(bytes: &[u8], started: Instant) -> ContentRecord {
    // file/time bounds, bounded lopdf load, encrypted/no-text/error statuses,
    // page text extraction, and the shared character limit
}
```

### 2. Format-specific persistence and reindex

Files: `apps/everything-plus/src-tauri/src/core/db.rs`,
`apps/everything-plus/src-tauri/src/commands/indexing.rs`,
`apps/everything-plus/src-tauri/src/lib.rs`,
`apps/everything-plus/src-tauri/src/core/watcher.rs`

- `ContentRecord` now carries its extractor version; DB upserts persist that
  value instead of assuming every row is `text-v1`.
- Added `pdf_reindex_required` and the `meta.pdf_extractor_version` marker.
  A missing/mismatched marker guarantees the first PDF scan on installation and
  parser-version transition even when no PDF content row exists; a stale row is
  also detected. A successfully completed full/PDF scan records the marker.
- Added a `Pdf` indexing filter. PDF-only reindex clears and rebuilds PDF rows
  while preserving ordinary content rows and filename rows for all files. If a
  new root/index request is queued while a PDF-only worker runs, the queued
  restart promotes its filter to `All` so the request cannot be lost.
- Full and watcher paths continue to share the same extractor dispatch and
  fixed error boundary. The watcher eligibility helper now delegates to the
  same content-candidate registry, including PDF; the legacy text-extension
  facade remains only for compatibility with existing internal references.
- Added DB and temporary-root integration fixtures proving PDF replacement
  does not remove a Markdown/text hit.

The reindex boundary is intentionally narrow:

```rust
match filter {
    IndexFilter::All => clear_all(&conn)?,
    IndexFilter::Pdf => {
        for root in &roots {
            clear_pdf(&conn, &root.path)?;
        }
    }
}
```

### 3. Dependency, documentation, and notices

Files: `apps/everything-plus/src-tauri/Cargo.toml`, `Cargo.lock`,
`THIRD_PARTY_NOTICES.md`, `apps/everything-plus/README.md`,
`docs/roadmap.md`, `docs/architecture.md`,
`docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`

- Added exact `lopdf = 0.44.0` with default features disabled. Cargo lock and
  generated notices record the MIT crate and its transitive dependencies.
- Updated the app README, architecture, roadmap, and P2-08 status to document
  the PDF-only boundary, limits, statuses, privacy behavior, and separate
  reindex contract.
- No network access, external converter, telemetry, raw path, secret, or
  credential payload is introduced.

## Verification Results

All commands ran from `/mnt/e/projects/devbox-worktrees/everything-pdf-extractor`.
Rust commands use `CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-issue298`,
`CARGO_INCREMENTAL=0`, and `-j2`; frontend test workers were capped at two.

```text
cargo fmt --all -- --check
passed

cargo test --locked -p everything-plus --all-targets -j2
57 passed; 0 failed

cargo check --locked -p everything-plus -j2
finished successfully

cargo clippy --locked -p everything-plus --all-targets -j2 -- -D warnings
finished successfully

cargo check --workspace --locked -j2
cargo clippy --workspace --all-targets --locked -j2 -- -D warnings
cargo test --workspace --locked -j2
all workspace gates passed

pnpm --filter ./apps/everything-plus build
pnpm --filter ./apps/everything-plus test -- --maxWorkers=2 --no-file-parallelism
pnpm --filter ./apps/everything-plus exec tsc --noEmit
build/typecheck passed; 16 tests passed

pnpm install --frozen-lockfile
pnpm build
all 17 runnable workspace builds passed

python3 .github/scripts/check-dependencies.py check
python3 .github/scripts/test-check-dependencies.py
python3 .github/scripts/test-build-manifest.py
bash .github/scripts/check-catalog.sh
pnpm audit --audit-level moderate
cargo deny --locked check
dependency policy, notices, regression fixtures, catalog, audit, advisories,
bans, licenses, and sources passed

git diff --check
passed
```

The focused suite covers PDF text extraction, image-only scan, password and
empty-password encryption, corrupt bytes including a false `/Encrypt` marker,
20 MiB/16 MiB/100,000-object/10,000-page/2M-character/10-second limits,
`resource_limit` metadata, first-install/version-marker reindex, queued
PDF-to-All promotion, watcher eligibility, privacy, failure isolation, and
deterministic temporary-root fixtures. The isolated Cargo target occupied about
21 GiB after the full workspace gates.

Windows W2 packaged smoke remains a release-gate checkpoint and was not run:
this WSL environment has no Windows `cargo`, `rustc`, or `pnpm` on `PATH`.

## Remaining release work

- Run Windows W2 packaged smoke with real Windows files, watcher updates,
  cancellation, and evidence that logs/screens contain no raw credentials or
  unsafe path/error details.
- Require all repository PR CI checks, including the Windows Rust compile gate,
  before merge.
