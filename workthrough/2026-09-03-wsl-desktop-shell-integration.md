# WSL Desktop Bash and Zsh shell integration

## Overview

WSL Desktop can now diagnose and explicitly install, repair, or remove marker-owned Bash/Zsh OSC 7 integration. Each terminal pane shows whether a live cwd signal has arrived, allowing users to distinguish an exact restorable cwd from the initial launch path.

## Context

The workspace schema already stored a cwd per pane, but the value only changed when the shell emitted OSC 7. Users previously had to know how to edit shell rc files manually, and the application did not explain when it only knew the starting directory.

## Changes Made

### Marker-owned shell blocks

- Added canonical Bash and Zsh hooks that percent-encode UTF-8 paths and emit OSC 7 before each prompt.
- Preserved existing Bash `PROMPT_COMMAND` hooks, Zsh `precmd` hooks, and the previous command exit status.
- Recognized the earlier manually installed `WSL Desktop OSC 7 cwd integration` marker as an upgradeable block.
- Added pure planning for install, repair, and removal without filesystem access.
- File: `/home/jihoon/projects/devbox/apps/wsl-desktop/src-tauri/src/core/shell_integration.rs`

```rust
let plan = plan_content(&snapshot.content, shell, action)?;
let Some(next) = plan.next else {
    return Ok(unchanged_result);
};
```

### Bounded WSL mutation boundary

- Queried `HOME` and `SHELL` through exact `wsl.exe --exec` arguments without loading login or rc scripts.
- Limited rc reads to 1 MiB and rejected non-UTF-8 input.
- Refused symbolic links, non-regular files, duplicate markers, and incomplete markers.
- Required an opaque preview revision and checked the source again immediately before rename.
- Created a preserved timestamped backup, wrote through a same-directory bounded temporary file, preserved file mode, and removed the temporary file on failure.
- Serialized mutation requests so two UI actions cannot race.
- Returned only `~/.bashrc`/`~/.zshrc`, status, revision, and the owned block; raw rc content and the real home path never reach the renderer.
- File: `/home/jihoon/projects/devbox/apps/wsl-desktop/src-tauri/src/commands/shell_integration.rs`

### Settings and pane diagnostics

- Added a Settings section with default-shell detection, status refresh, block copy, and confirmed install/repair/removal.
- Displayed the exact owned block in the final confirmation before mutation.
- Added `cwd 미확인` and `cwd 추적` pane badges driven by valid OSC 7 input.
- Files:
  - `/home/jihoon/projects/devbox/apps/wsl-desktop/src/components/ShellIntegrationSettings.tsx`
  - `/home/jihoon/projects/devbox/apps/wsl-desktop/src/components/SettingsPanel.tsx`
  - `/home/jihoon/projects/devbox/apps/wsl-desktop/src/components/TermPane.tsx`
  - `/home/jihoon/projects/devbox/apps/wsl-desktop/src/api.ts`
  - `/home/jihoon/projects/devbox/apps/wsl-desktop/src/types.ts`

### Tests and documentation

- Covered marker conflicts, legacy upgrade, idempotence, stale revisions, exact argv construction, blocked files, request races, confirmation cancellation, clipboard copy, and pane signal status.
- Executed the generated blocks through local Bash and Zsh for syntax and UTF-8 percent-encoding verification on Linux.
- Updated `/home/jihoon/projects/devbox/apps/wsl-desktop/README.md` with the safety and persistence contract.

## Verification Results

### Focused verification

```text
cargo test -p wsl-desktop shell_integration
13 passed; 0 failed

pnpm --filter wsl-desktop test -- ShellIntegrationSettings.test.tsx TermPane.test.tsx App.settings.test.tsx
Test Files  3 passed (3)
Tests       45 passed (45)
```

### Complete affected verification

```text
source ~/.cargo/env && pnpm verify:affected
frontend_packages=apps/wsl-desktop
rust_packages=wsl-desktop
Frontend build and bundle budgets passed.
Frontend: 28 files, 240 tests passed.
Rust: 115 tests passed.
Exit code: 0
```

The existing Vite advisory for a chunk larger than 500 kB remains non-blocking; repository raw and gzip budgets passed.

## Next Steps

- Validate the installed Windows application against a real Bash and Zsh distro after the Windows CI compile gate.
- Implement progressive, topology-preserving workspace restore in the next independent PR.
