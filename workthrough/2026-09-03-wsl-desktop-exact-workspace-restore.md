# WSL Desktop exact workspace restore

## Overview

WSL Desktop now restores the user's primary pane first, bounds the remaining PTY startup work, preserves failed panes as retryable layout slots, and persists exact split ratios in version 2 workspace data.

## Context

The previous restore loop started every pane sequentially and only constructed the visible workspace after all starts completed. Any failed start was omitted, so pane order, tab membership, active identity, and sizing topology could collapse. Split ratios were also held only inside `PaneCanvas`, which meant closing and reopening the app restored pane membership but not the layout the user had arranged.

## Changes Made

### Progressive and bounded restore

- Materialize the complete tab/pane topology with stable pane keys before starting any PTY.
- Start the requested active pane alone so the primary shell becomes interactive first.
- Start the remaining panes with two bounded workers; no promise or process fan-out is created for the full 32-pane limit.
- Keep normal terminal input usable while background panes restore.
- Guard late results with the restore generation and close a newly created session if its placeholder no longer exists.
- Files:
  - `/home/jihoon/projects/devbox/apps/wsl-desktop/src/App.tsx`
  - `/home/jihoon/projects/devbox/apps/wsl-desktop/src/lib/workspaceRestore.ts`

```ts
const restorePlan = orderWorkspacePanes(workspace);
await startDefinition(restorePlan.active);
await runWithConcurrencyLimit(
  restorePlan.remaining,
  RESTORE_START_CONCURRENCY,
  startDefinition,
);
```

### Topology-preserving failure and retry

- Represent a connecting or failed pane with its stable key until a runtime session ID exists.
- Render a fixed-position card containing the distro, cwd, requested multiplexer, and a safe fixed error instead of backend error text.
- Retry from that exact card and atomically replace the stable placeholder identity with the new session ID.
- Allow a failed slot or its containing tab to be closed without sending a fake session ID to the backend.
- Exclude placeholders from broadcast targets while preserving keyboard navigation and focus.
- Preserve the requested multiplexer separately from the actual one and show explicit fallback such as `zellij → native`.
- Files:
  - `/home/jihoon/projects/devbox/apps/wsl-desktop/src/components/PaneCanvas.tsx`
  - `/home/jihoon/projects/devbox/apps/wsl-desktop/src/components/TermPane.tsx`
  - `/home/jihoon/projects/devbox/apps/wsl-desktop/src/types.ts`

```ts
function paneIdentity(pane: Pane): string {
  return pane.sessionId ?? pane.key;
}
```

### Split sizing persistence and migration

- Lift column/row ratios into each runtime tab and persist them in both last-layout and named-profile definitions.
- Reset ratios to even values when pane membership, order, or layout changes, matching the existing topology invalidation behavior.
- Keep zoom transient so toggling or restarting never serializes an enlarged pane.
- Upgrade local last-layout data from version 1 to version 2 with even ratios.
- Upgrade Rust profile stores from version 1 in memory and write version 2 on the next profile mutation.
- Validate ratio count, finite positive values, normalized sums, tab/pane bounds, and reference integrity in Rust before disk writes.
- Files:
  - `/home/jihoon/projects/devbox/apps/wsl-desktop/src/lib/paneSizing.ts`
  - `/home/jihoon/projects/devbox/apps/wsl-desktop/src/lib/workspace.ts`
  - `/home/jihoon/projects/devbox/apps/wsl-desktop/src-tauri/src/core/workspace.rs`

```rust
if store.version == LEGACY_PROFILE_STORE_VERSION {
    for profile in &mut store.profiles {
        for tab in &mut profile.tabs {
            tab.sizing = PaneSizing::even(tab.layout, tab.pane_keys.len());
        }
    }
    store.version = PROFILE_STORE_VERSION;
}
```

### Start-command safety

- Preserve the existing single confirmation covering every stored start command before restore begins.
- Store the approved one-shot command only in transient placeholder state and suppress it when a multiplexer session resumes.
- Choosing “layout only” keeps the definition for future saves but never sends the command to the new terminal.
- Read dialog/overlay blockers through a render-current ref so an older window-listener closure cannot launch a terminal during the frame in which a confirmation dialog appears.

## Verification Results

### Focused verification

```text
Frontend restore/workspace/pane suites: 5 files, 62 tests passed.
Frontend production build passed.
Rust workspace and integration focused tests passed.
cargo clippy -p wsl-desktop --all-targets -- -D warnings passed.
```

### Complete package verification

```text
pnpm --filter wsl-desktop test
29 files, 248 tests passed.

cargo test -p wsl-desktop
117 tests passed.

cargo check -p wsl-desktop
passed.

source ~/.cargo/env && pnpm verify:affected
wsl-desktop frontend build and bundle budgets passed.
Frontend: 29 files, 248 tests passed.
Rust: 117 tests passed, check/clippy/format passed.
Exit code: 0.
```

The existing Vite advisory for a chunk larger than 500 kB remains informational; repository raw and gzip budgets are checked by `pnpm verify:affected` and CI.

## Next Steps

- Pass the GitHub Actions Linux, frontend, and Windows compile gates before merge.
- Add native global shortcut and tray-based quick summon in the next independent PR.
