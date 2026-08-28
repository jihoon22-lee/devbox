# Workbench preflight path-kind and link-boundary correction

## Overview

The relocation-ready audit of the old `workbench-workspace-preflight` worktree found two safety
hunks that were not present in the reviewed `v0.5.0-rc2` source. The current preflight accepted any
safe absolute path shape for both Windows and WSL probes, and it returned `Missing` for a missing
target before inspecting existing parent components for a symlink or Windows reparse point.

This correction keeps the current Workbench operation-budget, cancellation, process-tree, and
bounded-probe implementation intact. It ports only the missing fail-closed path boundary and adds
focused regressions. The obsolete worktree file was not restored wholesale because it lacks later
process ownership, deadline, and cancellation hardening.

## Changes

- Windows working-directory probes now accept only Windows drive or UNC paths. POSIX paths are
  rejected as `Unsafe` before filesystem I/O.
- WSL working-directory probes now accept only POSIX paths. Windows drive/UNC paths are rejected
  before `wsl.exe` can be spawned.
- Existing path components are inspected before final-target metadata. A missing descendant below
  an existing symlink/reparse component therefore remains `Unsafe` instead of being downgraded to
  an ordinary missing directory.
- The Workbench README records the path-kind and existing-component contract.

The change remains read-only. It does not create a directory, start a stopped WSL distro, follow a
link target, echo the submitted path, or alter the existing fixed-error IPC contract.

## Regression coverage

Focused tests cover:

- POSIX input at the Windows probe boundary;
- Windows input at the WSL probe boundary without child-process execution;
- direct symlink-component detection; and
- a missing descendant below a symlink.

Local verification from the dedicated worktree used the shared Linux-native target cache with
incremental compilation disabled and at most two Cargo jobs:

```text
cargo fmt --all -- --check                         PASS
git diff --check                                   PASS
cargo test -p workbench preflight -j2              PASS (16 passed; 0 failed)
cargo test -p workbench --lib -j2                  PASS (117 passed; 0 failed)
cargo clippy -p workbench --all-targets -- -D warnings
                                                    PASS
cargo test --workspace -j2                         PASS (all unit, integration, and doc tests)
cargo check --workspace -j2                        PASS
pnpm install --frozen-lockfile                     PASS (311 reused; 0 downloaded)
pnpm build                                          PASS (19 workspace projects)
pnpm --dir apps/workbench test                     PASS (6 files; 72 tests)
bash .github/scripts/check-catalog.sh               PASS
python3 .github/scripts/check-dependencies.py check PASS
```

The catalog gate also ran the packaged-smoke configuration check and release-verifier positive and
tamper fixtures. GitHub Actions CI and Windows packaged acceptance remain PR/release gates and are
not pre-declared as successful here.

## Release impact

RC2 remains immutable and publicly preserved, but this source-contract correction prevents its
promotion to stable. After the fix PR passes required CI and merges, a new annotated RC tag and new
32-asset release must be built and independently verified. Windows W1 through W4 then restart from
that exact package; no RC2 runtime result may be carried forward as RC3 evidence.

The old dirty worktree remains untouched until the final cleanup audit. Once this correction and
the stronger merged Workbench implementation are proven present in `main`, its remaining
intermediate source can be classified as superseded rather than silently deleted.
