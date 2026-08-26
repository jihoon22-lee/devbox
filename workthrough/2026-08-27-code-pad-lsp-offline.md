# Code Pad Managed LSP Offline Path Workthrough

- Date: 2026-08-27
- Issue: #310 `feat(code-pad): managed LSP offline path`
- Branch: `feat/code-pad/lsp-offline`
- Target: Code Pad 0.4.0 / v0.5.0 P2-12
- Status: root final review follow-up implemented and verified; ready for PR

## Overview

Code Pad의 reviewed managed LSP installer가 성공적으로 검증한 compressed archive를 app-owned
cache에 보관하고, 다음 설치에서 network 없이 재사용한다. native 서버는 catalog의 exact
manifest와 동일한 SHA-256·size·archive layout을 통과한 local archive를 명시적으로 가져올 수
있다. Node 서버는 native multi-file picker에서 reviewed dependency closure의 `.tgz`들을
선택하고, 검증된 cache와 결합해 완전한 closure를 만들 수 있다. IPC는 선택된 경로 목록만
받고, package identity·destination·source·URL은 native catalog와 reviewed lock이 소유한다.

설치 source와 마지막 검증 시각은 installed index metadata에 기록하지만, 사용자가 선택한
원본 archive path는 저장하거나 UI/IPC 오류에 반향하지 않는다. 실패한 검증과 promotion은
active install과 installed index를 건드리지 않으며, CodeMirror 편집·저장·preview·Quick Open
흐름과 독립적이다.

## Context and Scope

Issue #310 requires the P2-12 official catalog, cache reuse, local archive digest import, and
failure isolation. The existing installer already owned the reviewed catalog, exact-version
download, safe extraction, and atomic index boundary. This change extends that boundary without
adding LSP protocol behavior, automatic installation, editor behavior, or external archive storage.

## Changes Made

### 1. Reviewed cache and install metadata

- `apps/code-pad/src-tauri/src/lsp/installer.rs`
  - Added `lsp/downloads/cache/<sha256>.<ext>` paths with regular-file, size, and SHA-256 checks.
  - Streams verified downloads/local selections into nonce temporary files, syncs them, and
    promotes them to the app-owned cache only after verification.
  - Reuses a verified cache before starting a network request.
  - Tracks `network`, `archive_cache`, and `local_archive` source values plus
    `last_verified_at` in the installed metadata.
  - Adds archive depth, extraction size, entry count, and cache/index/staging path checks.
  - Reports unsafe cache links as a hard failure instead of silently treating them as a miss.
  - Matches every selected Node `.tgz` by the native reviewed lock's exact size, SHA-256, and
    SHA-512 integrity, rejects missing/duplicate/extra/ambiguous files, and combines selected
    files with verified cache entries when completing the supported closure.
  - Records `local_archive` whenever at least one selected archive participates in the install;
    cache-only and mixed network installs retain `archive_cache` and `network` respectively.
  - Captures a trusted RFC 3339 wall-clock value for catalog imports instead of confusing the
    artifact version with `installed_at`, preventing a successful promotion from failing schema
    validation and rolling back.
  - Requires selected archives to be absolute, lexically clean, regular, non-symlink/reparse, and
    non-hard-linked files; stale/unmounted selections fail without promotion.
- `apps/code-pad/src-tauri/src/lsp/catalog.rs`
  - Added backward-compatible `InstallSource` and optional last-verification metadata.
- `apps/code-pad/src-tauri/src/lsp/mod.rs`
  - Re-exports the archive-depth bound with the installer limits.

### 2. Explicit local archive import

- `apps/code-pad/src-tauri/src/commands/installer.rs`
  - Added `lsp_import_archive`, which accepts only exact catalog keys plus native picker paths.
  - Resolves the manifest and reviewed Node lock in the native process and maps parser, path, URL,
    and I/O details to a fixed safe IPC message.
- `apps/code-pad/src-tauri/src/lib.rs`
  - Registers the import command and the official Tauri dialog plugin.
- `apps/code-pad/src-tauri/capabilities/default.json`
  - Grants only `dialog:allow-open`; save and directory permissions remain absent. Node multi-file
    selection is constrained by the native picker and reviewed-lock matcher.
- `apps/code-pad/src/api.ts`
  - Adds a native multi-file archive picker and typed import wrapper.
- `apps/code-pad/src/components/ManagedInstallerPanel.tsx`
  - Keeps selected paths in transient pending action state until confirmation and never renders
    them. Native imports use one selected archive; Node imports use a multi-file `.tgz` selection
    matched by the reviewed closure in the native process.
  - Shows install source, last verification, and verified offline-cache state.
  - Uses a synchronous operation ref to serialize native picker, recovery, and confirmed
    mutations even before React state has re-rendered; generation and mounted guards discard
    superseded or post-unmount refresh results.
- `apps/code-pad/src/components/LspControlPanel.tsx`
  - Explains a verified archive cache as an offline install option while preserving the existing
    local/custom/editor fallback.
- `apps/code-pad/src/types.ts`
  - Adds the source, verification, and cache fields to the frontend DTOs.

## Code Examples

```rust
// lsp/installer.rs: IPC supplies paths only; native lock data supplies identity.
let (archives, source) = self.resolve_node_archive_set(
    &manifest.platform,
    &packages,
    archive_paths,
)?;
self.install_node_archives_with_lock(&manifest, &manifest.version, &lock, &archives, &staging, source)
```

```typescript
// api.ts: the dialog returns a transient path list; the native command owns matching.
const archivePaths = await pickLspArchives();
await importLspArchives(manifest.id, manifest.version, manifest.platform, archivePaths);
```

### 3. Dependencies, documentation, and notices

- `apps/code-pad/package.json`, `apps/code-pad/src-tauri/Cargo.toml`, `pnpm-lock.yaml`, and
  `Cargo.lock` register the already policy-approved Tauri dialog plugin needed for the native
  picker. No new transitive package graph entry was introduced.
- `docs/dependency-policy.md` records Code Pad's file-only dialog permission and path/privacy
  boundary alongside the existing plugin approval.
- `THIRD_PARTY_NOTICES.md` is regenerated from the locked Cargo and pnpm graphs.
- `apps/code-pad/README.md`, `docs/roadmap.md`,
  `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`, and the managed-installer
  portion of `docs/superpowers/specs/2026-08-12-code-pad-design.md` document the offline cache,
  local import, Node closure, and metadata contract.

## Security and Privacy Boundaries

| Risk | Control |
|---|---|
| A selected path becomes persisted user data | Only exact catalog/lock metadata and source enum/time are indexed; selected paths are transient and never returned. |
| A cache entry redirects filesystem writes | Cache, download, staging, servers, and index components reject symlink/reparse paths; cache links fail closed. |
| Archive traversal or special entries escape staging | Absolute, drive, backslash, empty, dot, parent, duplicate-separator, link, hard-link, and special entries are rejected. |
| Resource exhaustion | Compressed size, extracted bytes, entry count, and path depth are bounded. |
| Node package closure is incomplete | Every supported package in the reviewed lock must come from a selected archive or verified cache and pass exact size/SHA-256/SHA-512 verification. |
| A malformed/error detail leaks credentials or paths | The native command boundary replaces installer errors and status reasons with fixed safe messages. |
| A failed install damages active state | Extraction occurs in nonce staging; index update follows promotion and rolls back the promoted tree on commit failure. |
| Rapid clicks start duplicate external work | A ref-backed operation gate serializes picker, recovery, install, import, and uninstall boundaries. |
| A late refresh updates a closed/stale panel | Mounted and refresh-generation checks discard every late result before state or parent callbacks are updated. |

## Tests Added or Updated

Rust fixtures in `apps/code-pad/src-tauri/src/lsp/installer.rs` cover:

- verified native local import and cache persistence without the selected source path;
- Node multi-file closure matching by lock digest/integrity, with cache combination and duplicate/extra/missing rejection;
- absolute/clean path, symlink/reparse, non-regular, hard-link, stale/unmounted, and fixed-public-error failures;
- cache reuse with an intentionally unreachable artifact URL;
- wrong size/digest/version, archive-root mismatch, traversal, depth, extraction limits, missing
  entrypoint, links/special entries, symlinked selected/cache/install paths, and hard-linked files;
- failure isolation for an existing active installation and index;
- metadata source/last-verification serialization and unsafe-cache status failure.
- canonical local archive result assertions use the same filesystem canonicalization contract as
  production. This keeps the fixture portable when Windows exposes a temporary directory through an
  8.3 alias but `canonicalize` returns the extended long-name path.

React fixtures in `apps/code-pad/src/components/LspControlPanel.test.tsx` cover:

- keeping picked local paths out of the confirmation dialog and invoking import only after
  confirmation;
- serializing rapid picker/import clicks and dropping an installer refresh completed after
  unmount;
- exposing a verified archive cache as an offline option;
- preserving existing explicit install/uninstall, safe error, recovery, and managed-status flows.

## Verification Results

Focused Rust verification used the requested isolated target, `CARGO_INCREMENTAL=0`, and `-j2`:

```text
source ~/.cargo/env && cargo fmt --all -- --check
passed

CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-issue310 \
CARGO_INCREMENTAL=0 cargo test --manifest-path apps/code-pad/src-tauri/Cargo.toml lsp::installer -j2
27 installer tests passed
```

Additional bounded Code Pad verification completed after the workspace dependency install:

```text
CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-issue310 \
CARGO_INCREMENTAL=0 cargo test --manifest-path apps/code-pad/src-tauri/Cargo.toml -j2
175 library tests + lsp_client (3) + lsp_manager (13) + lsp_process (6) passed

CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-issue310 \
CARGO_INCREMENTAL=0 cargo clippy --manifest-path apps/code-pad/src-tauri/Cargo.toml --all-targets -j2 -- -D warnings
passed

CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-issue310 \
CARGO_INCREMENTAL=0 cargo check --manifest-path apps/code-pad/src-tauri/Cargo.toml -j2
passed

pnpm --filter code-pad exec tsc --noEmit
passed

pnpm --filter code-pad exec vitest run src/components/LspControlPanel.test.tsx --maxWorkers=2
1 file, 23 tests passed (after the final concurrency/lifecycle review fix)

pnpm --filter code-pad test -- --maxWorkers=2
14 files, 118 tests passed

pnpm --filter code-pad build
2171 modules transformed; Vite production build passed (50.79s)

python3 .github/scripts/check-dependencies.py generate
generated THIRD_PARTY_NOTICES.md

python3 .github/scripts/check-dependencies.py check
dependency policy OK; notices match Cargo.lock and pnpm-lock.yaml
```

The first PR-wide Windows run compiled and linted successfully, then exposed the portable-path fixture
above: the selected temp path used an 8.3 spelling while the production result correctly returned its
canonical extended spelling. The assertion now compares against `fs::canonicalize(&archive_path)` and
the focused regression was rerun locally before updating the PR.

Windows packaged smoke remains a release-gate/manual checkpoint because this WSL session cannot run
the Windows Tauri executable.

## Disk Usage

The isolated Cargo target is kept outside the repository at
`/home/jihoon/.cache/targets/devbox-issue310`; `du -sh` reports `9.7G` after the full test,
all-target check, and all-target clippy gates. The generated Code Pad
frontend `dist` directory is `4.4M` and is ignored build output. No downloaded production LSP
artifact is added to the repository or cache during tests.
