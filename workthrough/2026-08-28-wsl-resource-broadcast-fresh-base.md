# WSL Desktop #344/#345 fresh-base integration

## Overview

Rebased the grouped WSL Desktop resource-summary and broadcast-safety candidate onto
`origin/main` `c47dc270` (the post-#450 main tip). Only WSL Desktop code, focused tests and aligned
root documentation are changed; launcher, Log Lens, Port Manager and other-app implementation
changes are not part of this candidate. The merged cross-app window-state integration remains
intact in the rebased app.

## Selected implementation

- `runtime_snapshot.rs`: one bounded dashboard/runtime collection with fixed argv, child and
  collection deadlines, last-good atomic publication, and a collection lock held through revision
  allocation and publication so concurrent refreshes cannot publish an older collection under a
  newer revision.
- `core/resources.rs`: numeric-only `/proc/stat` CPU delta and memory/disk parsing with bounded
  output, checked arithmetic, first-sample/counter-reset `null`, JavaScript-safe integer bounds,
  and rejection of inconsistent `df` used/available totals.
- `core/parsers.rs` and runtime model/command layers: strict WSL/Docker parsing, validated distro
  identity (including the 128-byte name cap), bounded container/session data, fixed argv and
  privacy-safe error/publication behavior.
- `App.tsx`, `DistroPanel.tsx`, `TermPane.tsx`, `PaneCanvas.tsx`, `api.ts`, `types.ts` and CSS:
  one dashboard snapshot for distro/resource/Docker/session display, TTL-driven single-flight
  polling, stale/error/refreshing fail-closed broadcast state, explicit target selection,
  accessible status/controls and continued single-pane PTY operation.
- `broadcastSafety.ts`: conservative danger confirmation for every `sudo`/`rm` command and spaced or
  unspaced shell redirection, plus a 32-target selection bound.  Renderer startup/broadcast
  failures use fixed safe messages instead of raw native paths or credentials.
- App/applink, DistroPanel, TermPane, context-menu, resource-display and broadcast fixtures were
  selected with the implementation so the two issue acceptance paths remain independently
  testable inside one grouped PR.

## Documentation

`apps/wsl-desktop/README.md` and the root architecture/roadmap/native-first/terminal design records
were kept aligned with the code: fixed read-only resource
argv, bounded collection/privacy behavior, shared snapshot freshness, 2–32 broadcast targets and
confirmation for `<`, `>`, `<<` and `>>` even without whitespace.

## Root review corrections

- Replaced one-minute load-average normalization mislabeled as `cpuPercent` with consecutive
  aggregate `/proc/stat` counter deltas. The first sample and counter reset now render `—`, and
  stopped/removed distro baselines are discarded.
- Added a bounded TTL-driven renderer poll using the existing promise single-flight, plus a
  StrictMode-safe mounted guard so development effect replay cannot discard every response.
- Expanded danger confirmation to all `sudo` and `rm` commands, matching the issue acceptance,
  while retaining multiline and whitespace-free redirection confirmation.

## Verification and remaining risks

- Fresh worktree base is `c47dc27062bbf5dc3392e307aedc90b4008798a4`.
- `cargo test -p wsl-desktop -j1`: 81 passed.
- `cargo clippy -p wsl-desktop --all-targets -j1 -- -D warnings`: passed.
- `cargo check --workspace -j1`: passed.
- `pnpm --filter wsl-desktop test -- --maxWorkers=3`: 16 files, 130 tests passed. The first
  single-worker run exposed one obsolete non-recursive-`rm` expectation; the acceptance-aligned
  correction passed both its focused test and the full rerun.
- `pnpm --filter wsl-desktop build` and bounded-concurrency whole-workspace frontend build: passed.
- Cargo format, `git diff --check`, dependency-policy regression, build-manifest notice and catalog
  consistency checks: passed.
- CI and Windows W3 packaged smoke remain required. W3 must cover running/stopped distros, Docker
  available/missing/error, timeout/last-good behavior, stale broadcast gating, 2–32 target
  selection, dangerous cancellation/reconfirmation and continued normal PTY I/O.
