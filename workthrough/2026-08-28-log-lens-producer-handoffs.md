# Run Manager / WSL Desktop → Log Lens producer handoffs (#366/#367)

## Overview

This work groups the Run Manager and WSL Desktop Log Lens integrations into one
user-visible handoff flow. A producer publishes a short-lived, one-time
`log-source/v1` envelope only after an explicit user action; AppLink carries
only an opaque reference; Log Lens previews a fixed read-only source and waits
for an explicit user confirmation before acknowledging and reading it.

The implementation was reviewed as a security and lifecycle boundary, not just
as a UI shortcut. The review covered command/argv separation, path and secret
privacy, schema and identity checks at both ends, atomic one-time consumption,
TOCTOU and lease expiry, producer/receiver concurrency, bounded process output,
process-tree cancellation, stale frontend responses, unmount cleanup, and
keyboard accessibility. The worktree remains intentionally dirty and
uncommitted for the parent agent; no commit, push, PR, rebase, or worktree
cleanup was performed.

## Context and decisions

- Worktree: `/mnt/e/projects/devbox-worktrees/log-lens-producer-handoffs`
- Branch: `feat/integration/log-lens-producers`
- Original candidate base: `2b537440ad5510d7d636e36ac40e7b95398d939f`
- `origin/main` advanced during the session; the candidate is left on its
  original base so the parent can integrate it deliberately.
- A producer does not pass a log path, command, raw line, environment, or
  credential through argv, clipboard, or a durable app record. Run Manager
  passes only a stable run/stream identity; WSL Desktop passes a validated
  WSL file or journal descriptor. Log Lens resolves the identity inside its
  own fixed adapter boundary.
- Large external tools and arbitrary command execution remain out of scope.
  The handoff is an offline in-product workflow for the small, frequently
  repeated operation of opening an app-owned source in Log Lens.

## Changes made

### 1. Shared contract and catalog discovery

Files:

- `apps/catalog.json`
- `crates/catalog/tests/catalog.rs`
- `apps/devbox-manager/src-tauri/src/core/catalog.rs`
- `Cargo.lock`
- `THIRD_PARTY_NOTICES.md`
- `docs/superpowers/specs/2026-08-17-app-interop-design.md`

Both producers declare the `handoff:log-source/v1` capability and use the
catalog's installed-target lookup before publishing. Catalog revision and
lockfile edges were updated for the launch/AppLink/integration dependencies;
the generated third-party notice now matches the lockfile. Catalog tests cover
the capability and target contract. The interop specification now records the
producer single-flight rule, receiver re-validation, bounded latest-request
queue, stale/unmount guards, modal focus behavior, optional journal-unit
normalization, and process-tree cancellation policy.

### 2. Run Manager producer

Files:

- `apps/run-manager/src-tauri/src/core/log_handoff.rs`
- `apps/run-manager/src-tauri/src/core/mod.rs`
- `apps/run-manager/src-tauri/src/log_lens.rs`
- `apps/run-manager/src-tauri/src/lib.rs`
- `apps/run-manager/src-tauri/Cargo.toml`
- `apps/run-manager/src/components/RunHistory.tsx`
- `apps/run-manager/src/components/RunHistory.test.tsx`
- `apps/run-manager/src/App.test.tsx`
- `apps/run-manager/src/api.ts`
- `apps/run-manager/src/test/setup.ts`
- `apps/run-manager/vite.config.ts`
- `apps/run-manager/README.md`

The strict Run payload retains only `{ kind, sourceId, runId, stream }`, with
bounded identity values and unknown-field rejection. The command confirms run
existence and app-owned log availability, discovers an installed Log Lens
target, writes a ten-minute one-time envelope, and launches with direct argv
containing only `--handoff-kind`, `--handoff-id`, and `--from`. It never places
the retained log directory, command, cwd, environment, or log bytes in the
payload, argv, error, clipboard, or frontend state.

Publish and launch are guarded by a process-local single-flight mutex. A
concurrent request returns the fixed `handoff-busy` error before creating a
second envelope. Spawn failure does not fall back to shell or clipboard; the
bounded pending envelope remains available until its TTL expires. The history
button/context action has explicit confirmation and mounted/generation guards,
and its busy state is exposed to assistive technology. The frontend test setup
also registers `jest-dom` matchers used by the existing component tests.

### 3. WSL Desktop producer

Files:

- `apps/wsl-desktop/src-tauri/src/core/log_handoff.rs`
- `apps/wsl-desktop/src-tauri/src/core/mod.rs`
- `apps/wsl-desktop/src-tauri/src/log_lens.rs`
- `apps/wsl-desktop/src-tauri/src/lib.rs`
- `apps/wsl-desktop/src-tauri/Cargo.toml`
- `apps/wsl-desktop/src/App.tsx`
- `apps/wsl-desktop/src/api.ts`
- `apps/wsl-desktop/src/components/DistroPanel.tsx`
- `apps/wsl-desktop/src/components/DistroPanel.test.tsx`
- `apps/wsl-desktop/src/App.applink.test.tsx`
- `apps/wsl-desktop/src/App.contextMenu.test.tsx`
- `apps/wsl-desktop/README.md`

The only accepted WSL descriptors are:

```json
{ "sourceType": "wslFile", "distro": "Ubuntu", "wslPath": "/var/log/app.log" }
{ "sourceType": "wslJournal", "distro": "Ubuntu", "unit": "sshd.service" }
```

Values are bounded and reject root, empty/dot/dotdot path components, control
characters, and shell/argv injection characters. Journal units use an
allowlist-shaped identifier. The producer does not execute a user command,
shell, network request, clipboard fallback, or log read while publishing.
`wslPath` exists only in the short-lived envelope and process-local adapter
configuration; it is not copied to a durable source, saved view, localStorage,
argv, or error text. Publish and launch share the same single-flight behavior
as Run Manager, including the fixed busy error and no duplicate envelope.

### 4. Log Lens receiver and one-time lifecycle

Files:

- `apps/log-lens/src-tauri/src/applink.rs`
- `apps/log-lens/src-tauri/src/handoff.rs`
- `apps/log-lens/src-tauri/src/core/handoff.rs`
- `apps/log-lens/src-tauri/src/core/mod.rs`
- `apps/log-lens/src-tauri/src/core/model.rs`
- `apps/log-lens/src-tauri/src/lib.rs`
- `apps/log-lens/src-tauri/src/core/sources.rs`
- `apps/log-lens/src-tauri/Cargo.toml`
- `apps/log-lens/src/types.ts`
- `apps/log-lens/src/api.ts`
- `apps/log-lens/src/api.test.ts`
- `apps/log-lens/src/App.tsx`
- `apps/log-lens/src/App.css`
- `apps/log-lens/README.md`

AppLink accepts only the `log-source/v1` kind and a 32-character lowercase
hexadecimal opaque id. Native cold-start and hot single-instance delivery use
the same pending-open path. Handoff claim processing re-checks protocol
version, envelope id and claim token shape, target/producer/source-family
parity, creation/expiry bounds, and lease range even if an upstream producer
already validated them. The process-local state machine keeps the claim token
out of the UI and supports `preview`, `renew`, `accept`, `discard`, restore on
cancel/failure, and one-time ack semantics.

The source summary is always read-only and bounded. Run handoffs remain an
identity boundary until a later app-owned Run adapter is implemented; no raw
Run log is duplicated into the handoff. WSL file/journal descriptors are
converted to fixed adapter configuration. Native-to-frontend API responses
are parsed with exact allowed keys and bounds before reaching the source UI.
In particular, Rust's optional journal unit may serialize as `null`; the
frontend treats `null` and an omitted unit as “no unit” while rejecting other
malformed values.

### 5. Adapter resource and cancellation hardening

File: `apps/log-lens/src-tauri/src/core/sources.rs`

WSL/container adapters use fixed argv, no shell, a ten-second deadline, bounded
stdout reader (64 MiB), cancellation, and operation/generation identity. A
single-flight registry remains bounded under repeated generations. Process
tree cleanup now uses a Windows Job Object with kill-on-close and
`TerminateJobObject`, Unix process groups, and a bounded child reap; if tree
assignment/cleanup fails, the direct child is still terminated. Reader joins
are not allowed to wait forever after a root process exits. Windows Job Object
imports are cfg-gated and the dependency enables
`Win32_System_JobObjects`.

### 6. Frontend concurrency, stale state, and accessibility

The Log Lens modal has one current preview/action and a one-item latest opaque
request queue. Preview, accept, discard, renew, and unmount paths use mounted
and generation guards; stale native responses are discarded and cannot replace
sources or overwrite current errors. A pending native claim is discarded on
unmount. The modal exposes dialog semantics, an explicit read-only add action,
Escape cancellation, Tab/Shift+Tab containment, initial focus on cancel, and
focus restoration to the opener. Producer UI actions use explicit button types
and `aria-busy`; the WSL card no longer submits an enclosing form accidentally.

## Code examples

### Opaque AppLink boundary

```text
--handoff-kind log-source/v1 --handoff-id <32 lowercase hex chars> --from run-manager
```

The path, command, environment, credential, raw log, and WSL descriptor are
not present in this argv. They are recovered only after the receiver claims the
short-lived envelope and passes the contract checks.

### Bounded receiver flow

```text
pending envelope
  -> exclusive claim + lease validation
  -> read-only source summary preview
  -> explicit user confirmation
  -> fixed SourceSpec + atomic ack
  -> bounded adapter read
```

Cancel, malformed input, lease expiry, or pre-ack failure restores the envelope
where the shared contract permits retry; ack is one-time and cannot be replayed.

## Verification results

All commands below were run from this worktree. Cargo used the existing shared
target directory `/home/jihoon/.cache/targets/devbox-app-handoffs` with at most
two build jobs; no new target directory was created.

### Formatting, Rust tests, and static checks

```text
cargo fmt --all -- --check
  passed

CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-app-handoffs \
CARGO_BUILD_JOBS=2 cargo test -p log-lens --lib -j2
  45 passed, 0 failed

CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-app-handoffs \
CARGO_BUILD_JOBS=2 cargo test -p run-manager --lib -j2
  174 passed, 0 failed

CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-app-handoffs \
CARGO_BUILD_JOBS=2 cargo test -p wsl-desktop --lib -j2
  76 passed, 0 failed

CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-app-handoffs \
CARGO_BUILD_JOBS=2 cargo test -p run-manager core::log_handoff -j2
  3 passed, 0 failed

CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-app-handoffs \
CARGO_BUILD_JOBS=2 cargo test -p wsl-desktop core::log_handoff -j2
  4 passed, 0 failed

CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-app-handoffs \
CARGO_BUILD_JOBS=2 cargo check -p log-lens -p run-manager -p wsl-desktop -j2
  passed

CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-app-handoffs \
CARGO_BUILD_JOBS=2 cargo clippy -p log-lens -p run-manager -p wsl-desktop \
  --lib -j2 -- -D warnings
  passed

CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-app-handoffs \
CARGO_BUILD_JOBS=2 cargo clippy -p log-lens -p run-manager -p wsl-desktop \
  --all-targets -j2 -- -D warnings
  passed

git diff --check
  passed
```

### Frontend checks

```text
pnpm install --offline --frozen-lockfile --child-concurrency=1 \
  --network-concurrency=1 --ignore-scripts
  passed; all 20 workspace projects, 311 packages added without download

pnpm --filter log-lens exec vitest run src/api.test.ts \
  --maxWorkers=1 --no-file-parallelism
  2 passed, 0 failed

pnpm --filter run-manager test -- --maxWorkers=1 --no-file-parallelism
  6 files, 39 tests passed, 0 failed

pnpm --filter wsl-desktop exec vitest run src/components/DistroPanel.test.tsx \
  --maxWorkers=1 --no-file-parallelism
  1 file, 6 tests passed, 0 failed

pnpm --filter log-lens build
  passed
pnpm --filter run-manager build
  passed
pnpm --filter wsl-desktop build
  passed; existing large-chunk warning remains
```

The full Log Lens frontend suite had already passed its original 3 files/11
tests before the added API test; the added API boundary test was then run
focused as shown above. The full WSL Desktop Vitest suite was intentionally
interrupted after more than eight minutes when xterm/jsdom initialization and
the already saturated swap made it unsafe to continue. It is not reported as
passed; the focused DistroPanel suite and production build passed instead.

### Catalog and dependency policy

```text
bash .github/scripts/check-catalog.sh
  passed

python3 .github/scripts/test-check-dependencies.py
  dependency policy regression tests passed

python3 .github/scripts/check-dependencies.py generate
  regenerated THIRD_PARTY_NOTICES.md from Cargo.lock/pnpm-lock.yaml

python3 .github/scripts/check-dependencies.py check
  dependency policy OK; notices match Cargo.lock and pnpm-lock.yaml
```

### Windows cross-target check

```text
CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-app-handoffs \
CARGO_BUILD_JOBS=2 cargo check -p log-lens --target x86_64-pc-windows-gnu -j2
  blocked by the environment: tauri-winres could not find
  x86_64-w64-mingw32-windres
```

The check progressed through the Windows dependency graph and failed in the
Tauri resource build step, before packaged Windows compilation could complete.
The Job Object code is cfg-gated and follows the repository's existing Windows
Job Object API pattern, but a Windows host/toolchain check is still required.

## Remaining risks and follow-up

- `SourceSpec::Run` is intentionally not a raw-log adapter yet. A future
  app-owned adapter can resolve the identity, but must preserve this payload
  and privacy boundary.
- WSL/container adapter behavior, single-instance forwarding, real file
  identity, and focus/IME behavior still need a packaged Windows W3 smoke on a
  host with the installed WSL/Docker/Podman environment and resource compiler.
- Unix process-tree termination now uses a direct `libc::kill` syscall for the
  private process group, avoiding ambient PATH lookup. A same-user attacker
  could still replace a parent directory between layout validation and a later
  file operation; eliminating that race would require an `openat`/
  directory-handle design.
- The full WSL frontend suite should be rerun serially after swap pressure is
  reduced. The focused suite and build provide useful evidence but do not
  replace that full run.
- Worktree status is intentionally dirty and no branch/worktree cleanup was
  performed. Parent integration must review/rebase, run CI, and only then
  commit/PR/merge and clean the worktree according to repository policy.

## Follow-up static/security audit (2026-08-28)

The final bounded audit found concrete edge cases in the candidate handoff
implementation. These changes are intentionally left uncommitted in the
producer worktree for the parent agent's review.

### Remediations

- `crates/applink/src/handoff.rs` rejects claims before the envelope's
  advertised creation time, validates claim timestamps against the envelope,
  removes future-dated claim records during reconciliation, and opens managed
  state files without following a final symlink/reparse point. This prevents a
  clock-skewed or forged state file from being claimable early, reserving a
  payload indefinitely, or winning the metadata/open race at the final path
  component.
- `crates/applink/src/lib.rs` exposes a 64-argument/32 KiB AppLink argv bound,
  applies it before parsing, and no longer reflects malformed numeric values in
  parse errors. `crates/launch/src/lib.rs` applies the same bound before
  forwarding generated argv to a child. Unix/Windows target dependencies were
  added only for the no-follow handle boundary.
- `crates/wsl/src/distro.rs` rejects names beginning with `-`, preventing a
  validated distro value from being interpreted as a `wsl.exe` option.
- `apps/log-lens/src-tauri/src/core/sources.rs` kills Unix adapter process
  groups through `libc` instead of ambient PATH lookup and bounds reader thread
  joins after process-tree termination. `read_sources` rejects more than
  `MAX_SOURCES` before walking cursor/source data.
- `apps/log-lens/src/App.tsx` keeps one latest pending refresh request, cancels
  superseded native reads, drains the latest request after the current flight,
  clears pending work on unmount/empty-source transitions, and clears stale
  busy state. Malformed native previews restore their claim; lease failures
  also attempt best-effort discard.
- `apps/log-lens/src/api.ts` binds a preview response to the requested opaque
  id, measures text limits in UTF-8 bytes, and rejects root/option-like WSL
  values at the WebView boundary. `apps/log-lens/src/api.test.ts` covers the
  response identity and unsafe-value regressions.

### Verification

```text
cargo fmt --all -- --check                                  passed
git diff --check                                            passed
CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-app-handoffs \
  CARGO_BUILD_JOBS=1 cargo test -p applink --lib -j1        63 passed
CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-app-handoffs \
  CARGO_BUILD_JOBS=1 cargo test -p wsl --lib -j1            31 passed
CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-app-handoffs \
  CARGO_BUILD_JOBS=1 cargo test -p launch --lib -j1         23 passed
CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-app-handoffs \
  CARGO_BUILD_JOBS=1 cargo test -p log-lens --lib -j1       45 passed
pnpm --filter log-lens test -- --run                         13 passed
pnpm --filter log-lens build                                 passed
CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-app-handoffs \
  CARGO_BUILD_JOBS=1 cargo check -p log-lens -p launch -p applink -p wsl -j1 passed
CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-app-handoffs \
  CARGO_BUILD_JOBS=1 cargo check -p applink --target x86_64-pc-windows-gnu -j1 passed
CARGO_TARGET_DIR=/home/jihoon/.cache/targets/devbox-app-handoffs \
  CARGO_BUILD_JOBS=1 cargo clippy -p applink -p launch -p wsl -p log-lens \
    --lib -j1 -- -D warnings                                  passed
```

### Residual risks

- The shared handoff directory remains a same-user coordination surface;
  parent-directory replacement between layout validation and open/rename would
  require an `openat`/directory-handle design to eliminate completely.
- Claim/lease sidecars are file-based and cross-process renew/ack operations
  can still observe a concurrent state transition; token checks prevent
  resurrection, but callers must handle fixed failure responses and retry.
- Run handoffs still carry identity only; Log Lens reports that adapter as
  unavailable until a separate app-owned Run log adapter is implemented.
- Full Windows Tauri packaging/build and an end-to-end two-process handoff
  were not run in this WSL audit; Windows-specific AppLink handle code was
  compile-checked for the GNU target.
