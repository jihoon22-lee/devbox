# Knowledge Base Quick Capture (#303)

## Overview

This implementation adds an offline-first quick-capture flow to Knowledge. A user can
open the same modal from the Windows `Ctrl+Alt+K` global shortcut or the
in-app button, review a normalized title/body/tag payload, and explicitly save
one new Markdown note under the fixed `Inbox/` directory. The native side is
the final authority for validation, privacy, root containment, collision
handling, and SQLite indexing; the browser side only provides a preview and
never claims that a note was saved.

The work is intentionally limited to issue #303. Image assets, templates,
clipboard history, cloud sync, and cross-app handoff remain follow-up work.
The existing draft was checkpointed and rebased onto the latest `origin/main`
(`847e28f`, after #420) in the dedicated worktree
`/mnt/e/projects/devbox-worktrees/knowledge-base-quick-capture` on
`feat/knowledge-base/quick-capture`. No push, merge, or PR was made before the
parent review.

## Context and decisions

- The capture destination is not accepted from the request. It is always the
  root-relative `Inbox` directory, which prevents the modal from becoming a
  generic file writer.
- Preview has no filesystem or database side effect. Save creates a new file
  with a bounded UTC timestamp name and then indexes that exact Markdown in a
  SQLite transaction.
- `create_new`, `flush`, and `sync_all` prevent overwrite and reduce the
  chance of a silently incomplete note. If writing or indexing fails, the new
  file is removed on the failure path.
- The `Inbox` directory is intentionally lazy: preview validates the fixed
  destination without creating it, while save creates it only after the same
  root/ancestor checks pass.
- Credential-like assignment markers, token prefixes, bearer values, and
  private-key markers fail closed. Errors are fixed safe messages and do not
  echo input, absolute paths, or OS details.
- Clipboard text is read only after the explicit clipboard button action. No
  history, background polling, localStorage entry, or integration snapshot
  contains the clipboard payload.
- A new external plugin was not introduced for the global shortcut. The
  Windows registration uses the existing locked `windows 0.61.3` dependency
  already present in the workspace; the dependency is target-specific and no
  install or runtime sidecar was added.

## Changes made

### Native policy and persistence

- `apps/knowledge-base/src-tauri/src/core/capture.rs`
  - Added `QuickCaptureInput`, normalized DTOs, fixed error messages, LF
    normalization, and deterministic Markdown rendering.
  - Enforced title 200 Unicode scalars, body 64 KiB UTF-8, at most 20 tags,
    48 scalars per tag, and 1 KiB total tag bytes.
  - Rejected control/newline injection, unsafe tag punctuation, blank body,
    and conservative credential-like content.
  - Rejected unknown IPC fields, kept bearer-token scanning linear, and JSON-quoted
    title/tag frontmatter so punctuation cannot corrupt or reinterpret metadata.
  - Added UTC filename generation with a bounded collision ordinal.
- `apps/knowledge-base/src-tauri/src/core/frontmatter.rs`
  - Decodes the JSON-compatible quoted scalars emitted by quick capture while
    preserving the existing plain and list frontmatter forms.
- `apps/knowledge-base/src-tauri/src/core/mod.rs`
  - Registered the capture policy module.
- `apps/knowledge-base/src-tauri/src/core/store.rs`
  - Kept the existing default layout unchanged; the quick-capture `Inbox`
    directory is created lazily only after its fixed target passes validation,
    avoiding an unrelated startup write through a pre-existing symlink.
- `apps/knowledge-base/src-tauri/src/commands/docs.rs`
  - Added preview/save DTOs and Tauri commands.
  - Reused canonical root and existing-ancestor/symlink validation.
  - Added deterministic test injection for the timestamp, `create_new` file
    creation, cleanup on the database failure path, and root-relative result
  paths only. The write/commit cleanup branches remain explicit in the
  implementation for runtime I/O failures.
  - During the post-rebase audit, the collision fixture was corrected to create
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
  - Added the browser/native input mirror, tag parsing, bounded validation
    errors, preview/saved DTOs, and shortcut status types.
- `apps/knowledge-base/src/api.ts`
  - Added preview/save/status/event wrappers.
  - Performs local validation, validates the returned `Inbox/` path, and
    allowlists native error/status values so unexpected IPC strings cannot
    reach the UI.
  - Browser preview is local-only; browser save returns a fixed unavailable
    error instead of reporting success.
- `apps/knowledge-base/src/components/QuickCaptureDialog.tsx`
  - Added edit → preview → save modal stages with fixed target display,
    explicit clipboard action, bounded clipboard retention, and safe errors.
  - Added generation tokens, busy guards, duplicate-save protection, Escape,
    Tab focus trap, Ctrl/Cmd+Enter, ARIA dialog state, live errors, and focus
    restoration to the trigger.
  - Saves the exact normalized preview payload the user approved and ignores
    Enter/Escape shortcuts during IME composition.
- `apps/knowledge-base/src/App.tsx` and `apps/knowledge-base/src/App.css`
  - Wired the native event/status listeners and in-app fallback button.
  - Added success/conflict notices and responsive modal styling with bounded
    vertical preview scrolling and wrapped long content.

### Tests and regression mocks

- `apps/knowledge-base/src-tauri/src/core/capture.rs` tests cover
  normalization, deterministic rendering, title/body/tag bounds, injection,
  credential rejection, and filenames.
- `apps/knowledge-base/src-tauri/src/commands/docs.rs` tests cover fixed
  `Inbox` preview, Markdown/index persistence, same-second collision
  suffixing, rejection before file creation, and an Inbox symlink escape.
- `apps/knowledge-base/src-tauri/src/platform/mod.rs` tests cover bounded
  shortcut status values.
- `apps/knowledge-base/src/lib/quickCapture.test.ts` covers the frontend
  mirror policy and tag parser.
- `apps/knowledge-base/src/components/QuickCaptureDialog.test.tsx` covers
  explicit one-shot clipboard use, oversized clipboard rejection, preview
  before save, exact approved-payload persistence, IME composition,
  duplicate/busy behavior, and non-echoing safe errors.
- `apps/knowledge-base/src/App.applink.test.tsx`, `App.test.tsx`, and
  `App.wikilinks.test.tsx` include the new API mocks and verify event/button
  parity and shortcut conflict fallback without changing existing app-link,
  editor, or wikilink behavior.

### Post-rebase audit

- Re-read the native command boundary, capture policy, shortcut platform
  boundary, frontend API, dialog, capability, README, roadmap, and detailed
  plan after rebasing onto `origin/main`.
- Confirmed that preview has no filesystem/SQLite mutation, save has a fixed
  root-relative target and bounded collision loop, and clipboard access occurs
  only from the explicit button handler. Native and frontend error adapters
  continue to keep raw paths, credentials, and OS/IPC detail out of the UI.
- Confirmed that the existing generation/busy guards cover preview, save, and
  clipboard completion after close/unmount, while disabled controls prevent
  duplicate mutation and the dialog retains keyboard focus handling.
- The parent review additionally fixed frontmatter punctuation loss, a
  credential-scan worst case, unknown IPC fields, preview/save parity, and IME
  shortcut handling without broadening product scope.

### Documentation

- `apps/knowledge-base/README.md` documents the user flow, bounds, privacy,
  root/file/index transaction, Windows shortcut behavior, and explicit
  out-of-scope items.
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md` records the
  detailed #303 implementation contract and the Windows W2 evidence still
  required before merge.
- `docs/roadmap.md` records the local checkpoint draft, acceptance evidence,
  and the distinction between this quick-capture slice and future
  image/template/handoff work.

## Key implementation examples

### Native final validation and deterministic file creation

```rust
// apps/knowledge-base/src-tauri/src/commands/docs.rs
let normalized = capture::normalize(input).map_err(|error| error.to_string())?;
let document = capture::render_markdown(&normalized)
    .map_err(|error| error.to_string())?;
let rel = format!(
    "{}/{}",
    capture::INBOX_DIR,
    capture::filename_for_timestamp(now_seconds, ordinal),
);
let path = validated_new_entry(root, &rel).map_err(str::to_string)?;
create_new_capture_file(&path, document.as_bytes())?;
```

The command never accepts a destination path. `validated_new_entry` checks
the canonical root and existing ancestors, while `create_new_capture_file`
uses `OpenOptions::create_new` and cleans up a file that cannot be fully
flushed/synced.

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

## Verification results

The following focused checks were run without installing dependencies or
running a full workspace build:

```text
cargo fmt --all -- --check
exit 0

cargo metadata --locked --offline --no-deps --format-version 1
exit 0

cargo test --manifest-path apps/knowledge-base/src-tauri/Cargo.toml --lib
test result: ok. 65 passed; 0 failed

cargo clippy --manifest-path apps/knowledge-base/src-tauri/Cargo.toml --all-targets -- -D warnings
exit 0

tsc -p apps/knowledge-base/tsconfig.json --noEmit
exit 0

vitest run (Knowledge Base, all 9 test files)
Test Files  9 passed (9)
Tests       43 passed (43)

pnpm --filter knowledge-base build
exit 0

git diff --check
exit 0
```

The TypeScript, Vitest, and Vite checks used the repository's existing local
dependency snapshot through a Linux-native mirror; the feature worktree has no
`node_modules` directory. The normal filtered pnpm build completed successfully.
The focused Windows MSVC target check reached the Windows dependency graph but
could not finish because the WSL host has no `lib.exe` for the bundled SQLite
build. The Windows packaged W2 manual check was not run in WSL.

## Remaining risks and next steps

1. Run the Windows W2 acceptance on a packaged build: register the shortcut
   while another application owns it, verify focus after global invocation,
   verify preview-before-save and one-shot clipboard behavior, and collect
   collision/write/index failure evidence.
2. Run the normal repository-wide pre-PR gates and CI before merging. The
   Windows-only `RegisterHotKey` path still needs a real Windows runtime because
   WSL cannot execute it.
3. Keep image assets, templates, clipboard history, cloud sync, and handoff in
   separate issues/PRs as documented; they are not hidden in this draft.
4. Treat filesystem cleanup as bounded execution rollback only: an abrupt
   process/OS termination during the file/index sequence is not covered by a
   persistent journal. The credential gate is deliberately conservative
   pattern detection, so it should not be described as a complete secret
   scanner.
