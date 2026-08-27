# Knowledge Base Quick Capture (#303)

## Overview

This implementation adds an offline-first quick-capture flow to Knowledge. A user can
open the same modal from the Windows `Ctrl+Alt+K` global shortcut or the
in-app button, review a normalized title/body/tag payload, and explicitly save
one new Markdown note under the fixed `Inbox/` directory. The native side is
the final authority for validation, privacy, root containment, collision
handling, and SQLite indexing; the browser side only provides a preview and
never claims that a note was saved.

The acceptance below remains specific to issue #303. For the cohesive Knowledge
PR, issue #304 image assets are integrated in the same final worktree, with a
separate acceptance matrix and workthrough so the two rollback/evidence slices
remain independently reviewable. Templates, clipboard history, cloud sync,
and cross-app handoff remain follow-up work.
The grouped implementation was finalized in the dedicated worktree
`/mnt/e/projects/devbox-worktrees/knowledge-base-quick-capture` on
`feat/knowledge-base/quick-capture`. The quick-capture commits were rebased
onto current `main` (`a018065`, #441), then the independently developed #304
slice was integrated without preserving a separate final commit. The final
review and gates therefore exercised the exact combined PR tree while keeping
the two issue acceptance matrices separate.

## Context and decisions

- The capture destination is not accepted from the request. It is always the
  root-relative `Inbox` directory, which prevents the modal from becoming a
  generic file writer.
- Preview has no capture filesystem/database side effect: it reads the already
  configured root, normalizes the input, and metadata-checks the fixed `Inbox`
  target without creating it or initializing the default layout. Save
  creates only that one fixed directory after a fresh root/ancestor/symlink
  check, then indexes the exact Markdown in a SQLite transaction.
- `create_new`, `flush`, and `sync_all` prevent overwrite and reduce the
  chance of a silently incomplete note. If writing or indexing fails, the new
  file is removed on the failure path.
- The `Inbox` directory is intentionally lazy: preview validates the fixed
  destination without creating it, while save creates it only after the same
  root/ancestor checks pass.
- Credential-like assignment markers, token prefixes, bearer values, and
  private-key markers fail closed. Errors are fixed safe messages and do not
  echo input, absolute paths, or OS details.
- Rust and TypeScript now agree on Unicode scalar and UTF-8 byte bounds,
  C0/C1 plus Unicode line-separator rejection, `X-API-Key`/private-key
  assignments, and all-occurrence common token-prefix scanning. The renderer
  rechecks a manually constructed normalized DTO before serializing it.
- Clipboard text is read only after the explicit clipboard button action. No
  history, background polling, localStorage entry, or integration snapshot
  contains the clipboard payload.
- A new external plugin was not introduced for the global shortcut. The
  Windows registration uses the existing locked `windows 0.61.3` dependency
  already present in the workspace; the dependency is target-specific and no
  install or runtime sidecar was added.

## 2026-08-27 hardening checkpoint

The follow-up review identified that a path-only preview, direct save payload,
single `create_new` write, detached hotkey worker, and unbounded watcher/input
paths were not sufficient for the #303 acceptance contract. The following
changes close those boundaries; #304 is integrated separately in the same
worktree and reuses the vault/publication boundary described below:

- Preview now stores normalized values in one native app-managed slot and
  returns only an opaque, bounded `qc-<positive integer>` ID. A new preview
  replaces the old slot; save and discard consume only a matching ID, and the
  save command accepts no title/body/tags/path payload. The pending normalized
  capture is intentionally neither `Debug` nor `Serialize`.
- Preview releases the DB mutex before taking the preview-slot mutex; save uses
  the same slot-then-DB order, so concurrent preview/save IPC cannot deadlock
  while replacing or consuming the one-shot approval.
- `VaultIdentity` records the canonical root and platform filesystem identity
  (Unix device/inode or Windows volume/file index). Preview is read-only and
  never calls the default-root initializer. Save compares the selected vault
  again, revalidates before directory/file publication, rejects symlink and
  Windows reparse components, and checks every existing ancestor. A replaced
  root returns a fixed stale-preview message before `Inbox` creation. The
  image command uses the same identity object and shared no-replace publication
  and cleanup helpers, so #304 cannot fall back to path-only asset writes.
- Publication is staged in a sibling temporary file created with `create_new`,
  fully written, flushed, and synced. Unix publishes by same-directory
  hard-link then removes the temporary name and syncs the directory; Windows
  uses no-replace `MoveFileExW(..., MOVEFILE_WRITE_THROUGH)`. Existing same-
  second captures therefore advance a bounded suffix instead of being
  overwritten. Any publication/index/commit failure removes the new file and
  temporary residue.
- The credential scanner now covers assignment/header spellings (`api-key`,
  `x-api-key`, access/refresh/session/client secrets, authorization, cookie,
  connection/database/password/secret/token), PEM/private-key markers, GitHub,
  Slack, AWS/env keys, Google, Stripe, SendGrid, GitLab, npm, PyPI, Vault,
  Sentry, and app-token prefixes, plus Basic/Bearer/Token schemes, JWT-like
  three-segment values, and Telegram bot tokens. Rust and the browser mirror
  use the same conservative policy and fixed errors.
- The custom serde visitors borrow strings when possible and cap title/body,
  tag count/item bytes, and preview ID before app-managed storage. Clipboard
  reads remain explicit and one-shot; a raw value over 128 KiB or failing the
  capture policy is never put into controlled dialog state.
- The Windows hotkey worker owns its message queue and thread ID, unregisters
  on conflict/inactive/WM_QUIT, and is joined through `Drop`. The React
  listener handles registration promises after unmount, and a late preview
  response explicitly discards its native slot. Dialog cancel/close/edit,
  save failure, and unmount all clear approval state; preview/clipboard cancel
  invalidates the generation while an in-flight save remains modal-blocking
  until its already-consumed approval settles. Busy/generation guards suppress
  duplicate or stale mutations.
- The dialog exposes live UTF-8/LF/raw byte and tag-count budgets, fixed target,
  safe errors, focus trap, Escape/Ctrl+Enter/IME handling, and trigger focus
  restoration. The watcher uses a bounded sync channel, event/path/debounce
  caps, bounded reads, and bounded overflow reconciliation that skips
  symlink/reparse and oversized files.
- The final parent review made read-side identity checks as strict as writes:
  asset dedupe and Markdown image preview capture metadata from the open file,
  compare the current target identity after the bounded read, and cache image
  previews by mtime, length, and filesystem identity. Capture temp files are
  revalidated around create/write, poisoned approval mutexes return fixed
  errors, and overflow reconciliation charges every regular file—not only
  Markdown candidates—against its scan budget.
- The renderer safe-error adapter now handles both JavaScript `Error` objects
  and Tauri string rejections through the same allowlist. Explicit context
  paste consults the image Clipboard API only when image import is available;
  otherwise normal text paste remains intact. Targeted fixtures cover both
  paths and jsdom's CodeMirror layout boundary.

The integration checkpoint rebased the branch onto `a018065` before applying
#304. The roadmap conflict was manually merged with all main entries preserved.
The image worktree stayed read-only; its changes were applied without creating
a separate final commit, and the combined tree received one parent review and
one final gate.

## Acceptance matrix

| Review boundary | Implementation anchor | Static/fixture evidence | Deferred runtime evidence |
| --- | --- | --- | --- |
| Atomic publication and collision | `stage_capture_file` + shared `publish_new_vault_file` (Unix hard-link / Windows no-replace `MoveFileExW`), bounded timestamp suffix | deterministic filename, collision preservation, index-failure cleanup, no `.tmp` residue tests; image reuse/collision tests use the same helper | packaged Windows same-second collision and locked/disk-failure run |
| Vault identity, TOCTOU, reparse | `VaultIdentity` canonical path + device/inode or volume/file index; repeated root/ancestor/Inbox/assets checks; `existing_path` for cleanup/read | root replacement, root/child symlink, traversal, fixed Inbox and image `assets/` tests | Windows junction/reparse and concurrent replacement smoke |
| Save-preview bypass/default mutation | native one-slot `QuickCapturePreviewStore`; `save_quick_capture(QuickCaptureApproval)` only; `resolve_configured_root` | wire-shape rejects title/body/path; one-shot replace/take/discard; unconfigured preview leaves DB/root unchanged | packaged preview → save/cancel interaction |
| Credential completeness | shared Rust/browser marker, prefix, auth-scheme, JWT-like conservative gate | API/header/cookie/private-key/GitHub/provider prefix fixtures, no secret echo | false-positive review with representative user notes |
| Clipboard/serde bounds | explicit clipboard action; raw 128 KiB check; borrowed bounded serde visitors for body/tags/ID | oversized clipboard never enters draft; oversized body/tag/ID and unknown fields rejected | packaged clipboard one-shot and OS clipboard failure |
| Cancel/hotkey lifecycle/race | native `WM_QUIT` + unregister/join; React generation/busy/discard including late preview | busy duplicate/IME, cancel discard, late response discard, bounded status tests | Windows conflict/focus/reopen/close and shutdown smoke |
| Modal/byte-limit UX | live UTF-8/LF/raw counters, fixed target, ARIA/focus/Escape/Ctrl+Enter | dialog counter and keyboard fixture coverage | narrow-window + screen-reader/manual keyboard pass |
| Watcher bound | bounded sync channel, event/path/debounce/read/reconcile caps, overflow polling | debouncer/path/file reader bound fixtures | event storm/overflow convergence on packaged app |

## Changes made

### Native policy and persistence

- `apps/knowledge-base/src-tauri/src/core/capture.rs`
  - Added `QuickCaptureInput`, normalized DTOs, fixed error messages, LF
    normalization, and deterministic Markdown rendering.
  - Enforced title 200 Unicode scalars/800 bytes, LF-normalized body 64 KiB
    with a 128 KiB raw-input bound, at most 20 tags, 48 scalars/192 bytes per
    tag, and 1 KiB total tag bytes.
  - Rejected C0/C1 and Unicode line-separator injection, unsafe tag
    punctuation, blank body, and conservative credential-like content.
  - Rejected unknown IPC fields, kept bearer-token scanning linear, and JSON-quoted
    title/tag frontmatter so punctuation cannot corrupt or reinterpret metadata.
  - Scans all common token-prefix occurrences (including header-shaped
    `X-API-Key`) and rechecks the complete normalized DTO in the renderer so a
    future caller cannot bypass the native policy.
  - Added UTC filename generation with a bounded collision ordinal.
- `apps/knowledge-base/src-tauri/src/core/frontmatter.rs`
  - Decodes the JSON-compatible quoted scalars emitted by quick capture while
    preserving the existing plain and list frontmatter forms.
- `apps/knowledge-base/src-tauri/src/core/mod.rs`
  - Registered the capture policy module.
- `apps/knowledge-base/src-tauri/src/core/vault.rs`
  - Added canonical vault identity, existing-entry/path validation, and
    root-relative reparse checks shared by quick capture, image assets, and
    preview loading.
  - Windows identity is accepted only when both volume serial and file index
    are available; an unknown identity fails closed instead of weakening the
    root replacement check.
- `apps/knowledge-base/src-tauri/src/core/store.rs`
  - Kept the existing default layout unchanged; the quick-capture `Inbox`
    directory is created lazily only after its fixed target passes validation,
    avoiding an unrelated startup write through a pre-existing symlink.
- `apps/knowledge-base/src-tauri/src/commands/docs.rs`
  - Added preview/save DTOs and Tauri commands.
  - Reused canonical root and existing-ancestor/symlink validation. The image
    command uses the configured-root-only resolver and the shared no-replace
    publication/cleanup helpers rather than a path-only asset writer.
  - Preview validates the fixed target without creating it. Save creates only
    the one-level `Inbox` directory with `create_dir`, revalidates it, and no
    longer uses recursive `create_dir_all` before the new-file operation.
  - Added deterministic test injection for the timestamp, `create_new` file
    creation, cleanup on the database failure path, and root-relative result
  paths only. The write/commit cleanup branches remain explicit in the
  implementation for runtime I/O failures.
  - During the draft audit, the collision fixture was corrected to create
    the lazy `Inbox` explicitly, and the rejection fixture now asserts that a
    rejected secret does not create the directory as a side effect.
- `apps/knowledge-base/src-tauri/src/lib.rs`
  - Registered the commands and started the shortcut state/worker during app
    setup.
- `apps/knowledge-base/src-tauri/Cargo.toml` and `Cargo.lock`
  - Added the Windows-only `windows 0.61.3` API features required for
    `RegisterHotKey`, the message loop, and window activation. The locked
    package and license notice already existed through other workspace users.
- `apps/knowledge-base/src-tauri/src/platform/mod.rs`
  - Added a bounded shortcut status DTO and a payload-less quick-capture event.
  - Windows uses a named worker thread, `RegisterHotKey(Ctrl+Alt+K)`, and
    `GetMessageW`; conflict/unsupported/unavailable states expose no raw OS
    error. The app window is shown, unminimized, and focused before the event
    is emitted.

### Frontend flow

- `apps/knowledge-base/src/lib/quickCapture.ts` and
  `apps/knowledge-base/src/types.ts`
  - Added the browser/native input mirror, matching scalar/byte/raw-body and
    line-separator validation, tag parsing, bounded validation errors,
    preview/saved DTOs, and shortcut status types.
  - Added an exact root-relative timestamp filename predicate for the native
    save response and all-occurrence credential-prefix checks.
- `apps/knowledge-base/src/api.ts`
  - Added preview/save/status/event wrappers.
  - Performs local validation, validates the returned `Inbox/` path, and
    allowlists native error/status values so unexpected IPC strings cannot
    reach the UI.
  - Browser preview is local-only; browser save returns a fixed unavailable
    error instead of reporting success.
- `apps/knowledge-base/src/components/QuickCaptureDialog.tsx`
  - Added edit → preview → save modal stages with fixed target display,
    explicit clipboard action, policy-gated clipboard retention, and safe
    errors.
  - Added generation tokens, busy guards, duplicate-save protection, Escape,
    Tab focus trap, Ctrl/Cmd+Enter, ARIA dialog state, live errors, and focus
    restoration to the trigger.
  - Added explicit field descriptions and live progress status for screen
    readers; IME-composed actions remain ignored.
  - Saves the exact normalized preview payload the user approved and ignores
    Enter/Escape shortcuts during IME composition.
- `apps/knowledge-base/src/App.tsx` and `apps/knowledge-base/src/App.css`
  - Wired the native event/status listeners and in-app fallback button.
  - Added success/conflict notices and responsive modal styling with bounded
    vertical preview scrolling and wrapped long content.
  - Keeps the selected-note ref current during render as well as after commit,
    so an image paste immediately after note switching cannot target the prior
    note during the passive-effect window.

### Tests and regression mocks

- `apps/knowledge-base/src-tauri/src/core/capture.rs` tests cover
  normalization, deterministic rendering, title/body/tag scalar/byte bounds,
  line-separator injection, renderer revalidation, credential rejection, and
  filenames.
- `apps/knowledge-base/src-tauri/src/commands/docs.rs` tests cover fixed
  `Inbox` preview, Markdown/index persistence, same-second collision
  suffixing, lazy directory creation, rejection before file creation, and an
  Inbox symlink escape.
- `apps/knowledge-base/src-tauri/src/platform/mod.rs` tests cover bounded
  shortcut status values.
- `apps/knowledge-base/src/lib/quickCapture.test.ts` covers the frontend
  mirror scalar/byte policy, Unicode line separators, all-occurrence
  credential checks, fixed save-path validation, and tag parsing.
- `apps/knowledge-base/src/components/QuickCaptureDialog.test.tsx` covers
  explicit one-shot clipboard use, oversized clipboard rejection, preview
  before save, exact approved-payload persistence, IME composition,
  credential clipboard rejection without draft retention, duplicate/busy
  behavior, unmount stale completion, and non-echoing safe errors.
- `apps/knowledge-base/src/App.applink.test.tsx`, `App.test.tsx`, and
  `App.wikilinks.test.tsx` include the new API mocks and verify event/button
  parity and shortcut conflict fallback without changing existing app-link,
  editor, or wikilink behavior.

### Draft audit before hardening

- Re-read the native command boundary, capture policy, shortcut platform
  boundary, frontend API, dialog, capability, README, roadmap, and detailed
  plan after the explicit rebase onto current `main`. The image-assets commit
  was applied without a commit and its shared boundary was reviewed here.
- Confirmed that preview has no filesystem/SQLite mutation, save has a fixed
  root-relative target and bounded collision loop, and clipboard access occurs
  only from the explicit button handler. Native and frontend error adapters
  continue to keep raw paths, credentials, and OS/IPC detail out of the UI.
- Confirmed that the existing generation/busy guards cover preview, save, and
  clipboard completion after close/unmount, while disabled controls prevent
  duplicate mutation and the dialog retains keyboard focus handling. The
  clipboard path now runs the same validator before placing a value in state.
- The parent review additionally fixed frontmatter punctuation loss, a
  credential-scan worst case, unknown IPC fields, preview/save parity, and IME
  shortcut handling without broadening product scope.

### Documentation

- `apps/knowledge-base/README.md` documents the user flow, bounds, privacy,
  root/file/index transaction, Windows shortcut behavior, and the independently
  reviewed #303/#304 acceptance slices in this cohesive PR.
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md` records the
  detailed #303 implementation contract and the Windows W2 evidence still
  required before merge.
- `docs/roadmap.md` records the local checkpoint draft, acceptance evidence,
  and the distinction between the independently tested #303/#304 slices in
  this cohesive PR; template/handoff work remains out of scope.

## Key implementation examples

### Native approval and atomic publication

```rust
// apps/knowledge-base/src-tauri/src/commands/docs.rs
let pending = previews
    .lock().unwrap()
    .take(&approval.preview_id)
    .ok_or_else(|| "빠른 캡처 미리보기가 오래되어 다시 확인하세요")?;
let current_vault = VaultIdentity::inspect(&configured_root)?;
let document = capture::render_markdown(&pending.capture)?;
let temporary = stage_capture_file(&current_vault, &inbox, &filename, document.as_bytes())?;
publish_new_vault_file(&temporary, &path)?;
db_transaction.index_doc(&relative, &document)?;
```

The command accepts only the opaque approval ID; the destination is always
`Inbox`. `VaultIdentity` checks canonical root path plus filesystem identity
and existing ancestors around each mutation. `stage_capture_file` uses
`OpenOptions::create_new`, write/flush/sync, and same-directory no-replace
publication. SQLite indexing is committed only after publication succeeds and
failure cleanup removes the new note and its temporary sibling.

### Payload-less native shortcut event

```rust
// apps/knowledge-base/src-tauri/src/platform/mod.rs
if message.message == WM_HOTKEY && message.wParam.0 == HOTKEY_ID as usize {
    if let Some(window) = worker_app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    let _ = worker_app.emit(QUICK_CAPTURE_EVENT, ());
}
```

The event contains no note content, path, clipboard value, or platform error;
the React listener only opens the same modal as the visible button.

## Verification status

The exact grouped tree passed the final Linux gates after the parent boundary
review:

- `cargo test -p knowledge-base -j2`: 100 passed.
- `cargo check -p knowledge-base -j2`: passed.
- `cargo clippy -p knowledge-base --all-targets -j2 -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- `pnpm --filter knowledge-base exec vitest run --maxWorkers=2`: 11 files,
  68 tests passed. The focused regression subset also passed 25 tests.
- `pnpm --filter knowledge-base build`: TypeScript and Vite production build
  passed (2,156 modules transformed).
- dependency policy/notices and `git diff --check` are rerun on the committed
  PR tree; GitHub Actions remains the authoritative Windows compile gate.

The first latest-head Windows CI compile completed `cargo check` and then
exposed four Windows-only `-D warnings` findings during Clippy. The follow-up
fix removed a non-Unix directory-sync fallback that cannot be called on
Windows, made both reparse-point cfg branches expression-valued, and explicitly
discarded the Win32 `TranslateMessage`/`DispatchMessageW` return values. The
same grouped tree then passed Linux `cargo fmt --check` and
`cargo clippy -p knowledge-base --all-targets -- -D warnings`; a fresh Windows
CI run is required before merge.

The next Linux workspace run exposed a real root-identity race rather than a
test-only timing problem: after deleting a directory, the filesystem may reuse
its `(device, inode)` for an immediate replacement. `VaultIdentity` now keeps
an `Arc`-owned open directory lease from preview through save (a directory
`File` on Unix and a share-delete `CreateFileW` handle on Windows), and
cross-checks the lease identity against both path metadata snapshots. Keeping
the original object open prevents inode/file-index reuse while the approval is
valid, so replacement remains stale even under immediate reuse. Focused and
full Knowledge tests (100) plus strict Clippy passed after this fix.

The following Windows test run passed compile and Clippy but exposed a path-
spelling bug in the rollback identity fixture: `Path::canonicalize` adds the
Windows verbatim prefix while the caller can still hold the equivalent normal
drive spelling. `existing_path` previously compared those strings before
canonicalizing the child and rejected a valid in-vault regular file. It now
walks the caller spelling first to reject every symlink/reparse component,
canonicalizes the child, and only then derives the root-relative path. This
preserves the fail-closed link boundary while accepting equivalent Windows
prefix spellings; the replacement-safe rollback test remains unchanged and is
rerun by the fresh Windows gate.

The Windows-only hotkey, `MoveFileExW`, filesystem-identity, clipboard/drop,
and watcher overflow paths still require packaged runtime evidence at W2.

## Remaining risks and next steps

1. Run the Windows W2 acceptance on a packaged build: register the shortcut
   while another application owns it, verify focus after global invocation,
   verify preview-before-save and one-shot clipboard behavior, and collect
   collision/write/index failure evidence.
2. Require all GitHub Actions checks before merging. The Windows-only
   `RegisterHotKey` path still needs a real Windows runtime because WSL cannot
   execute it.
3. Keep #303 and #304 acceptance/test/workthrough evidence independently
   reviewable inside the cohesive PR. Templates, clipboard history, cloud sync,
   and handoff remain separate follow-up scope.
4. Keep the vault identity/path/reparse checks immediately around every save
   step if persistence is refactored; do not replace the staged no-replace
   publication with a path-only overwrite helper.
5. Treat filesystem cleanup as bounded execution rollback only: an abrupt
   process/OS termination during the file/index sequence is not covered by a
   persistent journal. The credential gate is deliberately conservative
   pattern coverage rather than a formal secret scanner, so representative
   false-positive review remains part of acceptance.
