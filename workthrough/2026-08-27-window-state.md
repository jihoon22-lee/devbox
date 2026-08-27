# Add the shared persistent window-state contract

## Overview

Issue #322 establishes the reusable prerequisite for the later #323–#336
cross-app window wiring. This change adds a pure Rust `window-state` crate from
the latest `main`; it stores ordinary-window bounds, maximized state, monitor
identity, saved work area, and scale factor, then computes safe restore
geometry when the display topology or DPI changes. No app wiring, Tauri API, or
file-system side effect is included.

## Context

- The v0.5.0 plan requires monitor removal, DPI/resolution changes, and a
  visible-titlebar clamp to be shared by persistent windows.
- The issue explicitly excludes Launcher/dialog/splash transient windows and
  app-specific integration.
- The crate must remain testable in WSL, so platform adapters and atomic file
  persistence stay with each consuming app.

## Changes Made

### 1. Added the bounded pure contract

File: `crates/window-state/src/lib.rs`

- Added strict v1 JSON (`schemaVersion`, physical-pixel `bounds`, `monitorId`,
  `monitorWorkArea`, `scaleFactor`, and `maximized`).
- Added 16 KiB document, 256-byte monitor identity, coordinate/dimension, and
  finite 0.5–8.0 scale-factor bounds.
- Rejected unknown fields, unsupported schema versions, invalid geometry,
  control characters, and non-finite scale factors with fixed, non-reflective
  errors.
- `encode_state`/`decode_state` are serialization-only APIs; they do not read
  or write paths.
- `restore_window` matches the saved monitor identity first, then falls back
  to primary → first valid monitor. It maps saved relative physical geometry
  through the saved/current work areas and scale ratio, preserves maximized
  state, and leaves a configurable portion of the top titlebar reachable.
- `restore_from_bytes` turns missing data into defaults and malformed,
  oversized, or future data into a safe default with a `CorruptState` source.
- Custom `Serialize` validation prevents public struct literals from bypassing
  persistence checks.

### 2. Added deterministic fixtures and regression coverage

Files:

- `crates/window-state/tests/fixtures/window-state-v1.json`
- `crates/window-state/tests/window_state.rs`

Coverage includes fixture round-trip, deterministic encoding, strict/future
schema and unknown-field rejection, oversized input, invalid monitor/scale/
geometry, DPI scaling, negative/changed monitor coordinates, removed-monitor
primary/first-valid fallback, resolution-shrink visible-titlebar clamping,
maximized preservation, empty monitor fallback, and non-reflective corruption handling.

### 3. Registered the workspace and documentation

Files:

- `Cargo.toml` / `Cargo.lock`
- `docs/architecture.md`
- `docs/roadmap.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`

The workspace now includes `crates/window-state`. Architecture documents the
physical-pixel JSON contract and ownership boundary; the roadmap marks #322
as the completed prerequisite and leaves #323–#336 as one later cross-app
wiring PR; the v0.5.0 plan no longer implies that the crate must wait for a
second consumer before being extracted.

### 4. Root review follow-up

- Split the roadmap wording into the standalone pure-crate prerequisite (#322) and the single
  cross-app wiring PR (#323–#336), removing the old contradictory one-PR statement.
- Made the checked-in future-schema fixture assert the exact `UnsupportedSchema` boundary.
- Added a resolution-shrink fixture that also proves deterministic first-valid fallback when the
  current monitor list has no primary display.

## Code Examples

```rust
let state = WindowState::capture(current_bounds, &current_monitor, is_maximized)?;
let bytes = encode_state(&state)?; // app-owned atomic file write follows

let restored = restore_from_bytes(
    persisted_bytes.as_deref(),
    &current_monitors,
    RestoreConfig::default(),
);
// Apply `restored.state` through the platform/Tauri adapter.
```

The crate intentionally has no `std::fs`, Tauri, Windows, network, or process
dependencies. Monitor identities are opaque strings and are never interpreted
as paths or commands.

## Verification Results

### Focused Rust checks

```text
cargo test -p window-state
9 unit tests + 5 integration tests + 0 doc tests: passed

cargo check -p window-state: passed
cargo clippy -p window-state --all-targets -- -D warnings: passed
cargo fmt --package window-state -- --check: passed
git diff --check: passed
```

The repository-wide `pnpm build`, full workspace Cargo build, and Windows
packaged smoke were intentionally not run because this prerequisite task asks
for low-load focused verification and contains no frontend or native app
wiring.

## Next Steps

- Keep app-specific persistence paths and atomic writes in the #323–#336
  cross-app wiring PR.
- Each consumer must translate its native monitor snapshot into
  `MonitorInfo`, apply `RestoredWindowState`, and add Windows packaged/DPI
  evidence without persisting transient windows.
- This worktree remains dirty and uncommitted by request; no commit, push, or
  PR was created.
