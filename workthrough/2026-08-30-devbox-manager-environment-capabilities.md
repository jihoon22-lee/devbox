# Workthrough: Devbox Manager environment capabilities and Dev Setup audit

**Date:** 2026-08-30
**Issue:** #483
**Branch:** `feat/devbox-manager/environment-capabilities`

## Overview

Devbox Manager now reconciles Docker Desktop evidence across four independent
signals: Desktop installation/launchability, the official Windows Docker CLI,
and the `docker-desktop` WSL registration/runtime state. The new Dev Setup tab
turns those signals plus WinGet availability into a bounded schema-v1,
read-only inventory and review plan.

This work covers the first PR boundary in #483. It does not import/export
WinGet Configuration, apply packages, start a distro, edit PATH or the
registry, reboot Windows, or run arbitrary `RunCommandOnSet` actions. Those
mutation and elevated-permission boundaries remain a follow-up PR.

## Context

The existing Related Tools detector treated a missing Desktop executable as an
ordinary `not-found` result. That could incorrectly imply that Docker Desktop
was uninstalled even when its WSL backend was registered and running. It also
did not expose the distinction between an executable being present, a tool
being launchable, a CLI being product-verified, and a WSL backend being
registered versus running.

The implementation keeps native probes in the command layer and keeps the
state/evidence/plan model pure and testable in `core/environment_capabilities`.
Only coarse, fixed evidence codes cross the Tauri and frontend boundaries; raw
paths, process output, environment values, and WSL output do not.

## Changes Made

### 1. Docker capability reconciliation

- `apps/devbox-manager/src-tauri/src/core/environment_capabilities.rs`
  - Added `present`, `absent`, and `unknown` installation states and explicit
    availability/backend states.
  - Added the four fixed evidence sources:
    `desktop-executable`, `windows-cli`, `wsl-registration`, and `wsl-runtime`.
  - Added the pure reconciliation model and read-only Dev Setup review-plan
    mapping.
  - Ensured backend-only evidence never becomes a false Desktop uninstall
    result or an automatic install recommendation.
- `apps/devbox-manager/src-tauri/src/commands/related_tools.rs`
  - Added Docker Desktop `%LOCALAPPDATA%` layout candidates while retaining
    the existing regular-file/reparse-point checks.
  - Product-verified the Windows CLI with a bounded `docker.exe --version`
    probe; compatible shims remain `unknown`.
  - Probed the OS-owned `wsl.exe` only with bounded quiet list commands,
    validating at most 128 distro names of at most 128 characters each. No
    distro is started.
  - Added the `dev_setup_audit` command and opaque DTOs with schema version,
    fixed evidence, and a read-only mode.
  - Reused the existing bounded process helper from `doctor.rs`.
- `apps/devbox-manager/src-tauri/src/commands/doctor.rs`
  - Exposed the existing bounded command runner within the crate so capability
    probes share its timeout/output/process-cleanup boundary.
- `apps/devbox-manager/src-tauri/src/core/mod.rs` and
  `apps/devbox-manager/src-tauri/src/lib.rs`
  - Registered the new core module and Tauri command.

### 2. Frontend contract and UI

- `apps/devbox-manager/src/types.ts`
  - Added Docker capability, evidence, and Dev Setup inventory/plan types.
- `apps/devbox-manager/src/api.ts`
  - Added deterministic browser fixtures and `devSetupAudit()`.
  - Validated fixed catalog metadata, state/evidence relationships, safe
    timestamps, plan actions, and Docker backend correlations before React
    renders native data.
  - Preserved fixed safe action messages and rejected arbitrary IDs, URLs, raw
    native errors, and unrecognized plan actions.
- `apps/devbox-manager/src/App.tsx` and `apps/devbox-manager/src/App.css`
  - Added the Dev Setup tab with capability cards, scope, fixed evidence labels,
    and review actions.
  - Added Docker CLI/backend facts and evidence chips to Related Tools.
  - Kept unknown installation state from showing a WinGet install control.
  - Communicated the read-only/no-PATH/no-registry/no-distro-start boundary.

### 3. Regression coverage and repository evidence

- `apps/devbox-manager/src-tauri/src/core/environment_capabilities.rs` and
  `apps/devbox-manager/src-tauri/src/commands/related_tools.rs`
  cover backend-only reconciliation, explicit absence, incompatible shims,
  bounded WSL parsing, Docker version identity, and opaque audit output.
- `apps/devbox-manager/src/api.test.ts` covers API tamper rejection,
  backend-only acceptance, contradictory evidence rejection, and exact
  read-only plan validation.
- `apps/devbox-manager/src/App.test.tsx` covers Docker backend presentation,
  suppression of unsafe installation suggestions, Dev Setup rendering, and
  native-error redaction.
- `apps/devbox-manager/README.md` records the user-visible capability and
  security boundary, including the deferred guarded-apply scope.

## Code Examples

### Backend-only evidence remains unknown for Desktop installation

```rust
let capability = model_docker_capability(
    DetectionSource::NotFound,
    DockerCliProbe::NotFound,
    WslBackendProbe::Running,
);

assert_eq!(capability.desktop_install, InstallState::Unknown);
assert_eq!(capability.wsl_backend, BackendState::Running);
```

### Dev Setup is explicitly non-mutating

```rust
/// This command never installs a package, starts a distro, edits PATH/the
/// registry, or returns a resolved executable path.
#[tauri::command]
pub async fn dev_setup_audit() -> Result<DevSetupAuditView, String> {
    // bounded, single-flight read-only audit
}
```

## Verification Results

### Finalization-run checks

```text
git diff --check                                      PASS
pnpm test (apps/devbox-manager)                       47 passed (47)
pnpm build (apps/devbox-manager)                      built successfully
cargo fmt --all -- --check                            PASS
CARGO_TARGET_DIR=... cargo test -p devbox-manager    130 passed; 0 failed
bash .github/scripts/check-catalog.sh                PASS
python3 .github/scripts/check-dependencies.py check   PASS
```

The catalog/dependency checks also confirmed the 15-app packaged contract and
that generated third-party notices match both lockfiles. The frontend test
runner emitted two existing jsdom navigation notices while all tests still
passed; no test failed.

### Previously isolated evidence supplied with the worktree

- `cargo check --all-targets`: PASS
- `cargo clippy --all-targets -- -D warnings`: PASS
- Rust test suite and formatting in the dedicated Linux-native target:
  PASS (130/130)

### Runtime boundary

Windows packaged/runtime acceptance is not claimed by this WSL run. Native
Windows verification should exercise real Docker Desktop layouts, official CLI
identity, registered/stopped/running WSL states, and the read-only UI after CI
passes.

## Files Modified

- `apps/devbox-manager/README.md`
- `apps/devbox-manager/src-tauri/src/commands/doctor.rs`
- `apps/devbox-manager/src-tauri/src/commands/related_tools.rs`
- `apps/devbox-manager/src-tauri/src/core/environment_capabilities.rs`
- `apps/devbox-manager/src-tauri/src/core/mod.rs`
- `apps/devbox-manager/src-tauri/src/lib.rs`
- `apps/devbox-manager/src/App.css`
- `apps/devbox-manager/src/App.test.tsx`
- `apps/devbox-manager/src/api.test.ts`
- `apps/devbox-manager/src/api.ts`
- `apps/devbox-manager/src/types.ts`
- `workthrough/2026-08-30-devbox-manager-environment-capabilities.md`
