# Code Pad safe multi-file rename

## Overview

Issue #356 hardens Code Pad's LSP `textDocument/rename` flow. Rename is now a
native, bounded transaction with an explicit preview/approval boundary: the
renderer receives only opaque plan handles and workspace-relative, bounded
diff data during preview; after explicit approval, native code returns only
the open-document synchronization payload needed to refresh the editor. Native
code retains the full snapshots and performs the disk/LSP commit. The
transaction can be cancelled, expires, rejects stale state, and recovers
private journals without overwriting an externally changed file.

## Context

The old LSP path immediately applied a text-only `WorkspaceEdit` to the
in-memory document store and returned dirty buffers. That did not cover
unopened workspace files, disk changes between request and write, dirty or
lossily decoded files, a server dying after a partial notification, or a
renderer receiving absolute paths and raw protocol errors. A rename is a
cross-file mutation, so it needs stronger ownership and failure boundaries
than completion/hover or ordinary editor edits.

## Changes Made

### 1. Native preview/apply transaction

- `apps/code-pad/src-tauri/src/lsp/manager.rs`
  - Split `rename` into bounded preview creation and `apply_rename` approval.
  - Generate UUID-v4 opaque plan IDs, retain plans natively, cap pending plans
    at 16, and expire them after five minutes. `discard_rename` removes stale
    approvals and `cancel_rename` signals an active transaction.
  - Capture the session pointer, session generation, workspace root, open
    document version/text/dirty state, canonical file identity, mtime, size,
    encoding, line ending, and SHA-256. Stop/restart/session failure bumps the
    rename epoch and invalidates plans; apply rechecks all of those ownership
    conditions before writing.
  - Parse only local file URIs, reject resource operations, sensitive/credential
    paths, outside-workspace targets, dirty/open-vs-disk mismatches, lossy or
    read-only files, overlapping/out-of-range edits, and no-op-only responses.
  - Bound URI length/count, file count and aggregate bytes, edit count,
    replacement bytes, preview excerpts, encoded output bytes, journal bytes,
    journal entries, and the total startup-recovery scan budget. Preview
    excerpts preserve UTF-8 boundaries and include the omission marker inside
    the wire-size cap, so a multibyte document cannot exceed the advertised
    preview bound.
  - Reject rename when the server did not negotiate `textDocument/didChange`;
    there is no safe disk-backed fallback that can keep the server mirror and
    editor generation coherent.
  - Keep the plan's workspace/session/document generation binding intact
    through approval. A changed active document, workspace token, or revision
    causes the native plan to be discarded rather than applied to a stale UI.

### 2. Safe file snapshots and private rollback

- `apps/code-pad/src-tauri/src/commands/file.rs`
  - Add no-follow filesystem identities to native-only open/save snapshots.
  - Read through a bounded file handle and compare handle/path metadata,
    identity, mtime, size, and byte count before accepting a snapshot. This
    avoids allocating unbounded data when a file grows during a pre-read; the
    apply and rollback workers use the same bound when rechecking files.
  - Create a `0700` app-local transaction directory, `create_new` backups,
    preserve permissions, flush/sync backup bytes, and retain backup identity,
    size, and hash. Restore uses identity/hash verification plus atomic sibling
    replacement; a replaced backup or target fails closed.
  - Journal writes use private atomic replacement and bounded, stable reads.
    Recovery accepts only valid journals under the private root, restores a
    target only when it still exactly contains the recorded post-write bytes,
    and leaves externally changed or symlinked targets/journals for explicit
    recovery instead of overwriting them. Recovery also applies the same
    per-file byte cap as preview/apply so malformed local journals cannot turn
    startup recovery into an unbounded read; a separate 64 MiB scan budget
    bounds the aggregate cost of examining multiple journal directories.
  - Bind each journal's plan id to its directory name before recovery. A
    copied or renamed journal directory is rejected instead of allowing its
    records to be interpreted under a different transaction identity.
  - Use `WorkspaceRoot::relative_path` for display/result paths. It applies the
    same component-aware, Windows case-insensitive boundary rules as URI
    resolution and therefore handles drive, UNC, and W3 long-path forms
    without relying on host-sensitive `strip_prefix` behavior.
  - In bounded rename saves and rollback restores, re-check the final path
    with no-follow filesystem identity immediately before replacement and
    compare it to the exact previously approved object, rather than accepting
    any replacement regular file at the same pathname. If that check fails,
    the temporary is removed before returning the integrity error. A regression
    test replaces a path with the same bytes under a new identity and confirms
    that the final guard rejects it.

### 3. Disk/LSP commit and lifecycle safety

- `apps/code-pad/src-tauri/src/lsp/manager.rs`
  - Recheck every file immediately before the first backup and again in each
    atomic save boundary. Disk writes run in `spawn_blocking` with a 30-second
    deadline and cancellation checkpoints between reads, backups, writes, and
    notifications.
  - Notify open documents with `didChange`/`didSave` only after all disk files
    are committed. Each notification write also observes the transaction
    cancellation flag and deadline, so a backpressured server pipe cannot
    make the apply task wait indefinitely. The authoritative document store is
    committed only after the complete notification batch succeeds. Any partial
    server notification tears down/restarts the mirror before disk rollback;
    rollback runs off the async runtime and reports per-file `applied`,
    `rolledBack`, `conflict`, `notApplied`, or `rollbackFailed` status.
  - Serialize document mutations with the rename gate and invalidate plans on
    stop, restart, config/workspace reconfiguration, and session failure.

- `apps/code-pad/src-tauri/src/lsp/process.rs`
  - Start Unix LSP children in an isolated process group and kill the group on
    explicit stop, drop, or leader exit. Existing Windows Job Object cleanup
    remains the Windows process-tree boundary. The cleanup context is shared
    by the owner and wait task so the group is terminated at most once; a
    closed kill channel is treated as a reap signal, not as permission to
    skip descendant cleanup.
  - Make each request deadline cover the complete outbound write (JSON
    serialization, writer-lock contention, and stdin backpressure) as well as
    the response wait. A timed-out/cancelled partial write terminates the
    child rather than reusing a potentially corrupt JSON-RPC stream.
  - Bound `$/cancelRequest`, `exit`, and writer shutdown waits. If the writer
    is wedged, the process-tree kill path is selected and the caller remains
    bounded instead of waiting forever on a full pipe or a competing writer.

### 4. IPC, frontend state, and UX

- `apps/code-pad/src-tauri/src/commands/lsp.rs`, `src-tauri/src/lsp/mod.rs`,
  `src-tauri/src/lib.rs`
  - Register preview/apply/cancel/discard commands. Rename errors are
    categorical; raw paths, protocol diagnostics, command lines, and server
    details do not cross IPC.

- `apps/code-pad/src/api.ts`, `src/types.ts`, `src/lspDocumentSync.ts`,
  `src/store/documentStore.ts`, `src/App.tsx`, `src/App.css`
  - Add typed preview/apply/cancel/discard transport and relative-path result
    mapping. The native LSP URI is recovered from the internal document sync
    state when the reducer refreshes an open document; it is not reconstructed
    from an untrusted relative result path. Apply requires the expected
    workspace root and document revisions, updates clean editor buffers with
    native mtime/size/hash, and invalidates diagnostics only after an accepted
    native result.
  - Discard superseded previews, gate rename on `didChange` capability, and
    keep the cancel action available while disk/LSP apply is running. Editor,
    save, encoding, line-ending, reload, and watcher paths are guarded against
    stale frontend mutations during apply, including async opens that started
    before approval. A cancellation intent is checked again after the mirror
    flush and before native apply, while workspace-change tokens, active
    document/revision checks, and post-await watch/save guards prevent late
    promises from mutating state. Watcher events are queued and cleared for
    successfully committed files.
  - Reuse the shared `ChangeSetPreview` with fixed all-file approval and a
    file-by-file rollback/error result view (`packages/diff-view/src/index.tsx`).

### 5. Fixtures, tests, and documentation

- `apps/code-pad/src-tauri/tests/fixtures/fake_lsp_server.rs` adds a server
  without text-document sync; `tests/lsp_manager.rs` covers preview-before-
  write, no-sync rejection, stale version and disk conflicts, and partial
  notification rollback.
- Rust unit tests cover bounded WorkspaceEdit URI/edit/replacement limits,
  sensitive path rejection, encoded output expansion, exact UTF-8 preview
  excerpts, backup tamper/replacement, and journal recovery/scan binding.
- `apps/code-pad/src/App.test.tsx` and `src/lspDocumentSync.test.ts` cover the
  preview/apply separation, relative result shape, conflict result, stale
  active-document preview disposal, cancellation during mirror flush, and
  disabled rename when sync is unavailable.
- `apps/code-pad/README.md` and
  `docs/superpowers/specs/2026-08-12-code-pad-design.md` document the preview,
  private journal, cancellation, bounds, failure matrix, and no-sync policy.
- Added dependency: `uuid` with the `v4` feature; Unix process groups use the
  target-specific `libc` dependency.

## Code Examples

### Opaque relative preview

```rust
// manager.rs — only this crosses the renderer boundary
RenamePreview {
    plan_id: "rename-<uuid-v4>".to_owned(),
    files: vec![RenamePreviewFile {
        path: "src/main.rs".to_owned(),
        ranges,
        before: bounded_excerpt,
        after: bounded_excerpt,
    }],
}
```

### Apply checkpoint

```rust
// manager.rs — every blocking phase observes both controls
fn rename_checkpoint(cancelled: &AtomicBool, deadline: Instant) -> Result<(), &'static str> {
    if cancelled.load(Ordering::Acquire) {
        return Err("이름 변경이 취소되었습니다");
    }
    if Instant::now() >= deadline {
        return Err("이름 변경 작업 시간이 초과되었습니다");
    }
    Ok(())
}
```

### Encoded output bound

```rust
// manager.rs — normalized preview text is not the save-byte size
let encoded = file_commands::encode_for_save(
    &change.text,
    disk.encoding,
    disk.line_ending,
)?;
if encoded.len() > MAX_RENAME_TOTAL_BYTES
    || encoded_total_bytes.saturating_add(encoded.len()) > MAX_RENAME_TOTAL_BYTES
{
    return Err("이름 변경 결과가 허용된 파일 크기를 초과했습니다");
}
```

### Cancellation after mirror flush

```typescript
// App.tsx — cancel intent wins before the native disk transaction starts
await lspSync.flush();
if (renameCancelRequestedRef.current) {
  await lspSync.discardRename(planId);
  return;
}
const result = await lspSync.applyRename(planId);
```

## Verification Results

### Rust focused verification

```text
cargo check -p code-pad --lib -j1
Finished `dev` profile

cargo check -p code-pad --all-targets -j1
Finished `dev` profile

cargo test -p code-pad --lib -- --test-threads=1
test result: ok. 185 passed; 0 failed

cargo test -p code-pad --test lsp_manager -- --test-threads=1
test result: ok. 15 passed; 0 failed

cargo test -p code-pad --test lsp_process -- --test-threads=1
test result: ok. 6 passed; 0 failed

cargo clippy -p code-pad --lib --tests -- -D warnings
Finished `dev` profile

cargo clippy -p code-pad --all-targets -j1 -- -D warnings
Finished `dev` profile
```

All Cargo commands used the isolated
`CARGO_TARGET_DIR=/home/jihoon/.cache/targets/code-pad-356` and
`CARGO_BUILD_JOBS=1` so parallel worktrees could not share compilation
artifacts. `cargo fmt --all -- --check` and `git diff --check` both passed.

### Frontend verification

The worktree initially had no `node_modules`, so the first test/build attempts
reported missing `vitest`/`tsc`. Dependencies were then installed only for the
Code Pad workspace closure (`pnpm install --filter code-pad... --frozen-lockfile
--ignore-scripts --child-concurrency=1 --network-concurrency=2`, five of 19
workspace projects), without changing tracked files.

```text
pnpm --dir apps/code-pad exec vitest run src/App.test.tsx \
  --maxWorkers=1 --no-file-parallelism --reporter=dot
Test Files  1 passed (1)
Tests       27 passed (27)

pnpm --dir apps/code-pad exec vitest run \
  src/lspDocumentSync.test.ts src/lspFeatures.test.ts src/store/documentStore.test.ts \
  --maxWorkers=1 --no-file-parallelism --reporter=dot
Test Files  3 passed (3)
Tests       36 passed (36)

pnpm --dir apps/code-pad build
✓ built in 50.25s

pnpm --filter code-pad test -- --maxWorkers=1 --no-file-parallelism
Test Files  14 passed (14)
Tests       122 passed (122)

pnpm --filter code-pad build
✓ built in 47.03s

python3 .github/scripts/check-dependencies.py check
bash .github/scripts/check-catalog.sh
PASS
```

The build completed TypeScript checking and Vite production bundling (2,171
modules). Vite emitted the existing large-chunk advisory; it is not a build
failure. A prior whole-package run exposed an assertion that queried
`"전체 적용"` while the accessible name included the file count; the test was
corrected to use the complete accessible-name prefix and the isolated App run
then passed all 27 tests. The full package test later passed all 122 tests.
The candidate was committed and rebased onto `origin/main` at
`abfc12b066918653f2b2705cdf756c55bfb1b978`; push and PR creation remain
pending final integration.

## Next Steps

- Exercise real Windows Job Object/long-path and WSL path behavior manually;
  W3 long-path packaging smoke remains unavailable in this WSL run.
- Keep the final check-to-atomic-replace interval in the review risk register:
  portable filesystem APIs do not provide a cross-platform compare-and-swap
  rename, so the implementation fails closed on all observed identity/path
  changes but cannot mathematically exclude a writer racing in the final OS
  syscall window. A future Windows-specific handle/CAS implementation can
  narrow that residual further.
- Review the final diff as one #356 candidate PR. Preview remains bounded and
  relative; only explicit post-approval open-document synchronization carries
  full text, and native absolute paths/protocol details must not be added to
  future preview/error IPC.
