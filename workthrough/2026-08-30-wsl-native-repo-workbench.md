# Workthrough: WSL-native Repo Manager and Workbench Git

**Date:** 2026-08-30  
**Branch:** `feat/repo-manager/wsl-project-targets`  
**Issue:** [#482](https://github.com/jihoon22-lee/devbox/issues/482)

## Outcome

Repo Manager now keeps WSL repositories inside their distro execution
namespace across every Git flow instead of passing WSL UNC paths to Git for
Windows. Workbench restores the distro and POSIX path from Life Log WSL UNC
entries and runs health Git only when that distro is already running.

This closes two confirmed defects found in the compatibility sweep. The other
path consumers were classified separately as supported boundaries or physical
acceptance gaps; no speculative filesystem/runtime rewrite was added to them.

## Implementation

### Shared Git target mapping

`crates/git/src/lib.rs` adds a validated explicit `GitTarget::wsl` constructor
and bidirectional namespace conversion:

```text
Git output:  /home/jihoon/Projects/DevBox/.git
Host path:   \\wsl$\Ubuntu\home\jihoon\Projects\DevBox\.git

Host target: E:\Projects\Linked
WSL argv:    /mnt/e/Projects/Linked
```

Relative metadata is resolved against the reviewed cwd. Same-distro WSL UNC
aliases and drive paths are accepted for reverse conversion; cross-distro and
ordinary UNC paths are rejected. Bounds, controls, traversal, path case, and
Windows-representability are checked before any conversion is used.

### Repo Manager

All read and mutation wrappers now create a `GitTarget` and call the shared
target-aware bounded/cancellable runner. Git common-directory, operation
marker, and worktree output is mapped back to a host path before filesystem
identity or canonicalization. Worktree create/remove performs the inverse
mapping at the final Git argv boundary.

Windows canonicalization can return `\\?\\` paths and otherwise erase the WSL
transport identity before target selection. Repo Manager therefore restores
only trusted canonical drive and UNC spellings after the inbound path has
already passed syntax and filesystem validation. It continues to reject all
user-supplied device namespaces and internal Volume GUID/device forms.

### Workbench

The Git status worker now accepts `GitTarget` rather than an unstructured cwd.
For a structured WSL profile, a Windows drive Git root is converted to POSIX
and paired with the profile distro. A WSL UNC Git root must name the same
distro. Life Log `projects/v1` WSL UNC entries populate both `windowsPath` and
`WslProfile { distro, path }`.

Project health observes `wsl.exe -l -v` before distro Git. Running proceeds;
Stopped, Missing, and unavailable states add a stable health failure and skip
Git entirely. An unreadable Git target or failed Git status is shown as an
explicit unavailable state rather than the misleading “0 changes” message.

## Compatibility sweep

- Repo Manager and Workbench: confirmed native-runner bypasses fixed here.
- Run Manager: WSL execution adapter is target-aware; direct POSIX import needs
  physical Windows contract verification.
- Everything+ and Knowledge Base: native filesystem/watcher consumers; WSL UNC
  indexing/vault behavior and alias identity remain release acceptance.
- Code Pad: Windows/UNC workspace and Windows-local LSP are its current
  documented boundary; POSIX input and WSL-hosted LSP need capability UX review.
- Log Lens: already uses fixed distro-scoped WSL argv and bounded readers;
  physical source/cancel/timeout acceptance remains.

## Verification

The following checks passed in the dedicated worktree:

```text
cargo test -p git -p repo-manager -p workbench -j2
  PASS: git 18, repo-manager 122, workbench 122
cargo clippy -p git -p repo-manager -p workbench \
  --all-targets --all-features -- -D warnings
  PASS
cargo fmt --all -- --check
  PASS
git diff --check
  PASS
```

Full workspace Rust/frontend validation and Windows CI are recorded on the PR.
The physical Windows+WSL matrix in the specification remains required release
evidence and is not replaced by Linux unit tests.
