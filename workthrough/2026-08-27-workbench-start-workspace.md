# Workbench Start Workspace grouped implementation (#312 + #313)

## Overview

This worktree combines the two P2-14 Workbench slices into one reviewable user
flow: project environment preparation (#312) and Start Workspace preflight
review (#313). The user sees one `Start Workspace` action, a read-only review,
and an explicit Continue action. The two issue contracts remain independent:
each keeps its own fixtures, error semantics, acceptance checklist, and
rollback responsibility.

The implementation was assembled in a new worktree from the latest
`origin/main` (`7a03b9b`, including the already merged Knowledge Base #443
changes). The source worktrees were inspected but not modified:

- `workbench-project-environment` supplied the committed #312 implementation,
  its uncommitted operation/cancellation hardening, and the environment tests.
- `workbench-workspace-preflight` supplied the uncommitted #313 core/command
  implementation, UI review contract, and preflight fixtures.
- `workbench-start-workspace` is the only worktree changed by this task.

No commit, push, PR, source-worktree deletion, or source-branch rewrite was
performed.

## Grouping decision and boundaries

The issues are grouped because they are two safety reviews of the same Start
Workspace transition and share the same profile reload, project identity,
single-flight, cancellation, child ownership, and rollback boundaries. They
are not merged into one undifferentiated contract.

### #312 acceptance retained

- The profile editor selects exactly one project-relative `.env` or
  `.env.<safe-name>` source. Absolute paths, traversal, separators, aliases,
  symlinks, reparse points, and canonical-root escape are rejected.
- Native parsing is offline and bounded: UTF-8 file 256 KiB, line 8 KiB, 128
  variables, key 128 bytes, value 64 KiB, and aggregate values 128 KiB.
  Expansion, command substitution, multiline shell syntax, malformed quotes,
  duplicate names, and reserved names do not become executable environment
  state.
- Profile and IPC DTOs contain only enabled/source/revision and variable
  metadata. Secret values and ciphertext are absent; sensitive names use the
  shared `secret-ref/v1` metadata reference. Preview values are masked.
- Disabled configuration is a no-read no-op. An empty source is a successful
  empty overlay. Changed/missing/malformed/oversized source and unavailable
  secret provider fail closed.
- Start re-reads and compares source revision and metadata at each child
  boundary. Values live only in the zeroizing, process-local injection holder
  and are passed through `crates/launch`'s ephemeral environment API.
- Existing operation controls cancel the blocking reader/native child, wait
  for worker cleanup, and prevent a stale invocation from overlapping a newer
  preview/health/start operation.

### #313 acceptance retained

- Native preflight checks required installed capabilities (`wsl-desktop:path`
  and `code-pad:workspace`), selected WSL distro existence/running state,
  Windows/WSL working directories, expected TCP ports, and configured Run
  Manager service dependencies.
- Probe commands use fixed argv, null stdin, discarded stderr, bounded output,
  and fixed timeout. A stopped WSL distro is not started merely to inspect its
  directory.
- `pass`, `warning`, `failure`, and `unavailable` states are stable DTO values.
  Resource provenance distinguishes available, existing, not-running, missing,
  conflict, unsafe, unavailable, and Workbench-started resources without
  returning executable paths, PID, raw service data, or stderr.
- The frontend opens the review only after an explicit Start action. Warnings
  are reviewable and require explicit Continue; failures/unavailable results
  disable Continue. Escape/Cancel, profile navigation, unmount, late results,
  and double submit have generation/busy guards.
- The backend repeats preflight immediately before launch. A failed repeat
  never reads `.env` and never opens either child. A child started by this
  transition is rolled back only when it belongs to the transition.
- Automatic service creation/start, destructive recovery, and forced cleanup
  remain out of scope.

## Implementation details

### Native modules and shared contracts

- `core/preflight.rs` owns pure status/provenance assessment and serialization.
  It has independent fixtures for required-app, directory, port, service
  snapshot, warning/readiness, and redaction behavior.
- `commands/preflight.rs` owns bounded read-only OS observations. It does not
  mutate WSL, services, ports, profiles, or project files.
- `core/health.rs` now exposes WSL running-state observation separately from
  distro presence so a stopped distro can be represented as unavailable rather
  than started as a probe side effect.
- `commands/workspace.rs` exposes only the bounded service-ID decoder needed by
  preflight and includes `PreflightStatus` on run steps plus stable
  `resource_provenance` on a committed run.
- The existing #312 operation module remains registered alongside preflight;
  module registration and Tauri command registration include both slices.

### Start transition ordering

The resulting backend order is:

```text
claim start/profile + operation budget
  → cancel/wait competing health operation
  → load and validate profile
  → repeat read-only #313 preflight
  → bounded expected-port wait
  → revalidate profile/root/source and resolve #312 environment
  → revalidate again immediately before WSL Desktop spawn
  → spawn with ephemeral overlay and record Workbench-owned PID
  → revalidate source/profile again for Code Pad
  → spawn with a newly resolved overlay
  → drop sensitive holder, revalidate, then atomically publish run ownership
```

All transition-integrity failure paths use the existing `StartedPidGuard`; the
run is published only after final checks. A profile or source mutation cannot
silently turn a preview into authority for a later child. An ordinary child
launch failure is instead recorded as a fixed partial-run step so the user can
inspect the result and stop only the process Workbench successfully started.

### Frontend flow

`App.tsx` now keeps preflight state separate from environment editor state:

- `onStart` requests the native/browser fixture and stores only the current
  generation/profile result.
- The modal renders fixed item/status/resource labels and requires Continue.
- `onContinueStart` invokes the existing native start lifecycle. The target
  profile remains selected while the backend transition is pending.
- Selection change, refresh, app-link delivery, editor transitions, Escape,
  unmount, and late completion invalidate the preflight generation. A stale
  result cannot open a modal or launch a child.
- The existing start cancellation button remains available after Continue and
  sends the exact profile cancellation to the native operation.
- Run steps show status labels and resource provenance; restore DTOs still do
  not expose steps, paths, or PIDs.

## Files changed

### Code

- `apps/workbench/src-tauri/src/core/preflight.rs`
- `apps/workbench/src-tauri/src/commands/preflight.rs`
- `apps/workbench/src-tauri/src/core/health.rs`
- `apps/workbench/src-tauri/src/commands/workspace.rs`
- `apps/workbench/src-tauri/src/commands/mod.rs`
- `apps/workbench/src-tauri/src/core/mod.rs`
- `apps/workbench/src-tauri/src/lib.rs`
- `apps/workbench/src-tauri/Cargo.toml`
- `apps/workbench/src/api.ts`
- `apps/workbench/src/App.tsx`
- `apps/workbench/src/App.css`
- `apps/workbench/src/App.test.tsx`

The #312 environment parser, metadata contract, operation control, secret
reference, platform boundary, and launch overlay files remain in this same
grouped worktree. The `tokio` `io-util` feature is added for bounded WSL output
reading.

### Documentation

- `apps/workbench/README.md` documents the combined Start review and independent
  #312/#313 scope, including no service lifecycle and no `.env` write/upload.
- `docs/architecture.md` records the observation → Continue → revalidation
  sequence and PID-only transition rollback.
- `docs/roadmap.md` records the grouped PR rationale and separate acceptance.
- `docs/superpowers/specs/2026-08-14-workbench-design.md` updates the flow/table,
  adds the grouped contract, fixtures, and Windows W2 gate.
- `docs/superpowers/specs/2026-08-22-v0.5.0-native-first-plan.md` expands P2-14
  and marks the grouped preparation status.
- `CONVENTIONS.md` records the grouped review boundary without collapsing the
  independent security/rollback contracts.

## Verification plan and status

The requested verification is intentionally low-load. Use a Linux-native,
task-specific `CARGO_TARGET_DIR`, `CARGO_INCREMENTAL=0`, and at most two Cargo
workers. Frontend tests/build use at most two workers. Do not run a full
workspace release build from this worktree while the parent final gate is
active.

Focused Rust checks:

```text
CARGO_TARGET_DIR=<native-workbench-start-workspace> CARGO_INCREMENTAL=0 \
  CARGO_BUILD_JOBS=2 cargo test -p workbench --lib
CARGO_TARGET_DIR=<native-workbench-start-workspace> CARGO_INCREMENTAL=0 \
  CARGO_BUILD_JOBS=2 cargo check -p workbench
```

The focused suite covers both `core/environment` and `core/preflight` plus
workspace ownership/rollback fixtures. Run `cargo fmt --check` over the changed
Rust files and `git diff --check` after any formatting correction.

Focused frontend checks:

```text
pnpm --filter workbench exec vitest run src/App.test.tsx \
  src/App.applink.test.tsx src/lib/profileEditor.test.ts \
  src/lib/applink.test.ts --maxWorkers=2
pnpm --filter workbench build
```

The App fixture set includes ready/warning provenance rendering, blocking
required-resource failure, late selected-profile response, Escape cancel,
backend stale rejection redaction, Continue target lock, unmount late result,
and native start cancellation. Existing #312 masked-preview and metadata-only
save fixtures remain in the same file.

### Parent review hardening and actual results

The parent review rebased the grouped worktree onto the #444 handoff merge and
closed five gaps before PR preparation:

- backend-owned child PIDs now use `serde(skip_serializing)` and are absent
  from both the start response type and restored ownership DTO;
- all configured TCP probes share one two-second monotonic deadline in a
  blocking worker, while WSL probe children use `kill_on_drop` in addition to
  explicit timeout kill/reap;
- `.env..name` and uppercase/non-canonical revisions are rejected identically
  by native and frontend validators;
- the preflight dialog traps forward/reverse Tab focus and the launch comments
  accurately state that unrelated host variables retain normal process
  inheritance while only the reviewed project overlay is added;
- required app discovery preserves the exact `(app, capability)` pair, so an
  installed app with the wrong handoff capability fails closed instead of
  passing through an app-ID-only union.

```text
cargo test -j2 -p devbox-launch                           23 passed; 0 failed
cargo test -j2 -p devbox-secrets                           5 passed; 0 failed
cargo test -j2 -p workbench --lib                         90 passed; 0 failed
cargo check -j2 -p workbench --all-targets                passed
cargo clippy -j2 -p workbench --all-targets -- -D warnings passed
pnpm --filter workbench test -- --maxWorkers=2             59 passed; 0 failed
pnpm --filter workbench build                             passed
dependency policy/catalog/manifest checks                passed
cargo fmt --all -- --check                                passed
git diff --check                                          passed
```

## Remaining gates and risks

- The packaged Windows W2 gate remains required: real installed capability
  discovery, WSL stopped/missing behavior, junction/reparse races, occupied
  port changes, changed/missing `.env`, DPAPI/secret provider behavior, child
  environment visibility, and StartedPid rollback.
- `workspace_preflight` itself is read-only and has no separate UI cancel IPC;
  generation invalidation prevents stale rendering, WSL children are killed
  on timeout/drop, and the complete port set shares one deadline. A dedicated
  preflight cancellation slot remains a possible follow-up rather than an
  unbounded worker in this PR.
- The current `project_health` display remains a legacy observation surface;
  parent review confirmed that its existing details are limited to fixed text,
  counts and configured port numbers. It does not expose project paths, distro
  names, service IDs, child output or environment values, so normalization is
  not required by this grouped PR.
- Source worktrees and branches are intentionally left in place for parent
  comparison. This task does not rebase or delete them.
