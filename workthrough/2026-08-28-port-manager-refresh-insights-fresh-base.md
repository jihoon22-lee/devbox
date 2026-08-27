# Port Manager #337–#339 fresh-base integration

## Overview

The grouped Port Manager refresh/insights candidate was selected from the dirty
`port-manager-refresh-insights-latest` worktree and applied to a new branch based on
`origin/main` `14716d0`.  The selection is limited to the Port Manager app, its focused tests,
the Port Manager README, and the precise architecture/roadmap/native-first plan hunks for
#337–#339.  Launcher, Log Lens, window-state, other apps, and stale root-document snapshots were
not carried over.

## Feature boundary retained

- #337 provides a bounded 1–60 second refresh interval, manual pause/resume, timer/manual
  single-flight behavior, request-generation and unmount guards, stable-row retention after a
  failed poll, and fail-closed kill/handoff controls while the snapshot is unhealthy.
- #338 establishes the first successful snapshot as a baseline and computes subsequent
  `new`/`closed`/`changed` endpoint changes using Windows FILETIME, WSL distro/PID/start-tick, or
  container engine/distro/ID identity.  Identity-less rows use only their own endpoint fallback;
  failed polls do not replace the baseline.
- #339 adds independent bounded port/process favorite lists and their pinned union, strict
  app-local preferences, source/provenance display, and atomic persistence without command line,
  executable path, credential, or arbitrary path fields.

## Selected files

### Native

- `apps/port-manager/src-tauri/src/core/preferences.rs`: strict preferences DTO, schema/unknown
  field rejection, 1–60 second interval, 64 KiB document and per-kind 256-entry bounds,
  endpoint/source/identity validation, duplicate rejection and atomic persistence.
- `apps/port-manager/src-tauri/src/commands/preferences.rs`: bounded app-owned load/save IPC.
- `apps/port-manager/src-tauri/src/commands/mod.rs`, `core/mod.rs`, `lib.rs`: command/module
  registration only; existing listener collectors and #285 endpoint+identity revalidation remain
  the safety boundary.

### Frontend

- `src/refresh.ts`: identity-aware lifecycle diff, stale/duplicate selection guards, and bounded
  preference/favorite helpers.
- `src/App.tsx`, `src/App.css`, `src/types.ts`, `src/api.ts`: single-flight refresh/pause state,
  baseline/diff/error behavior, provenance/detail rendering, favorite/pinned controls and
  accessible live status/focus handling.  Container row keys include engine as well as
  distro/ID/endpoint so Docker and Podman identities cannot collide in selection state.
- `src/App.test.tsx`: refresh race/cancel/failure, baseline and lifecycle diff, identity fallback,
  engine-distinct container keys, favorite persistence, provenance, stale selection and
  accessibility/privacy fixtures.
- `apps/port-manager/README.md`: app-local contract for bounds, source identity and persistence.

### Documentation hunks

Only the #337–#339 additions were merged into the latest versions of:

- `docs/architecture.md`
- `docs/roadmap.md`
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md`

No unrelated latest-main document content was replaced.  This workthrough records the fresh-base
selection and does not alter the older dirty source worktree.

## Verification

- Fresh branch HEAD: `14716d0d3963da5f98fe5cd09e1320e36992980b`.
- Final integration base: `origin/main` `952d2a7`, including Log Lens and the
  fourteen-app persistent window-state wiring. The rebase retained Port Manager's adapter
  dependency and added only its preference commands and Windows Job Object feature.
- Root integration review corrected three lifecycle edges before final rebase:
  - endpoint matching now reserves all exact identity+endpoint pairs before applying the
    identity-only move fallback, preventing a same-process multi-endpoint snapshot from producing
    two false `changed` rows;
  - a successful listener kill waits for any pre-kill single-flight poll and then starts one fresh
    native snapshot, so stale rows do not remain until the next timer interval;
  - fixed Windows discovery commands are assigned to a kill-on-close Job Object, so timeout,
    output failure, and successful root exit cannot leave WSL/container descendants holding the
    bounded stdout pipe across automatic polls.
- Port favorites with port zero are rejected by core validation and disabled consistently in the
  table, details, and context-menu surfaces.
- `cargo test -p port-manager -j1`: passed, 27 tests.
- `cargo check --workspace -j1`: passed for all crates and fifteen apps.
- `cargo clippy -p port-manager --all-targets -j1 -- -D warnings`: passed after replacing one
  test-only default-field reassignment with an explicit initializer.
- `pnpm --filter port-manager test -- --maxWorkers=1`: passed, 37 tests.
- `pnpm --filter port-manager build`: passed.
- `cargo fmt --all -- --check`, catalog consistency, dependency policy and regression tests,
  build-manifest tests, and `git diff --check`: passed.
- The roadmap's preceding #323–#336 checkbox was updated to completed after PR #449 landed.

## Remaining release verification

1. Require the PR's Windows compile check because the Job Object implementation is target-gated
   and WSL cannot compile or exercise that native branch.
2. Perform Windows W3 smoke for slow/cancelled netstat, WSL and Docker refreshes; Windows FILETIME
   and WSL start-tick identity changes; permission failures; atomic app-data replacement; timer
   pause/resume; stale selection; and keyboard/focus/a11y behavior.
