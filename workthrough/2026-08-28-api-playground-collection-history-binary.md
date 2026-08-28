# API Playground Collection, History, and Binary Response Workthrough

## Overview

This bounded implementation pass groups GitHub issues #346, #347, and #348 into one cohesive
API Playground PR candidate. It adds offline-first Collection/Environment JSON transfer, safe
History search and filtering, and bounded binary response projection with an explicit native save
path. The implementation stays inside `apps/api-playground`; it does not add an external tool,
network service, shared crate, workspace/catalog change, or cross-app integration.

## Context

The existing app already had v2 History/Collection persistence, environment references, native
HTTP cancellation, redacted response metadata, and a current-response raw-header vault. The three
new issues needed to build on those boundaries without turning export into a secret backup, making
History search inspect request payloads, or exposing arbitrary response bytes to the renderer.

The worktree was created from `origin/main` commit
`14716d0d3963da5f98fe5cd09e1320e36992980b` on branch
`feat/api-playground/collection-history-binary`, then rebased cleanly onto `origin/main`
`4c55be0871dd9340bfb7c8d51c4ac88a9888134b` immediately before final validation. The root
worktree and unrelated app changes were left untouched. No dependency or lockfile changes were made.

## Changes Made

### 1. Versioned, bounded Collection and Environment transfer (#346)

- Added `apps/api-playground/src/lib/transfer.ts` with strict schemas
  `devbox.api-playground.collection-export` and `devbox.api-playground.environment-export`, both
  at `schema_version: 1`.
- Added 1 MiB document, 256 Collection, 64 Environment, 256 variable, 120-character name,
  64 KiB field, 100-row request, and 50-part multipart bounds.
- Added allowlisted nested request validation. Unknown fields, invalid schema/version, malformed
  JSON, duplicate environment keys, and over-limit documents are rejected before application.
- Export re-sanitizes Collection requests and drops runtime multipart paths/generated bodies.
- Environment secret values and DPAPI blobs are never exported. Secret, sensitive-name, and
  token-shaped values become `${NAME}` references with `secret: true` and no `value` field.
- A hand-written document that lies with `secret: false` for a sensitive key or token-shaped value
  is rejected as well; imported secret references become empty `미설정` placeholders.
- Imports append fresh IDs and never overwrite existing Collection or Environment entries.
- Added `apps/api-playground/src-tauri/src/commands/transfer.rs` for native bounded UTF-8 reads,
  exact top-level and nested schema validation, reference-only secret enforcement, safe ancestor and
  final-file identity checks, native file dialogs, and atomic export writes. Renderer IPC cannot
  bypass the Collection/Environment allowlist or export a forged plaintext credential document.
- Registered native commands in `commands/mod.rs` and `src-tauri/src/lib.rs`.

### 2. Safe History search and filter (#347)

- Added `apps/api-playground/src/lib/history.ts` with a 128-character query bound and method plus
  success/error/all status filters.
- Search is limited to display name, method, URL, and status metadata. Headers, Cookies, auth,
  body, multipart paths, GraphQL variables, and environment secrets are not indexed.
- Added labelled search/select controls, no-result state, and transfer/send busy gating in
  `apps/api-playground/src/App.tsx`.
- Added `history.test.ts` fixtures for safe metadata, status classification, secret exclusion,
  and query bounds.

### 3. Bounded binary response projection and save (#348)

- Added `apps/api-playground/src/lib/binary.ts` with strict UTF-8 and conservative media/control
  classification, a 16 MiB response bound, and 4 KiB hex/text preview bounds.
- Added `BinaryResponse` to `src/types.ts`; raw bytes are not part of the DTO.
- Updated `src/api.ts` browser preview to use a bounded byte reader, content-length preflight when
  streaming is unavailable, strict decoding, and bounded safe MIME normalization. Browser binary
  save remains disabled.
- Updated native `commands/request.rs` to read bounded bytes into zeroizing storage, classify binary
  responses, redact binary/text previews, and keep only the latest response's bounded binary in a
  `Zeroizing` vault. Temporary text conversion and explicit-save clones are zeroized as well.
  The vault is addressed by an opaque `response-N` ID and rejects stale IDs.
- Added `save_response_binary`, which only uses a native save dialog, validates the selected
  regular destination, and writes with `crates/filesystem::atomic_write`. It does not execute,
  clipboard-copy, or automatically download response bytes.
- Added renderer-teardown disposal of the current native response authorization. This both drops an
  already retained raw body and prevents an in-flight late result from entering the vault.
- Updated `ResponseViewer.tsx` with accessible binary metadata, bounded previews, a disabled
  browser/native-unavailable state, and an explicit Save binary action.
- Added native and frontend fixtures for media classification, invalid UTF-8, secret masking,
  preview bounds, stale response IDs, and save availability.

### 4. Persistence and UI safety integration

- `src/lib/persistence.ts` now rebuilds the persisted request from an explicit allowlist during
  normalization, dropping manually injected request/auth/parameter/GraphQL fields before a new
  transfer or save.
- Native persistence sanitization skips empty imported secret placeholders when unsealing unrelated
  requests, while request resolution remains fail-closed until the secret is configured.
- `App.tsx` uses revision-aware, pre-commit transfer/persistence guards to prevent stale async
  sanitization from writing durable state, marks imported secret variables as unconfigured, and
  uses a hidden labelled browser JSON file input only after an explicit user action. Browser picker
  cancellation clears its ref/state so the action cannot remain permanently disabled.
- History rows are projected into bounded visible metadata before labels, method options, filters,
  or replay; manually edited controls, invalid status/method fields, and token-shaped name/URL data
  are not reflected into the UI.
- Browser transfer serialization reconstructs every nested field, removes file paths and generated
  GraphQL bodies, enforces exact variable references for sensitive values, and parses its own output
  before download. Imports are all-or-nothing on capacity or opaque-ID exhaustion.
- `App.css` adds transfer, filter, binary summary, and unconfigured-secret styling.
- Existing request sequence, AbortController, mounted guard, redirect policy, and response vault
  lifecycle remain in place for stale/cancelled requests.

### 5. Documentation

- Expanded `apps/api-playground/README.md` with transfer schemas/bounds, secret reference policy,
  History search scope, and binary preview/save behavior.
- Updated `docs/architecture.md` data-flow and security sections with transfer, search, binary
  vault, native dialog, and browser boundaries.
- Expanded P3-07 in `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md` with PR
  grouping, exact contracts, file ownership, acceptance matrix, exclusions, and W3 requirements.
- Added the grouped #346–#348 implementation candidate and remaining release gates to
  `docs/roadmap.md`.

## Code Examples

### Reference-only secret export

```typescript
// apps/api-playground/src/lib/transfer.ts
const secret = variable.secret || isSensitiveName(variable.key) || looksLikeSecret(variable.value);
return secret
  ? { key: variable.key, reference: `\${${variable.key}}`, secret: true }
  : { key: variable.key, reference: `\${${variable.key}}`, secret: false, value: variable.value };
```

### No raw binary in the response DTO

```rust
// apps/api-playground/src-tauri/src/commands/request.rs
let (body, binary_projection, raw_binary) = if binary {
    (String::new(), Some(project_binary_response(&media_type, &body_bytes, redactor)), Some(body_bytes))
} else {
    let raw_text = Zeroizing::new(String::from_utf8(body_bytes.to_vec())?);
    let body = redactor.redact_body(raw_text.as_str());
    (body, None, None)
};
```

### Safe History scope

```typescript
// apps/api-playground/src/lib/history.ts
const metadata = historyVisibleMetadata(item);
const haystack = [metadata.name, metadata.method, metadata.url,
  metadata.status === undefined ? "" : String(metadata.status)]
  .join(" ").toLocaleLowerCase();
```

## Verification Results

- `CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-log-lens cargo test -p api-playground -j1`:
  99 Rust tests passed, including native binary loopback, configured stream overflow, stale/current
  vault cleanup, forged transfer secret rejection, and Unix linked target/ancestor fixtures.
- `pnpm --dir apps/api-playground test -- --maxWorkers=3`: the final uninterrupted run passed all
  30 files and 214 tests, including the real DOM picker-cancel event regression fixture.
- `pnpm --dir apps/api-playground exec vitest run src/lib/transfer.test.ts src/lib/history.test.ts
  --maxWorkers=2`: 17 focused transfer/History tests passed after the final remediation.
- `pnpm --dir apps/api-playground build`: TypeScript check and Vite production build passed (the
  pre-existing Tauri event chunk and >500 kB bundle advisories remain warnings only).
- `cargo fmt --all -- --check`, app-only `cargo check -p api-playground -j1`, and strict
  `cargo clippy -p api-playground --all-targets -j1 -- -D warnings` passed after the final native
  fixes.
- After the clean rebase to `4c55be0`, `cargo test --workspace -j1` passed for every workspace
  crate and application, including 99 API Playground Rust tests, and `cargo check --workspace -j1`
  completed successfully.
- `pnpm --workspace-concurrency=3 -r build` passed for all 19 buildable frontend workspace projects.
  Existing bundle-size advisories for API Playground, Code Pad, Knowledge Base, and WSL Desktop,
  plus the existing API Playground Tauri event chunk advisory, remain warnings only.
- The rebased candidate passes `git diff --check`; no dependency or lockfile change was produced.
- Native Windows dialog/atomic-write behavior and packaged W3 evidence remain the release checkpoint.

## Acceptance Checklist

- [x] Versioned Collection/Environment schemas and bounded parser/exporter are present.
- [x] Secret values/DPAPI blobs are excluded; forged non-secret sensitive entries are rejected.
- [x] Collection/Environment imports append without overwrite and imported secret refs are visibly
  unconfigured.
- [x] History query is bounded and excludes request payload/auth metadata.
- [x] Binary response body, preview, media metadata, and native save are bounded and redacted.
- [x] Stale response IDs cannot recover an older binary buffer; renderer teardown invalidates the
  current/in-flight vault and browser save is disabled.
- [x] Native file operations use explicit dialogs, regular-file/path checks, and atomic writes.
- [x] App-only Rust tests and frontend tests/build are complete.
- [x] App-only fmt/check and strict all-target Clippy are complete.
- [x] Latest-main full workspace Rust tests/check and all frontend builds are complete.
- [ ] Windows packaged W3 evidence remains.

## Next Steps

1. Open the grouped PR and merge only after every GitHub Actions job, including Windows compile,
   is green.
2. Perform Windows W3 packaged smoke for offline transfer round-trip, secret plaintext absence,
   overflow rejection, binary save/cancel/stale response, and History keyboard/a11y behavior.
3. Preserve W3 as a release checkpoint if it cannot be executed from WSL.
