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
  restarting successful steps or an actually live Workbench-owned process.

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
`profile-templates.json` with the same no-follow identity checks and atomic
replacement policy as the profile store. Template reads now return an opaque
SHA-256 revision with the list; renderer update/delete requests send that
revision, and the backend checks it while holding the profile store lock before
writing. Applying a template fills only empty wizard fields and clears any
incoming environment metadata before the concrete profile is validated. Choosing
the wizard's direct-entry option preserves fields already entered by the user.

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
failure and skips successful steps. Historical `Existing` or `WorkbenchStarted`
provenance is not sufficient to skip a process: the command layer rechecks the
stored opaque `OwnedProcess` receipt and only an authoritative `Running`
observation is skipped; exited owned processes and provenance without a receipt
are retried.
`WorkspaceRun` and the safe ownership DTO
now carry retry count/availability/failure metadata without serializing PIDs.

The command reruns preflight and profile/environment validation before each
child. A separate `StartedProcessGuard` owns only receipts created by that retry
until the updated run is atomically installed. Profile changes, timeout,
cancellation, provider failures, or publish conflicts roll back those new receipts;
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

#### Owned receipts, rollback, and Stop What I Started

Every Workbench-owned process now retains a cloneable opaque `OwnedProcess` receipt from
the launch boundary. Cleanup never accepts a later renderer-supplied PID or performs a
creation-time lookup. Windows receipts retain an exact kill-on-close Job Object, query
active-process accounting, and wait for the full Job to become empty even after the root
exits. Unix receipts retain the launch-time private process group and perform bounded
TERM/KILL escalation with full-group disappearance checks.
Terminal emptiness is sticky, so a later Stop or clone cannot signal an already-empty
authority. A weak background reaper waits the root `Child` without keeping a dropped
receipt alive forever and immediately cleans remaining group/Job members after root exit;
the receipt's inner Drop remains the crash/rollback backstop.
Failed termination is kept in the registry and the UI re-reads the authoritative run
before clearing local state, preventing a stale successful Stop response from hiding a
process that still needs cleanup. Retry continues to use a separate `StartedProcessGuard`,
so only receipts created by that attempt can be rolled back.

On Unix and macOS, a descendant that deliberately calls `setsid()` can escape the private
group. A process group is also a numeric identity rather than a Windows-style retained
kernel handle, so an empty group could theoretically be reused before terminal emptiness
is observed. The immediate root-exit cleanup narrows that interval but does not justify an
exact PID-reuse claim. macOS has no Linux `/proc` identity fallback.

Short-lived native WSL probes use the same tree boundary: Unix commands are placed in a
private process group and Windows commands use CREATE_SUSPENDED, Job configuration/
assignment, sole-primary-thread resume, and a kill-on-close Job Object. Timeout,
cancellation, reader overflow, normal-root-exit cleanup, and a failed Job/group
assignment/resume all have bounded cleanup paths; the assignment-failure path reports the
probe as unavailable and directly kills/reaps the still-suspended root without starting a
helper process.

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
safety, opaque template revisions, template environment stripping, symlink
rejection, retry planning/liveness reconciliation and resource provenance merge.
Existing preflight fixtures cover installed/absent/conflict and service snapshot
states. React fixtures cover wizard payloads, template revision update/delete
requests, direct-entry draft preservation, dependency warning/conflict refresh,
and retry while preserving an actually live Workbench-owned WSL process.

The AppLink fixture mocks were extended for the new API calls so the existing
late-delivery tests remain isolated from the dependency-health effect.

### Follow-up resilience corrections

The template IPC now carries a bounded opaque revision snapshot. Update/delete
commands receive the renderer's expected revision and compare it again while
holding the existing writer lock before committing the atomic replacement. The
retry planner now accepts explicit process-liveness evidence; only a currently
matching owned receipt is skipped, while exited or provenance-only process entries
remain retryable and stale exited Workbench-owned process resources are
reconciled. Liveness is sampled again after profile/preflight work and at each
child boundary, so a process exiting while port-wait runs cannot remain hidden
behind an old plan sample. Unix launch environment forwarding now preserves
`XDG_RUNTIME_DIR` for Wayland sessions.
The wizard's `직접 입력` selection is treated as a mode switch and preserves the
current draft instead of resetting it.

These follow-up changes were implemented without running build or test commands;
the parent agent must run the normal Rust/frontend and Windows packaged gates.

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
let mut new_processes = StartedProcessGuard::new(&registry, profile_id.clone(), Some(run_id.clone()));
// ... launch only the pending suffix and push returned opaque receipts ...
updated.started_processes.extend(new_processes.commit());
```

## Verification Results

Template dialogs have an explicit `aria-describedby`/`aria-busy` contract, initial focus,
Escape close, Tab focus containment, and trigger-focus restore. Template list responses
are generation-guarded so a late request cannot replace the contents of a newly opened
dialog. Preflight cancellation is available from the loading state, and Stop What I
Started checks OS termination/authoritative ownership before removing a run.

The original dirty-candidate phase deferred heavy commands while concurrent grouped work
was running. That historical source-only phase is superseded by the mechanical validation
below; no packaged Windows run or whole-workspace frontend/Rust gate was attempted.

### Parent mechanical validation (2026-08-28)

All commands below ran in `/mnt/e/projects/devbox-worktrees/workbench-resilience-tools`.
Cargo used `CARGO_TARGET_DIR=/home/jihoon/.cache/targets/workbench-359-361` and `-j2`;
the cache was not removed.

```text
cargo fmt --all -- --check                         PASS
git diff --check                                   PASS
CARGO_TARGET_DIR=... cargo test -p launch -p workbench -j2       PASS
  launch: 28 passed; workbench library: 115 passed; main/doc tests: 0 failures
CARGO_TARGET_DIR=... cargo check -p launch -p workbench -j2      PASS
CARGO_TARGET_DIR=... cargo clippy -p launch -p workbench \
  --all-targets -j2 -- -D warnings                         PASS
CARGO_TARGET_DIR=... cargo check -p launch \
  --target x86_64-pc-windows-gnu -j2                         PASS
CARGO_TARGET_DIR=... cargo clippy -p launch \
  --target x86_64-pc-windows-gnu --all-targets -j2 \
  -- -D warnings                                             PASS
pnpm install --frozen-lockfile                               PASS
pnpm --dir apps/workbench test                               PASS (6 files, 72 tests)
pnpm --dir apps/workbench build                              PASS
pnpm audit --audit-level moderate                             PASS (no known vulnerabilities)
python3 .github/scripts/check-dependencies.py check          PASS
python3 .github/scripts/test-check-dependencies.py            PASS
python3 .github/scripts/test-build-manifest.py                PASS
bash .github/scripts/check-catalog.sh                         PASS
cargo deny --locked check                                     PASS
```

The dependency check initially detected a stale Cargo.lock digest in
`THIRD_PARTY_NOTICES.md`; the generated notice changed only that one digest line, after
which the policy check passed. `cargo deny` still prints the repository's existing
duplicate-crate diagnostics while exiting successfully. The launch-only Windows GNU
check/clippy validated the new conditional Windows code, but it did not link or run an
executable. A full Workbench/Tauri Windows build remains a Windows CI/packaged gate; this
WSL host has no `x86_64-w64-mingw32-windres` and cannot provide that evidence.

The final parent review found that the earlier prose overstated Unix process-group
identity as PID-reuse-proof after the root exited. The contract now distinguishes the
exact retained Windows Job handle from Unix's numeric process-group residual. The root
reaper also immediately invokes the same bounded authority cleanup after the root exits,
instead of leaving helpers alive until a later UI Stop. A new Unix regression fixture
launches `sleep 30 &`, waits for the shell root to exit, and proves the descendant group is
already empty before a later zero-timeout Stop. After that correction, the parent reran
the launch tests (28 passed), strict launch clippy, and launch Windows GNU check/clippy.

The Rust fixtures include template round-trip/unknown-field/duplicate/bounds/path/CAS
cases, preflight outcomes and cancellation-before-spawn, retry planning, runtime catalog
oversize fallback, owned-receipt terminal-state/failed-cleanup outcomes, and bounded
process-tree cleanup. Frontend fixtures include
template focus/stale handling, health refresh, retry rendering, preflight cancellation
from loading, and retaining a run after Stop reports failed ownership.

An earlier dirty-candidate Workbench production build and the old 6-file/69-test baseline
are retained as historical context; the current focused frontend run above passed 6
files/72 tests. Parent must run the full workspace gates, CI, and Windows packaged
acceptance.
Packaged checks should exercise stopped WSL distros,
capability mismatches,
junction/reparse races, port state changes, child environment, cancellation/timeout,
process-tree termination with a suspended-to-assigned-to-resumed child, Windows PID-reuse
attack scenarios, Unix group-ID residual behavior, failed-stop ownership retention, and
retry receipt rollback.

## Rollback and risk notes

- #359 template/profile writes are separate atomic/CAS operations. A crash
  between them can leave a new profile and an unchanged template; this is safe,
  but the PR should explain the non-transactional boundary.
- #360 has no rollback because it is observation-only. A stale renderer result
  is discarded rather than applied to the selected profile.
- #361 preserves the old run until final validation and commits only the retry's
  newly created opaque process receipts. It deliberately does not stop external
  processes or start missing services.
- `WorkspaceRun` keeps process receipts in backend-only state; only the stable ownership
  summary crosses IPC. Full-workspace verification and Windows packaged acceptance remain
  parent gates after this focused validation.

## Next Steps

1. Run the focused and latest Workbench Vitest when `/mnt/e` I/O and memory have headroom.
2. Run the full workspace gates and GitHub Actions CI.
3. Run Windows packaged W2 acceptance for path/reparse, capability, stopped
   distro, port race, child launch, cancellation/timeout, PID reuse, process-tree,
   and ownership rollback behavior.
4. Review the three independent acceptance/rollback sections in
   `docs/superpowers/plans/2026-08-28-workbench-resilience-tools.md` before
   opening the single grouped PR.
