# WSL Desktop multiplexer refresh and preference preservation

## Overview

WSL Desktop now re-detects tmux and zellij without restarting the application and keeps the user's preferred session mode when a probe is temporarily missing or fails. The backend remains responsible for safely falling back to native mode for an individual launch.

## Context

- Multiplexer detection previously ran only when the selected distro changed.
- The WSL panel refresh updated the dashboard snapshot but not multiplexer availability.
- A missing or failed probe overwrote the persisted tmux/zellij preference with `native`, so a later successful detection did not restore the user's choice.

## Changes Made

### Detection lifecycle

- Added a request generation and selected-distro check so stale asynchronous probe responses cannot replace newer results.
- Added an explicit scanning state for progress and duplicate-click protection.
- Removed renderer-side mutation of the saved preference on `missing` and `error` results.
- File: `/home/jihoon/projects/devbox/apps/wsl-desktop/src/App.tsx`

```tsx
const sequence = ++muxRequestSequence.current;
const availability = await detectMultiplexers(distro);
if (sequence !== muxRequestSequence.current || selectedRef.current !== distro) return;
setMuxAvailability(availability);
```

### User controls and status

- The WSL panel refresh and command-palette refresh now update both dashboard and multiplexer state.
- Settings provides a `다시 검색` action with a busy state.
- When a preferred external multiplexer is unavailable, the UI explains that the preference is retained and only the current launch falls back to native.
- Files:
  - `/home/jihoon/projects/devbox/apps/wsl-desktop/src/App.tsx`
  - `/home/jihoon/projects/devbox/apps/wsl-desktop/src/components/SettingsPanel.tsx`

### Tests and documentation

- Replaced the old destructive-fallback expectation with preference-preservation coverage.
- Added regression tests for install-after-launch rescan, probe failure, and the shared WSL refresh action.
- Updated the WSL Desktop behavior contract.
- Files:
  - `/home/jihoon/projects/devbox/apps/wsl-desktop/src/App.settings.test.tsx`
  - `/home/jihoon/projects/devbox/apps/wsl-desktop/README.md`

## Verification Results

### Focused test

```text
pnpm --filter wsl-desktop test -- App.settings.test.tsx
Test Files  1 passed (1)
Tests       16 passed (16)
```

### Affected verification

```text
source ~/.cargo/env && pnpm verify:affected
frontend_packages=apps/wsl-desktop
Frontend build completed for 1 selected package(s).
Frontend bundle budgets passed for 1 app.
Test Files  27 passed (27)
Tests       235 passed (235)
Exit code: 0
```

The existing Vite advisory for an initial chunk larger than 500 kB remains non-blocking; the repository's stricter raw and gzip bundle budgets both passed.

## Next Steps

- Add explicit Bash/Zsh shell-integration diagnostics and setup in the next independent PR.
