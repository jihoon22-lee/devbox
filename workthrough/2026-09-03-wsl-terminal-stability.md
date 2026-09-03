# WSL Desktop terminal interaction stability

## Overview

This change closes four race and state-generation gaps introduced around the recent WSL Desktop
terminal UX work. Broadcast approval is now bound to the exact target set that opened the dialog,
dialogs exclusively own keyboard shortcuts, an empty WSL installation never starts a guessed
`Ubuntu` distro, and pane sizing no longer resurrects a split from an obsolete pane topology.

The work is frontend-only. It does not change PTY argv construction, the Rust session store,
snapshot schemas, workspace/profile persistence, dependencies, package versions or release assets.

## Context

### Stale broadcast approval

`TermPane` captured `broadcastTargetIds` before awaiting the in-app danger confirmation. The owner
cleared its visible broadcast state when targets changed, but approving the already-open dialog
still used the captured array and could send the confirmed Enter or multiline payload to the old
sessions. Input queued behind that confirmation could then be replayed against a new generation.

### Shortcuts escaping dialogs

The app-level shortcut matcher remained active while Settings, Shortcut Reference, Action Palette
or `AppDialog` owned focus. A `Ctrl+Shift+T`, `Ctrl+Shift+D` or `Ctrl+Shift+W` key event could bubble
from a dialog and mutate the terminal workspace behind it. The same handler is also reachable from
xterm during the frame before dialog focus settles, so guarding only the window listener would not
cover the full path.

### Empty distro startup

A successful dashboard response with `distros: []` fell back to the literal string `Ubuntu`.
Automatic startup, the toolbar, the tab add button, shortcuts and AppLink could consequently call
the native start command with a distro that the snapshot had not reported. This is different from a
failed snapshot: it is a valid observation that no distro is registered and must remain an empty
workspace.

### Obsolete pane sizing

Pane sizing was stored only by tab id. `normalizeFractions` displayed an even split when the stored
array length did not match a new pane count, but it did not invalidate the stored array. Returning
from three panes to two panes, or switching a layout away and back, made the old two-pane split
reappear. That contradicted the documented rule that a topology change resets sizing.

## Changes made

### 1. Bind broadcast confirmation to its target generation

Files:

- `apps/wsl-desktop/src/components/TermPane.tsx`
- `apps/wsl-desktop/src/components/TermPane.test.tsx`

After the asynchronous confirmation resolves, `TermPane` compares the current armed state and
ordered target ids with the exact array that produced the question. A stale approval sends nothing,
clears the partially assessed command, and discards queued input. Fresh input after the invalidation
uses the new targets normally. Backend target validation remains an independent final guard.

### 2. Give dialogs exclusive shortcut ownership

Files:

- `apps/wsl-desktop/src/App.tsx`
- `apps/wsl-desktop/src/App.settings.test.tsx`

`handleShortcut` now returns while any app dialog is open. Placing the check in the shared handler
covers both the global `window` listener and the callback held by `TermPane`. Tests exercise a
regular Settings dialog and an asynchronous close confirmation.

### 3. Keep an empty distro list empty

Files:

- `apps/wsl-desktop/src/App.tsx`
- `apps/wsl-desktop/src/App.settings.test.tsx`

Dashboard hydration now chooses `""` when neither a default nor first distro exists. Automatic
startup requires a non-empty selected distro, both visible terminal creation buttons are disabled,
and `startInTab` rejects every remaining programmatic path before native IPC. The guard returns a
fixed user-facing error and does not expose backend details.

### 4. Version pane sizing by topology

Files:

- `apps/wsl-desktop/src/components/PaneCanvas.tsx`
- `apps/wsl-desktop/src/components/PaneCanvas.test.tsx`

Each tab's in-memory sizing now carries a topology signature made from its layout and ordered pane
ids. A mismatch renders evenly on the first commit and replaces the stored generation, so an old
shape cannot return later. Temporary zoom is deliberately excluded from the signature and renders
as one track, preserving the underlying proportions when zoom is released.

### 5. User-facing documentation

Files:

- `apps/wsl-desktop/README.md`
- `CHANGELOG.md`
- `workthrough/2026-09-03-wsl-terminal-stability.md`

The README and Unreleased changelog now state the target-invalidation, modal shortcut, empty-distro
and sizing-generation behavior. No new runtime or development dependency was added.

## Code examples

### Reject an approval from an obsolete target set

```tsx
// apps/wsl-desktop/src/components/TermPane.tsx
const currentTargets = broadcastTargetsRef.current;
const contextStillCurrent = broadcastRef.current
  && currentTargets.length === targets.length
  && currentTargets.every((target, index) => target === targets[index]);

if (approved.confirmed && contextStillCurrent) {
  sendBroadcast(targets, data);
} else if (!contextStillCurrent) {
  broadcastPendingCommandRef.current = "";
  queuedInputRef.current = [];
}
```

### Keep sizing attached to one pane topology

```tsx
// apps/wsl-desktop/src/components/PaneCanvas.tsx
const topology = activeTab ? JSON.stringify([baseLayout, ...allActivePaneIds]) : "";
const sizingMatchesTopology = currentSizing?.topology === topology;
const columns = zoomed
  ? [1]
  : normalizeFractions(sizingMatchesTopology ? currentSizing.columns : undefined, columnCount);
```

### Fail before a native start with no selected distro

```tsx
// apps/wsl-desktop/src/App.tsx
if (!distro.trim()) {
  setError("사용 가능한 WSL 배포판이 없습니다.");
  return false;
}
```

## Verification results

### Focused regressions

```text
pnpm --filter wsl-desktop exec vitest run \
  src/App.settings.test.tsx \
  src/components/TermPane.test.tsx \
  src/components/PaneCanvas.test.tsx

3 files / 53 tests passed
Exit code: 0
```

The new coverage verifies:

- stale approval and queued input never reach either the old or replacement broadcast targets;
- subsequent fresh input reaches only the replacement target set;
- Settings and confirm dialogs block app shortcuts behind the modal;
- an empty distro snapshot performs no native start through startup or keyboard paths;
- pane add/remove and layout round trips do not revive old sizing;
- zoom round trips retain the current valid split.

### WSL Desktop suite and production build

```text
pnpm --filter wsl-desktop test
27 files / 231 tests passed
Exit code: 0

pnpm --filter wsl-desktop build
TypeScript and Vite production build passed
76 modules transformed
initial JS 680.49 kB raw / 196.58 kB gzip
lazy WebGL chunk 110.23 kB raw / 29.92 kB gzip
Exit code: 0
```

The existing Vite large-chunk advisory remains informational and the WebGL renderer stays in its
existing lazy chunk. No Windows packaged runtime was launched from WSL; the affected behavior is
covered at the renderer boundary with mocked native commands.

### Affected workspace verification

```text
pnpm verify:affected
scope resolver and runner regression tests passed
frontend_scope=apps
frontend_packages=apps/wsl-desktop
rust_scope=none
dependency_scope=none
WSL Desktop build, bundle budget and 231 tests passed
Additional TypeScript packages: 0
Elapsed: 12.3 s
Exit code: 0
```

This is the first ordinary application change after the CI efficiency work. It confirms that the
local completion path did not start Cargo or dependency audits and did not build/test the other 14
applications.

### Static repository contracts

```text
node .github/scripts/check-frontend-accessibility.mjs   PASS (15 static app contracts)
bash .github/scripts/check-catalog.sh                   PASS
git diff --check                                        PASS
```

The repository-wide accessibility and catalog checks inspect source/contracts only; they do not
allocate per-app build or test processes.

## Next steps

- Require the PR's GitHub Actions CI checks to pass before merge.
- Continue API Playground MCP draft preservation and shared AppLink listener lifecycle as separate
  user-visible rollback boundaries.
