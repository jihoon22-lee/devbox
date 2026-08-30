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
pnpm install --frozen-lockfile
  PASS: lockfile unchanged
pnpm build
  PASS: all frontend workspace packages
pnpm test
  PASS: all frontend workspace packages, 1,284 tests
pnpm audit --audit-level moderate
  PASS: no known vulnerabilities
python3 .github/scripts/check-dependencies.py check
python3 .github/scripts/test-check-dependencies.py
python3 .github/scripts/test-build-manifest.py
python3 .github/scripts/test-validate-release-input.py
  PASS
cargo deny --locked check
  PASS: advisories, bans, licenses, and sources
bash .github/scripts/check-catalog.sh
  PASS: release catalog and 15 app contracts align
git diff --check
  PASS
```

The full workspace checks also passed after rebasing onto `main`:

```text
cargo check --workspace -j2
cargo test --workspace -j2
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Windows CI is recorded on the PR. The physical Windows+WSL matrix in the
specification remains required release evidence and is not replaced by Linux
unit tests.

The first PR Windows run exposed two portability assumptions in existing real
Git and loopback fixtures. Windows canonicalization returned extended `\\?\`
paths while the approved preview intentionally retained ordinary host spelling,
and accepted sockets inherited a fixture listener's non-blocking mode. The
repair compares only validated ordinary host spellings, keeps filesystem
identity as the authority, and explicitly restores blocking mode on accepted
fixture streams. Targeted regression validation after the repair passed:

```text
cargo test -p git -p repo-manager --lib -j2
  PASS: git 19, repo-manager 122
cargo fmt --all -- --check
  PASS
```
