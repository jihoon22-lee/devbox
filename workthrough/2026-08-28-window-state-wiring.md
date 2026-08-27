# Cross-app persistent window-state wiring audit

## Overview

This feature branch was rebased onto `origin/main` at `2b53744`, after the
`crates/window-state` contract (#322), Devbox Launcher (#320), and Log Lens
bootstrap (#321) had landed. It adds one Tauri adapter and applies the same
persistent-window policy to all fourteen applicable apps in #323–#336.

Devbox Launcher is the fifteenth app and the deliberate negative case: its
palette is transient, so it does not install the adapter or create a state
file.

## Scope and inventory

| issue | app | selected wiring |
|---|---|---|
| #323 | Port Manager | setup restore + move/resize/DPI/close save |
| #324 | Developer Toolbox | setup restore + move/resize/DPI/close save |
| #325 | WSL Desktop | setup restore + move/resize/DPI/close save |
| #326 | API Playground | setup restore + move/resize/DPI/close save |
| #327 | Everything+ | setup restore + move/resize/DPI/close save |
| #328 | Knowledge | setup restore + move/resize/DPI/close save |
| #329 | Life Log | same events + save before hide/prevent-close and tray quit |
| #330 | Devbox Manager | setup restore + move/resize/DPI/close save |
| #331 | Code Pad | same events + save before orderly `app.exit` |
| #332 | Run Manager | same events + save before hide/orderly `app.exit` |
| #333 | Workbench | setup restore + move/resize/DPI/close save |
| #334 | Webhook Lab | setup restore + move/resize/DPI/close save |
| #335 | Repo Manager | setup restore + move/resize/DPI/close save |
| #336 | Log Lens | setup restore + move/resize/DPI/close save |

Log Lens currently uses browser/native dialogs rather than additional Tauri
windows for source picking and export. The adapter nevertheless filters by the
`main` label, so any future picker, export, preview, splash, or child window
remains transient by default.

## Changes made

### Shared adapter

- Added `crates/window-state-tauri` and registered it in the Cargo workspace.
- Read at most `MAX_STATE_BYTES + 1` bytes from each app-local
  `window-state-v1.json` file and delegate strict decoding, monitor selection,
  DPI transform, resolution shrink, and visible-titlebar clamp to the pure
  contract.
- Convert Tauri physical monitor/window facts into that contract, retaining
  the last normal bounds while maximized.
- Coalesce move/resize/scale events into one latest-value slot with a 150 ms
  debounce instead of allocating an unbounded write queue.
- Persist complete validated bytes through the existing atomic filesystem
  helper. A failed write retains the latest bytes for a later event or explicit
  flush, without creating a self-sustaining hot retry loop.
- Ignore every window label except `main`.

### App integration

All fourteen target manifests include the adapter dependency. Their Tauri
builders restore during setup and register the global window-event callback.
Life Log and Run Manager save before their existing hide/prevent-close path.
Life Log, Run Manager, and Code Pad also save before explicit `app.exit` calls.
Log Lens has no separate programmatic exit or close-to-tray path.

No frontend command or capability permission is added: the adapter operates
entirely through native Rust/Tauri APIs.

## Review corrections

- Added a missing `restore_window` import found by the root integration review.
- Changed failed-write handling so pending bytes are retained for durability,
  but a failure is retried only by a new event or explicit flush. This prevents
  a locked or read-only filesystem from driving a continuous retry loop.
- Rebased after Launcher and Log Lens landed, resolving Manager and Run Manager
  setup conflicts additively so their existing app-link handlers remain active.
- Added Log Lens `main` wiring and retained Launcher as the transient negative
  case.

## Verification

- `cargo test -p window-state-tauri -j1`: passed, 6 tests.
- `cargo test -p window-state -j1`: passed, 14 unit/integration tests.
- `cargo check -p window-state-tauri -j1`: passed.
- `cargo test --workspace -j1`: passed for all crates and 15 apps.
- `cargo check --workspace -j1`: passed.
- `pnpm -r --workspace-concurrency=4 test -- --maxWorkers=1`: passed for
  every frontend test package. The worker cap avoids WSL `/mnt/e` startup
  timeouts while retaining bounded package parallelism.
- `pnpm build`: passed for all frontend workspaces. Existing Code Pad,
  Knowledge, and WSL Desktop chunk-size warnings remain informational.
- `cargo clippy --workspace --all-targets -j1 -- -D warnings`: passed.
- `cargo fmt --all -- --check`, catalog consistency, dependency policy and
  regression tests, build-manifest tests, and `git diff --check`: passed.
- Regenerated `THIRD_PARTY_NOTICES.md`; the only notice change is the expected
  Cargo.lock digest update for Log Lens consuming the internal adapter.

Windows W4 packaged smoke remains a release gate because WSL cannot reproduce
real multi-monitor removal, per-monitor DPI transitions, tray behavior, or the
installed app-local data paths. Its matrix is move → DPI/resolution → maximize
→ restart → close/tray/explicit-exit, plus Launcher transient-negative and Log
Lens `main` cases.
