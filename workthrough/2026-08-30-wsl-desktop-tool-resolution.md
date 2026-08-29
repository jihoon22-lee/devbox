# WSL Desktop distro-user multiplexer resolution

## Overview

WSL Desktop previously executed bare `tmux` and `zellij` names through non-interactive
`wsl.exe` calls. That context can omit user-local directories which an interactive terminal adds,
so an installation such as `/home/jihoon/.local/bin/zellij` was reported as missing. The app now
resolves bounded distro-user candidates without loading shell rc files, keeps the resolved path in
the backend, and uses that same absolute executable for version probing, session lookup and PTY
launch.

## Context

- Reported symptom: `which zellij` returned `/home/jihoon/.local/bin/zellij`, while WSL Desktop
  displayed zellij as unavailable.
- Root cause: the app probed and launched the bare `zellij` name in WSL's direct-exec environment,
  which is not the user's interactive shell environment.
- Security boundary: the fix must not run `bash -lc`, source rc files, capture the entire
  environment, reflect absolute executable paths to the renderer, or turn environment text into a
  shell command.
- Tracking: Part of GitHub issue #482 and the `v0.6.0` milestone.

## Changes Made

### 1. Bounded distro-user candidate policy

- `/home/jihoon/projects/devbox/apps/wsl-desktop/src-tauri/src/core/multiplexer.rs`
  - Added a strict parser for the two-line `HOME`/`PATH` response.
  - Added bounded, deduplicated candidate generation from safe absolute PATH entries,
    `$HOME/.local/bin`, `$HOME/.cargo/bin`, `/usr/local/bin`, `/usr/bin` and `/bin`.
  - Rejects relative entries, traversal components, controls, oversized environment output and
    mismatched executable names.
  - Updated version, session-probe and launch argv builders to require validated absolute
    executables for tmux/zellij.

### 2. Safe resolution and launch-time revalidation

- `/home/jihoon/projects/devbox/apps/wsl-desktop/src-tauri/src/commands/multiplexer.rs`
  - Reads only `HOME` and `PATH` through fixed `/usr/bin/printenv` or `/bin/printenv` argv.
  - Probes each candidate with a three-second deadline, null stdin/stderr and 4 KiB bounded stdout.
  - Distinguishes `available`, `missing` and `error`; missing means every bounded candidate returned
    command-not-found, while timeout, invalid output and process failures remain errors.
  - Keeps the absolute path in a non-serializable backend token. Public IPC returns only kind,
    status, normalized version and the safe source category.
  - Resolves again for every start request so stale renderer state cannot select an old path.
- `/home/jihoon/projects/devbox/apps/wsl-desktop/src-tauri/src/commands/terminal.rs`
  - Uses the same backend token for existing-session lookup and PTY launch.
  - Falls back to the complete native workspace when resolution no longer succeeds.

Key argv shape:

```text
wsl.exe -d Ubuntu -- /usr/bin/printenv HOME PATH
wsl.exe -d Ubuntu -- /home/jihoon/.local/bin/zellij --version
wsl.exe -d Ubuntu -- /home/jihoon/.local/bin/zellij list-sessions --short --no-formatting
wsl.exe -d Ubuntu -- /home/jihoon/.local/bin/zellij attach --create <session> ...
```

Every value is a separate argv element; no shell string or rc file is involved.

### 3. Tri-state user experience

- `/home/jihoon/projects/devbox/apps/wsl-desktop/src/types.ts`
- `/home/jihoon/projects/devbox/apps/wsl-desktop/src/api.ts`
- `/home/jihoon/projects/devbox/apps/wsl-desktop/src/App.tsx`
- `/home/jihoon/projects/devbox/apps/wsl-desktop/src/components/WorkspacePanel.tsx`
- `/home/jihoon/projects/devbox/apps/wsl-desktop/src/App.css`
  - Replaced the ambiguous boolean with `available | missing | error`.
  - Shows `설치됨`, `없음`, or `확인 오류` distinctly and always keeps native available.
  - Displays only a safe category such as `사용자 로컬`; the resolved path is never part of the
    IPC type or rendered state.

### 4. Tests and documentation

- Added resolver fixtures for the reported `~/.local/bin/zellij` location, safe Unicode/spaces,
  missing tools, timeouts, malformed environments, invalid distros and exact no-shell argv.
- Added `/home/jihoon/projects/devbox/apps/wsl-desktop/src/components/WorkspacePanel.test.tsx` for
  tri-state rendering and path non-disclosure.
- Updated existing App mocks to the new typed response.
- Updated `/home/jihoon/projects/devbox/apps/wsl-desktop/README.md` with the resolver and privacy
  contract.
- No dependency or version change was made; app version changes remain centralized in the v0.6.0
  release work.

## Verification Results

```text
CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-wsl-tool-resolver \
  CARGO_BUILD_JOBS=2 cargo test -p wsl-desktop
97 passed; 0 failed

CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-wsl-tool-resolver \
  CARGO_BUILD_JOBS=2 cargo check -p wsl-desktop
passed

CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-wsl-tool-resolver \
  CARGO_BUILD_JOBS=2 cargo clippy -p wsl-desktop --all-targets -- -D warnings
passed

pnpm --filter wsl-desktop test
17 files; 135 tests passed

pnpm --filter wsl-desktop build
passed (existing >500 KiB Vite chunk warning remains)

cargo fmt --manifest-path apps/wsl-desktop/src-tauri/Cargo.toml -- --check
passed

wsl.exe -d Ubuntu -- /usr/bin/printenv HOME
/home/jihoon

wsl.exe -d Ubuntu -- /home/jihoon/.local/bin/zellij --version
zellij 0.44.3

wsl.exe -d Ubuntu -- /definitely/not-a-command
exit code 127 (used for the safe missing classification)
```

The final Rust evidence uses an isolated Linux-native target directory; results from the shared
workspace target were deliberately excluded because parallel worktrees can collide on identical
crate/version artifacts. During verification, host resources remained within the requested safety
margin: 11–13 GiB of memory was available, and Rust compilation was limited to two jobs.

## Remaining Acceptance

- Windows packaged testing must confirm the reported `/home/jihoon/.local/bin/zellij` installation
  is shown as available and starts/reattaches successfully.
- Repeat with tmux, a stopped or missing distro, a candidate removed between UI detection and
  launch, and a deliberately hanging probe. Confirm native fallback and no residual owned probe
  process after timeout.
