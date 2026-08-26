# Everything+ bounded text content index

- Date: 2026-08-27
- Issue: `#297 feat(everything-plus): text content index 기반`
- Branch: `feat/everything-plus/text-content-index`
- Base: `4e10c30cebaac15e276c32ac7843e1a006ab84d0` (`origin/main`)
- Status: implementation, direct review fixes, local verification, and documentation complete;
  Windows W2 packaged smoke and GitHub CI remain with the parent PR workflow

## Overview

Everything+의 v0.4.1 기반 내용 검색은 UTF-8 `read_to_string`과 1 MiB 제한만 지원해
UTF-16 source/Markdown, 큰 파일의 이유 있는 실패 상태, 파일 변경 중 읽기, 증분 watcher
일관성을 설명할 수 없었다. #297은 외부 문서 도구를 내장하거나 네트워크에 위임하지
않고, 개발자가 오프라인에서도 사용할 수 있는 작은 plain-text/source/Markdown native
extractor를 full index와 watcher가 공유하도록 확장한다.

## Scope and explicit non-scope

Included:

- content-enabled root에서의 explicit source/Markdown/plain-text candidate selection
- strict UTF-8(선택 BOM), UTF-16 LE/BE(BOM 및 보수적인 no-BOM 판정) decoding
- file/text/processing bounds, deterministic failure status, metadata, FTS5 content search
- full/partial indexing, bounded transactions, cooperative cancellation, incremental updates
- sensitive filename skip, snippet redaction, fixed error boundary, stale root/read protection
- frontend status/cancel/busy/a11y behavior, TypeScript/React fixtures, README/roadmap/spec sync

Explicitly not included:

- PDF, DOCX, XLS/XLSX, ODS, archive/ZIP extraction
- OCR, macro execution, formatting/image extraction, semantic/vector search
- external converter/tool download or network content indexing
- saved query, advanced filter, or launcher snapshot work from later P3 scope

## Changes made

### 1. Shared bounded extractor

File: `apps/everything-plus/src-tauri/src/core/content.rs`

- Added the single app-owned extractor used by both full indexing and watcher updates.
- Candidate extensions are an explicit lower-case allowlist rather than “read every file”.
  Named extensionless developer files include `.dockerignore`, `.editorconfig`,
  `.gitattributes`, `.gitignore`, `Dockerfile`, `Makefile`, `LICENSE`, and `README`.
- Constants are intentionally visible to tests and documentation:

```rust
pub const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;
pub const MAX_TEXT_CHARS: usize = 2_000_000;
pub const PROCESSING_LIMIT: Duration = Duration::from_secs(10);
pub const EXTRACTOR_VERSION: &str = "text-v1";
pub const MAX_SNIPPET_CHARS: usize = 4 * 1024;
```

- `extract_file` checks sensitive names, expected size, regular-file type, pre/post size and
  modified time for both the opened handle and path, chunked reads, and cooperative elapsed
  time. Same-size rewrites are therefore rejected instead of being indexed as a coherent read.
  It never returns a filesystem/OS error string.
- `extract_bytes` applies the same byte bound for deterministic fixtures, decodes UTF-8 and
  UTF-16, rejects invalid sequences and NUL-containing binary-like data, and truncates by
  Unicode scalar value rather than byte offset. A truncated success records `text_limit`.
- `ContentStatus` values are `indexed`, `too_large`, `unsupported_encoding`, `read_error`,
  `timeout`, `changed_during_read`, and `skipped_sensitive`.
- Credential-like filename policy skips `.env*`, `.npmrc`, `.netrc`, credentials/secrets
  names, private-key names, and `.pem/.key/.p12/.pfx` before reading. This is deliberately a
  filename fail-closed rule, not an assertion that arbitrary source text can be classified as
  secret.
- FTS snippets are redacted for common Authorization/Bearer/password/token/secret/API-key
  patterns, private-key markers, provider token prefixes, AWS access keys, and JWT shapes, then
  bounded to 4,096 characters on a character boundary.

### 2. SQLite schema and search boundary

Files: `apps/everything-plus/src-tauri/src/core/db.rs`, `core/mod.rs`, `core/models.rs`,
`core/indexer.rs`

- Added `core::content` and moved the text-extension policy out of the obsolete 1 MiB
  indexer helper.
- Bumped the derived-index schema to v2. `file_content` now stores:
  `content_status`, `extractor_version`, `truncated`, `indexed_at`, `error_code`, `encoding`,
  and `text_chars` in addition to the FTS body.
- Legacy databases add missing columns safely, preserve registered roots, clear only derived
  `files`/`file_content` data on schema upgrade, and let setup schedule a re-index.
- `clear_root` escapes SQL `LIKE` wildcard characters so a literal `%` or `_` in a validated
  root cannot clear a sibling path.
- Failed content rows retain status metadata but have an empty FTS body. Content search
  filters to `content_status='indexed'`, caps result limits at 200, and redacts/bounds the
  snippet before constructing the frontend DTO. Filename search remains available for every
  file, including failed or sensitive candidates, and preserves the existing regex prefilter
  contract of up to 2,000 native candidates (200 by default).
- Added status aggregation for indexed/truncated/failed counts and latest indexed timestamp.

### 3. Full index, watcher, and cancellation lifecycle

Files: `commands/indexing.rs`, `commands/watcher.rs`, `core/watcher.rs`, `lib.rs`

- Root input is trimmed, bounded, absolute, control-free, non-traversal, existing, directory,
  non-symlink, and canonicalized before watcher/DB registration. Persisted roots are validated
  again during watcher restore.
- Full/partial scans use 250-file transactions, keep filesystem walking and content reads out
  of the DB mutex, and publish fixed `indexing_failed` state rather than raw path/SQLite/OS
  detail. Search/status commands use fixed safe errors and bounded query/result inputs.
- `cancel_index` stops at file/batch boundaries. A cancelled run leaves only committed partial
  rows and a later `Re-index` converges the whole root. Events received during a full scan are
  re-armed in the debouncer instead of silently dropped.
- The worker lifecycle has a mutex around start/finish/restart transitions. A root change or
  second re-index request therefore cannot race the final `indexing=false` store and strand a
  pending restart. A post-read root revalidation also prevents an update from resurrecting a
  file after its root was removed while extraction was in progress.
- Native indexer thread creation failure now returns the lifecycle to idle and records only the
  fixed `indexing_failed` status instead of panicking or leaving the UI permanently busy.
- `ContentRecord` is the one write DTO for full and incremental paths, so encoding, bounds,
  metadata, and failure semantics cannot drift between them.

### 4. Frontend contract and accessibility

Files: `apps/everything-plus/src/App.tsx`, `src/api.ts`, `src/types.ts`, `src/App.css`,
`src/App.test.tsx`

- Extended the native/TypeScript `IndexStatus` DTO with cancellation, content counters, last
  error, and timestamp fields; added the `cancel_index` invoke wrapper.
- Search input applies UTF-8 4 KiB and control-character checks before debounce/invoke and never
  interpolates the raw query into empty/error UI. Backend failures are mapped to fixed UI
  messages. Stale search responses remain sequence-guarded.
- Status uses `aria-live`, mode tabs use `aria-pressed`, search has a mode-specific accessible
  label, and the action button exposes `Cancel`/`Re-index`, `aria-busy`, and disabled state.
- Re-index/cancel has an explicit busy guard against double action. Metadata updates are guarded
  after unmount, while the existing pending-open and search effects retain their cancellation
  cleanup. StrictMode cleanup resets the mounted guard correctly.
- Added/updated fixtures for the expanded status mock, cancellation UI, and fixed result/error
  behavior without exposing a raw backend message.

### 5. Documentation

Files: `apps/everything-plus/README.md`, `docs/roadmap.md`,
`docs/architecture.md`, `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`

- Documented the native-first BASE boundary, exact bounds/status fields, privacy policy,
  cancellation/recovery behavior, migration behavior, and shared full/incremental extractor.
- Kept the planned PDF/Office item in the roadmap and plan as a later format-specific feature;
  #297 does not silently claim to implement it.
- Clarified the Everything+ architecture flow as validated roots → bounded extractor → FTS5
  and metadata → React, with no external tool/network dependency.

No dependency, lockfile, license notice, IPC payload secret, telemetry, or external state
changes were added.

## Verification

All commands were run from the dedicated worktree. Cargo used an isolated Linux-native target
directory, disabled incremental artifacts, and at most two build jobs.

```text
cargo test --locked -p everything-plus --lib -j2
running 47 tests
test result: ok. 47 passed; 0 failed

cargo check --locked -p everything-plus -j2
Finished `dev` profile

cargo clippy --locked -p everything-plus --lib --all-targets -j2 -- -D warnings
Finished `dev` profile

pnpm --filter everything-plus exec tsc --noEmit
exit code: 0

pnpm --filter everything-plus exec vitest run --config vite.config.ts \
  --pool=threads --maxWorkers=2 --no-file-parallelism
Test Files 2 passed; Tests 16 passed

pnpm --filter everything-plus build
✓ 43 modules transformed; built successfully

cargo test --locked --workspace -j2
all workspace unit, integration, and doc tests passed

cargo check --locked --workspace -j2
Finished `dev` profile

pnpm -r --workspace-concurrency=2 build
17 workspace projects built successfully

python3 .github/scripts/check-dependencies.py check
dependency policy OK; notices match Cargo.lock and pnpm-lock.yaml

python3 .github/scripts/test-check-dependencies.py
dependency policy regression tests passed

python3 .github/scripts/test-build-manifest.py
build-manifest notice tests passed

bash .github/scripts/check-catalog.sh
passed

pnpm audit --audit-level moderate
No known vulnerabilities found

git diff --check
passed
```

The first repository-wide frontend build attempt exposed that this dedicated worktree had not
created the two app-local symlinks for dependencies already present in the frozen lockfile.
`pnpm install --frozen-lockfile` repaired only `node_modules` (no source or lockfile change), and
the complete 17-project build then passed.

Focused Rust coverage includes UTF-8 English/Korean, UTF-16 LE/BE with and without BOM,
empty input, invalid/binary data, 20 MiB and 2M-character bounds, timeout, sensitive names,
assignment/provider/JWT/AWS snippet redaction and output cap, same-size metadata races, distinct
filename/content result caps, metadata filtering, wildcard-safe root clearing, schema upgrade,
and an end-to-end temporary-root full + incremental fixture. Frontend coverage includes
listener-first app-link behavior, stale response discard, result-menu actions, fixed errors,
and the cancellable indexing status surface.

## Remaining release work

- Run the required Windows W2 packaged smoke with real Windows UTF-8/UTF-16 files, watcher
  updates, cancellation, Explorer/open actions, and evidence that logs/screens never contain
  raw credentials or OS paths from failures.
- Parent PR workflow must run the repository CI/overall release gate, then review/merge. This
  worktree was intentionally not pushed and no PR was opened here.
- Future PDF/Office/OCR/semantic work must use separate issue/PR boundaries, format-specific
  extractor versions, and independently reviewed archive/ZIP/resource bounds.
