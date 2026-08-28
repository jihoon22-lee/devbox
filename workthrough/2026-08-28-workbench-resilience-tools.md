# Workbench resilience/inspection tools (#359 + #360 + #361)

## Overview

This candidate prepares Workbench P3-14 as one reviewable flow: reusable profile
templates and a new-project wizard, read-only dependency inspection, and a safe
retry from the first failed Workspace step. The three issues were grouped because
they share the same profile/health/Start Workspace lifecycle, while their data,
acceptance fixtures, and rollback boundaries remain independent.

The worktree was created from the latest `origin/main` on
`feat/workbench/resilience-tools`. It is intentionally left dirty: no commit,
push, PR, rebase, merge, or worktree deletion was performed.

## Context

Before this candidate, Workbench had ProjectProfile CRUD, Start Workspace
preflight, partial run reporting, and `Stop What I Started`, but it lacked:

- a safe, reusable template store and wizard for creating a concrete profile;
- a separately named dependency-health surface that reused the preflight contract;
- a persisted retry plan that could resume only unfinished steps without
  restarting already-existing or Workbench-owned resources.

The design keeps the existing ownership rule: Workbench can read other apps'
snapshots, but it does not edit their databases, install dependencies, start
Run Manager services, or perform destructive auto-repair.

## Changes Made

### 1. Profile template storage and wizard (#359)

Files:

- `apps/workbench/src-tauri/src/core/templates.rs`
- `apps/workbench/src-tauri/src/commands/templates.rs`
- `apps/workbench/src/lib/profileTemplateEditor.ts`
- `apps/workbench/src/lib/profileTemplateEditor.test.ts`
- `apps/workbench/src/api.ts`
- `apps/workbench/src/App.tsx`
- `apps/workbench/src/App.css`

`ProfileTemplateStore` is versioned and bounded (128 templates, 1 MiB file),
uses strict serde fields, validates safe paths/ports/services, and stores no
environment or secret data. The command layer writes a separate
`profile-templates.json` with the same no-follow identity checks, raw-byte CAS,
and atomic replacement policy as the profile store. Applying a template fills
only empty wizard fields and clears any incoming environment metadata before
the concrete profile is validated.

The renderer now exposes a template manager and wizard. Both keep invalid input
in draft state; the backend generates an ID for a new profile, revalidates the
template/profile, and reports only fixed UI errors.

### 2. Dependency health inspection (#360)

Files:

- `apps/workbench/src-tauri/src/core/preflight.rs`
- `apps/workbench/src-tauri/src/commands/preflight.rs`
- `apps/workbench/src-tauri/src/lib.rs`
- `apps/workbench/src/api.ts`
- `apps/workbench/src/App.tsx`
- `apps/workbench/src/App.css`
- `apps/workbench/src/App.test.tsx`
- `apps/workbench/src/App.applink.test.tsx`

`dependency_health` is an explicit read-only command returning the existing
bounded `WorkspacePreflight` shape (aliased as `DependencyHealth`). It shares
the fixed probes and `ResourceProvenance` states for required app capability,
WSL distro/path, TCP ports, and Run Manager service snapshots. Both it and the
preflight review now use the Workbench `health_operation` single-flight lane,
so a Start Workspace transition or another health request cannot overlap the
bounded native probes. A cancelled request waits for its already-running
probe to join before releasing the lane. The UI renders the states and
supports an independent refresh with a request generation guard. No health
command mutates or cleans up an external resource.

### 3. Idempotent failed-step retry (#361)

Files:

- `apps/workbench/src-tauri/src/core/retry.rs`
- `apps/workbench/src-tauri/src/commands/workspace.rs`
- `apps/workbench/src-tauri/src/lib.rs`
- `apps/workbench/src/api.ts`
- `apps/workbench/src/App.tsx`
- `apps/workbench/src/App.test.tsx`

The pure planner accepts only the bounded sequence
`wait-port → open-wsl-desktop → open-code-pad`. It resumes at the first known
failure, skips successful steps and existing (`Existing`) or
`WorkbenchStarted` processes, and never restarts an already-running resource.
`WorkspaceRun` and the safe ownership DTO
now carry retry count/availability/failure metadata without serializing PIDs.

The command reruns preflight and profile/environment validation before each
child. A separate `StartedPidGuard` owns only processes created by that retry
until the updated run is atomically installed. Profile changes, timeout,
cancellation, provider failures, or publish conflicts roll back those new PIDs;
ordinary launch failures remain an inspectable partial run.

### 4. Resilience remediation audit (#359–#361)

The review covered command/process/path/env/secret/privacy, TOCTOU and concurrency,
cancellation/timeout, process-tree cleanup, resource bounds, rollback, stale UI state,
and accessibility rather than treating the three features as happy-path CRUD.

#### Read-only health and preflight cancellation

`workspace_preflight` and `dependency_health` now use an exact NUL-delimited request key,
the shared `health_operation` single-flight lane, an operation token and bounded budget.
Native probes use fixed argv, null stdin, discarded stderr, bounded output, fixed status
errors, and a common deadline. A detached command worker retains its lane claim and joins
the port worker before returning, even if the Tauri invocation is dropped. Cancellation
commands target the exact profile/request pair. A read-only supersede cancels only older
work in the same operation family and its pending tickets; independent health surfaces
share the lane without cancelling one another, and it leaves an active `workspace-start`
transition token untouched. The renderer has an explicit cancellable loading status and
generation/unmount/profile guards so late results cannot replace the current health view.

#### Process identity, rollback, and Stop What I Started

Every Workbench-owned process records a creation identity in addition to its PID (Windows
creation time; Unix `/proc` start ticks for development tests). Cleanup kills only when
the identity matches; a reused PID is retained rather than signalled. Workbench launches
use a private Unix process group, so if the verified root exits first, Stop still performs
bounded TERM/KILL escalation against the remaining group; a root that exited before its
identity could be captured is only forgotten and never used as a PID-only authority.
Windows cleanup uses bounded `taskkill /PID /T /F` with suppressed raw output and bounded
waiting. Failed termination is kept in the registry and the UI re-reads the authoritative
run before clearing local state, preventing a stale successful Stop response from hiding a
process that still needs cleanup. Retry continues to use a separate `StartedPidGuard`, so
only PIDs created by that attempt can be rolled back.

Short-lived native WSL probes use the same tree boundary: Unix commands are placed in a
private process group and Windows commands are assigned to a kill-on-close Job Object.
Timeout, cancellation, reader overflow, normal-root-exit cleanup, and a failed Job/group
assignment all have bounded cleanup paths; the assignment-failure path reports the probe
as unavailable and invokes a fixed tree-kill fallback before reaping the root.

The environment launch boundary was also reconciled with the documented contract:
`crates/launch::launch_with_environment` clears inherited host variables before adding
only the platform runtime allowlist and the validated project overlay. This prevents
unrelated host tokens, cloud credentials, and shell hooks from reaching a Workbench
launched app; the plain no-overlay launch path is unchanged. A focused launch fixture
asserts that unrelated host values are dropped.

#### Resource bound

The installed-app capability discovery path now reads a runtime catalog through a 1 MiB
bound before parsing. This keeps a malformed or unexpectedly large catalog from turning
the health/preflight path into an unbounded allocation. Existing template, probe output,
snapshot and retry bounds remain in force.

### 5. Acceptance fixtures and mocks

Rust fixtures cover template round-trip/unknown fields/duplicates/bounds/path
safety, template environment stripping, symlink rejection, retry planning and
resource provenance merge. Existing preflight fixtures cover installed/absent/
conflict and service snapshot states. React fixtures cover wizard payloads,
template update, dependency warning/conflict refresh, and retry while preserving
an already Workbench-owned WSL process.

The AppLink fixture mocks were extended for the new API calls so the existing
late-delivery tests remain isolated from the dependency-health effect.

## Code Examples

### Template application does not import environment state

```rust
// apps/workbench/src-tauri/src/core/templates.rs
if profile.expected_ports.is_empty() {
    profile.expected_ports = self.expected_ports.clone();
}
// A template is never an authority for project environment values.
profile.environment = None;
profile.validate()?;
```

### Retry planner has a fixed command surface

```rust
// apps/workbench/src-tauri/src/core/retry.rs
pub const RETRY_STEP_ORDER: &[&str] = &[
    WAIT_PORT_STEP,
    OPEN_WSL_STEP,
    OPEN_CODE_PAD_STEP,
];
```

### Retry owns only new processes until commit

```rust
// apps/workbench/src-tauri/src/commands/workspace.rs
let mut new_pids = StartedPidGuard::new(&registry, profile_id.clone(), Some(run_id.clone()));
// ... launch only the pending suffix and push returned PIDs ...
updated.started_pids.extend(new_pids.commit());
```

## Verification Results

Template dialogs have an explicit `aria-describedby`/`aria-busy` contract, initial focus,
Escape close, Tab focus containment, and trigger-focus restore. Template list responses
are generation-guarded so a late request cannot replace the contents of a newly opened
dialog. Preflight cancellation is available from the loading state, and Stop What I
Started checks OS termination/authoritative ownership before removing a run.

The original dirty-candidate phase deferred heavy commands while concurrent grouped work
was running. After the remediation review, constrained focused verification was run:

```text
cargo fmt --all -- --check                         PASS
git diff --check                                   PASS
cargo check -p workbench -p launch -j1             PASS
cargo test -p workbench -p launch -j1              PASS
  workbench: 113 passed; launch: 25 passed
cargo clippy -p workbench -p launch --all-targets -j1 -- -D warnings
                                                     PASS
pnpm --dir apps/workbench exec tsc --noEmit        PASS
```

The Rust fixtures include template round-trip/unknown-field/duplicate/bounds/path/CAS
cases, preflight outcomes and cancellation-before-spawn, retry planning, runtime catalog
oversize fallback, and process identity/failed-cleanup outcomes. Frontend fixtures include
template focus/stale handling, health refresh, retry rendering, preflight cancellation
from loading, and retaining a run after Stop reports failed ownership.

The Workbench production build passed independently. The latest full Vitest attempt was
restricted to our own process with one worker, but `/mnt/e` 9p I/O remained stalled under
concurrent workspace activity, so it was stopped safely. The earlier baseline Vitest run
passed 6 files/69 tests before the newest two cancellation/Stop fixtures. Parent must
rerun the latest Workbench Vitest when host I/O is available, then run full workspace
gates, CI, and Windows packaged acceptance. Packaged checks should exercise stopped WSL distros,
capability mismatches,
junction/reparse races, port state changes, child environment, cancellation/timeout,
process-tree termination, PID reuse, failed-stop ownership retention, and retry PID
rollback.

A source-level Windows GNU check reached the Tauri build script but could not complete on
this WSL host because `x86_64-w64-mingw32-windres` is not installed. This is an environment
toolchain gap rather than a Rust source diagnostic; Windows packaged acceptance remains
mandatory.

## Rollback and risk notes

- #359 template/profile writes are separate atomic/CAS operations. A crash
  between them can leave a new profile and an unchanged template; this is safe,
  but the PR should explain the non-transactional boundary.
- #360 has no rollback because it is observation-only. A stale renderer result
  is discarded rather than applied to the selected profile.
- #361 preserves the old run until final validation and commits only the retry's
  newly created PIDs. It deliberately does not stop external processes or start
  missing services.
- `WorkspaceRun` persisted/IPC compatibility and TypeScript command payloads passed the
  focused check/compile. The latest frontend build passed, while the full Vitest run and
  full workspace review remain parent gates.

## Next Steps

1. Rerun the latest Workbench Vitest when `/mnt/e` I/O and memory have headroom.
2. Run the full workspace gates and GitHub Actions CI.
3. Run Windows packaged W2 acceptance for path/reparse, capability, stopped
   distro, port race, child launch, cancellation/timeout, PID reuse, process-tree,
   and ownership rollback behavior.
4. Review the three independent acceptance/rollback sections in
   `docs/superpowers/plans/2026-08-28-workbench-resilience-tools.md` before
   opening the single grouped PR.
