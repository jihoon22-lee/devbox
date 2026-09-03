# WSL Desktop Quick Summon

## Overview

WSL Desktop can now show, focus, or hide its existing window with a configurable system-wide
shortcut. An optional tray mode changes the title-bar close button from process exit to window hide,
while keeping the exact React tab/pane state and every live PTY in the same process. The settings UI
always states whether close means hide or exit and surfaces shortcut/tray failures without reflecting
raw platform errors.

## Context

The app already restored saved workspace topology after a full restart, but restarting is still a
heavier operation than returning to a terminal that is already running. A true quick-summon path must
reuse the one single-instance window so terminal buffers, transient zoom, active tab/pane, and native
PTY handles remain untouched. It also must not reserve arbitrary OS shortcuts, silently swallow a
registration conflict, or make a hidden window impossible to exit.

## Changes Made

### Existing-window summon lifecycle

- Register one official Tauri global-shortcut handler after the optional backend initializes.
- Hide only when the main window is visible, focused, and not minimized; otherwise show,
  unminimize, and focus it.
- Reuse the same reveal helper for the existing single-instance relaunch path.
- Keep shortcut events on `Pressed` only so release events cannot immediately reverse a toggle.
- Match incoming native events against the currently active registered shortcut before changing the
  window.
- Treat plugin initialization as optional. If the OS hotkey backend cannot start, WSL Desktop still
  launches and the settings panel reports the unavailable backend.

The window is never destroyed in the hide path, so no tab, pane, renderer, PTY, or active identity is
reconstructed.

```rust
fn toggle_action(visible: bool, focused: bool, minimized: bool) -> ToggleAction {
    if visible && focused && !minimized {
        ToggleAction::Hide
    } else {
        ToggleAction::Reveal
    }
}
```

Primary files:

- `apps/wsl-desktop/src-tauri/src/quick_summon.rs`
- `apps/wsl-desktop/src-tauri/src/lib.rs`

### Bounded shortcut configuration and conflict recovery

- The renderer offers and the Rust IPC boundary independently allow only four fixed combinations:
  `Ctrl+Alt+Space`, `Ctrl+Shift+Space`, `Alt+Shift+Space`, and `Ctrl+Alt+F12`.
- No global-shortcut plugin permission is granted to the webview. The renderer can call only the
  app-owned `configure_quick_summon` command, so it cannot register an arbitrary system key.
- Native register/unregister mutations are mutex-serialized; the renderer also queues setting
  changes in user order and renders only the latest response.
- A shortcut replacement unregisters the old shortcut, attempts the new one, and restores the old
  registration if the new combination is unavailable.
- Status uses fixed issue codes. Raw OS/plugin errors go only to local stderr and never cross IPC.
- The settings panel distinguishes invalid input, backend initialization failure, OS/app shortcut
  conflict, rollback failure, and successful registration. If rollback succeeds, it names the old
  shortcut that remains active.

### Optional tray and explicit close semantics

- `닫을 때 트레이에 유지` dynamically creates the Tauri tray only when requested.
- The tray exposes `WSL Desktop 열기`, `창 숨기기`, and `완전히 종료`; a Windows left click also
  reveals the existing main window.
- Title-bar close is intercepted only after tray creation succeeds. Window geometry is synchronously
  persisted by the existing shared window-state adapter before the window is hidden.
- Disabling the setting removes the tray and immediately restores normal close-to-exit behavior.
- If tray creation fails, the backend leaves interception disabled and reports `trayUnavailable`, so
  the close button exits instead of stranding a hidden process.
- Tray `완전히 종료` persists the current bounds and exits the app, which also ends native PTYs.

### Settings migration and UI

- Settings schema version 2 adds `quickSummonEnabled`, `quickSummonShortcut`, and `keepInTray`.
- Version 1 values remain readable field by field; new fields receive safe defaults and the next
  mutation writes version 2. Unsupported versions still reset to defaults.
- Quick Summon defaults to enabled with `Ctrl+Alt+Space`; tray retention defaults off, preserving the
  prior close-to-exit behavior.
- The settings dialog shows live registration state and the effective close behavior, including an
  applying state while native configuration is in flight.
- The existing dialog accessibility regression test covers the new controls and status regions.

Primary files:

- `apps/wsl-desktop/src/lib/settings.ts`
- `apps/wsl-desktop/src/api.ts`
- `apps/wsl-desktop/src/App.tsx`
- `apps/wsl-desktop/src/components/SettingsPanel.tsx`
- `apps/wsl-desktop/src/App.css`

## Runtime Dependency Decision

| Field | Evidence |
|---|---|
| Purpose | `tauri-plugin-global-shortcut` supplies the maintained cross-platform system registration and event adapter needed to summon the already-running Tauri window. Tauri's existing `tray-icon` feature supplies the opt-in tray and menu without another library. Both paths have native/offline value and do not start another WSL process or terminal. |
| Alternatives | Hand-written Win32 `RegisterHotKey` code would create a Windows-only unsafe FFI and message-loop maintenance surface. JavaScript guest bindings would add a pnpm dependency and require dangerous register/unregister permissions in the webview. AutoHotkey, PowerToys, or a shell script would make a core app flow depend on separately installed software and configuration. Restarting the app loses live native PTYs and transient terminal state. |
| Source | Official Tauri plugin documentation: <https://v2.tauri.app/plugin/global-shortcut/>. Official source: <https://github.com/tauri-apps/plugins-workspace/tree/v2/plugins/global-shortcut>. Native adapter source: <https://github.com/tauri-apps/global-hotkey>. The Linux-only keysym helper is <https://github.com/notgull/xkeysym>. All resolved artifacts come from crates.io; no git or binary dependency is added. |
| Pin | Direct manifest constraint is `tauri-plugin-global-shortcut = "2.3.2"`. `Cargo.lock` resolves plugin 2.3.2 checksum `b4dd9f4c5136c09cd962da0c86dc4accd4666db2ea591cf16e6597435843bd2b`, `global-hotkey` 0.8.0 checksum `8c386b0a4a70cb2d39fffd74480f985b6f0bfbcb934b6a6b6b7e630e448f242e`, and Linux-only `xkeysym` 0.2.1 checksum `b9cc00251562a284751c9973bace760d86c0276c471b4be569fe6b068ee97a56`. Tauri 2.11.5 already resolved `tray-icon` 0.24.2 elsewhere in the workspace; enabling the feature adds no new tray package. Locked builds remain mandatory. |
| License | The plugin and `global-hotkey` are `Apache-2.0 OR MIT`; `xkeysym` is `MIT OR Apache-2.0 OR Zlib`. These are allowed by the Cargo policy. `cargo deny --locked check` passes licenses, sources, advisories, and bans. The generated `THIRD_PARTY_NOTICES.md` records every exact package/version/source/checksum/license and was not edited by hand. |
| Size | The three newly resolved source trees occupy 1,266,479 logical bytes / 1,572,864 allocated bytes in this Cargo cache; `xkeysym` is Linux-only and is absent from the Windows normal dependency graph. No frontend package was added. The repository's level-9 bundle checker measures initial JS changing from 692,611 to 697,053 bytes (+4,442), gzip 199,597 to 200,841 (+1,244). The built CSS artifact changes from 24,380 to 24,713 bytes (+333), and `gzip -n` from 5,273 to 5,336 (+63). Both repository bundle limits remain satisfied. This WSL session cannot produce or run the Windows installer, so it makes no installer-size or Windows runtime-memory claim; those measurements remain a Windows package-checkpoint responsibility. Runtime ownership is bounded to one plugin manager/event handler, two small mutex-protected state records, one active shortcut, and an optional tray/menu. |
| Security | The Rust boundary rejects every shortcut outside the four presets and deserialization rejects unknown config fields. The webview receives no plugin registration permission. Only the current native shortcut can toggle the fixed `main` window. Errors returned to the renderer are fixed enums, registration changes are serialized, and a failed replacement attempts rollback. Tray creation failure cannot enable close interception. The dependency audit found no new actionable advisory and no exception was added. |
| Offline | Registration, window show/hide/focus, tray creation, menu actions, and settings migration are entirely in-process after installation. There is no runtime download, network request, shell, WSL command, or external helper. |
| Maintenance | WSL Desktop maintainers own the exact direct plugin edge and fixed shortcut list. Update it with the Tauri/plugin family, re-run Linux and Windows compile checks, conflict/serialization/UI tests, Cargo policy, notices, and package measurements. Rollback removes `quick_summon.rs`, one direct crate edge, the Tauri tray feature, and three settings fields without changing terminal/session code. |

## Verification Results

### Focused and package checks

```text
pnpm --filter wsl-desktop test
29 files, 252 tests passed.

pnpm --dir apps/wsl-desktop build
passed; initial JS/CSS remain below repository raw and gzip budgets.

cargo test -p wsl-desktop
121 tests passed.

cargo check -p wsl-desktop
passed.

cargo clippy -p wsl-desktop --all-targets -- -D warnings
passed.

node .github/scripts/check-frontend-accessibility.mjs
15 apps passed.

pnpm audit --audit-level moderate
no known vulnerabilities.

cargo deny --locked check
advisories/licenses/bans/sources passed; existing duplicate-version warnings remain informational.

python3 .github/scripts/check-dependencies.py generate
python3 .github/scripts/check-dependencies.py check
generated notices and lockfile policy passed.

source ~/.cargo/env && pnpm verify:affected
selected only apps/wsl-desktop and Rust package wsl-desktop;
dependency audit selected because Cargo.lock changed; all checks passed.
```

The local `x86_64-pc-windows-msvc` check compiled the new global-shortcut Windows dependency graph
but stopped in the existing Tauri application build script because this WSL environment has no
`llvm-rc`. The repository's Windows CI compile job is the authoritative Windows gate.

## Next Steps

- Pass GitHub Actions frontend, Linux Rust, dependency policy, catalog, and Windows Rust jobs before
  merge.
- Measure the installed Windows binary and idle/runtime memory delta at the next package checkpoint;
  do not infer either from Linux source-tree or frontend bundle measurements.
